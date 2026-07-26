// This is a generated file - do not edit.
//
// Generated from agent/v1/agent_session.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:async' as $async;
import 'dart:core' as $core;

import 'package:grpc/service_api.dart' as $grpc;
import 'package:protobuf/protobuf.dart' as $pb;

import 'agent_session.pb.dart' as $0;

export 'agent_session.pb.dart';

@$pb.GrpcServiceName('agent.v1.AgentSessionService')
class AgentSessionServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  AgentSessionServiceClient(super.channel, {super.options, super.interceptors});

  /// Live event stream. The first item is a `status_snapshot`, then the tail.
  $grpc.ResponseStream<$0.SessionEvent> subscribe(
    $0.SubscribeRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createStreamingCall(
        _$subscribe, $async.Stream.fromIterable([request]),
        options: options);
  }

  /// The current session state, one-shot.
  $grpc.ResponseFuture<$0.StatusSnapshot> snapshot(
    $0.SnapshotRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$snapshot, request, options: options);
  }

  // method descriptors

  static final _$subscribe =
      $grpc.ClientMethod<$0.SubscribeRequest, $0.SessionEvent>(
          '/agent.v1.AgentSessionService/Subscribe',
          ($0.SubscribeRequest value) => value.writeToBuffer(),
          $0.SessionEvent.fromBuffer);
  static final _$snapshot =
      $grpc.ClientMethod<$0.SnapshotRequest, $0.StatusSnapshot>(
          '/agent.v1.AgentSessionService/Snapshot',
          ($0.SnapshotRequest value) => value.writeToBuffer(),
          $0.StatusSnapshot.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.AgentSessionService')
abstract class AgentSessionServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.AgentSessionService';

  AgentSessionServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.SubscribeRequest, $0.SessionEvent>(
        'Subscribe',
        subscribe_Pre,
        false,
        true,
        ($core.List<$core.int> value) => $0.SubscribeRequest.fromBuffer(value),
        ($0.SessionEvent value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.SnapshotRequest, $0.StatusSnapshot>(
        'Snapshot',
        snapshot_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.SnapshotRequest.fromBuffer(value),
        ($0.StatusSnapshot value) => value.writeToBuffer()));
  }

  $async.Stream<$0.SessionEvent> subscribe_Pre($grpc.ServiceCall $call,
      $async.Future<$0.SubscribeRequest> $request) async* {
    yield* subscribe($call, await $request);
  }

  $async.Stream<$0.SessionEvent> subscribe(
      $grpc.ServiceCall call, $0.SubscribeRequest request);

  $async.Future<$0.StatusSnapshot> snapshot_Pre($grpc.ServiceCall $call,
      $async.Future<$0.SnapshotRequest> $request) async {
    return snapshot($call, await $request);
  }

  $async.Future<$0.StatusSnapshot> snapshot(
      $grpc.ServiceCall call, $0.SnapshotRequest request);
}
