// This is a generated file - do not edit.
//
// Generated from agent/v1/review_fleet.proto.

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

import 'review_fleet.pb.dart' as $0;

export 'review_fleet.pb.dart';

/// The roster control plane (mirrors ProviderRegistryService: one process holds
/// the roster while any number of clients drive it). `token_ref` rides as a
/// reference; the server never resolves it.
@$pb.GrpcServiceName('agent.v1.ReviewFleetService')
class ReviewFleetServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  ReviewFleetServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.FleetSessionList> list(
    $0.FleetListRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$list, request, options: options);
  }

  $grpc.ResponseFuture<$0.FleetSession> get(
    $0.FleetSessionRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$get, request, options: options);
  }

  $grpc.ResponseFuture<$0.FleetSession> put(
    $0.FleetSession request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$put, request, options: options);
  }

  $grpc.ResponseFuture<$0.FleetDeleteReply> delete(
    $0.FleetSessionRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$delete, request, options: options);
  }

  $grpc.ResponseFuture<$0.FleetSession> setEnabled(
    $0.FleetSetEnabledRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$setEnabled, request, options: options);
  }

  /// Manually queue a review (C8). Opt-in: served only by the full `--serve-fleet`
  /// process (which has an orchestrator); the bare control-plane endpoint returns
  /// UNIMPLEMENTED.
  $grpc.ResponseFuture<$0.ReviewNowReply> reviewNow(
    $0.ReviewNowRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$reviewNow, request, options: options);
  }

  /// Approve a persisted draft → post it to its forge (C17). Opt-in: served only by the
  /// full `--serve-fleet` process with persisted history wired (the approver needs to
  /// look the draft up + build the row's forge); the bare control plane, or a process
  /// with no history, returns UNIMPLEMENTED. This is the human approval gesture — the
  /// ONLY path that posts; the model's own forge stays read-only (`dry_run`).
  $grpc.ResponseFuture<$0.ApproveReply> approve(
    $0.ApproveRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$approve, request, options: options);
  }

  /// List persisted review drafts (C14), newest state per review. Opt-in: served only
  /// by a process with persisted fleet history wired (the `--serve-fleet` process); a
  /// process with no history returns UNIMPLEMENTED. Read-only.
  $grpc.ResponseFuture<$0.ListReviewsReply> listReviews(
    $0.ListReviewsRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$listReviews, request, options: options);
  }

  /// Fetch one draft's metadata + rendered markdown body (C14). Opt-in: needs the
  /// history reader (the `--serve-fleet` process); else UNIMPLEMENTED. Read-only.
  $grpc.ResponseFuture<$0.GetReviewReply> getReview(
    $0.GetReviewRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$getReview, request, options: options);
  }

  /// Rewrite a draft's markdown body (C14, the portal's edit). Opt-in: needs the draft
  /// editor (the `--serve-fleet` process); else UNIMPLEMENTED. A `posted`/`approved` draft
  /// is locked. This edits the local draft only — it never posts; posting stays the
  /// separate `Approve` gesture.
  $grpc.ResponseFuture<$0.UpdateReviewReply> updateReview(
    $0.UpdateReviewRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$updateReview, request, options: options);
  }

  /// Operational self-diagnosis (docs/design/doctor/). Opt-in: served only by the
  /// full `--serve-fleet` process (which has the config to build the probes); the
  /// bare control-plane endpoint returns UNIMPLEMENTED.
  $grpc.ResponseFuture<$0.PreflightReply> preflight(
    $0.PreflightRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$preflight, request, options: options);
  }

  // method descriptors

  static final _$list =
      $grpc.ClientMethod<$0.FleetListRequest, $0.FleetSessionList>(
          '/agent.v1.ReviewFleetService/List',
          ($0.FleetListRequest value) => value.writeToBuffer(),
          $0.FleetSessionList.fromBuffer);
  static final _$get = $grpc.ClientMethod<$0.FleetSessionRef, $0.FleetSession>(
      '/agent.v1.ReviewFleetService/Get',
      ($0.FleetSessionRef value) => value.writeToBuffer(),
      $0.FleetSession.fromBuffer);
  static final _$put = $grpc.ClientMethod<$0.FleetSession, $0.FleetSession>(
      '/agent.v1.ReviewFleetService/Put',
      ($0.FleetSession value) => value.writeToBuffer(),
      $0.FleetSession.fromBuffer);
  static final _$delete =
      $grpc.ClientMethod<$0.FleetSessionRef, $0.FleetDeleteReply>(
          '/agent.v1.ReviewFleetService/Delete',
          ($0.FleetSessionRef value) => value.writeToBuffer(),
          $0.FleetDeleteReply.fromBuffer);
  static final _$setEnabled =
      $grpc.ClientMethod<$0.FleetSetEnabledRequest, $0.FleetSession>(
          '/agent.v1.ReviewFleetService/SetEnabled',
          ($0.FleetSetEnabledRequest value) => value.writeToBuffer(),
          $0.FleetSession.fromBuffer);
  static final _$reviewNow =
      $grpc.ClientMethod<$0.ReviewNowRequest, $0.ReviewNowReply>(
          '/agent.v1.ReviewFleetService/ReviewNow',
          ($0.ReviewNowRequest value) => value.writeToBuffer(),
          $0.ReviewNowReply.fromBuffer);
  static final _$approve =
      $grpc.ClientMethod<$0.ApproveRequest, $0.ApproveReply>(
          '/agent.v1.ReviewFleetService/Approve',
          ($0.ApproveRequest value) => value.writeToBuffer(),
          $0.ApproveReply.fromBuffer);
  static final _$listReviews =
      $grpc.ClientMethod<$0.ListReviewsRequest, $0.ListReviewsReply>(
          '/agent.v1.ReviewFleetService/ListReviews',
          ($0.ListReviewsRequest value) => value.writeToBuffer(),
          $0.ListReviewsReply.fromBuffer);
  static final _$getReview =
      $grpc.ClientMethod<$0.GetReviewRequest, $0.GetReviewReply>(
          '/agent.v1.ReviewFleetService/GetReview',
          ($0.GetReviewRequest value) => value.writeToBuffer(),
          $0.GetReviewReply.fromBuffer);
  static final _$updateReview =
      $grpc.ClientMethod<$0.UpdateReviewRequest, $0.UpdateReviewReply>(
          '/agent.v1.ReviewFleetService/UpdateReview',
          ($0.UpdateReviewRequest value) => value.writeToBuffer(),
          $0.UpdateReviewReply.fromBuffer);
  static final _$preflight =
      $grpc.ClientMethod<$0.PreflightRequest, $0.PreflightReply>(
          '/agent.v1.ReviewFleetService/Preflight',
          ($0.PreflightRequest value) => value.writeToBuffer(),
          $0.PreflightReply.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.ReviewFleetService')
abstract class ReviewFleetServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.ReviewFleetService';

  ReviewFleetServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.FleetListRequest, $0.FleetSessionList>(
        'List',
        list_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.FleetListRequest.fromBuffer(value),
        ($0.FleetSessionList value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.FleetSessionRef, $0.FleetSession>(
        'Get',
        get_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.FleetSessionRef.fromBuffer(value),
        ($0.FleetSession value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.FleetSession, $0.FleetSession>(
        'Put',
        put_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.FleetSession.fromBuffer(value),
        ($0.FleetSession value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.FleetSessionRef, $0.FleetDeleteReply>(
        'Delete',
        delete_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.FleetSessionRef.fromBuffer(value),
        ($0.FleetDeleteReply value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.FleetSetEnabledRequest, $0.FleetSession>(
        'SetEnabled',
        setEnabled_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.FleetSetEnabledRequest.fromBuffer(value),
        ($0.FleetSession value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ReviewNowRequest, $0.ReviewNowReply>(
        'ReviewNow',
        reviewNow_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ReviewNowRequest.fromBuffer(value),
        ($0.ReviewNowReply value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ApproveRequest, $0.ApproveReply>(
        'Approve',
        approve_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ApproveRequest.fromBuffer(value),
        ($0.ApproveReply value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ListReviewsRequest, $0.ListReviewsReply>(
        'ListReviews',
        listReviews_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.ListReviewsRequest.fromBuffer(value),
        ($0.ListReviewsReply value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.GetReviewRequest, $0.GetReviewReply>(
        'GetReview',
        getReview_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.GetReviewRequest.fromBuffer(value),
        ($0.GetReviewReply value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.UpdateReviewRequest, $0.UpdateReviewReply>(
            'UpdateReview',
            updateReview_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.UpdateReviewRequest.fromBuffer(value),
            ($0.UpdateReviewReply value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PreflightRequest, $0.PreflightReply>(
        'Preflight',
        preflight_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PreflightRequest.fromBuffer(value),
        ($0.PreflightReply value) => value.writeToBuffer()));
  }

  $async.Future<$0.FleetSessionList> list_Pre($grpc.ServiceCall $call,
      $async.Future<$0.FleetListRequest> $request) async {
    return list($call, await $request);
  }

  $async.Future<$0.FleetSessionList> list(
      $grpc.ServiceCall call, $0.FleetListRequest request);

  $async.Future<$0.FleetSession> get_Pre($grpc.ServiceCall $call,
      $async.Future<$0.FleetSessionRef> $request) async {
    return get($call, await $request);
  }

  $async.Future<$0.FleetSession> get(
      $grpc.ServiceCall call, $0.FleetSessionRef request);

  $async.Future<$0.FleetSession> put_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.FleetSession> $request) async {
    return put($call, await $request);
  }

  $async.Future<$0.FleetSession> put(
      $grpc.ServiceCall call, $0.FleetSession request);

  $async.Future<$0.FleetDeleteReply> delete_Pre($grpc.ServiceCall $call,
      $async.Future<$0.FleetSessionRef> $request) async {
    return delete($call, await $request);
  }

  $async.Future<$0.FleetDeleteReply> delete(
      $grpc.ServiceCall call, $0.FleetSessionRef request);

  $async.Future<$0.FleetSession> setEnabled_Pre($grpc.ServiceCall $call,
      $async.Future<$0.FleetSetEnabledRequest> $request) async {
    return setEnabled($call, await $request);
  }

  $async.Future<$0.FleetSession> setEnabled(
      $grpc.ServiceCall call, $0.FleetSetEnabledRequest request);

  $async.Future<$0.ReviewNowReply> reviewNow_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ReviewNowRequest> $request) async {
    return reviewNow($call, await $request);
  }

  $async.Future<$0.ReviewNowReply> reviewNow(
      $grpc.ServiceCall call, $0.ReviewNowRequest request);

  $async.Future<$0.ApproveReply> approve_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ApproveRequest> $request) async {
    return approve($call, await $request);
  }

  $async.Future<$0.ApproveReply> approve(
      $grpc.ServiceCall call, $0.ApproveRequest request);

  $async.Future<$0.ListReviewsReply> listReviews_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ListReviewsRequest> $request) async {
    return listReviews($call, await $request);
  }

  $async.Future<$0.ListReviewsReply> listReviews(
      $grpc.ServiceCall call, $0.ListReviewsRequest request);

  $async.Future<$0.GetReviewReply> getReview_Pre($grpc.ServiceCall $call,
      $async.Future<$0.GetReviewRequest> $request) async {
    return getReview($call, await $request);
  }

  $async.Future<$0.GetReviewReply> getReview(
      $grpc.ServiceCall call, $0.GetReviewRequest request);

  $async.Future<$0.UpdateReviewReply> updateReview_Pre($grpc.ServiceCall $call,
      $async.Future<$0.UpdateReviewRequest> $request) async {
    return updateReview($call, await $request);
  }

  $async.Future<$0.UpdateReviewReply> updateReview(
      $grpc.ServiceCall call, $0.UpdateReviewRequest request);

  $async.Future<$0.PreflightReply> preflight_Pre($grpc.ServiceCall $call,
      $async.Future<$0.PreflightRequest> $request) async {
    return preflight($call, await $request);
  }

  $async.Future<$0.PreflightReply> preflight(
      $grpc.ServiceCall call, $0.PreflightRequest request);
}
