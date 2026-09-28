import 'dart:async';

import 'package:agent_portal/src/auth/auth_gate.dart';
import 'package:agent_portal/src/auth/auth_interceptor.dart';
import 'package:agent_portal/src/auth/auth_platform.dart';
import 'package:agent_portal/src/auth/auth_state.dart';
import 'package:agent_portal/src/pages/fleet_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../fake_gateway.dart';
import '../fakes/auth_service.dart';
import '../fakes/review_fleet_service.dart';
import '../recording.dart';
import 'robot.dart';

/// A refresh the [AuthState] asked for, captured instead of armed.
class ScheduledRefresh {
  ScheduledRefresh(this.delay, this.callback);

  final Duration delay;
  final void Function() callback;
  bool cancelled = false;
}

class _CapturedTimer implements Timer {
  _CapturedTimer(this._entry);

  final ScheduledRefresh _entry;

  @override
  void cancel() => _entry.cancelled = true;

  @override
  bool get isActive => !_entry.cancelled;

  @override
  int get tick => 0;
}

/// Robot for browser sign-in (security-hardening S13b). Hosts the real
/// [AuthGate] over a [FakeGateway] serving [FakeAuthService] and
/// [FakeReviewFleetService]; the signed-in "app" is the Fleet page, so a test
/// can see the shell appear and assert the bearer on its `ListReviews`. Every
/// client carries the portal's real [AuthInterceptor]. The clock is fixed at
/// [now] and refresh timers are captured in [scheduled], never armed.
class LoginRobot extends Robot {
  LoginRobot._(super.tester, this.gw, this.authFake, this.fleet, this.platform);

  static const now = 1000000;

  /// A PKCE verifier, stored as if `Begin` had run.
  static const verifier = 'dBjftJeZ4CVP-mJ92K9mSf3VVh8lK5xbWf0KX5gRRLQ';

  static const issuers = 'agent.v1.AuthService/Issuers';
  static const begin = 'agent.v1.AuthService/Begin';
  static const exchange = 'agent.v1.AuthService/Exchange';
  static const whoAmI = 'agent.v1.AuthService/WhoAmI';
  static const refresh = 'agent.v1.AuthService/Refresh';
  static const logout = 'agent.v1.AuthService/Logout';
  static const listReviews = 'agent.v1.ReviewFleetService/ListReviews';

  final FakeGateway gw;
  final FakeAuthService authFake;
  final FakeReviewFleetService fleet;
  final MemoryAuthPlatform platform;
  final List<ScheduledRefresh> scheduled = [];
  late final AuthState auth;
  late final clients =
      gw.clients(interceptors: [AuthInterceptor(() => auth.token)]);

  RecordingLog get log => gw.log;

  static Future<LoginRobot> create(
    WidgetTester tester, {
    String uri = 'http://127.0.0.1:8092/',
    bool canRedirect = true,
  }) async {
    late FakeAuthService authFake;
    late FakeReviewFleetService fleet;
    late FakeGateway gw;
    await tester.runAsync(() async {
      gw = await FakeGateway.start((log) {
        authFake = FakeAuthService(log);
        fleet = FakeReviewFleetService(log);
        return [authFake, fleet];
      });
    });
    final platform =
        MemoryAuthPlatform(uri: Uri.parse(uri), canRedirect: canRedirect);
    final robot = LoginRobot._(tester, gw, authFake, fleet, platform);
    addTearDown(() => tester.runAsync(() async {
          robot.auth.dispose();
          await robot.clients.shutdown();
          await gw.shutdown();
        }));
    return robot;
  }

  /// Build the [AuthState] in [mode], mount the gate and run `start()`.
  Future<void> load({AuthMode mode = AuthMode.on, String preferred = ''}) async {
    auth = AuthState(
      client: clients.auth,
      platform: platform,
      mode: mode,
      preferredIssuer: preferred,
      now: () => now,
      schedule: (delay, cb) {
        final entry = ScheduledRefresh(delay, cb);
        scheduled.add(entry);
        return _CapturedTimer(entry);
      },
    );
    await pumpPage(AuthGate(
      auth: auth,
      builder: (context, account) => Row(children: [
        if (account != null) SizedBox(width: 72, child: account),
        Expanded(child: FleetPage(clients: clients)),
      ]),
    ));
    await real(auth.start);
    await pumpUntil(() => auth.phase != AuthPhase.checking,
        reason: 'sign-in to settle');
    await pumpReal();
  }

  /// Run [body] (an [AuthState] call that talks to the fake) on the real event
  /// loop, then rebuild.
  Future<void> act(Future<void> Function() body) async {
    await real(body);
    await pumpReal();
  }

  /// Rebuild on the real event loop. Signing in mounts the shell, and the Fleet
  /// page's `initState` RPC only progresses if that build ran there (see
  /// [Robot.pumpPage]).
  Future<void> pumpReal() async {
    await real(() => tester.pump());
    await tester.pump();
  }

  /// Store a pending sign-in, as `Begin` would have, for a callback test.
  void storePending(String state) => platform.storage[AuthState.pendingKey] =
      '{"state":"$state","verifier":"$verifier","issuer":"google"}';

  bool get shellShown => find.byType(FleetPage).evaluate().isNotEmpty;

  String? get errorText {
    final f = byKey('login.error');
    if (f.evaluate().isEmpty) return null;
    return (tester.widget(f) as Text).data;
  }
}
