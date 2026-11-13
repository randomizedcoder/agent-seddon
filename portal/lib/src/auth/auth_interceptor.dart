import 'dart:async';

import 'package:flutter/foundation.dart' show visibleForTesting;
import 'package:grpc/grpc_or_grpcweb.dart' show GrpcError, StatusCode;
import 'package:grpc/service_api.dart';

/// Adds the signed-in user's credentials to every call, unary and streaming, on
/// every channel (security-hardening S13b, S15c): `authorization: Bearer <agent
/// token>`, and the identity the agent scopes the call by (see
/// [AuthState.identityHeaders]). Both are read per call, so a refreshed token is
/// used from the next call on; signed out, or sign-in off, adds nothing. A
/// call, unary or streaming, that comes back unauthenticated is reported to
/// [onUnauthenticated] (S31: the front door's session ended); the caller still
/// sees the error.
class AuthInterceptor extends ClientInterceptor {
  AuthInterceptor(this.token, {this.identity, this.onUnauthenticated});

  final String? Function() token;

  /// `x-agent-user-id` / `x-agent-session-id` for a signed-in portal.
  final Map<String, String> Function()? identity;

  final void Function()? onUnauthenticated;

  @override
  ResponseFuture<R> interceptUnary<Q, R>(ClientMethod<Q, R> method, Q request,
      CallOptions options, ClientUnaryInvoker<Q, R> invoker) {
    final call =
        invoker(method, request, withCredentials(options, token(), identity?.call()));
    final hook = onUnauthenticated;
    if (hook != null) {
      // A side listener: it handles its own copy of the error, so it is never
      // reported as unhandled, and the caller's future is returned untouched.
      call.then<void>((_) {}, onError: (Object e) {
        if (e is GrpcError && e.code == StatusCode.unauthenticated) hook();
      });
    }
    return call;
  }

  @override
  ResponseStream<R> interceptStreaming<Q, R>(
          ClientMethod<Q, R> method,
          Stream<Q> requests,
          CallOptions options,
          ClientStreamingInvoker<Q, R> invoker) {
    final call =
        invoker(method, requests, withCredentials(options, token(), identity?.call()));
    final hook = onUnauthenticated;
    return hook == null ? call : ReportingResponseStream(call, hook);
  }
}

/// [inner] with every event passed through unchanged, and [onUnauthenticated]
/// called when an error is `unauthenticated`. A stream's error reaches only its
/// listener, so a side listener (as for unary calls) isn't possible on a
/// single-subscription stream. Cancelling the listener cancels [inner], so the
/// call is cancelled as before.
@visibleForTesting
class ReportingResponseStream<R> extends StreamView<R>
    implements ResponseStream<R> {
  ReportingResponseStream(this.inner, void Function() onUnauthenticated)
      : super(inner.transform(StreamTransformer.fromHandlers(
            handleError: (Object e, StackTrace st, EventSink<R> sink) {
          if (e is GrpcError && e.code == StatusCode.unauthenticated) {
            onUnauthenticated();
          }
          sink.addError(e, st);
        })));

  final ResponseStream<R> inner;

  @override
  ResponseFuture<R> get single => inner.single;

  @override
  Future<Map<String, String>> get headers => inner.headers;

  @override
  Future<Map<String, String>> get trailers => inner.trailers;

  @override
  Future<void> cancel() => inner.cancel();
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
