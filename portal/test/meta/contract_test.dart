import 'package:agent_portal/src/gen/agent/v1/graph.pbenum.dart';
import 'package:agent_portal/src/graph_json.dart' show graphEdgeKinds;
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../testkit/fakes/agent_session_service.dart';
import '../testkit/fakes/config_service.dart';
import '../testkit/fakes/graph_service.dart';
import '../testkit/fakes/llm_pool_service.dart';
import '../testkit/fakes/metrics_proxy_service.dart';
import '../testkit/fakes/prompt_service.dart';
import '../testkit/fakes/provider_registry_service.dart';
import '../testkit/fakes/review_fleet_service.dart';
import '../testkit/fakes/session_registry_service.dart';
import '../testkit/recording.dart';
import 'registry.dart';

/// The **contract drift guard** (design 03) — the portal twin of the #418 graph
/// parity test. It asserts the seam the portal is coded against still matches the
/// generated proto descriptors, so a proto change can't silently strand the UI:
///
///   * every RPC the spec tables say the portal invokes (`expectedRpc`) resolves
///     to a real method on its generated `*ServiceBase` (a renamed/removed proto
///     method fails here), and
///   * the proto enums the UI renders from a **hardcoded** list still cover the
///     generated enum's values (a new `GraphEdge_Kind` the UI forgot to offer
///     fails here). Enums the UI renders straight from `Enum.values`
///     (TaskMode/RouteRole/PoolTier in the Router) can't drift and need no guard.
///
/// It also records the *invoked* RPC set per service — the reference the
/// completeness critic + 02's "unwired stubs" note lean on.
void main() {
  // Every generated service the portal dials, keyed by its wire FQN. The fakes
  // extend the generated `*ServiceBase`, so `$lookupMethod` reads the real
  // descriptor's registered method names.
  final log = RecordingLog();
  final Map<String, Service> services = {
    for (final s in <Service>[
      FakePromptService(log),
      FakeGraphService(log),
      FakeProviderRegistryService(log),
      FakeReviewFleetService(log),
      FakeConfigService(log),
      FakeAgentSessionService(log),
      FakeSessionRegistryService(log),
      FakeLlmPoolService(log),
      FakeMetricsProxyService(log),
    ])
      s.$name: s,
  };

  test('contract: every spec expected_rpc resolves on the generated service', () {
    final invoked = <String, Set<String>>{};
    final problems = <String>[];
    for (final spec in allSpecs) {
      for (final row in spec.rows) {
        final rpc = row.expectedRpc;
        if (rpc == 'local') continue;
        final slash = rpc.indexOf('/');
        if (slash < 0) {
          problems.add('${spec.page}/${row.label}: malformed expected_rpc "$rpc"');
          continue;
        }
        final svcName = rpc.substring(0, slash);
        final method = rpc.substring(slash + 1);
        final svc = services[svcName];
        if (svc == null) {
          problems.add('${spec.page}/${row.label}: no generated service "$svcName"');
          continue;
        }
        if (svc.$lookupMethod(method) == null) {
          problems.add(
              '${spec.page}/${row.label}: "$svcName" has no method "$method" '
              '(proto renamed/removed it?)');
          continue;
        }
        invoked.putIfAbsent(svcName, () => <String>{}).add(method);
      }
    }
    // Record the invoked set (the "unwired stubs" reference) — visible, not silent.
    final summary = (invoked.keys.toList()..sort())
        .map((s) => '$s: ${(invoked[s]!.toList()..sort()).join(", ")}')
        .join('\n  ');
    // ignore: avoid_print
    print('contract — invoked RPC set:\n  $summary');
    expect(problems, isEmpty, reason: problems.join('\n'));
  });

  test('contract: hardcoded graphEdgeKinds covers GraphEdge_Kind (minus unspecified)',
      () {
    final generated = GraphEdge_Kind.values
        .where((k) => k != GraphEdge_Kind.KIND_UNSPECIFIED)
        .toSet();
    expect(
      graphEdgeKinds.toSet(),
      generated,
      reason: 'the Graph edge-kind picker drifted from the proto GraphEdge_Kind '
          '— a new kind must be offered (or an old one removed) in graph_json.dart',
    );
    // The UI must never offer the unspecified zero value.
    expect(graphEdgeKinds, isNot(contains(GraphEdge_Kind.KIND_UNSPECIFIED)));
  });

  test(r'contract: $lookupMethod discriminates (check-the-check)', () {
    // A guard that always resolved would be worthless: assert a real method
    // resolves and a bogus one does not.
    final prompts = services['agent.v1.PromptService']!;
    expect(prompts.$lookupMethod('List'), isNotNull);
    expect(prompts.$lookupMethod('NoSuchMethod'), isNull);
  });
}
