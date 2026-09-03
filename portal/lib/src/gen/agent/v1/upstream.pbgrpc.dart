// This is a generated file - do not edit.
//
// Generated from agent/v1/upstream.proto.

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

import 'upstream.pb.dart' as $0;

export 'upstream.pb.dart';

/// The registry control plane (mirrors PromptService / SchedulerService: one
/// process holds the fleet while any number of agents drive it).
@$pb.GrpcServiceName('agent.v1.ProviderRegistryService')
class ProviderRegistryServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  ProviderRegistryServiceClient(super.channel,
      {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.UpstreamList> list(
    $0.UpstreamListRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$list, request, options: options);
  }

  $grpc.ResponseFuture<$0.Upstream> get(
    $0.UpstreamRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$get, request, options: options);
  }

  $grpc.ResponseFuture<$0.Upstream> put(
    $0.Upstream request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$put, request, options: options);
  }

  $grpc.ResponseFuture<$0.UpstreamDeleteReply> delete(
    $0.UpstreamRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$delete, request, options: options);
  }

  $grpc.ResponseFuture<$0.Upstream> enable(
    $0.UpstreamEnableRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$enable, request, options: options);
  }

  $grpc.ResponseFuture<$0.RoutePolicy> getPolicy(
    $0.RoutePolicyRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$getPolicy, request, options: options);
  }

  $grpc.ResponseFuture<$0.RoutePolicy> putPolicy(
    $0.RoutePolicy request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$putPolicy, request, options: options);
  }

  $grpc.ResponseFuture<$0.RouteDecision> route(
    $0.RouteRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$route, request, options: options);
  }

  $grpc.ResponseFuture<$0.UpstreamHealthList> health(
    $0.UpstreamHealthRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$health, request, options: options);
  }

  // method descriptors

  static final _$list =
      $grpc.ClientMethod<$0.UpstreamListRequest, $0.UpstreamList>(
          '/agent.v1.ProviderRegistryService/List',
          ($0.UpstreamListRequest value) => value.writeToBuffer(),
          $0.UpstreamList.fromBuffer);
  static final _$get = $grpc.ClientMethod<$0.UpstreamRef, $0.Upstream>(
      '/agent.v1.ProviderRegistryService/Get',
      ($0.UpstreamRef value) => value.writeToBuffer(),
      $0.Upstream.fromBuffer);
  static final _$put = $grpc.ClientMethod<$0.Upstream, $0.Upstream>(
      '/agent.v1.ProviderRegistryService/Put',
      ($0.Upstream value) => value.writeToBuffer(),
      $0.Upstream.fromBuffer);
  static final _$delete =
      $grpc.ClientMethod<$0.UpstreamRef, $0.UpstreamDeleteReply>(
          '/agent.v1.ProviderRegistryService/Delete',
          ($0.UpstreamRef value) => value.writeToBuffer(),
          $0.UpstreamDeleteReply.fromBuffer);
  static final _$enable =
      $grpc.ClientMethod<$0.UpstreamEnableRequest, $0.Upstream>(
          '/agent.v1.ProviderRegistryService/Enable',
          ($0.UpstreamEnableRequest value) => value.writeToBuffer(),
          $0.Upstream.fromBuffer);
  static final _$getPolicy =
      $grpc.ClientMethod<$0.RoutePolicyRef, $0.RoutePolicy>(
          '/agent.v1.ProviderRegistryService/GetPolicy',
          ($0.RoutePolicyRef value) => value.writeToBuffer(),
          $0.RoutePolicy.fromBuffer);
  static final _$putPolicy = $grpc.ClientMethod<$0.RoutePolicy, $0.RoutePolicy>(
      '/agent.v1.ProviderRegistryService/PutPolicy',
      ($0.RoutePolicy value) => value.writeToBuffer(),
      $0.RoutePolicy.fromBuffer);
  static final _$route = $grpc.ClientMethod<$0.RouteRequest, $0.RouteDecision>(
      '/agent.v1.ProviderRegistryService/Route',
      ($0.RouteRequest value) => value.writeToBuffer(),
      $0.RouteDecision.fromBuffer);
  static final _$health =
      $grpc.ClientMethod<$0.UpstreamHealthRequest, $0.UpstreamHealthList>(
          '/agent.v1.ProviderRegistryService/Health',
          ($0.UpstreamHealthRequest value) => value.writeToBuffer(),
          $0.UpstreamHealthList.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.ProviderRegistryService')
abstract class ProviderRegistryServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.ProviderRegistryService';

  ProviderRegistryServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.UpstreamListRequest, $0.UpstreamList>(
        'List',
        list_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.UpstreamListRequest.fromBuffer(value),
        ($0.UpstreamList value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.UpstreamRef, $0.Upstream>(
        'Get',
        get_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.UpstreamRef.fromBuffer(value),
        ($0.Upstream value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.Upstream, $0.Upstream>(
        'Put',
        put_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.Upstream.fromBuffer(value),
        ($0.Upstream value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.UpstreamRef, $0.UpstreamDeleteReply>(
        'Delete',
        delete_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.UpstreamRef.fromBuffer(value),
        ($0.UpstreamDeleteReply value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.UpstreamEnableRequest, $0.Upstream>(
        'Enable',
        enable_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.UpstreamEnableRequest.fromBuffer(value),
        ($0.Upstream value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.RoutePolicyRef, $0.RoutePolicy>(
        'GetPolicy',
        getPolicy_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.RoutePolicyRef.fromBuffer(value),
        ($0.RoutePolicy value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.RoutePolicy, $0.RoutePolicy>(
        'PutPolicy',
        putPolicy_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.RoutePolicy.fromBuffer(value),
        ($0.RoutePolicy value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.RouteRequest, $0.RouteDecision>(
        'Route',
        route_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.RouteRequest.fromBuffer(value),
        ($0.RouteDecision value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.UpstreamHealthRequest, $0.UpstreamHealthList>(
            'Health',
            health_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.UpstreamHealthRequest.fromBuffer(value),
            ($0.UpstreamHealthList value) => value.writeToBuffer()));
  }

  $async.Future<$0.UpstreamList> list_Pre($grpc.ServiceCall $call,
      $async.Future<$0.UpstreamListRequest> $request) async {
    return list($call, await $request);
  }

  $async.Future<$0.UpstreamList> list(
      $grpc.ServiceCall call, $0.UpstreamListRequest request);

  $async.Future<$0.Upstream> get_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.UpstreamRef> $request) async {
    return get($call, await $request);
  }

  $async.Future<$0.Upstream> get(
      $grpc.ServiceCall call, $0.UpstreamRef request);

  $async.Future<$0.Upstream> put_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.Upstream> $request) async {
    return put($call, await $request);
  }

  $async.Future<$0.Upstream> put($grpc.ServiceCall call, $0.Upstream request);

  $async.Future<$0.UpstreamDeleteReply> delete_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.UpstreamRef> $request) async {
    return delete($call, await $request);
  }

  $async.Future<$0.UpstreamDeleteReply> delete(
      $grpc.ServiceCall call, $0.UpstreamRef request);

  $async.Future<$0.Upstream> enable_Pre($grpc.ServiceCall $call,
      $async.Future<$0.UpstreamEnableRequest> $request) async {
    return enable($call, await $request);
  }

  $async.Future<$0.Upstream> enable(
      $grpc.ServiceCall call, $0.UpstreamEnableRequest request);

  $async.Future<$0.RoutePolicy> getPolicy_Pre($grpc.ServiceCall $call,
      $async.Future<$0.RoutePolicyRef> $request) async {
    return getPolicy($call, await $request);
  }

  $async.Future<$0.RoutePolicy> getPolicy(
      $grpc.ServiceCall call, $0.RoutePolicyRef request);

  $async.Future<$0.RoutePolicy> putPolicy_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.RoutePolicy> $request) async {
    return putPolicy($call, await $request);
  }

  $async.Future<$0.RoutePolicy> putPolicy(
      $grpc.ServiceCall call, $0.RoutePolicy request);

  $async.Future<$0.RouteDecision> route_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.RouteRequest> $request) async {
    return route($call, await $request);
  }

  $async.Future<$0.RouteDecision> route(
      $grpc.ServiceCall call, $0.RouteRequest request);

  $async.Future<$0.UpstreamHealthList> health_Pre($grpc.ServiceCall $call,
      $async.Future<$0.UpstreamHealthRequest> $request) async {
    return health($call, await $request);
  }

  $async.Future<$0.UpstreamHealthList> health(
      $grpc.ServiceCall call, $0.UpstreamHealthRequest request);
}
