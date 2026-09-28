import 'dart:convert';

import 'package:agent_portal/src/auth/auth_state.dart';
import 'package:agent_portal/src/auth/pkce.dart';
import 'package:agent_portal/src/gen/agent/v1/auth.pb.dart';
import 'package:agent_portal/src/gen/agent/v1/review_fleet.pb.dart';
import 'package:fixnum/fixnum.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../testkit/robots/login_robot.dart';
import 'login_spec.dart';

/// Layer-A tests for browser sign-in (security-hardening S13b) — iterates
/// [loginSpec]; each row is one test, driven through the real [AuthGate],
/// [AuthState] and `AuthInterceptor` against the fake gateway.
void main() {
  const now = LoginRobot.now;
  const callback = 'http://127.0.0.1:8092/?code=c1&state=s1';

  IssuersResponse issuers(List<String> names) => IssuersResponse(issuers: [
        for (final n in names) LoginIssuer(name: n, profile: n),
      ]);

  WhoAmIResponse principal() => WhoAmIResponse(
        tenant: 'example.com',
        subject: 'user:google/1',
        email: 'ada@example.com',
        permissions: ['read:review', 'read:fleet'],
        sid: 'sid-1',
      );

  ExchangeResponse signedIn(String token, {int ttl = 900}) => ExchangeResponse(
        accessToken: token,
        tokenType: 'Bearer',
        expiresAt: Int64(now + ttl),
        refreshHandle: 'h-$token',
        principal: principal(),
      );

  /// Sign in through a scripted callback; the shell is up afterwards.
  Future<LoginRobot> signIn(WidgetTester tester, {int ttl = 900}) async {
    final robot = await LoginRobot.create(tester, uri: callback);
    robot.authFake.issuersResponse = issuers(['google']);
    robot.authFake.exchangeResponse = signedIn('tok-1', ttl: ttl);
    robot.storePending('s1');
    await robot.load();
    await robot.pumpUntilFound('login.account');
    await robot.pumpUntil(() => robot.log.fired(LoginRobot.listReviews),
        reason: 'the shell to call ListReviews');
    return robot;
  }

  for (final row in loginSpec.rows) {
    testWidgets('login ${row.label} — ${row.description}', (tester) async {
      switch (row.label) {
        case 'positive_issuer_button_begins_sign_in':
        case 'positive_progress_while_redirecting':
          final robot = await LoginRobot.create(tester);
          robot.authFake.issuersResponse = issuers(['google']);
          robot.authFake.beginResponse = BeginResponse(
              authorizeUrl: 'https://idp.test/authorize?x=1', state: 'st-1');
          await robot.load();
          expect(robot.exists('login.issuer.google'), isTrue);
          await robot.tap('login.issuer.google',
              until: () => robot.platform.navigated.isNotEmpty);
          final begin = robot.log.last(LoginRobot.begin)!.request as BeginRequest;
          expect(begin.issuer, 'google');
          expect(begin.redirectUri, 'http://127.0.0.1:8092/');
          final pending = jsonDecode(
              robot.platform.storage[AuthState.pendingKey]!) as Map<String, dynamic>;
          expect(pending['state'], 'st-1');
          expect(begin.codeChallenge, challengeOf(pending['verifier'] as String));
          expect(begin.codeChallenge.length, 43);
          expect(robot.platform.navigated, ['https://idp.test/authorize?x=1']);
          expect(robot.exists('login.progress'), isTrue);
          expect(robot.exists('login.issuer.google'), isFalse);
          break;

        case 'positive_round_trip_sets_bearer':
          final robot = await signIn(tester);
          final ex = robot.log.last(LoginRobot.exchange)!.request as ExchangeRequest;
          expect(ex.code, 'c1');
          expect(ex.state, 's1');
          expect(ex.codeVerifier, LoginRobot.verifier);
          expect(ex.clientKind, 'portal');
          expect(ex.idToken, isEmpty);
          // The sign-in RPCs carry no bearer; the app's calls carry the token.
          expect(robot.log.authorizationFor('/${LoginRobot.exchange}'), [null]);
          expect(robot.log.authorizationFor('/${LoginRobot.listReviews}'),
              everyElement('Bearer tok-1'));
          // ...and name the verified tenant and the auth session, which a scoped
          // service needs once a token is present (found by the S15c browser run).
          final app = robot.log.headers
              .where((h) => h.path == '/${LoginRobot.listReviews}')
              .toList();
          expect(app, isNotEmpty);
          for (final h in app) {
            expect(h.metadata['x-agent-user-id'], 'example.com');
            expect(h.metadata['x-agent-session-id'], 'sid-1');
          }
          expect(robot.log.headers
              .where((h) => h.path == '/${LoginRobot.exchange}')
              .map((h) => h.metadata['x-agent-session-id']), [null]);
          // The code and state are gone from the address bar; the pending
          // sign-in is spent; the session is kept for a reload.
          expect(robot.platform.currentUri.hasQuery, isFalse);
          expect(robot.platform.storage.containsKey(AuthState.pendingKey), isFalse);
          expect(robot.platform.storage[AuthState.sessionKey], contains('tok-1'));
          expect(robot.shellShown, isTrue);
          expect(robot.auth.tenant, 'example.com');
          break;

        case 'positive_idp_error_is_shown':
          final robot = await LoginRobot.create(tester,
              uri: 'http://127.0.0.1:8092/?error=access_denied&state=s1');
          robot.authFake.issuersResponse = issuers(['google']);
          robot.storePending('s1');
          await robot.load();
          expect(robot.errorText, contains('access_denied'));
          expect(robot.log.fired(LoginRobot.exchange), isFalse);
          expect(robot.exists('login.issuer.google'), isTrue);
          break;

        case 'positive_retry_relists_issuers':
          final robot = await LoginRobot.create(tester);
          robot.authFake.issuersError = const GrpcError.unavailable('down');
          await robot.load();
          expect(robot.errorText, contains('UNAVAILABLE'));
          expect(robot.exists('login.retry'), isTrue);
          robot.authFake.issuersError = null;
          robot.authFake.issuersResponse = issuers(['google']);
          await robot.tap('login.retry',
              until: () => robot.exists('login.issuer.google'));
          expect(robot.log.countOf(LoginRobot.issuers), 2);
          break;

        case 'positive_expired_after_refused_refresh':
          final robot = await signIn(tester);
          robot.authFake.refreshError = const GrpcError.unauthenticated('revoked');
          await robot.act(robot.auth.refresh);
          await robot.pumpUntilFound('login.expired');
          expect(robot.errorText, contains('Sign in again'));
          expect(robot.shellShown, isFalse);
          expect(robot.platform.storage.containsKey(AuthState.sessionKey), isFalse);
          break;

        case 'positive_signout_revokes_and_returns_to_login':
          final robot = await signIn(tester);
          await robot.tap('login.signout',
              until: () => robot.exists('login.issuer.google'));
          expect(robot.log.authorizationFor('/${LoginRobot.logout}'),
              ['Bearer tok-1']);
          expect(robot.auth.token, isNull);
          expect(robot.platform.storage.containsKey(AuthState.sessionKey), isFalse);
          expect(robot.exists('login.expired'), isFalse);
          expect(robot.scheduled.last.cancelled, isTrue);
          break;

        case 'positive_refresh_swaps_the_bearer':
          final robot = await signIn(tester);
          robot.authFake.refreshResponse = signedIn('tok-2');
          await robot.act(robot.auth.refresh);
          final r = robot.log.last(LoginRobot.refresh)!.request as RefreshRequest;
          expect(r.refreshHandle, 'h-tok-1');
          expect(robot.auth.token, 'tok-2');
          expect(robot.scheduled.first.cancelled, isTrue);
          expect(robot.scheduled.last.cancelled, isFalse);
          await robot.act(() => robot.clients.fleet.listReviews(ListReviewsRequest()));
          expect(robot.log.authorizationFor('/${LoginRobot.listReviews}').last,
              'Bearer tok-2');
          break;

        case 'negative_unsigned_in_shell_hidden':
          final robot = await LoginRobot.create(tester);
          robot.authFake.issuersResponse = issuers(['google']);
          await robot.load();
          expect(robot.exists('login.issuer.google'), isTrue);
          expect(robot.shellShown, isFalse);
          expect(robot.exists('login.account'), isFalse);
          expect(robot.log.methods, [LoginRobot.issuers]);
          break;

        case 'negative_exchange_refused_shows_error':
          final robot = await LoginRobot.create(tester, uri: callback);
          robot.authFake.issuersResponse = issuers(['google']);
          robot.authFake.exchangeError =
              const GrpcError.unauthenticated('pkce_mismatch');
          robot.storePending('s1');
          await robot.load();
          expect(robot.errorText, contains('UNAUTHENTICATED'));
          expect(robot.auth.token, isNull);
          expect(robot.shellShown, isFalse);
          expect(robot.platform.storage.containsKey(AuthState.sessionKey), isFalse);
          break;

        case 'negative_native_cannot_redirect':
          final robot = await LoginRobot.create(tester, canRedirect: false);
          robot.authFake.issuersResponse = issuers(['google']);
          await robot.load();
          await robot.tap('login.issuer.google',
              until: () => robot.errorText != null);
          expect(robot.errorText, contains('web portal'));
          expect(robot.log.fired(LoginRobot.begin), isFalse);
          expect(robot.platform.navigated, isEmpty);
          break;

        case 'negative_on_mode_with_no_issuers_explains':
          final robot = await LoginRobot.create(tester);
          await robot.load();
          expect(robot.errorText, contains('redirect_uris'));
          expect(robot.exists('login.retry'), isTrue);
          expect(robot.shellShown, isFalse);
          break;

        case 'corner_auth_off_runs_anonymously':
          final robot = await LoginRobot.create(tester);
          await robot.load(mode: AuthMode.off);
          await robot.pumpUntil(() => robot.log.fired(LoginRobot.listReviews));
          expect(robot.shellShown, isTrue);
          expect(robot.exists('login.account'), isFalse);
          expect(robot.log.fired(LoginRobot.issuers), isFalse);
          expect(robot.log.authorizationFor('/${LoginRobot.listReviews}'),
              everyElement(isNull));
          break;

        case 'corner_auto_mode_without_issuers_runs_anonymously':
          final robot = await LoginRobot.create(tester);
          robot.authFake.issuersError = const GrpcError.unimplemented('old agent');
          await robot.load(mode: AuthMode.auto);
          expect(robot.auth.phase, AuthPhase.off);
          expect(robot.shellShown, isTrue);
          expect(robot.exists('login.account'), isFalse);
          break;

        case 'corner_stored_session_resumes':
          final robot = await LoginRobot.create(tester);
          robot.authFake.whoAmIResponse = principal();
          robot.platform.storage[AuthState.sessionKey] = jsonEncode({
            'access_token': 'tok-s',
            'expires_at': now + 600,
            'refresh_handle': 'h-s',
          });
          await robot.load();
          await robot.pumpUntilFound('login.account');
          expect(robot.log.authorizationFor('/${LoginRobot.whoAmI}'),
              ['Bearer tok-s']);
          expect(robot.log.fired(LoginRobot.issuers), isFalse);
          expect(robot.log.fired(LoginRobot.refresh), isFalse);
          expect(robot.auth.token, 'tok-s');
          break;

        case 'corner_preferred_issuer_narrows_buttons':
          final robot = await LoginRobot.create(tester);
          robot.authFake.issuersResponse = issuers(['google', 'entra']);
          await robot.load(preferred: 'entra');
          expect(robot.exists('login.issuer.entra'), isTrue);
          expect(robot.exists('login.issuer.google'), isFalse);
          break;

        case 'boundary_refresh_one_minute_before_expiry':
          final robot = await signIn(tester);
          expect(robot.scheduled.single.delay, const Duration(seconds: 840));
          // Less than a minute left: refresh at once, never a negative delay.
          robot.authFake.refreshResponse = signedIn('tok-2', ttl: 30);
          await robot.act(robot.auth.refresh);
          expect(robot.scheduled.last.delay, Duration.zero);
          expect(refreshDelaySecs(now + 60, now), 0);
          expect(refreshDelaySecs(now + 61, now), 1);
          expect(refreshDelaySecs(now - 5, now), 0);
          break;

        case 'adversarial_callback_state_mismatch_rejected':
          final robot = await LoginRobot.create(tester,
              uri: 'http://127.0.0.1:8092/?code=c1&state=forged');
          robot.authFake.issuersResponse = issuers(['google']);
          robot.storePending('s1');
          await robot.load();
          expect(robot.log.fired(LoginRobot.exchange), isFalse);
          expect(robot.errorText, contains('did not start in this tab'));
          expect(robot.platform.storage.containsKey(AuthState.pendingKey), isFalse);
          expect(robot.platform.currentUri.hasQuery, isFalse);
          break;

        case 'adversarial_callback_without_pending_rejected':
          final robot = await LoginRobot.create(tester, uri: callback);
          robot.authFake.issuersResponse = issuers(['google']);
          await robot.load();
          expect(robot.log.fired(LoginRobot.exchange), isFalse);
          expect(robot.errorText, contains('did not start in this tab'));
          expect(robot.shellShown, isFalse);
          break;

        case 'adversarial_idp_error_text_not_echoed':
          final robot = await LoginRobot.create(tester,
              uri: 'http://127.0.0.1:8092/?error=%3Cscript%3Ealert(1)%3C/script%3E');
          robot.authFake.issuersResponse = issuers(['google']);
          await robot.load();
          expect(robot.errorText, contains('(unknown)'));
          expect(robot.errorText, isNot(contains('script')));
          expect(robot.log.fired(LoginRobot.exchange), isFalse);
          break;

        default:
          fail('no test body for row ${row.label}');
      }
    });
  }
}
