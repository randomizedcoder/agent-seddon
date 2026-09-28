import 'package:grpc/service_api.dart';

/// Adds the signed-in user's credentials to every call, unary and streaming, on
/// every channel (security-hardening S13b, S15c): `authorization: Bearer <agent
/// token>`, and the identity the agent scopes the call by (see
/// [AuthState.identityHeaders]). Both are read per call, so a refreshed token is
/// used from the next call on; signed out, or sign-in off, adds nothing.
class AuthInterceptor extends ClientInterceptor {
  AuthInterceptor(this.token, {this.identity});

  final String? Function() token;

  /// `x-agent-user-id` / `x-agent-session-id` for a signed-in portal.
  final Map<String, String> Function()? identity;

  @override
  ResponseFuture<R> interceptUnary<Q, R>(ClientMethod<Q, R> method, Q request,
          CallOptions options, ClientUnaryInvoker<Q, R> invoker) =>
      invoker(method, request, withCredentials(options, token(), identity?.call()));

  @override
  ResponseStream<R> interceptStreaming<Q, R>(
          ClientMethod<Q, R> method,
          Stream<Q> requests,
          CallOptions options,
          ClientStreamingInvoker<Q, R> invoker) =>
      invoker(method, requests, withCredentials(options, token(), identity?.call()));
}

/// [options] plus the bearer and the identity headers. The bearer replaces any
/// the call set (the signed-in session is the credential). An identity header
/// the call set itself is kept: the Agent page scopes its calls to the session
/// it opened. Empty values are never sent.
CallOptions withCredentials(
    CallOptions options, String? token, Map<String, String>? identity) {
  final add = <String, String>{};
  if (token != null && token.isNotEmpty) add['authorization'] = 'Bearer $token';
  for (final e in (identity ?? const <String, String>{}).entries) {
    if (e.value.isNotEmpty && !options.metadata.containsKey(e.key)) {
      add[e.key] = e.value;
    }
  }
  return add.isEmpty ? options : options.mergedWith(CallOptions(metadata: add));
}
