/// Endpoints the portal talks to. Defaults match `nix/constants.nix` (the gateway)
/// and the observability stack's documented ports. All overridable so the app is
/// portable across hosts.
class PortalConfig {
  /// The `--serve-all` gRPC gateway (native builds dial this directly).
  final String gatewayHost;
  final int gatewayPort;

  /// The grpc-web proxy the *web* build dials (envoy in front of the gateway).
  final String grpcWebUrl;

  /// External observability UIs, opened in the system browser from the Launcher.
  final String grafanaUrl;
  final String hyperdxUrl;
  final String prometheusUrl;

  const PortalConfig({
    this.gatewayHost = '127.0.0.1',
    this.gatewayPort = 50100,
    this.grpcWebUrl = 'http://localhost:8090',
    this.grafanaUrl = 'http://localhost:3000',
    this.hyperdxUrl = 'http://localhost:8080',
    this.prometheusUrl = 'http://localhost:9090',
  });
}
