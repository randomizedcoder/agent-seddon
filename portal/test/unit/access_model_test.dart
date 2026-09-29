import 'package:agent_portal/src/access_model.dart';
import 'package:agent_portal/src/gen/agent/v1/role.pb.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

/// Tables for the Access tab's pure checks (security-hardening S22). Each case
/// name carries its class prefix, as in the Rust `rstest` tables.
void main() {
  group('checkId', () {
    final cases = <String, (String, bool)>{
      'positive_plain': ('alice-admins', true),
      'positive_dots_and_underscores': ('team.core_1', true),
      'boundary_128_chars': ('a' * 128, true),
      'boundary_129_chars': ('a' * 129, false),
      'negative_empty': ('', false),
      'negative_leading_dash': ('-x', false),
      'corner_leading_dot': ('.hidden', false),
      'adversarial_traversal': ('../x', false),
      'adversarial_double_dot_inside': ('a..b', false),
      'adversarial_slash': ('a/b', false),
      'adversarial_space': ('a b', false),
      'adversarial_unicode_lookalike': ('аdmin', false), // Cyrillic а
    };
    cases.forEach((name, c) {
      test(name, () => expect(checkId(c.$1) == null, c.$2, reason: c.$1));
    });
  });

  group('checkBindingDraft', () {
    String? check({
      String id = 'b1',
      String kind = 'email',
      String subject = 'a@example.com',
      List<String> roles = const ['viewer'],
      String expires = '',
    }) =>
        checkBindingDraft(
            id: id,
            subjectKind: kind,
            subject: subject,
            roles: roles,
            expiresInDays: expires);

    final cases = <String, (String? Function(), String?)>{
      'positive_minimal': (() => check(), null),
      'boundary_subject_at_cap': (() => check(subject: 'a' * 320), null),
      'boundary_subject_over_cap': (() => check(subject: 'a' * 321), 'longer than 320'),
      'boundary_32_roles': (() => check(roles: List.filled(32, 'r')), null),
      'boundary_33_roles': (() => check(roles: List.filled(33, 'r')), 'At most 32'),
      'negative_no_roles': (() => check(roles: const []), 'at least one role'),
      'negative_blank_subject': (() => check(subject: '   '), 'subject is required'),
      'negative_unknown_kind': (() => check(kind: 'group'), 'subject kind'),
      'corner_expiry_zero_is_never': (() => check(expires: '0'), null),
      'negative_expiry_negative': (() => check(expires: '-1'), 'whole number'),
      'adversarial_control_char': (() => check(subject: 'a\u0000b'), 'control'),
      'adversarial_newline_injection': (() => check(subject: 'a\nb'), 'control'),
      'adversarial_bad_id': (() => check(id: '../etc'), 'not ".."'),
    };
    cases.forEach((name, c) {
      test(name, () {
        final got = c.$1();
        if (c.$2 == null) {
          expect(got, isNull);
        } else {
          expect(got, contains(c.$2!));
        }
      });
    });
  });

  group('parseExpiryDays', () {
    final cases = <String, (String, int?)>{
      'positive_days': ('30', 30),
      'corner_blank_is_never': ('  ', 0),
      'boundary_cap': ('3650', 3650),
      'boundary_over_cap': ('3651', null),
      'negative_fraction': ('1.5', null),
      'negative_word': ('ten', null),
      'adversarial_huge': ('9' * 40, null),
    };
    cases.forEach((name, c) {
      test(name, () => expect(parseExpiryDays(c.$1), c.$2));
    });
  });

  test('positive_expires_at_for', () {
    expect(expiresAtFor(0, 1000), 0);
    expect(expiresAtFor(2, 1000), 1000 + 2 * 86400);
  });

  group('parseRoleCard', () {
    final cases = <String, (String, String, String?)>{
      'positive_pairs': ('r1', 'read:fleet\nwrite:review', null),
      'positive_actions_on_all': ('r1', 'read:*, observe:*', null),
      'positive_everything': ('r1', ' * ', null),
      'negative_nothing': ('r1', '\n , \n', 'must grant something'),
      'negative_star_mixed': ('r1', '*\nread:fleet', 'nothing else'),
      'negative_shapes_mixed': ('r1', 'read:*\nwrite:fleet', 'not both'),
      'negative_not_a_pair': ('r1', 'readfleet', 'not action:resource'),
      'boundary_64_entries': ('r1', List.filled(64, 'read:fleet').join('\n'), null),
      'boundary_65_entries': ('r1', List.filled(65, 'read:fleet').join('\n'), 'At most 64'),
      'adversarial_builtin_id': ('org_admin', 'read:fleet', 'built-in'),
      'adversarial_reader_alias_id': ('reader', 'read:fleet', 'built-in'),
      'adversarial_uppercase_injection': ('r1', 'READ:fleet', 'not action:resource'),
      'adversarial_extra_colon': ('r1', 'read:fleet:x', 'not action:resource'),
      'adversarial_traversal_id': ('../r', 'read:fleet', 'not ".."'),
    };
    cases.forEach((name, c) {
      test(name, () {
        final got = parseRoleCard(id: c.$1, crossesTenants: false, permissions: c.$2);
        if (c.$3 == null) {
          expect(got.error, isNull);
          expect(got.card, isNotNull);
        } else {
          expect(got.card, isNull);
          expect(got.error, contains(c.$3!));
        }
      });
    });

    test('positive_shapes_round_trip_through_the_editor_text', () {
      for (final text in ['*', 'read:*\nobserve:*', 'read:fleet\nwrite:review']) {
        final card = parseRoleCard(id: 'r1', crossesTenants: true, permissions: text).card!;
        expect(formatPermissions(card), text);
        expect(card.crossesTenants, isTrue);
      }
    });
  });

  group('summarizePermissions', () {
    final cases = <String, (RoleCard, String)>{
      'positive_all': (RoleCard(all: true), 'everything'),
      'positive_pairs': (
        RoleCard(pairs: [RolePermission(action: 'read', resourceType: 'fleet')]),
        'read:fleet'
      ),
      'positive_on_all': (RoleCard(actionsOnAll: ['read']), 'read on every resource'),
      'corner_empty_grants_nothing': (RoleCard(), 'nothing'),
    };
    cases.forEach((name, c) {
      test(name, () => expect(summarizePermissions(c.$1), c.$2));
    });
  });

  group('describeAccessError', () {
    final cases = <String, (Object, String)>{
      'positive_last_admin_text_passes_through': (
        const GrpcError.failedPrecondition('nobody who can manage role bindings'),
        'nobody who can manage'
      ),
      'negative_denied_explains_the_rules': (
        const GrpcError.permissionDenied('permission denied'),
        'only permissions you hold'
      ),
      'negative_invalid_argument': (const GrpcError.invalidArgument('unknown role'), 'Rejected: unknown role'),
      'corner_not_found': (const GrpcError.notFound(''), 'no longer exists'),
      'corner_unimplemented': (const GrpcError.unimplemented(''), 'does not serve'),
      'corner_unauthenticated': (const GrpcError.unauthenticated(''), 'sign in again'),
      'boundary_empty_precondition': (const GrpcError.failedPrecondition(''), 'refused the change'),
      'adversarial_other_code_without_message': (const GrpcError.dataLoss(), 'code 15'),
      'positive_non_grpc': (StateError('boom'), 'boom'),
    };
    cases.forEach((name, c) {
      test(name, () => expect(describeAccessError(c.$1), contains(c.$2)));
    });
  });

  group('formatUnix', () {
    test('positive_utc', () => expect(formatUnix(0 + 86400 + 3661), '1970-01-02 01:01 UTC'));
    test('corner_zero_is_never', () => expect(formatUnix(0), 'never'));
    test('adversarial_negative_is_never', () => expect(formatUnix(-5), 'never'));
  });
}
