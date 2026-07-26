// This is a generated file - do not edit.
//
// Generated from agent/v1/forge.proto.

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

import 'forge.pb.dart' as $0;

export 'forge.pb.dart';

@$pb.GrpcServiceName('agent.v1.ForgeService')
class ForgeServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  ForgeServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.ForgePullRequest> getPr(
    $0.ForgeNumber request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$getPr, request, options: options);
  }

  $grpc.ResponseFuture<$0.ForgePrPage> listPrs(
    $0.ForgePage request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$listPrs, request, options: options);
  }

  $grpc.ResponseFuture<$0.ForgeIssuePage> listIssues(
    $0.ForgePage request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$listIssues, request, options: options);
  }

  $grpc.ResponseFuture<$0.ForgeIssue> importIssue(
    $0.ForgeNumber request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$importIssue, request, options: options);
  }

  $grpc.ResponseFuture<$0.ForgePullRequest> createPr(
    $0.ForgeCreatePrRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$createPr, request, options: options);
  }

  $grpc.ResponseFuture<$0.ForgeComment> comment(
    $0.ForgeCommentRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$comment, request, options: options);
  }

  $grpc.ResponseFuture<$0.ForgeComment> reviewPr(
    $0.ForgeReviewRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$reviewPr, request, options: options);
  }

  // method descriptors

  static final _$getPr =
      $grpc.ClientMethod<$0.ForgeNumber, $0.ForgePullRequest>(
          '/agent.v1.ForgeService/GetPr',
          ($0.ForgeNumber value) => value.writeToBuffer(),
          $0.ForgePullRequest.fromBuffer);
  static final _$listPrs = $grpc.ClientMethod<$0.ForgePage, $0.ForgePrPage>(
      '/agent.v1.ForgeService/ListPrs',
      ($0.ForgePage value) => value.writeToBuffer(),
      $0.ForgePrPage.fromBuffer);
  static final _$listIssues =
      $grpc.ClientMethod<$0.ForgePage, $0.ForgeIssuePage>(
          '/agent.v1.ForgeService/ListIssues',
          ($0.ForgePage value) => value.writeToBuffer(),
          $0.ForgeIssuePage.fromBuffer);
  static final _$importIssue =
      $grpc.ClientMethod<$0.ForgeNumber, $0.ForgeIssue>(
          '/agent.v1.ForgeService/ImportIssue',
          ($0.ForgeNumber value) => value.writeToBuffer(),
          $0.ForgeIssue.fromBuffer);
  static final _$createPr =
      $grpc.ClientMethod<$0.ForgeCreatePrRequest, $0.ForgePullRequest>(
          '/agent.v1.ForgeService/CreatePr',
          ($0.ForgeCreatePrRequest value) => value.writeToBuffer(),
          $0.ForgePullRequest.fromBuffer);
  static final _$comment =
      $grpc.ClientMethod<$0.ForgeCommentRequest, $0.ForgeComment>(
          '/agent.v1.ForgeService/Comment',
          ($0.ForgeCommentRequest value) => value.writeToBuffer(),
          $0.ForgeComment.fromBuffer);
  static final _$reviewPr =
      $grpc.ClientMethod<$0.ForgeReviewRequest, $0.ForgeComment>(
          '/agent.v1.ForgeService/ReviewPr',
          ($0.ForgeReviewRequest value) => value.writeToBuffer(),
          $0.ForgeComment.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.ForgeService')
abstract class ForgeServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.ForgeService';

  ForgeServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.ForgeNumber, $0.ForgePullRequest>(
        'GetPr',
        getPr_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ForgeNumber.fromBuffer(value),
        ($0.ForgePullRequest value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ForgePage, $0.ForgePrPage>(
        'ListPrs',
        listPrs_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ForgePage.fromBuffer(value),
        ($0.ForgePrPage value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ForgePage, $0.ForgeIssuePage>(
        'ListIssues',
        listIssues_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ForgePage.fromBuffer(value),
        ($0.ForgeIssuePage value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ForgeNumber, $0.ForgeIssue>(
        'ImportIssue',
        importIssue_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ForgeNumber.fromBuffer(value),
        ($0.ForgeIssue value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.ForgeCreatePrRequest, $0.ForgePullRequest>(
            'CreatePr',
            createPr_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.ForgeCreatePrRequest.fromBuffer(value),
            ($0.ForgePullRequest value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ForgeCommentRequest, $0.ForgeComment>(
        'Comment',
        comment_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.ForgeCommentRequest.fromBuffer(value),
        ($0.ForgeComment value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ForgeReviewRequest, $0.ForgeComment>(
        'ReviewPr',
        reviewPr_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.ForgeReviewRequest.fromBuffer(value),
        ($0.ForgeComment value) => value.writeToBuffer()));
  }

  $async.Future<$0.ForgePullRequest> getPr_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.ForgeNumber> $request) async {
    return getPr($call, await $request);
  }

  $async.Future<$0.ForgePullRequest> getPr(
      $grpc.ServiceCall call, $0.ForgeNumber request);

  $async.Future<$0.ForgePrPage> listPrs_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.ForgePage> $request) async {
    return listPrs($call, await $request);
  }

  $async.Future<$0.ForgePrPage> listPrs(
      $grpc.ServiceCall call, $0.ForgePage request);

  $async.Future<$0.ForgeIssuePage> listIssues_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.ForgePage> $request) async {
    return listIssues($call, await $request);
  }

  $async.Future<$0.ForgeIssuePage> listIssues(
      $grpc.ServiceCall call, $0.ForgePage request);

  $async.Future<$0.ForgeIssue> importIssue_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.ForgeNumber> $request) async {
    return importIssue($call, await $request);
  }

  $async.Future<$0.ForgeIssue> importIssue(
      $grpc.ServiceCall call, $0.ForgeNumber request);

  $async.Future<$0.ForgePullRequest> createPr_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ForgeCreatePrRequest> $request) async {
    return createPr($call, await $request);
  }

  $async.Future<$0.ForgePullRequest> createPr(
      $grpc.ServiceCall call, $0.ForgeCreatePrRequest request);

  $async.Future<$0.ForgeComment> comment_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ForgeCommentRequest> $request) async {
    return comment($call, await $request);
  }

  $async.Future<$0.ForgeComment> comment(
      $grpc.ServiceCall call, $0.ForgeCommentRequest request);

  $async.Future<$0.ForgeComment> reviewPr_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ForgeReviewRequest> $request) async {
    return reviewPr($call, await $request);
  }

  $async.Future<$0.ForgeComment> reviewPr(
      $grpc.ServiceCall call, $0.ForgeReviewRequest request);
}

@$pb.GrpcServiceName('agent.v1.TaskService')
class TaskServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  TaskServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.TaskList> write(
    $0.TaskWriteRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$write, request, options: options);
  }

  $grpc.ResponseFuture<$0.TaskList> update(
    $0.TaskUpdateRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$update, request, options: options);
  }

  $grpc.ResponseFuture<$0.TaskList> list(
    $0.TaskListRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$list, request, options: options);
  }

  $grpc.ResponseFuture<$0.TaskClearResponse> clear(
    $0.TaskClearRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$clear, request, options: options);
  }

  // method descriptors

  static final _$write = $grpc.ClientMethod<$0.TaskWriteRequest, $0.TaskList>(
      '/agent.v1.TaskService/Write',
      ($0.TaskWriteRequest value) => value.writeToBuffer(),
      $0.TaskList.fromBuffer);
  static final _$update = $grpc.ClientMethod<$0.TaskUpdateRequest, $0.TaskList>(
      '/agent.v1.TaskService/Update',
      ($0.TaskUpdateRequest value) => value.writeToBuffer(),
      $0.TaskList.fromBuffer);
  static final _$list = $grpc.ClientMethod<$0.TaskListRequest, $0.TaskList>(
      '/agent.v1.TaskService/List',
      ($0.TaskListRequest value) => value.writeToBuffer(),
      $0.TaskList.fromBuffer);
  static final _$clear =
      $grpc.ClientMethod<$0.TaskClearRequest, $0.TaskClearResponse>(
          '/agent.v1.TaskService/Clear',
          ($0.TaskClearRequest value) => value.writeToBuffer(),
          $0.TaskClearResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.TaskService')
abstract class TaskServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.TaskService';

  TaskServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.TaskWriteRequest, $0.TaskList>(
        'Write',
        write_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.TaskWriteRequest.fromBuffer(value),
        ($0.TaskList value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.TaskUpdateRequest, $0.TaskList>(
        'Update',
        update_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.TaskUpdateRequest.fromBuffer(value),
        ($0.TaskList value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.TaskListRequest, $0.TaskList>(
        'List',
        list_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.TaskListRequest.fromBuffer(value),
        ($0.TaskList value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.TaskClearRequest, $0.TaskClearResponse>(
        'Clear',
        clear_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.TaskClearRequest.fromBuffer(value),
        ($0.TaskClearResponse value) => value.writeToBuffer()));
  }

  $async.Future<$0.TaskList> write_Pre($grpc.ServiceCall $call,
      $async.Future<$0.TaskWriteRequest> $request) async {
    return write($call, await $request);
  }

  $async.Future<$0.TaskList> write(
      $grpc.ServiceCall call, $0.TaskWriteRequest request);

  $async.Future<$0.TaskList> update_Pre($grpc.ServiceCall $call,
      $async.Future<$0.TaskUpdateRequest> $request) async {
    return update($call, await $request);
  }

  $async.Future<$0.TaskList> update(
      $grpc.ServiceCall call, $0.TaskUpdateRequest request);

  $async.Future<$0.TaskList> list_Pre($grpc.ServiceCall $call,
      $async.Future<$0.TaskListRequest> $request) async {
    return list($call, await $request);
  }

  $async.Future<$0.TaskList> list(
      $grpc.ServiceCall call, $0.TaskListRequest request);

  $async.Future<$0.TaskClearResponse> clear_Pre($grpc.ServiceCall $call,
      $async.Future<$0.TaskClearRequest> $request) async {
    return clear($call, await $request);
  }

  $async.Future<$0.TaskClearResponse> clear(
      $grpc.ServiceCall call, $0.TaskClearRequest request);
}
