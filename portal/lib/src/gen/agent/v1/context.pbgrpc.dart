// This is a generated file - do not edit.
//
// Generated from agent/v1/context.proto.

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
import 'context.pb.dart' as $1;

export 'context.pb.dart';

/// Named `ContextService` (not `Context`) to avoid colliding with
/// `std::task::Context` in tonic's generated Tower `Service` impls.
@$pb.GrpcServiceName('agent.v1.ContextService')
class ContextServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  ContextServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$1.AssembleResponse> assemble(
    $0.ContextInput request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$assemble, request, options: options);
  }

  $grpc.ResponseFuture<$1.CompactResponse> compact(
    $1.CompactRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$compact, request, options: options);
  }

  // method descriptors

  static final _$assemble =
      $grpc.ClientMethod<$0.ContextInput, $1.AssembleResponse>(
          '/agent.v1.ContextService/Assemble',
          ($0.ContextInput value) => value.writeToBuffer(),
          $1.AssembleResponse.fromBuffer);
  static final _$compact =
      $grpc.ClientMethod<$1.CompactRequest, $1.CompactResponse>(
          '/agent.v1.ContextService/Compact',
          ($1.CompactRequest value) => value.writeToBuffer(),
          $1.CompactResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.ContextService')
abstract class ContextServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.ContextService';

  ContextServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.ContextInput, $1.AssembleResponse>(
        'Assemble',
        assemble_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ContextInput.fromBuffer(value),
        ($1.AssembleResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$1.CompactRequest, $1.CompactResponse>(
        'Compact',
        compact_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $1.CompactRequest.fromBuffer(value),
        ($1.CompactResponse value) => value.writeToBuffer()));
  }

  $async.Future<$1.AssembleResponse> assemble_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.ContextInput> $request) async {
    return assemble($call, await $request);
  }

  $async.Future<$1.AssembleResponse> assemble(
      $grpc.ServiceCall call, $0.ContextInput request);

  $async.Future<$1.CompactResponse> compact_Pre($grpc.ServiceCall $call,
      $async.Future<$1.CompactRequest> $request) async {
    return compact($call, await $request);
  }

  $async.Future<$1.CompactResponse> compact(
      $grpc.ServiceCall call, $1.CompactRequest request);
}
