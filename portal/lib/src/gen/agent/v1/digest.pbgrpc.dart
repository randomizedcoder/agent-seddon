// This is a generated file - do not edit.
//
// Generated from agent/v1/digest.proto.

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

import 'digest.pb.dart' as $0;

export 'digest.pb.dart';

@$pb.GrpcServiceName('agent.v1.DigestService')
class DigestServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  DigestServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.PutDigestResponse> put(
    $0.PutDigestRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$put, request, options: options);
  }

  $grpc.ResponseFuture<$0.QueryDigestsResponse> query(
    $0.QueryDigestsRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$query, request, options: options);
  }

  // method descriptors

  static final _$put =
      $grpc.ClientMethod<$0.PutDigestRequest, $0.PutDigestResponse>(
          '/agent.v1.DigestService/Put',
          ($0.PutDigestRequest value) => value.writeToBuffer(),
          $0.PutDigestResponse.fromBuffer);
  static final _$query =
      $grpc.ClientMethod<$0.QueryDigestsRequest, $0.QueryDigestsResponse>(
          '/agent.v1.DigestService/Query',
          ($0.QueryDigestsRequest value) => value.writeToBuffer(),
          $0.QueryDigestsResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.DigestService')
abstract class DigestServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.DigestService';

  DigestServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.PutDigestRequest, $0.PutDigestResponse>(
        'Put',
        put_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PutDigestRequest.fromBuffer(value),
        ($0.PutDigestResponse value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.QueryDigestsRequest, $0.QueryDigestsResponse>(
            'Query',
            query_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.QueryDigestsRequest.fromBuffer(value),
            ($0.QueryDigestsResponse value) => value.writeToBuffer()));
  }

  $async.Future<$0.PutDigestResponse> put_Pre($grpc.ServiceCall $call,
      $async.Future<$0.PutDigestRequest> $request) async {
    return put($call, await $request);
  }

  $async.Future<$0.PutDigestResponse> put(
      $grpc.ServiceCall call, $0.PutDigestRequest request);

  $async.Future<$0.QueryDigestsResponse> query_Pre($grpc.ServiceCall $call,
      $async.Future<$0.QueryDigestsRequest> $request) async {
    return query($call, await $request);
  }

  $async.Future<$0.QueryDigestsResponse> query(
      $grpc.ServiceCall call, $0.QueryDigestsRequest request);
}
