import 'package:agent_portal/src/auth/auth_platform.dart';
import 'package:agent_portal/src/auth/install_callback.dart';
import 'package:agent_portal/src/pages/forges_page.dart';
import 'package:flutter_test/flutter_test.dart';

/// L0 tables for the GitHub App install return (forge-auth F5d1): telling it
/// apart from a sign-in return, the fail-closed shape checks, the single-use
/// pending `state`, the stash that survives a sign-in, and the two guards the
/// Forges page applies to wire values (install URL, forge-card id).
void main() {
  Map<String, String> ret({
    String? code = 'c1',
    String? state = 's1',
    String? id = '42',
    String? action = 'install',
  }) =>
      {
        if (code != null) 'code': code,
        if (state != null) 'state': state,
        if (id != null) 'installation_id': id,
        if (action != null) 'setup_action': action,
      };

  group('parseInstallCallback', () {
    for (final (name, q, recognised, malformed) in [
      ('positive_install', ret(), true, false),
      ('positive_update', ret(action: 'update'), true, false),
      ('corner_request_needs_no_install', ret(action: 'request', id: null, code: null), true, false),
      ('negative_sign_in_return_is_not_ours', ret(id: null, action: null), false, false),
      ('negative_no_query', <String, String>{}, false, false),
      ('negative_idp_error_is_not_ours', {'error': 'access_denied', 'state': 's1'}, false, false),
      ('boundary_twenty_digit_id', ret(id: '1' * 20), true, false),
      ('boundary_twenty_one_digit_id', ret(id: '1' * 21), true, true),
      ('boundary_state_at_cap', ret(state: 'a' * maxInstallStateBytes), true, false),
      ('boundary_state_over_cap', ret(state: 'a' * (maxInstallStateBytes + 1)), true, true),
      ('boundary_code_over_cap', ret(code: 'c' * (maxInstallCodeBytes + 1)), true, true),
      ('negative_missing_code', ret(code: null), true, true),
      ('negative_missing_state', ret(state: null), true, true),
      ('negative_missing_action', ret(action: null), true, true),
      ('adversarial_traversal_id', ret(id: '../../1'), true, true),
      ('adversarial_signed_id', ret(id: '-1'), true, true),
      ('adversarial_unicode_digit_id', ret(id: '٤٢'), true, true),
      ('adversarial_unknown_action', ret(action: 'uninstall'), true, true),
      ('adversarial_crlf_state', ret(state: 's\r\nSet-Cookie: x'), true, true),
      ('adversarial_space_in_code', ret(code: 'c 1'), true, true),
      ('adversarial_only_setup_action', {'setup_action': 'install'}, true, true),
    ]) {
      test(name, () {
        final cb = parseInstallCallback(q);
        expect(cb != null, recognised);
        if (cb != null) expect(cb.malformed, malformed);
      });
    }

    test('positive_fields_carried', () {
      final cb = parseInstallCallback(ret())!;
      expect((cb.code, cb.state, cb.installationId, cb.setupAction),
          ('c1', 's1', '42', 'install'));
      expect(cb.awaitingApproval, isFalse);
    });

    test('corner_malformed_request_is_not_awaiting', () {
      expect(parseInstallCallback(ret(action: 'request'))!.awaitingApproval, isTrue);
      expect(parseInstallCallback(ret(action: 'nope'))!.awaitingApproval, isFalse);
    });
  });

  group('pending state', () {
    test('positive_round_trip_then_spent', () {
      final p = MemoryAuthPlatform();
      storeInstallPending(p, 's1', 100);
      expect(takeInstallPending(p, 100), 's1');
      expect(takeInstallPending(p, 100), isNull, reason: 'single use');
    });

    test('boundary_one_second_late_is_gone', () {
      final p = MemoryAuthPlatform();
      storeInstallPending(p, 's1', 100);
      expect(takeInstallPending(p, 101), isNull);
      expect(p.storage.containsKey(installPendingKey), isFalse);
    });

    for (final (name, raw) in [
      ('adversarial_not_json', 'not json'),
      ('adversarial_list', '["s1"]'),
      ('adversarial_string_expiry', '{"state":"s1","expires_at":"9999999999"}'),
      ('adversarial_empty_state', '{"state":"","expires_at":100}'),
    ]) {
      test(name, () {
        final p = MemoryAuthPlatform()..storage[installPendingKey] = raw;
        expect(takeInstallPending(p, 1), isNull);
        expect(p.storage.containsKey(installPendingKey), isFalse);
      });
    }
  });

  group('stash', () {
    test('positive_survives_and_is_single_use', () {
      final p = MemoryAuthPlatform();
      stashInstallCallback(p, parseInstallCallback(ret())!);
      final back = takeStashedInstallCallback(p)!;
      expect((back.code, back.state, back.installationId), ('c1', 's1', '42'));
      expect(takeStashedInstallCallback(p), isNull);
    });

    test('adversarial_tampered_stash_is_reparsed', () {
      final p = MemoryAuthPlatform()
        ..storage[installCallbackKey] =
            '{"code":"c1","state":"s1","installation_id":"../1","setup_action":"install"}';
      expect(takeStashedInstallCallback(p)!.malformed, isTrue);
    });

    test('adversarial_garbage_stash_is_dropped', () {
      final p = MemoryAuthPlatform()..storage[installCallbackKey] = '{';
      expect(takeStashedInstallCallback(p), isNull);
      expect(p.storage.containsKey(installCallbackKey), isFalse);
    });
  });

  group('safeInstallUrl', () {
    for (final (name, url, ok) in [
      ('positive_github', 'https://github.com/apps/a/installations/new?state=s', true),
      ('positive_ghes', 'https://ghe.example.com/apps/a/installations/new', true),
      ('corner_loopback_http', 'http://127.0.0.1:9000/apps/a/installations/new', true),
      ('negative_plain_http', 'http://github.com/apps/a', false),
      ('negative_relative', '/apps/a', false),
      ('negative_empty', '', false),
      ('adversarial_javascript', 'javascript:alert(1)', false),
      ('adversarial_data', 'data:text/html,<script>', false),
      ('adversarial_scheme_relative', '//evil.example/x', false),
      ('adversarial_loopback_lookalike', 'http://127.0.0.1.evil.example/', false),
    ]) {
      test(name, () => expect(safeInstallUrl(url), ok));
    }
  });

  group('validForgeId', () {
    for (final (name, id, ok) in [
      ('positive_default', 'github-42', true),
      ('positive_dots_underscores', 'acme_gh.v2', true),
      ('boundary_max_len', 'a' * maxForgeIdLen, true),
      ('boundary_over_max', 'a' * (maxForgeIdLen + 1), false),
      ('negative_empty', '', false),
      ('negative_space', 'a b', false),
      ('adversarial_traversal', '../../x', false),
      ('adversarial_separator', 'a/b', false),
      ('adversarial_double_dot', 'a..b', false),
      ('adversarial_leading_dash', '-rf', false),
      ('adversarial_leading_dot', '.git', false),
      ('adversarial_non_ascii', 'gïthub', false),
    ]) {
      test(name, () => expect(validForgeId(id), ok));
    }
  });
}
