// This is a generated file - do not edit.
//
// Generated from agent/v1/session.proto.

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

import 'common.pb.dart' as $1;
import 'session.pb.dart' as $0;

export 'session.pb.dart';

@$pb.GrpcServiceName('agent.v1.SessionService')
class SessionServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  SessionServiceClient(super.channel, {super.options, super.interceptors});

  /// Append an immutable checkpoint; same content + parent + label ⇒ same id.
  $grpc.ResponseFuture<$0.SessionCheckpointRef> checkpoint(
    $0.SessionCheckpointRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$checkpoint, request, options: options);
  }

  /// The branch tree: every checkpoint reachable from any head.
  $grpc.ResponseFuture<$0.SessionCheckpointList> list(
    $0.SessionRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$list, request, options: options);
  }

  /// Rehydrate the working set stored at an id.
  $grpc.ResponseFuture<$1.WorkingSet> restore(
    $0.SessionCheckpointRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$restore, request, options: options);
  }

  /// Create a new branch head off a checkpoint (and switch to it).
  $grpc.ResponseFuture<$0.SessionBranchResponse> branch(
    $0.SessionBranchRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$branch, request, options: options);
  }

  /// Move the current head back n turns; returns the new head id.
  $grpc.ResponseFuture<$0.SessionCheckpointRef> undo(
    $0.SessionUndoRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$undo, request, options: options);
  }

  /// Fork into an independent session sharing the immutable objects.
  $grpc.ResponseFuture<$0.SessionRef> fork(
    $0.SessionRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$fork, request, options: options);
  }

  /// The message/turn delta between two checkpoints.
  $grpc.ResponseFuture<$0.SessionCheckpointDiff> diff(
    $0.SessionDiffRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$diff, request, options: options);
  }

  /// GC checkpoints unreachable from any live head; returns the count reclaimed.
  $grpc.ResponseFuture<$0.SessionPruneResponse> prune(
    $0.SessionRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$prune, request, options: options);
  }

  // method descriptors

  static final _$checkpoint =
      $grpc.ClientMethod<$0.SessionCheckpointRequest, $0.SessionCheckpointRef>(
          '/agent.v1.SessionService/Checkpoint',
          ($0.SessionCheckpointRequest value) => value.writeToBuffer(),
          $0.SessionCheckpointRef.fromBuffer);
  static final _$list =
      $grpc.ClientMethod<$0.SessionRef, $0.SessionCheckpointList>(
          '/agent.v1.SessionService/List',
          ($0.SessionRef value) => value.writeToBuffer(),
          $0.SessionCheckpointList.fromBuffer);
  static final _$restore =
      $grpc.ClientMethod<$0.SessionCheckpointRef, $1.WorkingSet>(
          '/agent.v1.SessionService/Restore',
          ($0.SessionCheckpointRef value) => value.writeToBuffer(),
          $1.WorkingSet.fromBuffer);
  static final _$branch =
      $grpc.ClientMethod<$0.SessionBranchRequest, $0.SessionBranchResponse>(
          '/agent.v1.SessionService/Branch',
          ($0.SessionBranchRequest value) => value.writeToBuffer(),
          $0.SessionBranchResponse.fromBuffer);
  static final _$undo =
      $grpc.ClientMethod<$0.SessionUndoRequest, $0.SessionCheckpointRef>(
          '/agent.v1.SessionService/Undo',
          ($0.SessionUndoRequest value) => value.writeToBuffer(),
          $0.SessionCheckpointRef.fromBuffer);
  static final _$fork = $grpc.ClientMethod<$0.SessionRef, $0.SessionRef>(
      '/agent.v1.SessionService/Fork',
      ($0.SessionRef value) => value.writeToBuffer(),
      $0.SessionRef.fromBuffer);
  static final _$diff =
      $grpc.ClientMethod<$0.SessionDiffRequest, $0.SessionCheckpointDiff>(
          '/agent.v1.SessionService/Diff',
          ($0.SessionDiffRequest value) => value.writeToBuffer(),
          $0.SessionCheckpointDiff.fromBuffer);
  static final _$prune =
      $grpc.ClientMethod<$0.SessionRef, $0.SessionPruneResponse>(
          '/agent.v1.SessionService/Prune',
          ($0.SessionRef value) => value.writeToBuffer(),
          $0.SessionPruneResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.SessionService')
abstract class SessionServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.SessionService';

  SessionServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.SessionCheckpointRequest,
            $0.SessionCheckpointRef>(
        'Checkpoint',
        checkpoint_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.SessionCheckpointRequest.fromBuffer(value),
        ($0.SessionCheckpointRef value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.SessionRef, $0.SessionCheckpointList>(
        'List',
        list_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.SessionRef.fromBuffer(value),
        ($0.SessionCheckpointList value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.SessionCheckpointRef, $1.WorkingSet>(
        'Restore',
        restore_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.SessionCheckpointRef.fromBuffer(value),
        ($1.WorkingSet value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.SessionBranchRequest, $0.SessionBranchResponse>(
            'Branch',
            branch_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.SessionBranchRequest.fromBuffer(value),
            ($0.SessionBranchResponse value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.SessionUndoRequest, $0.SessionCheckpointRef>(
            'Undo',
            undo_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.SessionUndoRequest.fromBuffer(value),
            ($0.SessionCheckpointRef value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.SessionRef, $0.SessionRef>(
        'Fork',
        fork_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.SessionRef.fromBuffer(value),
        ($0.SessionRef value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.SessionDiffRequest, $0.SessionCheckpointDiff>(
            'Diff',
            diff_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.SessionDiffRequest.fromBuffer(value),
            ($0.SessionCheckpointDiff value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.SessionRef, $0.SessionPruneResponse>(
        'Prune',
        prune_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.SessionRef.fromBuffer(value),
        ($0.SessionPruneResponse value) => value.writeToBuffer()));
  }

  $async.Future<$0.SessionCheckpointRef> checkpoint_Pre($grpc.ServiceCall $call,
      $async.Future<$0.SessionCheckpointRequest> $request) async {
    return checkpoint($call, await $request);
  }

  $async.Future<$0.SessionCheckpointRef> checkpoint(
      $grpc.ServiceCall call, $0.SessionCheckpointRequest request);

  $async.Future<$0.SessionCheckpointList> list_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.SessionRef> $request) async {
    return list($call, await $request);
  }

  $async.Future<$0.SessionCheckpointList> list(
      $grpc.ServiceCall call, $0.SessionRef request);

  $async.Future<$1.WorkingSet> restore_Pre($grpc.ServiceCall $call,
      $async.Future<$0.SessionCheckpointRef> $request) async {
    return restore($call, await $request);
  }

  $async.Future<$1.WorkingSet> restore(
      $grpc.ServiceCall call, $0.SessionCheckpointRef request);

  $async.Future<$0.SessionBranchResponse> branch_Pre($grpc.ServiceCall $call,
      $async.Future<$0.SessionBranchRequest> $request) async {
    return branch($call, await $request);
  }

  $async.Future<$0.SessionBranchResponse> branch(
      $grpc.ServiceCall call, $0.SessionBranchRequest request);

  $async.Future<$0.SessionCheckpointRef> undo_Pre($grpc.ServiceCall $call,
      $async.Future<$0.SessionUndoRequest> $request) async {
    return undo($call, await $request);
  }

  $async.Future<$0.SessionCheckpointRef> undo(
      $grpc.ServiceCall call, $0.SessionUndoRequest request);

  $async.Future<$0.SessionRef> fork_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.SessionRef> $request) async {
    return fork($call, await $request);
  }

  $async.Future<$0.SessionRef> fork(
      $grpc.ServiceCall call, $0.SessionRef request);

  $async.Future<$0.SessionCheckpointDiff> diff_Pre($grpc.ServiceCall $call,
      $async.Future<$0.SessionDiffRequest> $request) async {
    return diff($call, await $request);
  }

  $async.Future<$0.SessionCheckpointDiff> diff(
      $grpc.ServiceCall call, $0.SessionDiffRequest request);

  $async.Future<$0.SessionPruneResponse> prune_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.SessionRef> $request) async {
    return prune($call, await $request);
  }

  $async.Future<$0.SessionPruneResponse> prune(
      $grpc.ServiceCall call, $0.SessionRef request);
}
