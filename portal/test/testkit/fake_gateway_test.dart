import 'package:agent_portal/src/gen/agent/v1/prompt.pb.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import 'builders.dart';
import 'fake_gateway.dart';

/// Self-test of the testkit's key feasibility claim: a page's `PortalClients`,
/// pointed at the in-process [FakeGateway], drives real RPCs over the loopback
/// wire — so a test can assert the exact RPC fired, with the exact encoded args,
/// and can inject transport faults. If this passes, the Layer-A widget tests in
/// inc 3+ rest on solid ground.
void main() {
  late FakeGateway gw;

  setUp(() async {
    gw = await FakeGateway.start();
  });
  tearDown(() async {
    await gw.shutdown();
  });

  test('positive_list_fires_correct_rpc_and_returns_scripted_response',
      () async {
    gw.prompts.listResponse =
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
    gw.prompts.error = const GrpcError.unavailable('backend down');
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
}
