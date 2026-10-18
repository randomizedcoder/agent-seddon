import 'dart:convert';

import 'auth_platform.dart';

/// GitHub's return from a GitHub App install (forge-auth F5d1).
///
/// With "Request user authorization (OAuth) during installation" on, GitHub
/// sends the browser back to the App's callback URL — the portal — with
/// `?code=&installation_id=&setup_action=&state=`. That carries a `code` and a
/// `state` just like an IdP sign-in return, so [AuthState] must tell the two
/// apart before it spends the sign-in entry: an install return is recognised
/// by `installation_id` or `setup_action`, which an IdP never sends.
///
/// Every field is attacker-controlled (anyone can craft the link), so a
/// malformed return is still *consumed* — stripped from the address bar — but
/// flagged [malformed] and never relayed. The server re-proves everything
/// (`CompleteInstall`); these checks only keep junk off the wire.
class InstallCallback {
  const InstallCallback({
    required this.code,
    required this.state,
    required this.installationId,
    required this.setupAction,
    this.malformed = false,
  });

  final String code;
  final String state;
  final String installationId;

  /// `install` | `update` | `request` (an org owner must approve; GitHub sends
  /// no usable installation yet).
  final String setupAction;
  final bool malformed;

  /// An org member asked an owner to approve; nothing to complete yet.
  bool get awaitingApproval => !malformed && setupAction == 'request';

  Map<String, String> toJson() => {
        'code': code,
        'state': state,
        'installation_id': installationId,
        'setup_action': setupAction,
      };
}

/// Longest `state` worth relaying; the agent's are 43 characters.
const maxInstallStateBytes = 128;

/// Largest OAuth `code` worth relaying (the agent's own cap).
const maxInstallCodeBytes = 4096;

const _setupActions = {'install', 'update', 'request'};

/// A GitHub installation id: 1–20 ASCII digits (the agent's
/// `valid_installation_id`).
bool validInstallationId(String s) =>
    s.isNotEmpty && s.length <= 20 && RegExp(r'^[0-9]+$').hasMatch(s);

bool _printable(String s) => !s.codeUnits.any((c) => c < 0x21 || c > 0x7e);

/// The install return in [q], or `null` when [q] is not one (a sign-in return,
/// or no callback at all).
InstallCallback? parseInstallCallback(Map<String, String> q) {
  if (!q.containsKey('installation_id') && !q.containsKey('setup_action')) {
    return null;
  }
  final code = q['code'] ?? '';
  final state = q['state'] ?? '';
  final id = q['installation_id'] ?? '';
  final action = q['setup_action'] ?? '';
  final actionOk = _setupActions.contains(action);
  final stateOk = state.isNotEmpty &&
      state.length <= maxInstallStateBytes &&
      _printable(state);
  // A `request` carries no installation to complete; only its action matters.
  final completable = action == 'request' ||
      (validInstallationId(id) &&
          code.isNotEmpty &&
          code.length <= maxInstallCodeBytes &&
          _printable(code) &&
          stateOk);
  return InstallCallback(
    code: code,
    state: state,
    installationId: id,
    setupAction: action,
    malformed: !actionOk || !completable,
  );
}

/// The `state` a `BeginInstall` in this tab is waiting on, in per-tab storage.
/// Single use: [takeInstallPending] removes it whatever it held.
const installPendingKey = 'agent-seddon.ghinstall';

void storeInstallPending(AuthPlatform p, String state, int expiresAt) =>
    p.write(installPendingKey,
        jsonEncode({'state': state, 'expires_at': expiresAt}));

/// The pending install `state`, or `null` when none, unreadable or lapsed at
/// [now] (unix seconds). Removed either way.
String? takeInstallPending(AuthPlatform p, int now) {
  final raw = p.read(installPendingKey);
  p.remove(installPendingKey);
  if (raw == null) return null;
  try {
    final v = jsonDecode(raw);
    if (v is! Map<String, dynamic>) return null;
    final state = v['state'];
    final expiresAt = v['expires_at'];
    if (state is! String || state.isEmpty || expiresAt is! int) return null;
    return now <= expiresAt ? state : null;
  } on FormatException {
    return null;
  }
}

/// Where a callback waits across a sign-in round trip (the user may land
/// signed out, sign in, and only then can it be relayed).
const installCallbackKey = 'agent-seddon.ghinstall.cb';

void stashInstallCallback(AuthPlatform p, InstallCallback cb) =>
    p.write(installCallbackKey, jsonEncode(cb.toJson()));

/// The stashed callback, re-parsed (storage is no more trusted than the URL);
/// removed either way.
InstallCallback? takeStashedInstallCallback(AuthPlatform p) {
  final raw = p.read(installCallbackKey);
  p.remove(installCallbackKey);
  if (raw == null) return null;
  try {
    final v = jsonDecode(raw);
    if (v is! Map<String, dynamic>) return null;
    return parseInstallCallback({
      for (final e in v.entries)
        if (e.value is String) e.key: e.value as String,
    });
  } on FormatException {
    return null;
  }
}
