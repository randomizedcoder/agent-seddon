// This is a generated file - do not edit.
//
// Generated from agent/v1/scheduler.proto.

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

import 'scheduler.pb.dart' as $0;

export 'scheduler.pb.dart';

@$pb.GrpcServiceName('agent.v1.SchedulerService')
class SchedulerServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  SchedulerServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.SchedJobRef> schedule(
    $0.SchedScheduleRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$schedule, request, options: options);
  }

  $grpc.ResponseFuture<$0.SchedJobList> list(
    $0.SchedListRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$list, request, options: options);
  }

  $grpc.ResponseFuture<$0.SchedCancelResponse> cancel(
    $0.SchedJobRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$cancel, request, options: options);
  }

  $grpc.ResponseFuture<$0.SchedRunList> history(
    $0.SchedJobRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$history, request, options: options);
  }

  // method descriptors

  static final _$schedule =
      $grpc.ClientMethod<$0.SchedScheduleRequest, $0.SchedJobRef>(
          '/agent.v1.SchedulerService/Schedule',
          ($0.SchedScheduleRequest value) => value.writeToBuffer(),
          $0.SchedJobRef.fromBuffer);
  static final _$list =
      $grpc.ClientMethod<$0.SchedListRequest, $0.SchedJobList>(
          '/agent.v1.SchedulerService/List',
          ($0.SchedListRequest value) => value.writeToBuffer(),
          $0.SchedJobList.fromBuffer);
  static final _$cancel =
      $grpc.ClientMethod<$0.SchedJobRef, $0.SchedCancelResponse>(
          '/agent.v1.SchedulerService/Cancel',
          ($0.SchedJobRef value) => value.writeToBuffer(),
          $0.SchedCancelResponse.fromBuffer);
  static final _$history = $grpc.ClientMethod<$0.SchedJobRef, $0.SchedRunList>(
      '/agent.v1.SchedulerService/History',
      ($0.SchedJobRef value) => value.writeToBuffer(),
      $0.SchedRunList.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.SchedulerService')
abstract class SchedulerServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.SchedulerService';

  SchedulerServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.SchedScheduleRequest, $0.SchedJobRef>(
        'Schedule',
        schedule_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.SchedScheduleRequest.fromBuffer(value),
        ($0.SchedJobRef value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.SchedListRequest, $0.SchedJobList>(
        'List',
        list_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.SchedListRequest.fromBuffer(value),
        ($0.SchedJobList value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.SchedJobRef, $0.SchedCancelResponse>(
        'Cancel',
        cancel_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.SchedJobRef.fromBuffer(value),
        ($0.SchedCancelResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.SchedJobRef, $0.SchedRunList>(
        'History',
        history_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.SchedJobRef.fromBuffer(value),
        ($0.SchedRunList value) => value.writeToBuffer()));
  }

  $async.Future<$0.SchedJobRef> schedule_Pre($grpc.ServiceCall $call,
      $async.Future<$0.SchedScheduleRequest> $request) async {
    return schedule($call, await $request);
  }

  $async.Future<$0.SchedJobRef> schedule(
      $grpc.ServiceCall call, $0.SchedScheduleRequest request);

  $async.Future<$0.SchedJobList> list_Pre($grpc.ServiceCall $call,
      $async.Future<$0.SchedListRequest> $request) async {
    return list($call, await $request);
  }

  $async.Future<$0.SchedJobList> list(
      $grpc.ServiceCall call, $0.SchedListRequest request);

  $async.Future<$0.SchedCancelResponse> cancel_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.SchedJobRef> $request) async {
    return cancel($call, await $request);
  }

  $async.Future<$0.SchedCancelResponse> cancel(
      $grpc.ServiceCall call, $0.SchedJobRef request);

  $async.Future<$0.SchedRunList> history_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.SchedJobRef> $request) async {
    return history($call, await $request);
  }

  $async.Future<$0.SchedRunList> history(
      $grpc.ServiceCall call, $0.SchedJobRef request);
}
