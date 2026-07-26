import 'package:grpc/service_api.dart';

import 'config.dart';
import 'gen/agent/v1/agent_session.pbgrpc.dart';
import 'gen/agent/v1/llm_pool.pbgrpc.dart';
import 'gen/agent/v1/metrics_proxy.pbgrpc.dart';
import 'gen/agent/v1/prompt.pbgrpc.dart';
import 'transport/channel_factory.dart';

/// One channel to the gateway (`:50100` native, or the grpc-web proxy on web),
/// and the service clients built from it. Add a seam to the gateway → it is
/// reachable here with no transport work.
class PortalClients {
  final ClientChannel channel;
  late final PromptServiceClient prompts = PromptServiceClient(channel);
  late final MetricsProxyServiceClient metrics =
      MetricsProxyServiceClient(channel);
  late final AgentSessionServiceClient session =
      AgentSessionServiceClient(channel);
  late final LlmPoolServiceClient pool = LlmPoolServiceClient(channel);

  PortalClients(PortalConfig cfg) : channel = createGatewayChannel(cfg);

  Future<void> shutdown() => channel.shutdown();
}
