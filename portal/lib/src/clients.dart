import 'package:grpc/service_api.dart';

import 'config.dart';
import 'gen/agent/v1/agent_session.pbgrpc.dart';
import 'gen/agent/v1/llm_pool.pbgrpc.dart';
import 'gen/agent/v1/metrics_proxy.pbgrpc.dart';
import 'gen/agent/v1/prompt.pbgrpc.dart';
import 'gen/agent/v1/session_registry.pbgrpc.dart';
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
class PortalClients {
  final ClientChannel gatewayChannel;
  final ClientChannel sessionsChannel;

  late final PromptServiceClient prompts = PromptServiceClient(gatewayChannel);
  late final MetricsProxyServiceClient metrics =
      MetricsProxyServiceClient(gatewayChannel);
  late final LlmPoolServiceClient pool = LlmPoolServiceClient(gatewayChannel);

  late final AgentSessionServiceClient session =
      AgentSessionServiceClient(sessionsChannel);
  late final SessionRegistryServiceClient registry =
      SessionRegistryServiceClient(sessionsChannel);

  PortalClients(PortalConfig cfg)
      : gatewayChannel = createGatewayChannel(cfg),
        sessionsChannel = createSessionsChannel(cfg);

  Future<void> shutdown() async {
    await gatewayChannel.shutdown();
    await sessionsChannel.shutdown();
  }
}
