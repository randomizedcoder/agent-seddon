import 'dart:async';
import 'dart:convert';
import 'dart:io';

import '../config.dart';
import 'auth_platform.dart';
import 'cli_login.dart';

/// Native desktop: no browser to redirect, so sign-in state lives in memory.
AuthPlatform createAuthPlatform() => MemoryAuthPlatform();

/// Native desktop signs in with the `agent` CLI's stored login (S23).
CliLogin? createCliLogin(PortalConfig cfg) => ProcessCliLogin(
      bin: cfg.agentBin,
      issuer: cfg.authIssuer,
      config: cfg.agentConfig,
    );

/// One finished run: exit code, stdout, stderr.
typedef CliRun = ({int exitCode, String stdout, String stderr});

/// Runs `bin args`, killing it after `timeout`.
typedef CliRunner = Future<CliRun> Function(
    String bin, List<String> args, Duration timeout);

/// [CliLogin] by running `agent token --json [--issuer NAME] [--config FILE]`.
class ProcessCliLogin implements CliLogin {
  ProcessCliLogin({
    required this.bin,
    this.issuer = '',
    this.config = '',
    this.timeout = const Duration(seconds: 30),
    CliRunner? runner,
  }) : _run = runner ?? _runProcess;

  /// `PORTAL_AGENT_BIN`: the `agent` binary (a path, or a name on `PATH`).
  final String bin;

  /// `PORTAL_AUTH_ISSUER`: which stored login; empty ⇒ the CLI decides.
  final String issuer;

  /// `PORTAL_AGENT_CONFIG`: the CLI's config file; empty ⇒ its default.
  final String config;
  final Duration timeout;
  final CliRunner _run;

  List<String> get args => [
        'token',
        '--json',
        if (issuer.isNotEmpty) ...['--issuer', issuer],
        if (config.isNotEmpty) ...['--config', config],
      ];

  @override
  Future<CliToken> token() async {
    final CliRun r;
    try {
      r = await _run(bin, args, timeout);
    } on ProcessException catch (e) {
      throw CliLoginError('Could not run `$bin` (${e.message}). Install the agent '
          'CLI, or point PORTAL_AGENT_BIN at it.');
    } on TimeoutException {
      throw CliLoginError(
          '`$bin token` did not answer within ${timeout.inSeconds} s.');
    }
    if (r.exitCode != 0) {
      throw CliLoginError(cliMessage(r.stderr, r.exitCode),
          signInNeeded: r.exitCode == 2 || r.exitCode == 3);
    }
    return parseCliToken(r.stdout);
  }
}

Future<CliRun> _runProcess(
    String bin, List<String> args, Duration timeout) async {
  final p = await Process.start(bin, args);
  await p.stdin.close();
  final out = p.stdout.transform(utf8.decoder).join();
  final err = p.stderr.transform(utf8.decoder).join();
  final code = await p.exitCode.timeout(timeout, onTimeout: () {
    p.kill();
    throw TimeoutException('agent token', timeout);
  });
  return (exitCode: code, stdout: await out, stderr: await err);
}
