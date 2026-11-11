import 'dart:math';

import 'package:agent_portal/src/auth/auth_interceptor.dart';
import 'package:agent_portal/src/auth/auth_state.dart';
import 'package:agent_portal/src/auth/capabilities.dart';
import 'package:agent_portal/src/auth/pkce.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/service_api.dart';

/// L0 tests for the sign-in helpers (security-hardening S13b): PKCE, the
/// capability set, the refresh delay, the `PORTAL_AUTH` parser, the front
/// door's sign-in URL and the credentials every call carries.
void main() {
  final unreserved = RegExp(r'^[A-Za-z0-9\-._~]+$');

  group('pkce', () {
    test('positive_matches_the_agent_s256_vector', () {
      // The vector the agent's code_flow tests check, so both ends agree.
      expect(challengeOf('dBjftJeZ4CVP-mJ92K9mSf3VVh8lK5xbWf0KX5gRRLQ'),
          'hdKE8aDCdMC36lG0aDk4DcPbPWvL8u4gnuo2Zywds7Y');
    });

    test('boundary_verifier_is_43_unreserved_chars', () {
      final v = newVerifier(Random(7));
      expect(v.length, 43);
      expect(unreserved.hasMatch(v), isTrue);
      expect(challengeOf(v).length, 43);
    });

    test('negative_two_verifiers_differ', () {
      expect(newVerifier(), isNot(newVerifier()));
    });

    test('corner_no_padding', () {
      expect(base64UrlNoPad([0xff]), '_w');
    });
  });

  group('capabilities', () {
    test('positive_listed_pair_allowed', () {
      expect(Capabilities.of(['approve:review']).can('approve', 'review'), isTrue);
    });

    test('negative_unlisted_pair_denied', () {
      final c = Capabilities.of(['read:review']);
      expect(c.can('approve', 'review'), isFalse);
      expect(c.can('read', 'fleet'), isFalse);
    });

    test('corner_all_allows_everything', () {
      expect(Capabilities.all.can('delete', 'registry'), isTrue);
    });

    test('adversarial_near_miss_strings_denied', () {
      final c = Capabilities.of(['read:review', '*', 'approve:*']);
      expect(c.can('approve', 'review'), isFalse);
      expect(c.can('read', 'review '), isFalse);
      expect(c.can('read:review', ''), isFalse);
    });
  });

  group('refreshDelaySecs', () {
    for (final (name, expiresAt, now, want) in [
      ('positive_fifteen_minute_token', 1900, 1000, 840),
      ('boundary_exactly_one_minute_left', 1060, 1000, 0),
      ('boundary_just_over_one_minute', 1061, 1000, 1),
      ('corner_already_lapsed', 900, 1000, 0),
    ]) {
      test(name, () => expect(refreshDelaySecs(expiresAt, now), want));
    }
  });

  group('withCredentials', () {
    const id = {'x-agent-user-id': 'acme', 'x-agent-session-id': 'sid-1'};

    test('positive_bearer_and_identity_added', () {
      final o = withCredentials(CallOptions(), 'tok', id);
      expect(o.metadata, {
        'authorization': 'Bearer tok',
        'x-agent-user-id': 'acme',
        'x-agent-session-id': 'sid-1',
      });
    });

    test('negative_signed_out_adds_nothing', () {
      final given = CallOptions(metadata: {'k': 'v'});
      expect(identical(withCredentials(given, null, const {}), given), isTrue);
      expect(identical(withCredentials(given, '', null), given), isTrue);
    });

    test('corner_call_scoped_session_kept', () {
      // The Agent page names the session it opened; that one wins.
      final o = withCredentials(
          CallOptions(metadata: {'x-agent-session-id': 'registry-7'}), 'tok', id);
      expect(o.metadata['x-agent-session-id'], 'registry-7');
      expect(o.metadata['x-agent-user-id'], 'acme');
    });

    test('boundary_empty_identity_values_not_sent', () {
      final o = withCredentials(
          CallOptions(), 'tok', {'x-agent-user-id': '', 'x-agent-session-id': ''});
      expect(o.metadata.keys, ['authorization']);
    });

    test('adversarial_call_cannot_override_the_bearer', () {
      final o = withCredentials(
          CallOptions(metadata: {'authorization': 'Bearer forged'}), 'tok', id);
      expect(o.metadata['authorization'], 'Bearer tok');
    });
  });

  group('parseAuthMode', () {
    for (final (name, raw, want) in [
      ('positive_on', 'on', AuthMode.on),
      ('positive_off', 'off', AuthMode.off),
      ('corner_blank_is_auto', '', AuthMode.auto),
      ('corner_whitespace_trimmed', ' off ', AuthMode.off),
      ('negative_unknown_is_auto', 'yes', AuthMode.auto),
      ('negative_upper_case_not_folded', 'OFF', AuthMode.auto),
      ('positive_door', 'door', AuthMode.door),
      ('negative_door_prefix_is_auto', 'doors', AuthMode.auto),
    ]) {
      test(name, () => expect(parseAuthMode(raw), want));
    }
  });

  // S31: where a signed-out portal behind the front door sends the browser.
  group('doorLoginUrl', () {
    for (final (name, page, want) in [
      ('positive_root', 'https://h:8443/', '/auth/login?rd=%2F'),
      ('positive_deep_path', 'https://h:8443/app/x', '/auth/login?rd=%2Fapp%2Fx'),
      ('boundary_no_path', 'https://h:8443', '/auth/login?rd=%2F'),
      ('corner_query_and_fragment_dropped', 'https://h:8443/a?code=c1&state=s1#f',
          '/auth/login?rd=%2Fa'),
      ('adversarial_encoded_slashes_stay_encoded', 'https://h:8443/%2F%2Fevil.example',
          '/auth/login?rd=%2F%252F%252Fevil.example'),
      ('adversarial_ampersand_cannot_add_a_param', 'https://h:8443/a&rd=//evil',
          '/auth/login?rd=%2Fa%26rd%3D%2F%2Fevil'),
    ]) {
      test(name, () => expect(doorLoginUrl(Uri.parse(page)), want));
    }
  });
}
