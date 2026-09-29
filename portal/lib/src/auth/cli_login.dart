import 'dart:convert';

/// Native desktop sign-in through the `agent` CLI (security-hardening S23).
///
/// The desktop app cannot take a browser redirect, so it borrows the login
/// `agent login` stored: it runs `agent token --json`, which refreshes the login
/// under the token file's lock when stale and prints the agent token. The portal
/// never reads or refreshes the file itself. Dart's file lock is `fcntl`-based and
/// does not exclude the CLI's `flock`, so two refreshers could spend one
/// single-use handle and end the session.
///
/// This file holds the web-safe part (the output format and its checks); the
/// process runner is in `platform_io.dart`.

/// What `agent token --json` printed.
class CliToken {
  const CliToken(this.accessToken, this.expiresAt, this.issuer);

  final String accessToken;

  /// Unix seconds.
  final int expiresAt;

  /// The login issuer's name (for display).
  final String issuer;
}

/// Why the CLI gave no token. [signInNeeded] when the user must run
/// `agent login` (exit 2, not signed in; exit 3, the session ended).
class CliLoginError implements Exception {
  const CliLoginError(this.message, {this.signInNeeded = false});

  final String message;
  final bool signInNeeded;

  @override
  String toString() => message;
}

/// Where a signed-in native portal gets its agent token.
abstract class CliLogin {
  Future<CliToken> token();
}

/// Most bytes of `agent token --json` output accepted.
const maxCliOutputBytes = 64 * 1024;

/// Longest access token accepted (an agent token is a compact JWT).
const maxAccessTokenBytes = 16 * 1024;

// A compact JWT, or any bearer made of token68 characters (RFC 7235): no space,
// no CR/LF, so it cannot split the `authorization` header it is put in.
final _token68 = RegExp(r'^[A-Za-z0-9._~+/=-]+$');
final _issuerName = RegExp(r'^[A-Za-z0-9_.-]{0,64}$');

/// Parse `agent token --json` output. Anything unexpected is refused: the token
/// goes into a request header.
CliToken parseCliToken(String out) {
  const bad = CliLoginError('`agent token` printed something unexpected.');
  if (out.length > maxCliOutputBytes) throw bad;
  final Object? v;
  try {
    v = jsonDecode(out.trim());
  } on FormatException {
    throw bad;
  }
  if (v is! Map<String, dynamic>) throw bad;
  final token = v['access_token'];
  final expiresAt = v['expires_at'];
  final issuer = v['issuer'] ?? '';
  if (token is! String ||
      token.isEmpty ||
      token.length > maxAccessTokenBytes ||
      !_token68.hasMatch(token)) {
    throw bad;
  }
  if (expiresAt is! int || expiresAt <= 0) throw bad;
  if (issuer is! String || !_issuerName.hasMatch(issuer)) throw bad;
  return CliToken(token, expiresAt, issuer);
}

/// The CLI's refusal, fit to show on the sign-in page: the first line of its
/// stderr, printable characters only, at most 200 characters. With no stderr, a
/// generic line (and, for exit 2 or 3, what to run).
String cliMessage(String stderr, int exitCode) {
  final first = const LineSplitter().convert(stderr).firstWhere(
        (l) => l.trim().isNotEmpty,
        orElse: () => '',
      );
  final clean = first.replaceAll(RegExp(r'[\x00-\x1f\x7f]'), '').trim();
  final shown = clean.length > 200 ? '${clean.substring(0, 200)}…' : clean;
  if (shown.isNotEmpty) return shown; // the CLI's own text says what to run
  final hint = switch (exitCode) {
    2 || 3 => ' Run `agent login` in a terminal, then try again.',
    _ => '',
  };
  return '`agent token` failed (exit $exitCode).$hint';
}
