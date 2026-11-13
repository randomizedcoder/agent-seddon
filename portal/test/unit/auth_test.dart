import 'dart:async';
import 'dart:math';

import 'package:agent_portal/src/auth/auth_interceptor.dart';
import 'package:agent_portal/src/auth/auth_state.dart';
import 'package:agent_portal/src/auth/capabilities.dart';
import 'package:agent_portal/src/auth/pkce.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc_or_grpcweb.dart' show GrpcError;
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

  // S31 carry (#722's draft): a stream that ends unauthenticated reports the
  // lost session like a unary call does, whichever page opened it.
  group('AuthInterceptor streams', () {
    final method = ClientMethod<String, String>('/t.S/M', (s) => s.codeUnits,
        (b) => String.fromCharCodes(b));
    // (name, events then the end: an error, or null for a clean close, reports)
    for (final (name, events, Object? end, reports) in [
      ('positive_unauthenticated_reported', ['a'], const GrpcError.unauthenticated('x'), 1),
      ('negative_unavailable_not_reported', ['a'], const GrpcError.unavailable('x'), 0),
      ('negative_clean_end_not_reported', ['a', 'b'], null, 0),
      ('corner_custom_code_16_reported', <String>[], GrpcError.custom(16, 'x'), 1),
      ('adversarial_plain_error_named_unauthenticated', ['a'],
          Exception('unauthenticated'), 0),
      ('adversarial_permission_denied_not_reported', ['a'],
          const GrpcError.permissionDenied('x'), 0),
      ('boundary_after_many_events', List.generate(1000, (i) => '$i'),
          const GrpcError.unauthenticated('x'), 1),
    ]) {
      test(name, () async {
        var lost = 0;
        final inner = _FakeStream<String>(events, end);
        final out = AuthInterceptor(() => 'tok', onUnauthenticated: () => lost++)
            .interceptStreaming(method, const Stream.empty(), CallOptions(),
                (m, r, options) => inner);
        final got = <String>[];
        Object? err;
        final done = Completer<void>();
        out.listen(got.add, onError: (Object e) => err = e, onDone: done.complete);
        await done.future;
        expect(got, events, reason: 'every event passes through');
        expect(err, end, reason: 'the caller still sees the error');
        expect(lost, reports);
      });
    }

    test('corner_no_hook_returns_the_call_untouched', () {
      final inner = _FakeStream<String>(const [], null);
      final out = AuthInterceptor(() => 'tok').interceptStreaming(
          method, const Stream.empty(), CallOptions(), (m, r, options) => inner);
      expect(identical(out, inner), isTrue);
    });

    test('positive_cancel_reaches_the_call', () async {
      final inner = _FakeStream<String>(const ['a'], null, hold: true);
      final out = AuthInterceptor(() => 'tok', onUnauthenticated: () {})
          .interceptStreaming(method, const Stream.empty(), CallOptions(),
              (m, r, options) => inner);
      final sub = out.listen((_) {});
      await pumpEventQueue();
      await sub.cancel();
      expect(inner.cancelled, isTrue);
    });

    test('positive_stream_carries_the_bearer', () {
      CallOptions? seen;
      AuthInterceptor(() => 'tok', onUnauthenticated: () {}).interceptStreaming(
          method, const Stream.empty(), CallOptions(), (m, r, options) {
        seen = options;
        return _FakeStream<String>(const [], null);
      });
      expect(seen?.metadata['authorization'], 'Bearer tok');
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

/// A server stream: [events], then [end] as an error (or a clean close), unless
/// [hold] keeps it open. Records whether its listener cancelled it.
class _FakeStream<R> extends StreamView<R> implements ResponseStream<R> {
  _FakeStream(List<R> events, Object? end, {bool hold = false})
      : this._(StreamController<R>(), events, end, hold);

  _FakeStream._(this._c, List<R> events, Object? end, bool hold)
      : super(_c.stream) {
    _c.onCancel = () => cancelled = true;
    events.forEach(_c.add);
    if (hold) return;
    if (end != null) _c.addError(end);
    _c.close();
  }

  final StreamController<R> _c;
  bool cancelled = false;

  @override
  ResponseFuture<R> get single => throw UnimplementedError();

  @override
  Future<Map<String, String>> get headers async => const {};

  @override
  Future<Map<String, String>> get trailers async => const {};

  @override
  Future<void> cancel() async => cancelled = true;
}
