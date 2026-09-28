import 'dart:async';

import 'package:agent_portal/src/clients.dart';
import 'package:agent_portal/src/gen/agent/v1/agent_session.pb.dart';
import 'package:agent_portal/src/gen/agent/v1/prompt.pb.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import 'builders.dart';
import 'fake_gateway.dart';
import 'fakes/agent_session_service.dart';
import 'fakes/prompt_service.dart';

/// Self-test of the testkit's key feasibility claim: a page's `PortalClients`,
/// pointed at the in-process [FakeGateway], drives real RPCs over the loopback
/// wire — so a test can assert the exact RPC fired, with the exact encoded args,
/// and can inject transport faults. If this passes, the Layer-A widget tests in
/// inc 3+ rest on solid ground.
void main() {
  late FakeGateway gw;
  late FakePromptService prompts;

  setUp(() async {
    gw = await FakeGateway.start((log) {
      prompts = FakePromptService(log);
      return [prompts];
    });
  });
  tearDown(() async {
    await gw.shutdown();
  });

  test('positive_list_fires_correct_rpc_and_returns_scripted_response',
      () async {
    prompts.listResponse =
        promptList([promptEntry(id: 'a'), promptEntry(id: 'b')]);

    final clients = gw.clients();
    final resp = await clients.prompts.list(PromptListRequest());

    expect(resp.entries.map((e) => e.id), ['a', 'b']);
    // Exact recorded set — nothing else was dialed.
    expect(gw.log.methods, ['agent.v1.PromptService/List']);
    await clients.shutdown();
  });

  test('positive_request_arguments_are_recorded_at_the_wire', () async {
    final clients = gw.clients();
    await clients.prompts.get(PromptRef()
      ..id = 'xyz'
      ..kind = PromptKind.PROMPT_KIND_SYSTEM);

    final call = gw.log.last('agent.v1.PromptService/Get');
    expect(call, isNotNull);
    expect((call!.request as PromptRef).id, 'xyz');
    expect((call.request as PromptRef).kind, PromptKind.PROMPT_KIND_SYSTEM);
    await clients.shutdown();
  });

  test('adversarial_injected_transport_fault_surfaces_as_grpc_error', () async {
    prompts.error = const GrpcError.unavailable('backend down');
    final clients = gw.clients();

    await expectLater(
      clients.prompts.list(PromptListRequest()),
      throwsA(isA<GrpcError>()
          .having((e) => e.code, 'code', StatusCode.unavailable)),
    );
    // The fault is one-shot: the next call succeeds.
    final ok = await clients.prompts.list(PromptListRequest());
    expect(ok.entries, isEmpty);
    await clients.shutdown();
  });

  // Teardown with a call still in flight (the robots' `clients.terminate()`).
  // `shutdown()` waits for the call, so an open server stream wedges it — that
  // hung whole page files in the gate for their 10-minute timeout, cascading
  // "Reentrant call to runAsync" into the tests after. `terminate()` cancels it.
  group('teardown with a call in flight', () {
    late FakeGateway sgw;
    late FakeAgentSessionService session;

    setUp(() async {
      sgw = await FakeGateway.start((log) {
        session = FakeAgentSessionService(log);
        return [session];
      });
    });
    tearDown(() async {
      await session.disposeControllers();
      await sgw.shutdown();
    });

    /// Open a `Subscribe` and wait until the server holds its stream open.
    Future<Completer<Object?>> openStream(PortalClients clients) async {
      final ended = Completer<Object?>();
      clients.session.subscribe(SubscribeRequest()).listen((_) {},
          onError: (Object e) {
        if (!ended.isCompleted) ended.complete(e);
      }, onDone: () {
        if (!ended.isCompleted) ended.complete(null);
      });
      while (session.lastSubscribe?.hasListener != true) {
        await Future<void>.delayed(const Duration(milliseconds: 5));
      }
      return ended;
    }

    test('corner_shutdown_waits_on_an_open_stream', () async {
      final clients = sgw.clients();
      await openStream(clients);
      var closed = false;
      unawaited(clients.shutdown().then((_) => closed = true));
      await Future<void>.delayed(const Duration(milliseconds: 300));
      expect(closed, isFalse, reason: 'shutdown() waits for the open stream');
      await clients.terminate();
    });

    test('positive_terminate_cancels_an_open_stream', () async {
      final clients = sgw.clients();
      final ended = await openStream(clients);
      await clients.terminate().timeout(const Duration(seconds: 5));
      final e = await ended.future.timeout(const Duration(seconds: 5));
      expect(e, isA<GrpcError>());
    });

    test('boundary_terminate_with_nothing_in_flight', () async {
      final clients = sgw.clients();
      await clients.terminate().timeout(const Duration(seconds: 5));
    });
  });
}
