// This is a generated file - do not edit.
//
// Generated from agent/v1/search.proto.

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

import 'search.pb.dart' as $0;

export 'search.pb.dart';

@$pb.GrpcServiceName('agent.v1.SearchService')
class SearchServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  SearchServiceClient(super.channel, {super.options, super.interceptors});

  /// Index freshness (one entry per served backend, or the selected one).
  $grpc.ResponseFuture<$0.StatusResponse> status(
    $0.StatusRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$status, request, options: options);
  }

  /// Advertised capabilities per backend.
  $grpc.ResponseFuture<$0.SearchCapabilitiesResponse> capabilities(
    $0.SearchCapabilitiesRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$capabilities, request, options: options);
  }

  /// Bring the index up to date, streaming progress.
  $grpc.ResponseStream<$0.ReindexProgress> reindex(
    $0.ReindexRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createStreamingCall(
        _$reindex, $async.Stream.fromIterable([request]),
        options: options);
  }

  /// Run a query against the selected backend.
  $grpc.ResponseFuture<$0.SearchResponse> search(
    $0.SearchRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$search, request, options: options);
  }

  /// List indexed file paths (optionally glob-filtered) from the selected backend.
  $grpc.ResponseFuture<$0.ListFilesResponse> listFiles(
    $0.ListFilesRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$listFiles, request, options: options);
  }

  // method descriptors

  static final _$status =
      $grpc.ClientMethod<$0.StatusRequest, $0.StatusResponse>(
          '/agent.v1.SearchService/Status',
          ($0.StatusRequest value) => value.writeToBuffer(),
          $0.StatusResponse.fromBuffer);
  static final _$capabilities = $grpc.ClientMethod<$0.SearchCapabilitiesRequest,
          $0.SearchCapabilitiesResponse>(
      '/agent.v1.SearchService/Capabilities',
      ($0.SearchCapabilitiesRequest value) => value.writeToBuffer(),
      $0.SearchCapabilitiesResponse.fromBuffer);
  static final _$reindex =
      $grpc.ClientMethod<$0.ReindexRequest, $0.ReindexProgress>(
          '/agent.v1.SearchService/Reindex',
          ($0.ReindexRequest value) => value.writeToBuffer(),
          $0.ReindexProgress.fromBuffer);
  static final _$search =
      $grpc.ClientMethod<$0.SearchRequest, $0.SearchResponse>(
          '/agent.v1.SearchService/Search',
          ($0.SearchRequest value) => value.writeToBuffer(),
          $0.SearchResponse.fromBuffer);
  static final _$listFiles =
      $grpc.ClientMethod<$0.ListFilesRequest, $0.ListFilesResponse>(
          '/agent.v1.SearchService/ListFiles',
          ($0.ListFilesRequest value) => value.writeToBuffer(),
          $0.ListFilesResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.SearchService')
abstract class SearchServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.SearchService';

  SearchServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.StatusRequest, $0.StatusResponse>(
        'Status',
        status_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.StatusRequest.fromBuffer(value),
        ($0.StatusResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.SearchCapabilitiesRequest,
            $0.SearchCapabilitiesResponse>(
        'Capabilities',
        capabilities_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.SearchCapabilitiesRequest.fromBuffer(value),
        ($0.SearchCapabilitiesResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ReindexRequest, $0.ReindexProgress>(
        'Reindex',
        reindex_Pre,
        false,
        true,
        ($core.List<$core.int> value) => $0.ReindexRequest.fromBuffer(value),
        ($0.ReindexProgress value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.SearchRequest, $0.SearchResponse>(
        'Search',
        search_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.SearchRequest.fromBuffer(value),
        ($0.SearchResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ListFilesRequest, $0.ListFilesResponse>(
        'ListFiles',
        listFiles_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ListFilesRequest.fromBuffer(value),
        ($0.ListFilesResponse value) => value.writeToBuffer()));
  }

  $async.Future<$0.StatusResponse> status_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.StatusRequest> $request) async {
    return status($call, await $request);
  }

  $async.Future<$0.StatusResponse> status(
      $grpc.ServiceCall call, $0.StatusRequest request);

  $async.Future<$0.SearchCapabilitiesResponse> capabilities_Pre(
      $grpc.ServiceCall $call,
      $async.Future<$0.SearchCapabilitiesRequest> $request) async {
    return capabilities($call, await $request);
  }

  $async.Future<$0.SearchCapabilitiesResponse> capabilities(
      $grpc.ServiceCall call, $0.SearchCapabilitiesRequest request);

  $async.Stream<$0.ReindexProgress> reindex_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ReindexRequest> $request) async* {
    yield* reindex($call, await $request);
  }

  $async.Stream<$0.ReindexProgress> reindex(
      $grpc.ServiceCall call, $0.ReindexRequest request);

  $async.Future<$0.SearchResponse> search_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.SearchRequest> $request) async {
    return search($call, await $request);
  }

  $async.Future<$0.SearchResponse> search(
      $grpc.ServiceCall call, $0.SearchRequest request);

  $async.Future<$0.ListFilesResponse> listFiles_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ListFilesRequest> $request) async {
    return listFiles($call, await $request);
  }

  $async.Future<$0.ListFilesResponse> listFiles(
      $grpc.ServiceCall call, $0.ListFilesRequest request);
}
