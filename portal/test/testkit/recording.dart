import 'package:protobuf/protobuf.dart';

/// One server-side RPC the fakes served: its fully-qualified method name and the
/// decoded request proto. Fakes append to a shared [RecordingLog] so a test can
/// assert exactly which RPCs a page fired — and with what arguments — at the
/// wire. This is what proves both "the correct RPC fired with the right args"
/// and "no unintended/chatty RPCs" (the recorded set is exact).
class RecordedCall {
  RecordedCall(this.method, this.request);

  /// e.g. `agent.v1.PromptService/List`.
  final String method;

  /// The decoded request message; cast to its concrete type in assertions.
  final GeneratedMessage request;

  @override
  String toString() => '$method(${request.toProto3Json()})';
}

/// An ordered, shared record of every RPC the fakes handled. One per
/// [FakeGateway]; the Dart analogue of `agent-testkit`'s recording doubles.
class RecordingLog {
  final List<RecordedCall> calls = [];

  /// The request metadata of every call the gateway served, in order, as
  /// `(path, headers)` — what proves the portal sent (or withheld) a bearer.
  final List<RecordedHeaders> headers = [];

  void recordHeaders(String path, Map<String, String> metadata) =>
      headers.add(RecordedHeaders(path, Map.unmodifiable(metadata)));

  /// The `authorization` value of every call to [path], in order (null where the
  /// call carried none).
  List<String?> authorizationFor(String path) => [
        for (final h in headers)
          if (h.path == path) h.authorization,
      ];

  void record(String method, GeneratedMessage request) =>
      calls.add(RecordedCall(method, request));

  /// The ordered method names, e.g. `['agent.v1.PromptService/List']`.
  List<String> get methods => [for (final c in calls) c.method];

  /// Every recorded call to [method], in order.
  List<RecordedCall> forMethod(String method) =>
      [for (final c in calls) if (c.method == method) c];

  int countOf(String method) => forMethod(method).length;

  bool fired(String method) => countOf(method) > 0;

  /// The most recent call to [method], or null if it never fired.
  RecordedCall? last(String method) {
    final m = forMethod(method);
    return m.isEmpty ? null : m.last;
  }

  void clear() {
    calls.clear();
    headers.clear();
  }
}

/// One served call's request metadata (see [RecordingLog.headers]).
class RecordedHeaders {
  RecordedHeaders(this.path, this.metadata);

  /// e.g. `/agent.v1.ReviewFleetService/ListReviews`.
  final String path;
  final Map<String, String> metadata;

  String? get authorization => metadata['authorization'];

  @override
  String toString() => '$path authorization=${authorization ?? "-"}';
}
