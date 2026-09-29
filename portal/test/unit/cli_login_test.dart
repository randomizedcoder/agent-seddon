import 'dart:async';
import 'dart:io';

import 'package:agent_portal/src/auth/auth_state.dart';
import 'package:agent_portal/src/auth/cli_login.dart';
import 'package:agent_portal/src/auth/platform_io.dart';
import 'package:flutter_test/flutter_test.dart';

/// Native desktop sign-in through `agent token --json` (security-hardening S23):
/// the output check, the shown message, the refresh schedule, and the process
/// runner with a scripted [CliRunner].
void main() {
  group('parseCliToken', () {
    final ok = <String, (String, String, int)>{
      'positive_jwt': (
        '{"access_token":"eyJh.eyJz.c2ln","expires_at":1700000000,"issuer":"google","endpoint":"https://a:1"}',
        'eyJh.eyJz.c2ln',
        1700000000
      ),
      'corner_trailing_newline': ('{"access_token":"t","expires_at":1}\n', 't', 1),
      'corner_no_issuer': ('{"access_token":"t","expires_at":1}', 't', 1),
      'boundary_token_at_cap': (
        '{"access_token":"${'a' * maxAccessTokenBytes}","expires_at":1}',
        'a' * maxAccessTokenBytes,
        1
      ),
    };
    ok.forEach((name, c) {
      test(name, () {
        final t = parseCliToken(c.$1);
        expect(t.accessToken, c.$2);
        expect(t.expiresAt, c.$3);
      });
    });

    final refused = <String, String>{
      'negative_not_json': 'eyJh.eyJz.c2ln',
      'negative_array': '["t"]',
      'negative_no_token': '{"expires_at":1}',
      'negative_empty_token': '{"access_token":"","expires_at":1}',
      'negative_no_expiry': '{"access_token":"t"}',
      'negative_expiry_string': '{"access_token":"t","expires_at":"1"}',
      'boundary_expiry_zero': '{"access_token":"t","expires_at":0}',
      'boundary_token_over_cap':
          '{"access_token":"${'a' * (maxAccessTokenBytes + 1)}","expires_at":1}',
      'boundary_output_over_cap':
          '{"access_token":"t","expires_at":1,"pad":"${'x' * maxCliOutputBytes}"}',
      'adversarial_crlf_header_injection':
          r'{"access_token":"t\r\nx-agent-user-id: evil","expires_at":1}',
      'adversarial_space_in_token': '{"access_token":"a b","expires_at":1}',
      'adversarial_hostile_issuer': '{"access_token":"t","expires_at":1,"issuer":"../x"}',
      'adversarial_nul_in_token': r'{"access_token":"a\u0000b","expires_at":1}',
    };
    refused.forEach((name, out) {
      test(name, () {
        expect(() => parseCliToken(out),
            throwsA(isA<CliLoginError>().having((e) => e.message, 'message',
                contains('unexpected'))));
      });
    });
  });

  group('cliMessage', () {
    final cases = <String, (String, int, String)>{
      'positive_first_line': ('not signed in: run `agent login`\nmore', 2,
          'not signed in: run `agent login`'),
      'corner_empty_stderr_exit_2': ('', 2, 'Run `agent login`'),
      'corner_empty_stderr_exit_1': ('\n  \n', 1, 'failed (exit 1).'),
      'boundary_long_line_clipped': ('x' * 300, 1, '${'x' * 200}…'),
      'adversarial_escape_codes_stripped': ('\u001b[31mbad\u0007', 1, '[31mbad'),
    };
    cases.forEach((name, c) {
      test(name, () {
        final got = cliMessage(c.$1, c.$2);
        expect(got, contains(c.$3));
        expect(got, isNot(contains('\u001b')));
      });
    });
  });

  group('cliRefreshDelaySecs', () {
    final cases = <String, (int, int, int)>{
      'positive_twenty_seconds_before': (1900, 1000, 880),
      'boundary_inside_the_window': (1025, 1000, 10),
      'corner_already_expired': (900, 1000, 10),
      'adversarial_far_past_never_spins': (0, 1000, 10),
    };
    cases.forEach((name, c) {
      test(name, () => expect(cliRefreshDelaySecs(c.$1, c.$2), c.$3));
    });
  });

  group('ProcessCliLogin', () {
    ProcessCliLogin withRun(CliRun Function(String, List<String>) f,
            {String issuer = '', String config = ''}) =>
        ProcessCliLogin(
          bin: '/opt/agent',
          issuer: issuer,
          config: config,
          runner: (bin, args, timeout) async => f(bin, args),
        );

    test('positive_runs_token_json_with_the_options', () async {
      late List<String> seen;
      final cli = withRun((bin, args) {
        seen = [bin, ...args];
        return (exitCode: 0, stdout: '{"access_token":"t","expires_at":9}', stderr: '');
      }, issuer: 'google', config: '/etc/agent.toml');
      final t = await cli.token();
      expect(t.accessToken, 't');
      expect(seen,
          ['/opt/agent', 'token', '--json', '--issuer', 'google', '--config', '/etc/agent.toml']);
    });

    test('corner_no_options_when_unset', () {
      expect(withRun((_, __) => throw StateError('unused')).args, ['token', '--json']);
    });

    final exits = <String, (int, bool)>{
      'negative_not_signed_in_exit_2': (2, true),
      'negative_session_ended_exit_3': (3, true),
      'negative_other_failure_exit_1': (1, false),
    };
    exits.forEach((name, c) {
      test(name, () async {
        final cli = withRun(
            (_, __) => (exitCode: c.$1, stdout: '', stderr: 'why it failed'));
        await expectLater(
            cli.token(),
            throwsA(isA<CliLoginError>()
                .having((e) => e.signInNeeded, 'signInNeeded', c.$2)
                .having((e) => e.message, 'message', 'why it failed')));
      });
    });

    test('negative_missing_binary_says_how_to_fix', () async {
      final cli = ProcessCliLogin(
          bin: 'agent',
          runner: (_, __, ___) async =>
              throw const ProcessException('agent', [], 'No such file'));
      await expectLater(
          cli.token(),
          throwsA(isA<CliLoginError>()
              .having((e) => e.message, 'message', contains('PORTAL_AGENT_BIN'))));
    });

    test('boundary_timeout_is_reported', () async {
      final cli = ProcessCliLogin(
          bin: 'agent',
          timeout: const Duration(seconds: 30),
          runner: (_, __, t) async => throw TimeoutException('agent token', t));
      await expectLater(
          cli.token(),
          throwsA(isA<CliLoginError>()
              .having((e) => e.message, 'message', contains('within 30 s'))));
    });

    test('adversarial_exit_0_with_junk_is_refused', () async {
      final cli = withRun((_, __) =>
          (exitCode: 0, stdout: 'Bearer abc\r\nX: y', stderr: ''));
      await expectLater(cli.token(), throwsA(isA<CliLoginError>()));
    });

    /// An executable script standing in for `agent`.
    Future<String> script(String body) async {
      final dir = await Directory.systemTemp.createTemp('fake-agent');
      addTearDown(() => dir.delete(recursive: true));
      final f = File('${dir.path}/agent');
      await f.writeAsString('#!/bin/sh\n$body\n');
      await Process.run('chmod', ['700', f.path]);
      return f.path;
    }

    test('positive_default_runner_reads_a_real_process', () async {
      final bin = await script(
          'test "\$1 \$2" = "token --json" || exit 9\n'
          'echo \'{"access_token":"real-1","expires_at":42}\'');
      final t = await ProcessCliLogin(bin: bin).token();
      expect(t.accessToken, 'real-1');
      expect(t.expiresAt, 42);
    });

    test('negative_default_runner_maps_a_real_exit_2', () async {
      final bin = await script('echo "not signed in: run \\`agent login\\`" >&2\nexit 2');
      await expectLater(
          ProcessCliLogin(bin: bin).token(),
          throwsA(isA<CliLoginError>()
              .having((e) => e.signInNeeded, 'signInNeeded', isTrue)
              .having((e) => e.message, 'message', contains('not signed in'))));
    });
  });
}
