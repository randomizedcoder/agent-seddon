// This is a generated file - do not edit.
//
// Generated from agent/v1/provider.proto.

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

import 'common.pb.dart' as $1;
import 'provider.pb.dart' as $0;

export 'provider.pb.dart';

@$pb.GrpcServiceName('agent.v1.Provider')
class ProviderClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  ProviderClient(super.channel, {super.options, super.interceptors});

  /// Provider + model capabilities (tool support, context window).
  $grpc.ResponseFuture<$1.ModelCapabilities> capabilities(
    $0.CapabilitiesRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$capabilities, request, options: options);
  }

  /// Buffered completion.
  $grpc.ResponseFuture<$1.CompletionResponse> complete(
    $1.CompletionRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$complete, request, options: options);
  }

  /// Streaming completion — one CompletionChunk per increment.
  $grpc.ResponseStream<$1.CompletionChunk> stream(
    $1.CompletionRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createStreamingCall(_$stream, $async.Stream.fromIterable([request]),
        options: options);
  }

  // method descriptors

  static final _$capabilities =
      $grpc.ClientMethod<$0.CapabilitiesRequest, $1.ModelCapabilities>(
          '/agent.v1.Provider/Capabilities',
          ($0.CapabilitiesRequest value) => value.writeToBuffer(),
          $1.ModelCapabilities.fromBuffer);
  static final _$complete =
      $grpc.ClientMethod<$1.CompletionRequest, $1.CompletionResponse>(
          '/agent.v1.Provider/Complete',
          ($1.CompletionRequest value) => value.writeToBuffer(),
          $1.CompletionResponse.fromBuffer);
  static final _$stream =
      $grpc.ClientMethod<$1.CompletionRequest, $1.CompletionChunk>(
          '/agent.v1.Provider/Stream',
          ($1.CompletionRequest value) => value.writeToBuffer(),
          $1.CompletionChunk.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.Provider')
abstract class ProviderServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.Provider';

  ProviderServiceBase() {
    $addMethod(
        $grpc.ServiceMethod<$0.CapabilitiesRequest, $1.ModelCapabilities>(
            'Capabilities',
            capabilities_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.CapabilitiesRequest.fromBuffer(value),
            ($1.ModelCapabilities value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$1.CompletionRequest, $1.CompletionResponse>(
        'Complete',
        complete_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $1.CompletionRequest.fromBuffer(value),
        ($1.CompletionResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$1.CompletionRequest, $1.CompletionChunk>(
        'Stream',
        stream_Pre,
        false,
        true,
        ($core.List<$core.int> value) => $1.CompletionRequest.fromBuffer(value),
        ($1.CompletionChunk value) => value.writeToBuffer()));
  }

  $async.Future<$1.ModelCapabilities> capabilities_Pre($grpc.ServiceCall $call,
      $async.Future<$0.CapabilitiesRequest> $request) async {
    return capabilities($call, await $request);
  }

  $async.Future<$1.ModelCapabilities> capabilities(
      $grpc.ServiceCall call, $0.CapabilitiesRequest request);

  $async.Future<$1.CompletionResponse> complete_Pre($grpc.ServiceCall $call,
      $async.Future<$1.CompletionRequest> $request) async {
    return complete($call, await $request);
  }

  $async.Future<$1.CompletionResponse> complete(
      $grpc.ServiceCall call, $1.CompletionRequest request);

  $async.Stream<$1.CompletionChunk> stream_Pre($grpc.ServiceCall $call,
      $async.Future<$1.CompletionRequest> $request) async* {
    yield* stream($call, await $request);
  }

  $async.Stream<$1.CompletionChunk> stream(
      $grpc.ServiceCall call, $1.CompletionRequest request);
}
