// This is a generated file - do not edit.
//
// Generated from agent/v1/lsp.proto.

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

import 'lsp.pb.dart' as $0;

export 'lsp.pb.dart';

@$pb.GrpcServiceName('agent.v1.LspService')
class LspServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  LspServiceClient(super.channel, {super.options, super.interceptors});

  /// Tell the server about a file's current contents (unsaved buffers).
  $grpc.ResponseFuture<$0.LspOpenResponse> open(
    $0.LspOpenRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$open, request, options: options);
  }

  $grpc.ResponseFuture<$0.LspResultMsg> request(
    $0.LspRequestMsg request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$request, request, options: options);
  }

  $grpc.ResponseFuture<$0.LspCapabilities> capabilities(
    $0.LspCapabilitiesRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$capabilities, request, options: options);
  }

  $grpc.ResponseFuture<$0.LspShutdownResponse> shutdown(
    $0.LspShutdownRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$shutdown, request, options: options);
  }

  // method descriptors

  static final _$open =
      $grpc.ClientMethod<$0.LspOpenRequest, $0.LspOpenResponse>(
          '/agent.v1.LspService/Open',
          ($0.LspOpenRequest value) => value.writeToBuffer(),
          $0.LspOpenResponse.fromBuffer);
  static final _$request =
      $grpc.ClientMethod<$0.LspRequestMsg, $0.LspResultMsg>(
          '/agent.v1.LspService/Request',
          ($0.LspRequestMsg value) => value.writeToBuffer(),
          $0.LspResultMsg.fromBuffer);
  static final _$capabilities =
      $grpc.ClientMethod<$0.LspCapabilitiesRequest, $0.LspCapabilities>(
          '/agent.v1.LspService/Capabilities',
          ($0.LspCapabilitiesRequest value) => value.writeToBuffer(),
          $0.LspCapabilities.fromBuffer);
  static final _$shutdown =
      $grpc.ClientMethod<$0.LspShutdownRequest, $0.LspShutdownResponse>(
          '/agent.v1.LspService/Shutdown',
          ($0.LspShutdownRequest value) => value.writeToBuffer(),
          $0.LspShutdownResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.LspService')
abstract class LspServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.LspService';

  LspServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.LspOpenRequest, $0.LspOpenResponse>(
        'Open',
        open_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.LspOpenRequest.fromBuffer(value),
        ($0.LspOpenResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.LspRequestMsg, $0.LspResultMsg>(
        'Request',
        request_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.LspRequestMsg.fromBuffer(value),
        ($0.LspResultMsg value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.LspCapabilitiesRequest, $0.LspCapabilities>(
            'Capabilities',
            capabilities_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.LspCapabilitiesRequest.fromBuffer(value),
            ($0.LspCapabilities value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.LspShutdownRequest, $0.LspShutdownResponse>(
            'Shutdown',
            shutdown_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.LspShutdownRequest.fromBuffer(value),
            ($0.LspShutdownResponse value) => value.writeToBuffer()));
  }

  $async.Future<$0.LspOpenResponse> open_Pre($grpc.ServiceCall $call,
      $async.Future<$0.LspOpenRequest> $request) async {
    return open($call, await $request);
  }

  $async.Future<$0.LspOpenResponse> open(
      $grpc.ServiceCall call, $0.LspOpenRequest request);

  $async.Future<$0.LspResultMsg> request_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.LspRequestMsg> $request) async {
    return request($call, await $request);
  }

  $async.Future<$0.LspResultMsg> request(
      $grpc.ServiceCall call, $0.LspRequestMsg request);

  $async.Future<$0.LspCapabilities> capabilities_Pre($grpc.ServiceCall $call,
      $async.Future<$0.LspCapabilitiesRequest> $request) async {
    return capabilities($call, await $request);
  }

  $async.Future<$0.LspCapabilities> capabilities(
      $grpc.ServiceCall call, $0.LspCapabilitiesRequest request);

  $async.Future<$0.LspShutdownResponse> shutdown_Pre($grpc.ServiceCall $call,
      $async.Future<$0.LspShutdownRequest> $request) async {
    return shutdown($call, await $request);
  }

  $async.Future<$0.LspShutdownResponse> shutdown(
      $grpc.ServiceCall call, $0.LspShutdownRequest request);
}
