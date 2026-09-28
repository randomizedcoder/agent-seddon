import 'package:grpc/service_api.dart';

import 'config.dart';
import 'gen/agent/v1/agent_session.pbgrpc.dart';
import 'gen/agent/v1/auth.pbgrpc.dart';
import 'gen/agent/v1/config.pbgrpc.dart';
import 'gen/agent/v1/graph.pbgrpc.dart';
import 'gen/agent/v1/llm_pool.pbgrpc.dart';
import 'gen/agent/v1/metrics_proxy.pbgrpc.dart';
import 'gen/agent/v1/prompt.pbgrpc.dart';
import 'gen/agent/v1/review_fleet.pbgrpc.dart';
import 'gen/agent/v1/session_registry.pbgrpc.dart';
import 'gen/agent/v1/upstream.pbgrpc.dart';
import 'transport/channel_factory.dart';

/// The portal's gRPC clients, over **two** channels:
///
/// - the `--serve-all` **gateway** (`:50100`) for the read-only seams the portal
///   consumes — prompts, metrics, and the GPU pool; and
/// - the opt-in `--serve-sessions` **sessions gateway** (`:50080`) for everything
///   session-related — the driving [AgentSessionServiceClient] (`Send` + observe) and
///   the [SessionRegistryServiceClient] (mint a session to attribute a `Send` to).
///
/// Observe (`Subscribe`) and drive (`Send`) share the sessions channel because a
/// driven session's events live in that process (docs/design/portal).
///
/// The **Fleet** tab dials a third channel — the full `--serve-fleet` process
/// (`:50086`) — because roster writes (`SetEnabled`/`ReviewNow`) and the review
/// draft read/edit/approve RPCs need the orchestrator + approver + history that
/// only that process wires (a bare gateway answers `UNIMPLEMENTED`).
///
/// Every client carries [interceptors] — the portal's `AuthInterceptor`, which
/// adds the signed-in user's agent token to each call (security-hardening S13b).
/// One agent token is valid at every seam, so all three channels share it.
class PortalClients {
  final ClientChannel gatewayChannel;
  final ClientChannel sessionsChannel;
  final ClientChannel fleetChannel;
  final List<ClientInterceptor> interceptors;

  // Sign-in (`Issuers` / `Begin` / `Exchange` / `Refresh` / `WhoAmI` / `Logout`).
  late final AuthServiceClient auth =
      AuthServiceClient(gatewayChannel, interceptors: interceptors);

  late final PromptServiceClient prompts = PromptServiceClient(gatewayChannel, interceptors: interceptors);
  late final MetricsProxyServiceClient metrics =
      MetricsProxyServiceClient(gatewayChannel, interceptors: interceptors);
  late final LlmPoolServiceClient pool = LlmPoolServiceClient(gatewayChannel, interceptors: interceptors);
  late final GraphServiceClient graph = GraphServiceClient(gatewayChannel, interceptors: interceptors);
  // The model-router / provider registry (live, no-restart control plane).
  late final ProviderRegistryServiceClient providers =
      ProviderRegistryServiceClient(gatewayChannel, interceptors: interceptors);
  // The whole-config seam (schema-driven Settings; write-TOML, restart-to-apply).
  late final ConfigServiceClient config = ConfigServiceClient(gatewayChannel, interceptors: interceptors);

  late final AgentSessionServiceClient session =
      AgentSessionServiceClient(sessionsChannel, interceptors: interceptors);
  late final SessionRegistryServiceClient registry =
      SessionRegistryServiceClient(sessionsChannel, interceptors: interceptors);

  // The full review-fleet process (roster + review drafts + approve).
  late final ReviewFleetServiceClient fleet =
      ReviewFleetServiceClient(fleetChannel, interceptors: interceptors);

  PortalClients(PortalConfig cfg, {this.interceptors = const []})
      : gatewayChannel = createGatewayChannel(cfg),
        sessionsChannel = createSessionsChannel(cfg),
        fleetChannel = createFleetChannel(cfg);

  Future<void> shutdown() async {
    await gatewayChannel.shutdown();
    await sessionsChannel.shutdown();
    await fleetChannel.shutdown();
  }
}
