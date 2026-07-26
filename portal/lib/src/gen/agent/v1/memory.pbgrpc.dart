// This is a generated file - do not edit.
//
// Generated from agent/v1/memory.proto.

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

import 'common.pb.dart' as $0;
import 'memory.pb.dart' as $1;

export 'memory.pb.dart';

/// The whole-store facade (agent-core `MemoryStore`).
@$pb.GrpcServiceName('agent.v1.Memory')
class MemoryClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  MemoryClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$1.RecallResponse> recall(
    $0.RecallQuery request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$recall, request, options: options);
  }

  $grpc.ResponseFuture<$1.AppendResponse> append(
    $0.MemoryEvent request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$append, request, options: options);
  }

  $grpc.ResponseFuture<$1.DistillResponse> distill(
    $1.DistillRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$distill, request, options: options);
  }

  // method descriptors

  static final _$recall = $grpc.ClientMethod<$0.RecallQuery, $1.RecallResponse>(
      '/agent.v1.Memory/Recall',
      ($0.RecallQuery value) => value.writeToBuffer(),
      $1.RecallResponse.fromBuffer);
  static final _$append = $grpc.ClientMethod<$0.MemoryEvent, $1.AppendResponse>(
      '/agent.v1.Memory/Append',
      ($0.MemoryEvent value) => value.writeToBuffer(),
      $1.AppendResponse.fromBuffer);
  static final _$distill =
      $grpc.ClientMethod<$1.DistillRequest, $1.DistillResponse>(
          '/agent.v1.Memory/Distill',
          ($1.DistillRequest value) => value.writeToBuffer(),
          $1.DistillResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.Memory')
abstract class MemoryServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.Memory';

  MemoryServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.RecallQuery, $1.RecallResponse>(
        'Recall',
        recall_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.RecallQuery.fromBuffer(value),
        ($1.RecallResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.MemoryEvent, $1.AppendResponse>(
        'Append',
        append_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.MemoryEvent.fromBuffer(value),
        ($1.AppendResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$1.DistillRequest, $1.DistillResponse>(
        'Distill',
        distill_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $1.DistillRequest.fromBuffer(value),
        ($1.DistillResponse value) => value.writeToBuffer()));
  }

  $async.Future<$1.RecallResponse> recall_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.RecallQuery> $request) async {
    return recall($call, await $request);
  }

  $async.Future<$1.RecallResponse> recall(
      $grpc.ServiceCall call, $0.RecallQuery request);

  $async.Future<$1.AppendResponse> append_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.MemoryEvent> $request) async {
    return append($call, await $request);
  }

  $async.Future<$1.AppendResponse> append(
      $grpc.ServiceCall call, $0.MemoryEvent request);

  $async.Future<$1.DistillResponse> distill_Pre($grpc.ServiceCall $call,
      $async.Future<$1.DistillRequest> $request) async {
    return distill($call, await $request);
  }

  $async.Future<$1.DistillResponse> distill(
      $grpc.ServiceCall call, $1.DistillRequest request);
}

/// The append-only "what happened" layer (agent-core `EpisodicStore`).
@$pb.GrpcServiceName('agent.v1.Episodic')
class EpisodicClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  EpisodicClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$1.AppendResponse> append(
    $0.MemoryEvent request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$append, request, options: options);
  }

  $grpc.ResponseFuture<$1.RecentResponse> recent(
    $1.RecentRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$recent, request, options: options);
  }

  // method descriptors

  static final _$append = $grpc.ClientMethod<$0.MemoryEvent, $1.AppendResponse>(
      '/agent.v1.Episodic/Append',
      ($0.MemoryEvent value) => value.writeToBuffer(),
      $1.AppendResponse.fromBuffer);
  static final _$recent =
      $grpc.ClientMethod<$1.RecentRequest, $1.RecentResponse>(
          '/agent.v1.Episodic/Recent',
          ($1.RecentRequest value) => value.writeToBuffer(),
          $1.RecentResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.Episodic')
abstract class EpisodicServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.Episodic';

  EpisodicServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.MemoryEvent, $1.AppendResponse>(
        'Append',
        append_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.MemoryEvent.fromBuffer(value),
        ($1.AppendResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$1.RecentRequest, $1.RecentResponse>(
        'Recent',
        recent_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $1.RecentRequest.fromBuffer(value),
        ($1.RecentResponse value) => value.writeToBuffer()));
  }

  $async.Future<$1.AppendResponse> append_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.MemoryEvent> $request) async {
    return append($call, await $request);
  }

  $async.Future<$1.AppendResponse> append(
      $grpc.ServiceCall call, $0.MemoryEvent request);

  $async.Future<$1.RecentResponse> recent_Pre(
      $grpc.ServiceCall $call, $async.Future<$1.RecentRequest> $request) async {
    return recent($call, await $request);
  }

  $async.Future<$1.RecentResponse> recent(
      $grpc.ServiceCall call, $1.RecentRequest request);
}

/// The "what is true" layer (agent-core `SemanticStore`).
@$pb.GrpcServiceName('agent.v1.Semantic')
class SemanticClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  SemanticClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$1.RecallResponse> recall(
    $0.RecallQuery request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$recall, request, options: options);
  }

  $grpc.ResponseFuture<$1.DistillResponse> distill(
    $1.SemanticDistillRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$distill, request, options: options);
  }

  // method descriptors

  static final _$recall = $grpc.ClientMethod<$0.RecallQuery, $1.RecallResponse>(
      '/agent.v1.Semantic/Recall',
      ($0.RecallQuery value) => value.writeToBuffer(),
      $1.RecallResponse.fromBuffer);
  static final _$distill =
      $grpc.ClientMethod<$1.SemanticDistillRequest, $1.DistillResponse>(
          '/agent.v1.Semantic/Distill',
          ($1.SemanticDistillRequest value) => value.writeToBuffer(),
          $1.DistillResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.Semantic')
abstract class SemanticServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.Semantic';

  SemanticServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.RecallQuery, $1.RecallResponse>(
        'Recall',
        recall_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.RecallQuery.fromBuffer(value),
        ($1.RecallResponse value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$1.SemanticDistillRequest, $1.DistillResponse>(
            'Distill',
            distill_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $1.SemanticDistillRequest.fromBuffer(value),
            ($1.DistillResponse value) => value.writeToBuffer()));
  }

  $async.Future<$1.RecallResponse> recall_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.RecallQuery> $request) async {
    return recall($call, await $request);
  }

  $async.Future<$1.RecallResponse> recall(
      $grpc.ServiceCall call, $0.RecallQuery request);

  $async.Future<$1.DistillResponse> distill_Pre($grpc.ServiceCall $call,
      $async.Future<$1.SemanticDistillRequest> $request) async {
    return distill($call, await $request);
  }

  $async.Future<$1.DistillResponse> distill(
      $grpc.ServiceCall call, $1.SemanticDistillRequest request);
}
