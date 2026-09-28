import 'package:grpc/service_api.dart';

/// Adds `authorization: Bearer <agent token>` to every call, unary and
/// streaming, on every channel (security-hardening S13b). [token] is read per
/// call, so a refreshed token is used from the next call on; `null` (signed out,
/// or sign-in off) sends no header.
class AuthInterceptor extends ClientInterceptor {
  AuthInterceptor(this.token);

  final String? Function() token;

  CallOptions _withBearer(CallOptions options) {
    final t = token();
    if (t == null || t.isEmpty) return options;
    return options.mergedWith(
      CallOptions(metadata: {'authorization': 'Bearer $t'}),
    );
  }

  @override
  ResponseFuture<R> interceptUnary<Q, R>(ClientMethod<Q, R> method, Q request,
          CallOptions options, ClientUnaryInvoker<Q, R> invoker) =>
      invoker(method, request, _withBearer(options));

  @override
  ResponseStream<R> interceptStreaming<Q, R>(
          ClientMethod<Q, R> method,
          Stream<Q> requests,
          CallOptions options,
          ClientStreamingInvoker<Q, R> invoker) =>
      invoker(method, requests, _withBearer(options));
}
