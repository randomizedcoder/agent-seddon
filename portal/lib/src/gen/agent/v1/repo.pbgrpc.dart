// This is a generated file - do not edit.
//
// Generated from agent/v1/repo.proto.

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

import 'repo.pb.dart' as $0;

export 'repo.pb.dart';

@$pb.GrpcServiceName('agent.v1.RepoService')
class RepoServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  RepoServiceClient(super.channel, {super.options, super.interceptors});

  /// Object-level, read-only, revision-addressed.
  $grpc.ResponseFuture<$0.ResolveResponse> resolve(
    $0.ResolveRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$resolve, request, options: options);
  }

  $grpc.ResponseFuture<$0.BlobContent> readFile(
    $0.ReadFileRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$readFile, request, options: options);
  }

  $grpc.ResponseFuture<$0.ListTreeResponse> listTree(
    $0.ListTreeRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$listTree, request, options: options);
  }

  $grpc.ResponseFuture<$0.DiffResult> diff(
    $0.DiffRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$diff, request, options: options);
  }

  $grpc.ResponseFuture<$0.GrepResponse> grep(
    $0.GrepRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$grep, request, options: options);
  }

  $grpc.ResponseFuture<$0.LogResponse> log(
    $0.LogRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$log, request, options: options);
  }

  $grpc.ResponseFuture<$0.BranchesResponse> branches(
    $0.BranchesRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$branches, request, options: options);
  }

  /// Mirror / worktree / ref lifecycle.
  $grpc.ResponseFuture<$0.RepoStatus> status(
    $0.RepoStatusRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$status, request, options: options);
  }

  $grpc.ResponseFuture<$0.RepoStatus> fetch(
    $0.FetchRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$fetch, request, options: options);
  }

  $grpc.ResponseFuture<$0.WorktreeHandle> worktreeAdd(
    $0.WorktreeSpec request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$worktreeAdd, request, options: options);
  }

  $grpc.ResponseFuture<$0.WorktreeListResponse> worktreeList(
    $0.WorktreeListRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$worktreeList, request, options: options);
  }

  $grpc.ResponseFuture<$0.WorktreeRemoveResponse> worktreeRemove(
    $0.WorktreeRemoveRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$worktreeRemove, request, options: options);
  }

  /// Named `CreateCheckpoint` (not `Checkpoint`) so the method name doesn't shadow
  /// the `Checkpoint` message type when protoc resolves the return type.
  $grpc.ResponseFuture<$0.Checkpoint> createCheckpoint(
    $0.CheckpointRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$createCheckpoint, request, options: options);
  }

  $grpc.ResponseFuture<$0.PushResponse> push(
    $0.PushRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$push, request, options: options);
  }

  // method descriptors

  static final _$resolve =
      $grpc.ClientMethod<$0.ResolveRequest, $0.ResolveResponse>(
          '/agent.v1.RepoService/Resolve',
          ($0.ResolveRequest value) => value.writeToBuffer(),
          $0.ResolveResponse.fromBuffer);
  static final _$readFile =
      $grpc.ClientMethod<$0.ReadFileRequest, $0.BlobContent>(
          '/agent.v1.RepoService/ReadFile',
          ($0.ReadFileRequest value) => value.writeToBuffer(),
          $0.BlobContent.fromBuffer);
  static final _$listTree =
      $grpc.ClientMethod<$0.ListTreeRequest, $0.ListTreeResponse>(
          '/agent.v1.RepoService/ListTree',
          ($0.ListTreeRequest value) => value.writeToBuffer(),
          $0.ListTreeResponse.fromBuffer);
  static final _$diff = $grpc.ClientMethod<$0.DiffRequest, $0.DiffResult>(
      '/agent.v1.RepoService/Diff',
      ($0.DiffRequest value) => value.writeToBuffer(),
      $0.DiffResult.fromBuffer);
  static final _$grep = $grpc.ClientMethod<$0.GrepRequest, $0.GrepResponse>(
      '/agent.v1.RepoService/Grep',
      ($0.GrepRequest value) => value.writeToBuffer(),
      $0.GrepResponse.fromBuffer);
  static final _$log = $grpc.ClientMethod<$0.LogRequest, $0.LogResponse>(
      '/agent.v1.RepoService/Log',
      ($0.LogRequest value) => value.writeToBuffer(),
      $0.LogResponse.fromBuffer);
  static final _$branches =
      $grpc.ClientMethod<$0.BranchesRequest, $0.BranchesResponse>(
          '/agent.v1.RepoService/Branches',
          ($0.BranchesRequest value) => value.writeToBuffer(),
          $0.BranchesResponse.fromBuffer);
  static final _$status =
      $grpc.ClientMethod<$0.RepoStatusRequest, $0.RepoStatus>(
          '/agent.v1.RepoService/Status',
          ($0.RepoStatusRequest value) => value.writeToBuffer(),
          $0.RepoStatus.fromBuffer);
  static final _$fetch = $grpc.ClientMethod<$0.FetchRequest, $0.RepoStatus>(
      '/agent.v1.RepoService/Fetch',
      ($0.FetchRequest value) => value.writeToBuffer(),
      $0.RepoStatus.fromBuffer);
  static final _$worktreeAdd =
      $grpc.ClientMethod<$0.WorktreeSpec, $0.WorktreeHandle>(
          '/agent.v1.RepoService/WorktreeAdd',
          ($0.WorktreeSpec value) => value.writeToBuffer(),
          $0.WorktreeHandle.fromBuffer);
  static final _$worktreeList =
      $grpc.ClientMethod<$0.WorktreeListRequest, $0.WorktreeListResponse>(
          '/agent.v1.RepoService/WorktreeList',
          ($0.WorktreeListRequest value) => value.writeToBuffer(),
          $0.WorktreeListResponse.fromBuffer);
  static final _$worktreeRemove =
      $grpc.ClientMethod<$0.WorktreeRemoveRequest, $0.WorktreeRemoveResponse>(
          '/agent.v1.RepoService/WorktreeRemove',
          ($0.WorktreeRemoveRequest value) => value.writeToBuffer(),
          $0.WorktreeRemoveResponse.fromBuffer);
  static final _$createCheckpoint =
      $grpc.ClientMethod<$0.CheckpointRequest, $0.Checkpoint>(
          '/agent.v1.RepoService/CreateCheckpoint',
          ($0.CheckpointRequest value) => value.writeToBuffer(),
          $0.Checkpoint.fromBuffer);
  static final _$push = $grpc.ClientMethod<$0.PushRequest, $0.PushResponse>(
      '/agent.v1.RepoService/Push',
      ($0.PushRequest value) => value.writeToBuffer(),
      $0.PushResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.RepoService')
abstract class RepoServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.RepoService';

  RepoServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.ResolveRequest, $0.ResolveResponse>(
        'Resolve',
        resolve_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ResolveRequest.fromBuffer(value),
        ($0.ResolveResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ReadFileRequest, $0.BlobContent>(
        'ReadFile',
        readFile_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ReadFileRequest.fromBuffer(value),
        ($0.BlobContent value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ListTreeRequest, $0.ListTreeResponse>(
        'ListTree',
        listTree_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ListTreeRequest.fromBuffer(value),
        ($0.ListTreeResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.DiffRequest, $0.DiffResult>(
        'Diff',
        diff_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.DiffRequest.fromBuffer(value),
        ($0.DiffResult value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.GrepRequest, $0.GrepResponse>(
        'Grep',
        grep_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.GrepRequest.fromBuffer(value),
        ($0.GrepResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.LogRequest, $0.LogResponse>(
        'Log',
        log_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.LogRequest.fromBuffer(value),
        ($0.LogResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.BranchesRequest, $0.BranchesResponse>(
        'Branches',
        branches_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.BranchesRequest.fromBuffer(value),
        ($0.BranchesResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.RepoStatusRequest, $0.RepoStatus>(
        'Status',
        status_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.RepoStatusRequest.fromBuffer(value),
        ($0.RepoStatus value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.FetchRequest, $0.RepoStatus>(
        'Fetch',
        fetch_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.FetchRequest.fromBuffer(value),
        ($0.RepoStatus value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.WorktreeSpec, $0.WorktreeHandle>(
        'WorktreeAdd',
        worktreeAdd_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.WorktreeSpec.fromBuffer(value),
        ($0.WorktreeHandle value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.WorktreeListRequest, $0.WorktreeListResponse>(
            'WorktreeList',
            worktreeList_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.WorktreeListRequest.fromBuffer(value),
            ($0.WorktreeListResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.WorktreeRemoveRequest,
            $0.WorktreeRemoveResponse>(
        'WorktreeRemove',
        worktreeRemove_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.WorktreeRemoveRequest.fromBuffer(value),
        ($0.WorktreeRemoveResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.CheckpointRequest, $0.Checkpoint>(
        'CreateCheckpoint',
        createCheckpoint_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.CheckpointRequest.fromBuffer(value),
        ($0.Checkpoint value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PushRequest, $0.PushResponse>(
        'Push',
        push_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PushRequest.fromBuffer(value),
        ($0.PushResponse value) => value.writeToBuffer()));
  }

  $async.Future<$0.ResolveResponse> resolve_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ResolveRequest> $request) async {
    return resolve($call, await $request);
  }

  $async.Future<$0.ResolveResponse> resolve(
      $grpc.ServiceCall call, $0.ResolveRequest request);

  $async.Future<$0.BlobContent> readFile_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ReadFileRequest> $request) async {
    return readFile($call, await $request);
  }

  $async.Future<$0.BlobContent> readFile(
      $grpc.ServiceCall call, $0.ReadFileRequest request);

  $async.Future<$0.ListTreeResponse> listTree_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ListTreeRequest> $request) async {
    return listTree($call, await $request);
  }

  $async.Future<$0.ListTreeResponse> listTree(
      $grpc.ServiceCall call, $0.ListTreeRequest request);

  $async.Future<$0.DiffResult> diff_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.DiffRequest> $request) async {
    return diff($call, await $request);
  }

  $async.Future<$0.DiffResult> diff(
      $grpc.ServiceCall call, $0.DiffRequest request);

  $async.Future<$0.GrepResponse> grep_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.GrepRequest> $request) async {
    return grep($call, await $request);
  }

  $async.Future<$0.GrepResponse> grep(
      $grpc.ServiceCall call, $0.GrepRequest request);

  $async.Future<$0.LogResponse> log_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.LogRequest> $request) async {
    return log($call, await $request);
  }

  $async.Future<$0.LogResponse> log(
      $grpc.ServiceCall call, $0.LogRequest request);

  $async.Future<$0.BranchesResponse> branches_Pre($grpc.ServiceCall $call,
      $async.Future<$0.BranchesRequest> $request) async {
    return branches($call, await $request);
  }

  $async.Future<$0.BranchesResponse> branches(
      $grpc.ServiceCall call, $0.BranchesRequest request);

  $async.Future<$0.RepoStatus> status_Pre($grpc.ServiceCall $call,
      $async.Future<$0.RepoStatusRequest> $request) async {
    return status($call, await $request);
  }

  $async.Future<$0.RepoStatus> status(
      $grpc.ServiceCall call, $0.RepoStatusRequest request);

  $async.Future<$0.RepoStatus> fetch_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.FetchRequest> $request) async {
    return fetch($call, await $request);
  }

  $async.Future<$0.RepoStatus> fetch(
      $grpc.ServiceCall call, $0.FetchRequest request);

  $async.Future<$0.WorktreeHandle> worktreeAdd_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.WorktreeSpec> $request) async {
    return worktreeAdd($call, await $request);
  }

  $async.Future<$0.WorktreeHandle> worktreeAdd(
      $grpc.ServiceCall call, $0.WorktreeSpec request);

  $async.Future<$0.WorktreeListResponse> worktreeList_Pre(
      $grpc.ServiceCall $call,
      $async.Future<$0.WorktreeListRequest> $request) async {
    return worktreeList($call, await $request);
  }

  $async.Future<$0.WorktreeListResponse> worktreeList(
      $grpc.ServiceCall call, $0.WorktreeListRequest request);

  $async.Future<$0.WorktreeRemoveResponse> worktreeRemove_Pre(
      $grpc.ServiceCall $call,
      $async.Future<$0.WorktreeRemoveRequest> $request) async {
    return worktreeRemove($call, await $request);
  }

  $async.Future<$0.WorktreeRemoveResponse> worktreeRemove(
      $grpc.ServiceCall call, $0.WorktreeRemoveRequest request);

  $async.Future<$0.Checkpoint> createCheckpoint_Pre($grpc.ServiceCall $call,
      $async.Future<$0.CheckpointRequest> $request) async {
    return createCheckpoint($call, await $request);
  }

  $async.Future<$0.Checkpoint> createCheckpoint(
      $grpc.ServiceCall call, $0.CheckpointRequest request);

  $async.Future<$0.PushResponse> push_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.PushRequest> $request) async {
    return push($call, await $request);
  }

  $async.Future<$0.PushResponse> push(
      $grpc.ServiceCall call, $0.PushRequest request);
}
