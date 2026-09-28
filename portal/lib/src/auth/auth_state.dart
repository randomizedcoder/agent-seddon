import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:flutter/foundation.dart';
// `GrpcError` from the entry point that builds for both web and native.
import 'package:grpc/grpc_or_grpcweb.dart' show GrpcError;
import 'package:grpc/service_api.dart';

import '../gen/agent/v1/auth.pbgrpc.dart';
import 'auth_platform.dart';
import 'capabilities.dart';
import 'pkce.dart';

/// `--dart-define=PORTAL_AUTH=`: `auto` (default) signs in when the agent offers
/// browser sign-in and runs anonymously when it does not; `on` insists on
/// sign-in; `off` never asks (loopback dev).
enum AuthMode { auto, on, off }

AuthMode parseAuthMode(String raw) => switch (raw.trim()) {
      'on' => AuthMode.on,
      'off' => AuthMode.off,
      _ => AuthMode.auto,
    };

/// Where sign-in stands.
enum AuthPhase {
  /// Looking for a stored session or a redirect, or listing issuers.
  checking,

  /// Sign-in is off: the portal runs without a bearer, as before S13.
  off,

  /// Showing the sign-in page.
  signedOut,

  /// On the way to the identity provider.
  redirecting,

  /// Back from the identity provider; trading the code.
  exchanging,
  signedIn,
}

/// The signed-in session: the agent token, when it lapses, the refresh handle,
/// and who it says the user is.
class SignedIn {
  SignedIn(this.accessToken, this.expiresAt, this.refreshHandle, this.principal);

  final String accessToken;

  /// Unix seconds.
  final int expiresAt;
  final String refreshHandle;
  final WhoAmIResponse principal;
}

/// Seconds until the token should be refreshed: one minute before it lapses,
/// never negative.
int refreshDelaySecs(int expiresAt, int now) => max(0, expiresAt - 60 - now);

/// Browser sign-in for the portal (security-hardening S13b,
/// docs/design/security-hardening/06-portal-and-edge.md).
///
/// The flow: [beginSignIn] makes a PKCE verifier, asks `AuthService.Begin` for
/// the IdP URL, keeps `{state, verifier}` in tab storage and navigates away. The
/// IdP sends the browser back to the portal with `?code&state`; [start] sees it,
/// checks the `state` is the one this tab stored (else no exchange at all),
/// trades the code at `Exchange`, strips the query from the address bar, and
/// schedules a refresh one minute before the token lapses. A failed refresh
/// signs out with a notice.
class AuthState extends ChangeNotifier {
  AuthState({
    required this.client,
    required this.platform,
    this.mode = AuthMode.auto,
    this.preferredIssuer = '',
    this.redirectUriOverride = '',
    int Function()? now,
    Timer Function(Duration, void Function())? schedule,
    this.random,
  })  : _now = now ?? _unixNow,
        _schedule = schedule ?? Timer.new;

  static const pendingKey = 'agent-seddon.signin';
  static const sessionKey = 'agent-seddon.session';

  final AuthServiceClient client;
  final AuthPlatform platform;
  final AuthMode mode;

  /// `PORTAL_AUTH_ISSUER`: offer only this issuer when the agent lists it.
  final String preferredIssuer;

  /// `PORTAL_REDIRECT_URI`; empty ⇒ this page's own address.
  final String redirectUriOverride;
  final int Function() _now;
  final Timer Function(Duration, void Function()) _schedule;

  /// The verifier's source; null ⇒ `Random.secure()`.
  final Random? random;

  AuthPhase phase = AuthPhase.checking;
  List<LoginIssuer> issuers = const [];
  SignedIn? session;

  /// Shown on the sign-in page; never a token or a code.
  String? error;

  /// The last session ended by itself (refresh refused), not by signing out.
  bool expired = false;
  Timer? _refreshTimer;

  static int _unixNow() => DateTime.now().millisecondsSinceEpoch ~/ 1000;

  /// The bearer the interceptor sends; `null` when signed out or sign-in is off.
  String? get token => session?.accessToken;

  /// The verified tenant, for the advisory `x-agent-user-id` header.
  String? get tenant => session?.principal.tenant;

  /// The identity headers of a signed-in portal: the verified tenant (advisory;
  /// the agent takes the token's) and the auth session id as the session key.
  /// A signed-in call carries a principal, so the agent refuses a scoped service
  /// (prompts, providers, graphs, config) that names no session (S2, found by
  /// the S15c browser run). Empty when signed out or sign-in is off.
  Map<String, String> get identityHeaders {
    final p = session?.principal;
    if (p == null) return const {};
    return {
      if (p.tenant.isNotEmpty) 'x-agent-user-id': p.tenant,
      if (p.sid.isNotEmpty) 'x-agent-session-id': p.sid,
    };
  }

  /// What the UI may offer. Everything when sign-in is off or the permission
  /// list was not embedded (`perms_ref`); the server decides either way.
  Capabilities get capabilities {
    final p = session?.principal;
    if (p == null || p.permsRef) return Capabilities.all;
    return Capabilities.of(p.permissions);
  }

  /// The redirect URI the IdP returns to: `PORTAL_REDIRECT_URI`, else this page's
  /// own address without query or fragment.
  String get redirectUri {
    if (redirectUriOverride.isNotEmpty) return redirectUriOverride;
    final u = platform.currentUri;
    return Uri(
      scheme: u.scheme,
      host: u.host,
      port: u.hasPort ? u.port : null,
      path: u.path.isEmpty ? '/' : u.path,
    ).toString();
  }

  /// The issuers to offer, narrowed to [preferredIssuer] when the agent lists it.
  List<LoginIssuer> get offered {
    final preferred = issuers.where((i) => i.name == preferredIssuer).toList();
    return preferred.isNotEmpty ? preferred : issuers;
  }

  void _set(AuthPhase p, {String? error}) {
    phase = p;
    this.error = error;
    notifyListeners();
  }

  /// Decide where sign-in stands: finish a redirect, resume a stored session, or
  /// list the issuers.
  Future<void> start() async {
    if (mode == AuthMode.off) {
      _set(AuthPhase.off);
      return;
    }
    final q = platform.currentUri.queryParameters;
    if (q.containsKey('code') || q.containsKey('error') || q.containsKey('state')) {
      await _finish(q);
      return;
    }
    if (await _resume()) return;
    await _listIssuers();
  }

  Future<void> _listIssuers() async {
    try {
      issuers = (await client.issuers(IssuersRequest())).issuers;
    } on GrpcError catch (e) {
      issuers = const [];
      if (mode == AuthMode.auto) {
        _set(AuthPhase.off);
        return;
      }
      _set(AuthPhase.signedOut,
          error: 'Sign-in is unavailable (${e.codeName}).');
      return;
    }
    if (issuers.isEmpty) {
      if (mode == AuthMode.auto) {
        _set(AuthPhase.off);
        return;
      }
      _set(AuthPhase.signedOut,
          error: 'The agent offers no browser sign-in '
              '(set `[auth] redirect_uris`).');
      return;
    }
    _set(AuthPhase.signedOut, error: error);
  }

  /// Start signing in with [issuer]: `Begin`, remember the verifier, go.
  Future<void> beginSignIn(String issuer) async {
    if (!platform.canRedirect) {
      _set(AuthPhase.signedOut,
          error: 'Browser sign-in needs the web portal (`nix run .#portal-web`).');
      return;
    }
    final verifier = newVerifier(random);
    try {
      final begun = await client.begin(BeginRequest(
        issuer: issuer,
        redirectUri: redirectUri,
        codeChallenge: challengeOf(verifier),
      ));
      platform.write(
        pendingKey,
        jsonEncode({'state': begun.state, 'verifier': verifier, 'issuer': issuer}),
      );
      _set(AuthPhase.redirecting);
      platform.navigate(begun.authorizeUrl);
    } on GrpcError catch (e) {
      _set(AuthPhase.signedOut,
          error: 'Could not start sign-in: ${e.message ?? e.codeName}');
    }
  }

  /// Back from the IdP: the `state` must be the one this tab stored, and is
  /// spent either way.
  Future<void> _finish(Map<String, String> q) async {
    final pending = _takePending();
    platform.replaceUri(Uri.parse(redirectUri));
    final idpError = q['error'];
    if (idpError != null) {
      await _listIssuers();
      _set(AuthPhase.signedOut,
          error: 'The identity provider did not sign you in (${_safe(idpError)}).');
      return;
    }
    final code = q['code'] ?? '';
    final state = q['state'] ?? '';
    if (pending == null || state.isEmpty || pending['state'] != state || code.isEmpty) {
      await _listIssuers();
      _set(AuthPhase.signedOut,
          error: 'That sign-in did not start in this tab, or was already used. '
              'Sign in again.');
      return;
    }
    _set(AuthPhase.exchanging);
    try {
      final resp = await client.exchange(ExchangeRequest(
        code: code,
        state: state,
        codeVerifier: pending['verifier'] ?? '',
        clientKind: 'portal',
      ));
      _signedIn(resp);
    } on GrpcError catch (e) {
      await _listIssuers();
      _set(AuthPhase.signedOut, error: 'Sign-in failed (${e.codeName}). Sign in again.');
    }
  }

  Map<String, dynamic>? _takePending() {
    final raw = platform.read(pendingKey);
    platform.remove(pendingKey);
    if (raw == null) return null;
    try {
      final v = jsonDecode(raw);
      return v is Map<String, dynamic> ? v : null;
    } on FormatException {
      return null;
    }
  }

  /// A session this tab stored: still valid ⇒ confirm it with `WhoAmI`; lapsed ⇒
  /// refresh it. Anything wrong ⇒ forget it.
  Future<bool> _resume() async {
    final raw = platform.read(sessionKey);
    if (raw == null) return false;
    Map<String, dynamic> stored;
    try {
      stored = jsonDecode(raw) as Map<String, dynamic>;
    } on Object {
      platform.remove(sessionKey);
      return false;
    }
    final token = stored['access_token'];
    final expiresAt = stored['expires_at'];
    final handle = stored['refresh_handle'];
    if (token is! String || expiresAt is! int || handle is! String) {
      platform.remove(sessionKey);
      return false;
    }
    if (expiresAt > _now() + 60) {
      try {
        final me = await client.whoAmI(WhoAmIRequest(),
            options: CallOptions(metadata: {'authorization': 'Bearer $token'}));
        session = SignedIn(token, expiresAt, handle, me);
        _scheduleRefresh();
        _set(AuthPhase.signedIn);
        return true;
      } on GrpcError {
        // Fall through to a refresh.
      }
    }
    return _refreshWith(handle, quiet: true);
  }

  void _signedIn(ExchangeResponse resp) {
    session = SignedIn(resp.accessToken, resp.expiresAt.toInt(), resp.refreshHandle,
        resp.principal);
    expired = false;
    platform.write(
      sessionKey,
      jsonEncode({
        'access_token': resp.accessToken,
        'expires_at': resp.expiresAt.toInt(),
        'refresh_handle': resp.refreshHandle,
      }),
    );
    _scheduleRefresh();
    _set(AuthPhase.signedIn);
  }

  void _scheduleRefresh() {
    _refreshTimer?.cancel();
    final s = session;
    if (s == null) return;
    _refreshTimer = _schedule(
      Duration(seconds: refreshDelaySecs(s.expiresAt, _now())),
      () => unawaited(refresh()),
    );
  }

  /// Trade the refresh handle for a new token. Called by the timer; a refusal
  /// ends the session with a notice.
  Future<bool> refresh() async {
    final s = session;
    if (s == null) return false;
    return _refreshWith(s.refreshHandle, quiet: false);
  }

  Future<bool> _refreshWith(String handle, {required bool quiet}) async {
    try {
      _signedIn(await client.refresh(RefreshRequest(refreshHandle: handle)));
      return true;
    } on GrpcError {
      _clear();
      if (quiet) return false;
      expired = true;
      await _listIssuers();
      _set(AuthPhase.signedOut, error: 'Your session ended. Sign in again.');
      return false;
    }
  }

  /// Revoke the session at the agent (best effort) and forget it here.
  Future<void> signOut() async {
    if (session != null) {
      try {
        await client.logout(LogoutRequest());
      } on GrpcError {
        // Signed out here either way; the token lapses on its own.
      }
    }
    _clear();
    expired = false;
    await _listIssuers();
  }

  void _clear() {
    _refreshTimer?.cancel();
    _refreshTimer = null;
    session = null;
    platform.remove(sessionKey);
  }

  @override
  void dispose() {
    _refreshTimer?.cancel();
    super.dispose();
  }

  /// An IdP error code fit to show: short, plain characters, else `unknown`.
  static String _safe(String s) =>
      RegExp(r'^[A-Za-z0-9_.\-]{1,64}$').hasMatch(s) ? s : 'unknown';
}
