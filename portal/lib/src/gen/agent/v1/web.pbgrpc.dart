// This is a generated file - do not edit.
//
// Generated from agent/v1/web.proto.

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

import 'web.pb.dart' as $0;

export 'web.pb.dart';

@$pb.GrpcServiceName('agent.v1.WebService')
class WebServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  WebServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.WebFetchResponse> fetch(
    $0.WebFetchRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$fetch, request, options: options);
  }

  // method descriptors

  static final _$fetch =
      $grpc.ClientMethod<$0.WebFetchRequest, $0.WebFetchResponse>(
          '/agent.v1.WebService/Fetch',
          ($0.WebFetchRequest value) => value.writeToBuffer(),
          $0.WebFetchResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.WebService')
abstract class WebServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.WebService';

  WebServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.WebFetchRequest, $0.WebFetchResponse>(
        'Fetch',
        fetch_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.WebFetchRequest.fromBuffer(value),
        ($0.WebFetchResponse value) => value.writeToBuffer()));
  }

  $async.Future<$0.WebFetchResponse> fetch_Pre($grpc.ServiceCall $call,
      $async.Future<$0.WebFetchRequest> $request) async {
    return fetch($call, await $request);
  }

  $async.Future<$0.WebFetchResponse> fetch(
      $grpc.ServiceCall call, $0.WebFetchRequest request);
}

@$pb.GrpcServiceName('agent.v1.WebSearchService')
class WebSearchServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  WebSearchServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.WebSearchResponse> search(
    $0.WebSearchRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$search, request, options: options);
  }

  /// Whether a query is cached, and how fresh — so a caller can report a cache
  /// hit without paying for the search.
  $grpc.ResponseFuture<$0.WebCacheStatus> status(
    $0.WebSearchRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$status, request, options: options);
  }

  $grpc.ResponseFuture<$0.WebSearchCapabilities> capabilities(
    $0.WebSearchCapabilitiesRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$capabilities, request, options: options);
  }

  // method descriptors

  static final _$search =
      $grpc.ClientMethod<$0.WebSearchRequest, $0.WebSearchResponse>(
          '/agent.v1.WebSearchService/Search',
          ($0.WebSearchRequest value) => value.writeToBuffer(),
          $0.WebSearchResponse.fromBuffer);
  static final _$status =
      $grpc.ClientMethod<$0.WebSearchRequest, $0.WebCacheStatus>(
          '/agent.v1.WebSearchService/Status',
          ($0.WebSearchRequest value) => value.writeToBuffer(),
          $0.WebCacheStatus.fromBuffer);
  static final _$capabilities = $grpc.ClientMethod<
          $0.WebSearchCapabilitiesRequest, $0.WebSearchCapabilities>(
      '/agent.v1.WebSearchService/Capabilities',
      ($0.WebSearchCapabilitiesRequest value) => value.writeToBuffer(),
      $0.WebSearchCapabilities.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.WebSearchService')
abstract class WebSearchServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.WebSearchService';

  WebSearchServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.WebSearchRequest, $0.WebSearchResponse>(
        'Search',
        search_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.WebSearchRequest.fromBuffer(value),
        ($0.WebSearchResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.WebSearchRequest, $0.WebCacheStatus>(
        'Status',
        status_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.WebSearchRequest.fromBuffer(value),
        ($0.WebCacheStatus value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.WebSearchCapabilitiesRequest,
            $0.WebSearchCapabilities>(
        'Capabilities',
        capabilities_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.WebSearchCapabilitiesRequest.fromBuffer(value),
        ($0.WebSearchCapabilities value) => value.writeToBuffer()));
  }

  $async.Future<$0.WebSearchResponse> search_Pre($grpc.ServiceCall $call,
      $async.Future<$0.WebSearchRequest> $request) async {
    return search($call, await $request);
  }

  $async.Future<$0.WebSearchResponse> search(
      $grpc.ServiceCall call, $0.WebSearchRequest request);

  $async.Future<$0.WebCacheStatus> status_Pre($grpc.ServiceCall $call,
      $async.Future<$0.WebSearchRequest> $request) async {
    return status($call, await $request);
  }

  $async.Future<$0.WebCacheStatus> status(
      $grpc.ServiceCall call, $0.WebSearchRequest request);

  $async.Future<$0.WebSearchCapabilities> capabilities_Pre(
      $grpc.ServiceCall $call,
      $async.Future<$0.WebSearchCapabilitiesRequest> $request) async {
    return capabilities($call, await $request);
  }

  $async.Future<$0.WebSearchCapabilities> capabilities(
      $grpc.ServiceCall call, $0.WebSearchCapabilitiesRequest request);
}
