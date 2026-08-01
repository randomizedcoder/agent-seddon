/// Endpoints the portal talks to. Defaults match `nix/constants.nix` (the gateway)
/// and the observability stack's documented ports. All overridable so the app is
/// portable across hosts.
class PortalConfig {
  /// The `--serve-all` gRPC gateway (native builds dial this directly).
  final String gatewayHost;
  final int gatewayPort;

  /// The opt-in `--serve-sessions` gateway (docs/design/portal): hosts the
  /// `SessionRegistryService` + the *driving* `AgentSessionService` (the `Send` RPC).
  /// A separate process/endpoint from `--serve-all` because `Send` runs arbitrary
  /// goals; observe + drive share it, since a driven session's events live here.
  final String sessionsHost;
  final int sessionsPort;

  /// The grpc-web proxy the *web* build dials (envoy in front of the gateway).
  final String grpcWebUrl;

  /// The grpc-web proxy for the sessions gateway (a second envoy → `:50080`).
  /// Native builds ignore this and dial `sessionsHost:sessionsPort` directly.
  final String sessionsGrpcWebUrl;

  /// External observability UIs, opened in the system browser from the Launcher.
  final String grafanaUrl;
  final String hyperdxUrl;
  final String prometheusUrl;

  const PortalConfig({
    this.gatewayHost = '127.0.0.1',
    this.gatewayPort = 50100,
    this.sessionsHost = '127.0.0.1',
    this.sessionsPort = 50080,
    this.grpcWebUrl = 'http://localhost:8090',
    this.sessionsGrpcWebUrl = 'http://localhost:8091',
    this.grafanaUrl = 'http://localhost:3000',
    this.hyperdxUrl = 'http://localhost:8080',
    this.prometheusUrl = 'http://localhost:9090',
  });
}
