// This is a generated file - do not edit.
//
// Generated from agent/v1/prompt.proto.

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

import 'prompt.pb.dart' as $0;

export 'prompt.pb.dart';

@$pb.GrpcServiceName('agent.v1.PromptService')
class PromptServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  PromptServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.PromptList> list(
    $0.PromptListRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$list, request, options: options);
  }

  $grpc.ResponseFuture<$0.PromptEntry> get(
    $0.PromptRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$get, request, options: options);
  }

  $grpc.ResponseFuture<$0.PromptEntry> put(
    $0.PromptEntry request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$put, request, options: options);
  }

  $grpc.ResponseFuture<$0.DeleteReply> delete(
    $0.PromptRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$delete, request, options: options);
  }

  $grpc.ResponseFuture<$0.AssembledContext> previewAssembled(
    $0.PreviewRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$previewAssembled, request, options: options);
  }

  // method descriptors

  static final _$list = $grpc.ClientMethod<$0.PromptListRequest, $0.PromptList>(
      '/agent.v1.PromptService/List',
      ($0.PromptListRequest value) => value.writeToBuffer(),
      $0.PromptList.fromBuffer);
  static final _$get = $grpc.ClientMethod<$0.PromptRef, $0.PromptEntry>(
      '/agent.v1.PromptService/Get',
      ($0.PromptRef value) => value.writeToBuffer(),
      $0.PromptEntry.fromBuffer);
  static final _$put = $grpc.ClientMethod<$0.PromptEntry, $0.PromptEntry>(
      '/agent.v1.PromptService/Put',
      ($0.PromptEntry value) => value.writeToBuffer(),
      $0.PromptEntry.fromBuffer);
  static final _$delete = $grpc.ClientMethod<$0.PromptRef, $0.DeleteReply>(
      '/agent.v1.PromptService/Delete',
      ($0.PromptRef value) => value.writeToBuffer(),
      $0.DeleteReply.fromBuffer);
  static final _$previewAssembled =
      $grpc.ClientMethod<$0.PreviewRequest, $0.AssembledContext>(
          '/agent.v1.PromptService/PreviewAssembled',
          ($0.PreviewRequest value) => value.writeToBuffer(),
          $0.AssembledContext.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.PromptService')
abstract class PromptServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.PromptService';

  PromptServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.PromptListRequest, $0.PromptList>(
        'List',
        list_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PromptListRequest.fromBuffer(value),
        ($0.PromptList value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PromptRef, $0.PromptEntry>(
        'Get',
        get_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PromptRef.fromBuffer(value),
        ($0.PromptEntry value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PromptEntry, $0.PromptEntry>(
        'Put',
        put_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PromptEntry.fromBuffer(value),
        ($0.PromptEntry value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PromptRef, $0.DeleteReply>(
        'Delete',
        delete_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PromptRef.fromBuffer(value),
        ($0.DeleteReply value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PreviewRequest, $0.AssembledContext>(
        'PreviewAssembled',
        previewAssembled_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PreviewRequest.fromBuffer(value),
        ($0.AssembledContext value) => value.writeToBuffer()));
  }

  $async.Future<$0.PromptList> list_Pre($grpc.ServiceCall $call,
      $async.Future<$0.PromptListRequest> $request) async {
    return list($call, await $request);
  }

  $async.Future<$0.PromptList> list(
      $grpc.ServiceCall call, $0.PromptListRequest request);

  $async.Future<$0.PromptEntry> get_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.PromptRef> $request) async {
    return get($call, await $request);
  }

  $async.Future<$0.PromptEntry> get(
      $grpc.ServiceCall call, $0.PromptRef request);

  $async.Future<$0.PromptEntry> put_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.PromptEntry> $request) async {
    return put($call, await $request);
  }

  $async.Future<$0.PromptEntry> put(
      $grpc.ServiceCall call, $0.PromptEntry request);

  $async.Future<$0.DeleteReply> delete_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.PromptRef> $request) async {
    return delete($call, await $request);
  }

  $async.Future<$0.DeleteReply> delete(
      $grpc.ServiceCall call, $0.PromptRef request);

  $async.Future<$0.AssembledContext> previewAssembled_Pre(
      $grpc.ServiceCall $call,
      $async.Future<$0.PreviewRequest> $request) async {
    return previewAssembled($call, await $request);
  }

  $async.Future<$0.AssembledContext> previewAssembled(
      $grpc.ServiceCall call, $0.PreviewRequest request);
}
