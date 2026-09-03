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

  // Every endpoint is overridable at build time via `--dart-define=<KEY>=<value>`
  // (threaded through `nix run .#portal` / `.#portal-web` from env vars or flags —
  // see nix/portal/default.nix). `fromEnvironment` bakes the value at compile time,
  // so the defaults below are what ships unless a define overrides them; they still
  // match nix/constants.nix (gateway) and the documented obs ports.
  const PortalConfig({
    this.gatewayHost = const String.fromEnvironment(
      'PORTAL_GATEWAY_HOST',
      defaultValue: '127.0.0.1',
    ),
    this.gatewayPort = const int.fromEnvironment(
      'PORTAL_GATEWAY_PORT',
      defaultValue: 50100,
    ),
    this.sessionsHost = const String.fromEnvironment(
      'PORTAL_SESSIONS_HOST',
      defaultValue: '127.0.0.1',
    ),
    this.sessionsPort = const int.fromEnvironment(
      'PORTAL_SESSIONS_PORT',
      defaultValue: 50080,
    ),
    this.grpcWebUrl = const String.fromEnvironment(
      'PORTAL_GRPC_WEB_URL',
      defaultValue: 'http://localhost:8090',
    ),
    this.sessionsGrpcWebUrl = const String.fromEnvironment(
      'PORTAL_SESSIONS_GRPC_WEB_URL',
      defaultValue: 'http://localhost:8091',
    ),
    this.grafanaUrl = const String.fromEnvironment(
      'PORTAL_GRAFANA_URL',
      defaultValue: 'http://localhost:3000',
    ),
    this.hyperdxUrl = const String.fromEnvironment(
      'PORTAL_HYPERDX_URL',
      defaultValue: 'http://localhost:8080',
    ),
    this.prometheusUrl = const String.fromEnvironment(
      'PORTAL_PROMETHEUS_URL',
      defaultValue: 'http://localhost:9090',
    ),
  });
}
