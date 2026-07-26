// This is a generated file - do not edit.
//
// Generated from agent/v1/exec.proto.

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

import 'exec.pb.dart' as $0;

export 'exec.pb.dart';

@$pb.GrpcServiceName('agent.v1.SandboxService')
class SandboxServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  SandboxServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.ExecResult> exec(
    $0.ExecRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$exec, request, options: options);
  }

  $grpc.ResponseFuture<$0.ExecCapabilities> capabilities(
    $0.ExecCapabilitiesRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$capabilities, request, options: options);
  }

  // method descriptors

  static final _$exec = $grpc.ClientMethod<$0.ExecRequest, $0.ExecResult>(
      '/agent.v1.SandboxService/Exec',
      ($0.ExecRequest value) => value.writeToBuffer(),
      $0.ExecResult.fromBuffer);
  static final _$capabilities =
      $grpc.ClientMethod<$0.ExecCapabilitiesRequest, $0.ExecCapabilities>(
          '/agent.v1.SandboxService/Capabilities',
          ($0.ExecCapabilitiesRequest value) => value.writeToBuffer(),
          $0.ExecCapabilities.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.SandboxService')
abstract class SandboxServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.SandboxService';

  SandboxServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.ExecRequest, $0.ExecResult>(
        'Exec',
        exec_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ExecRequest.fromBuffer(value),
        ($0.ExecResult value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.ExecCapabilitiesRequest, $0.ExecCapabilities>(
            'Capabilities',
            capabilities_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.ExecCapabilitiesRequest.fromBuffer(value),
            ($0.ExecCapabilities value) => value.writeToBuffer()));
  }

  $async.Future<$0.ExecResult> exec_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.ExecRequest> $request) async {
    return exec($call, await $request);
  }

  $async.Future<$0.ExecResult> exec(
      $grpc.ServiceCall call, $0.ExecRequest request);

  $async.Future<$0.ExecCapabilities> capabilities_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ExecCapabilitiesRequest> $request) async {
    return capabilities($call, await $request);
  }

  $async.Future<$0.ExecCapabilities> capabilities(
      $grpc.ServiceCall call, $0.ExecCapabilitiesRequest request);
}

@$pb.GrpcServiceName('agent.v1.PtyService')
class PtyServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  PtyServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.PtySessionRef> open(
    $0.PtyOpenRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$open, request, options: options);
  }

  $grpc.ResponseFuture<$0.PtyWriteResponse> write(
    $0.PtyWriteRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$write, request, options: options);
  }

  $grpc.ResponseFuture<$0.PtyReadResponse> read(
    $0.PtyReadRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$read, request, options: options);
  }

  $grpc.ResponseFuture<$0.PtyResizeResponse> resize(
    $0.PtyResizeRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$resize, request, options: options);
  }

  $grpc.ResponseFuture<$0.PtyCloseResponse> close(
    $0.PtySessionRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$close, request, options: options);
  }

  $grpc.ResponseFuture<$0.PtySessionList> list(
    $0.PtyListRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$list, request, options: options);
  }

  $grpc.ResponseFuture<$0.PtySessionInfo> get(
    $0.PtySessionRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$get, request, options: options);
  }

  // method descriptors

  static final _$open = $grpc.ClientMethod<$0.PtyOpenRequest, $0.PtySessionRef>(
      '/agent.v1.PtyService/Open',
      ($0.PtyOpenRequest value) => value.writeToBuffer(),
      $0.PtySessionRef.fromBuffer);
  static final _$write =
      $grpc.ClientMethod<$0.PtyWriteRequest, $0.PtyWriteResponse>(
          '/agent.v1.PtyService/Write',
          ($0.PtyWriteRequest value) => value.writeToBuffer(),
          $0.PtyWriteResponse.fromBuffer);
  static final _$read =
      $grpc.ClientMethod<$0.PtyReadRequest, $0.PtyReadResponse>(
          '/agent.v1.PtyService/Read',
          ($0.PtyReadRequest value) => value.writeToBuffer(),
          $0.PtyReadResponse.fromBuffer);
  static final _$resize =
      $grpc.ClientMethod<$0.PtyResizeRequest, $0.PtyResizeResponse>(
          '/agent.v1.PtyService/Resize',
          ($0.PtyResizeRequest value) => value.writeToBuffer(),
          $0.PtyResizeResponse.fromBuffer);
  static final _$close =
      $grpc.ClientMethod<$0.PtySessionRef, $0.PtyCloseResponse>(
          '/agent.v1.PtyService/Close',
          ($0.PtySessionRef value) => value.writeToBuffer(),
          $0.PtyCloseResponse.fromBuffer);
  static final _$list =
      $grpc.ClientMethod<$0.PtyListRequest, $0.PtySessionList>(
          '/agent.v1.PtyService/List',
          ($0.PtyListRequest value) => value.writeToBuffer(),
          $0.PtySessionList.fromBuffer);
  static final _$get = $grpc.ClientMethod<$0.PtySessionRef, $0.PtySessionInfo>(
      '/agent.v1.PtyService/Get',
      ($0.PtySessionRef value) => value.writeToBuffer(),
      $0.PtySessionInfo.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.PtyService')
abstract class PtyServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.PtyService';

  PtyServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.PtyOpenRequest, $0.PtySessionRef>(
        'Open',
        open_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PtyOpenRequest.fromBuffer(value),
        ($0.PtySessionRef value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PtyWriteRequest, $0.PtyWriteResponse>(
        'Write',
        write_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PtyWriteRequest.fromBuffer(value),
        ($0.PtyWriteResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PtyReadRequest, $0.PtyReadResponse>(
        'Read',
        read_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PtyReadRequest.fromBuffer(value),
        ($0.PtyReadResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PtyResizeRequest, $0.PtyResizeResponse>(
        'Resize',
        resize_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PtyResizeRequest.fromBuffer(value),
        ($0.PtyResizeResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PtySessionRef, $0.PtyCloseResponse>(
        'Close',
        close_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PtySessionRef.fromBuffer(value),
        ($0.PtyCloseResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PtyListRequest, $0.PtySessionList>(
        'List',
        list_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PtyListRequest.fromBuffer(value),
        ($0.PtySessionList value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PtySessionRef, $0.PtySessionInfo>(
        'Get',
        get_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PtySessionRef.fromBuffer(value),
        ($0.PtySessionInfo value) => value.writeToBuffer()));
  }

  $async.Future<$0.PtySessionRef> open_Pre($grpc.ServiceCall $call,
      $async.Future<$0.PtyOpenRequest> $request) async {
    return open($call, await $request);
  }

  $async.Future<$0.PtySessionRef> open(
      $grpc.ServiceCall call, $0.PtyOpenRequest request);

  $async.Future<$0.PtyWriteResponse> write_Pre($grpc.ServiceCall $call,
      $async.Future<$0.PtyWriteRequest> $request) async {
    return write($call, await $request);
  }

  $async.Future<$0.PtyWriteResponse> write(
      $grpc.ServiceCall call, $0.PtyWriteRequest request);

  $async.Future<$0.PtyReadResponse> read_Pre($grpc.ServiceCall $call,
      $async.Future<$0.PtyReadRequest> $request) async {
    return read($call, await $request);
  }

  $async.Future<$0.PtyReadResponse> read(
      $grpc.ServiceCall call, $0.PtyReadRequest request);

  $async.Future<$0.PtyResizeResponse> resize_Pre($grpc.ServiceCall $call,
      $async.Future<$0.PtyResizeRequest> $request) async {
    return resize($call, await $request);
  }

  $async.Future<$0.PtyResizeResponse> resize(
      $grpc.ServiceCall call, $0.PtyResizeRequest request);

  $async.Future<$0.PtyCloseResponse> close_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.PtySessionRef> $request) async {
    return close($call, await $request);
  }

  $async.Future<$0.PtyCloseResponse> close(
      $grpc.ServiceCall call, $0.PtySessionRef request);

  $async.Future<$0.PtySessionList> list_Pre($grpc.ServiceCall $call,
      $async.Future<$0.PtyListRequest> $request) async {
    return list($call, await $request);
  }

  $async.Future<$0.PtySessionList> list(
      $grpc.ServiceCall call, $0.PtyListRequest request);

  $async.Future<$0.PtySessionInfo> get_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.PtySessionRef> $request) async {
    return get($call, await $request);
  }

  $async.Future<$0.PtySessionInfo> get(
      $grpc.ServiceCall call, $0.PtySessionRef request);
}
