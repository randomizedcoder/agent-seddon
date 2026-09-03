// This is a generated file - do not edit.
//
// Generated from agent/v1/ast.proto.

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

import 'ast.pb.dart' as $0;
import 'search.pb.dart' as $1;

export 'ast.pb.dart';

@$pb.GrpcServiceName('agent.v1.AstService')
class AstServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  AstServiceClient(super.channel, {super.options, super.interceptors});

  /// Index freshness (one entry per served backend, or the selected one).
  $grpc.ResponseFuture<$0.AstStatusResponse> status(
    $0.AstStatusRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$status, request, options: options);
  }

  /// Advertised verbs + languages per backend.
  $grpc.ResponseFuture<$0.AstCapabilitiesResponse> capabilities(
    $0.AstCapabilitiesRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$capabilities, request, options: options);
  }

  /// Build/refresh the code graph, streaming progress.
  $grpc.ResponseStream<$1.ReindexProgress> reindex(
    $0.AstReindexRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createStreamingCall(
        _$reindex, $async.Stream.fromIterable([request]),
        options: options);
  }

  /// Find symbols by name.
  $grpc.ResponseFuture<$0.SymbolList> findSymbol(
    $0.FindSymbolRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$findSymbol, request, options: options);
  }

  /// Concrete types that satisfy an interface (implicit satisfaction in Go).
  $grpc.ResponseFuture<$0.SymbolList> implementations(
    $0.ImplementationsRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$implementations, request, options: options);
  }

  /// Interfaces a concrete type satisfies.
  $grpc.ResponseFuture<$0.SymbolList> interfaceOf(
    $0.InterfaceOfRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$interfaceOf, request, options: options);
  }

  /// Callers of the target, out to `hops` levels.
  $grpc.ResponseFuture<$0.AstCallGraph> callers(
    $0.CallersRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$callers, request, options: options);
  }

  /// Callees of the target, out to `hops` levels.
  $grpc.ResponseFuture<$0.AstCallGraph> callees(
    $0.CalleesRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$callees, request, options: options);
  }

  /// Distinct call paths from one symbol to another.
  $grpc.ResponseFuture<$0.CallchainResponse> callchain(
    $0.CallchainRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$callchain, request, options: options);
  }

  /// Blast radius: callers of every symbol in the changed files.
  $grpc.ResponseFuture<$0.AstCallGraph> blastRadius(
    $0.BlastRadiusRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$blastRadius, request, options: options);
  }

  /// An import path from one package to another.
  $grpc.ResponseFuture<$0.DependencyPathResponse> dependencyPath(
    $0.DependencyPathRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$dependencyPath, request, options: options);
  }

  // method descriptors

  static final _$status =
      $grpc.ClientMethod<$0.AstStatusRequest, $0.AstStatusResponse>(
          '/agent.v1.AstService/Status',
          ($0.AstStatusRequest value) => value.writeToBuffer(),
          $0.AstStatusResponse.fromBuffer);
  static final _$capabilities =
      $grpc.ClientMethod<$0.AstCapabilitiesRequest, $0.AstCapabilitiesResponse>(
          '/agent.v1.AstService/Capabilities',
          ($0.AstCapabilitiesRequest value) => value.writeToBuffer(),
          $0.AstCapabilitiesResponse.fromBuffer);
  static final _$reindex =
      $grpc.ClientMethod<$0.AstReindexRequest, $1.ReindexProgress>(
          '/agent.v1.AstService/Reindex',
          ($0.AstReindexRequest value) => value.writeToBuffer(),
          $1.ReindexProgress.fromBuffer);
  static final _$findSymbol =
      $grpc.ClientMethod<$0.FindSymbolRequest, $0.SymbolList>(
          '/agent.v1.AstService/FindSymbol',
          ($0.FindSymbolRequest value) => value.writeToBuffer(),
          $0.SymbolList.fromBuffer);
  static final _$implementations =
      $grpc.ClientMethod<$0.ImplementationsRequest, $0.SymbolList>(
          '/agent.v1.AstService/Implementations',
          ($0.ImplementationsRequest value) => value.writeToBuffer(),
          $0.SymbolList.fromBuffer);
  static final _$interfaceOf =
      $grpc.ClientMethod<$0.InterfaceOfRequest, $0.SymbolList>(
          '/agent.v1.AstService/InterfaceOf',
          ($0.InterfaceOfRequest value) => value.writeToBuffer(),
          $0.SymbolList.fromBuffer);
  static final _$callers =
      $grpc.ClientMethod<$0.CallersRequest, $0.AstCallGraph>(
          '/agent.v1.AstService/Callers',
          ($0.CallersRequest value) => value.writeToBuffer(),
          $0.AstCallGraph.fromBuffer);
  static final _$callees =
      $grpc.ClientMethod<$0.CalleesRequest, $0.AstCallGraph>(
          '/agent.v1.AstService/Callees',
          ($0.CalleesRequest value) => value.writeToBuffer(),
          $0.AstCallGraph.fromBuffer);
  static final _$callchain =
      $grpc.ClientMethod<$0.CallchainRequest, $0.CallchainResponse>(
          '/agent.v1.AstService/Callchain',
          ($0.CallchainRequest value) => value.writeToBuffer(),
          $0.CallchainResponse.fromBuffer);
  static final _$blastRadius =
      $grpc.ClientMethod<$0.BlastRadiusRequest, $0.AstCallGraph>(
          '/agent.v1.AstService/BlastRadius',
          ($0.BlastRadiusRequest value) => value.writeToBuffer(),
          $0.AstCallGraph.fromBuffer);
  static final _$dependencyPath =
      $grpc.ClientMethod<$0.DependencyPathRequest, $0.DependencyPathResponse>(
          '/agent.v1.AstService/DependencyPath',
          ($0.DependencyPathRequest value) => value.writeToBuffer(),
          $0.DependencyPathResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.AstService')
abstract class AstServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.AstService';

  AstServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.AstStatusRequest, $0.AstStatusResponse>(
        'Status',
        status_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.AstStatusRequest.fromBuffer(value),
        ($0.AstStatusResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.AstCapabilitiesRequest,
            $0.AstCapabilitiesResponse>(
        'Capabilities',
        capabilities_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.AstCapabilitiesRequest.fromBuffer(value),
        ($0.AstCapabilitiesResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.AstReindexRequest, $1.ReindexProgress>(
        'Reindex',
        reindex_Pre,
        false,
        true,
        ($core.List<$core.int> value) => $0.AstReindexRequest.fromBuffer(value),
        ($1.ReindexProgress value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.FindSymbolRequest, $0.SymbolList>(
        'FindSymbol',
        findSymbol_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.FindSymbolRequest.fromBuffer(value),
        ($0.SymbolList value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ImplementationsRequest, $0.SymbolList>(
        'Implementations',
        implementations_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.ImplementationsRequest.fromBuffer(value),
        ($0.SymbolList value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.InterfaceOfRequest, $0.SymbolList>(
        'InterfaceOf',
        interfaceOf_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.InterfaceOfRequest.fromBuffer(value),
        ($0.SymbolList value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.CallersRequest, $0.AstCallGraph>(
        'Callers',
        callers_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.CallersRequest.fromBuffer(value),
        ($0.AstCallGraph value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.CalleesRequest, $0.AstCallGraph>(
        'Callees',
        callees_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.CalleesRequest.fromBuffer(value),
        ($0.AstCallGraph value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.CallchainRequest, $0.CallchainResponse>(
        'Callchain',
        callchain_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.CallchainRequest.fromBuffer(value),
        ($0.CallchainResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.BlastRadiusRequest, $0.AstCallGraph>(
        'BlastRadius',
        blastRadius_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.BlastRadiusRequest.fromBuffer(value),
        ($0.AstCallGraph value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.DependencyPathRequest,
            $0.DependencyPathResponse>(
        'DependencyPath',
        dependencyPath_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.DependencyPathRequest.fromBuffer(value),
        ($0.DependencyPathResponse value) => value.writeToBuffer()));
  }

  $async.Future<$0.AstStatusResponse> status_Pre($grpc.ServiceCall $call,
      $async.Future<$0.AstStatusRequest> $request) async {
    return status($call, await $request);
  }

  $async.Future<$0.AstStatusResponse> status(
      $grpc.ServiceCall call, $0.AstStatusRequest request);

  $async.Future<$0.AstCapabilitiesResponse> capabilities_Pre(
      $grpc.ServiceCall $call,
      $async.Future<$0.AstCapabilitiesRequest> $request) async {
    return capabilities($call, await $request);
  }

  $async.Future<$0.AstCapabilitiesResponse> capabilities(
      $grpc.ServiceCall call, $0.AstCapabilitiesRequest request);

  $async.Stream<$1.ReindexProgress> reindex_Pre($grpc.ServiceCall $call,
      $async.Future<$0.AstReindexRequest> $request) async* {
    yield* reindex($call, await $request);
  }

  $async.Stream<$1.ReindexProgress> reindex(
      $grpc.ServiceCall call, $0.AstReindexRequest request);

  $async.Future<$0.SymbolList> findSymbol_Pre($grpc.ServiceCall $call,
      $async.Future<$0.FindSymbolRequest> $request) async {
    return findSymbol($call, await $request);
  }

  $async.Future<$0.SymbolList> findSymbol(
      $grpc.ServiceCall call, $0.FindSymbolRequest request);

  $async.Future<$0.SymbolList> implementations_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ImplementationsRequest> $request) async {
    return implementations($call, await $request);
  }

  $async.Future<$0.SymbolList> implementations(
      $grpc.ServiceCall call, $0.ImplementationsRequest request);

  $async.Future<$0.SymbolList> interfaceOf_Pre($grpc.ServiceCall $call,
      $async.Future<$0.InterfaceOfRequest> $request) async {
    return interfaceOf($call, await $request);
  }

  $async.Future<$0.SymbolList> interfaceOf(
      $grpc.ServiceCall call, $0.InterfaceOfRequest request);

  $async.Future<$0.AstCallGraph> callers_Pre($grpc.ServiceCall $call,
      $async.Future<$0.CallersRequest> $request) async {
    return callers($call, await $request);
  }

  $async.Future<$0.AstCallGraph> callers(
      $grpc.ServiceCall call, $0.CallersRequest request);

  $async.Future<$0.AstCallGraph> callees_Pre($grpc.ServiceCall $call,
      $async.Future<$0.CalleesRequest> $request) async {
    return callees($call, await $request);
  }

  $async.Future<$0.AstCallGraph> callees(
      $grpc.ServiceCall call, $0.CalleesRequest request);

  $async.Future<$0.CallchainResponse> callchain_Pre($grpc.ServiceCall $call,
      $async.Future<$0.CallchainRequest> $request) async {
    return callchain($call, await $request);
  }

  $async.Future<$0.CallchainResponse> callchain(
      $grpc.ServiceCall call, $0.CallchainRequest request);

  $async.Future<$0.AstCallGraph> blastRadius_Pre($grpc.ServiceCall $call,
      $async.Future<$0.BlastRadiusRequest> $request) async {
    return blastRadius($call, await $request);
  }

  $async.Future<$0.AstCallGraph> blastRadius(
      $grpc.ServiceCall call, $0.BlastRadiusRequest request);

  $async.Future<$0.DependencyPathResponse> dependencyPath_Pre(
      $grpc.ServiceCall $call,
      $async.Future<$0.DependencyPathRequest> $request) async {
    return dependencyPath($call, await $request);
  }

  $async.Future<$0.DependencyPathResponse> dependencyPath(
      $grpc.ServiceCall call, $0.DependencyPathRequest request);
}
