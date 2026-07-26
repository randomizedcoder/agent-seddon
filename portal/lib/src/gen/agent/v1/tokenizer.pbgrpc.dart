// This is a generated file - do not edit.
//
// Generated from agent/v1/tokenizer.proto.

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

import 'tokenizer.pb.dart' as $0;

export 'tokenizer.pb.dart';

@$pb.GrpcServiceName('agent.v1.TokenizerService')
class TokenizerServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  TokenizerServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.TokCount> count(
    $0.TokCountRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$count, request, options: options);
  }

  $grpc.ResponseFuture<$0.TokCount> countMessages(
    $0.TokCountMessagesRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$countMessages, request, options: options);
  }

  // method descriptors

  static final _$count = $grpc.ClientMethod<$0.TokCountRequest, $0.TokCount>(
      '/agent.v1.TokenizerService/Count',
      ($0.TokCountRequest value) => value.writeToBuffer(),
      $0.TokCount.fromBuffer);
  static final _$countMessages =
      $grpc.ClientMethod<$0.TokCountMessagesRequest, $0.TokCount>(
          '/agent.v1.TokenizerService/CountMessages',
          ($0.TokCountMessagesRequest value) => value.writeToBuffer(),
          $0.TokCount.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.TokenizerService')
abstract class TokenizerServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.TokenizerService';

  TokenizerServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.TokCountRequest, $0.TokCount>(
        'Count',
        count_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.TokCountRequest.fromBuffer(value),
        ($0.TokCount value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.TokCountMessagesRequest, $0.TokCount>(
        'CountMessages',
        countMessages_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.TokCountMessagesRequest.fromBuffer(value),
        ($0.TokCount value) => value.writeToBuffer()));
  }

  $async.Future<$0.TokCount> count_Pre($grpc.ServiceCall $call,
      $async.Future<$0.TokCountRequest> $request) async {
    return count($call, await $request);
  }

  $async.Future<$0.TokCount> count(
      $grpc.ServiceCall call, $0.TokCountRequest request);

  $async.Future<$0.TokCount> countMessages_Pre($grpc.ServiceCall $call,
      $async.Future<$0.TokCountMessagesRequest> $request) async {
    return countMessages($call, await $request);
  }

  $async.Future<$0.TokCount> countMessages(
      $grpc.ServiceCall call, $0.TokCountMessagesRequest request);
}
