// This is a generated file - do not edit.
//
// Generated from agent/v1/embed.proto.

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

import 'embed.pb.dart' as $0;

export 'embed.pb.dart';

@$pb.GrpcServiceName('agent.v1.EmbedService')
class EmbedServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  EmbedServiceClient(super.channel, {super.options, super.interceptors});

  /// The fixed dimensionality and batch ceiling. Fetched so a client can VERIFY
  /// the remote against its configured `[embedder] dimensions` rather than
  /// assuming — a mismatch silently corrupts a vector index.
  $grpc.ResponseFuture<$0.EmbCapabilities> capabilities(
    $0.EmbCapabilitiesRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$capabilities, request, options: options);
  }

  $grpc.ResponseFuture<$0.EmbVector> embedQuery(
    $0.EmbQueryRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$embedQuery, request, options: options);
  }

  $grpc.ResponseFuture<$0.EmbVectors> embedDocs(
    $0.EmbDocsRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$embedDocs, request, options: options);
  }

  // method descriptors

  static final _$capabilities =
      $grpc.ClientMethod<$0.EmbCapabilitiesRequest, $0.EmbCapabilities>(
          '/agent.v1.EmbedService/Capabilities',
          ($0.EmbCapabilitiesRequest value) => value.writeToBuffer(),
          $0.EmbCapabilities.fromBuffer);
  static final _$embedQuery =
      $grpc.ClientMethod<$0.EmbQueryRequest, $0.EmbVector>(
          '/agent.v1.EmbedService/EmbedQuery',
          ($0.EmbQueryRequest value) => value.writeToBuffer(),
          $0.EmbVector.fromBuffer);
  static final _$embedDocs =
      $grpc.ClientMethod<$0.EmbDocsRequest, $0.EmbVectors>(
          '/agent.v1.EmbedService/EmbedDocs',
          ($0.EmbDocsRequest value) => value.writeToBuffer(),
          $0.EmbVectors.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.EmbedService')
abstract class EmbedServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.EmbedService';

  EmbedServiceBase() {
    $addMethod(
        $grpc.ServiceMethod<$0.EmbCapabilitiesRequest, $0.EmbCapabilities>(
            'Capabilities',
            capabilities_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.EmbCapabilitiesRequest.fromBuffer(value),
            ($0.EmbCapabilities value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.EmbQueryRequest, $0.EmbVector>(
        'EmbedQuery',
        embedQuery_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.EmbQueryRequest.fromBuffer(value),
        ($0.EmbVector value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.EmbDocsRequest, $0.EmbVectors>(
        'EmbedDocs',
        embedDocs_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.EmbDocsRequest.fromBuffer(value),
        ($0.EmbVectors value) => value.writeToBuffer()));
  }

  $async.Future<$0.EmbCapabilities> capabilities_Pre($grpc.ServiceCall $call,
      $async.Future<$0.EmbCapabilitiesRequest> $request) async {
    return capabilities($call, await $request);
  }

  $async.Future<$0.EmbCapabilities> capabilities(
      $grpc.ServiceCall call, $0.EmbCapabilitiesRequest request);

  $async.Future<$0.EmbVector> embedQuery_Pre($grpc.ServiceCall $call,
      $async.Future<$0.EmbQueryRequest> $request) async {
    return embedQuery($call, await $request);
  }

  $async.Future<$0.EmbVector> embedQuery(
      $grpc.ServiceCall call, $0.EmbQueryRequest request);

  $async.Future<$0.EmbVectors> embedDocs_Pre($grpc.ServiceCall $call,
      $async.Future<$0.EmbDocsRequest> $request) async {
    return embedDocs($call, await $request);
  }

  $async.Future<$0.EmbVectors> embedDocs(
      $grpc.ServiceCall call, $0.EmbDocsRequest request);
}
