// This is a generated file - do not edit.
//
// Generated from agent/v1/reference.proto.

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

import 'reference.pb.dart' as $0;

export 'reference.pb.dart';

@$pb.GrpcServiceName('agent.v1.ReferenceService')
class ReferenceServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  ReferenceServiceClient(super.channel, {super.options, super.interceptors});

  /// Expand a prompt's `@`-mentions into context blocks, budget-bounded.
  ///
  /// NOTE the seam's contract, mirrored here: resolution NEVER fails. An
  /// unresolved, denied, or errored reference becomes a warning, so one bad
  /// mention can't fail the turn. `blocked` (over the hard budget) means the
  /// prompt is left unmodified — it is not an error either.
  $grpc.ResponseFuture<$0.RefResolution> resolve(
    $0.RefResolveRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$resolve, request, options: options);
  }

  // method descriptors

  static final _$resolve =
      $grpc.ClientMethod<$0.RefResolveRequest, $0.RefResolution>(
          '/agent.v1.ReferenceService/Resolve',
          ($0.RefResolveRequest value) => value.writeToBuffer(),
          $0.RefResolution.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.ReferenceService')
abstract class ReferenceServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.ReferenceService';

  ReferenceServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.RefResolveRequest, $0.RefResolution>(
        'Resolve',
        resolve_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.RefResolveRequest.fromBuffer(value),
        ($0.RefResolution value) => value.writeToBuffer()));
  }

  $async.Future<$0.RefResolution> resolve_Pre($grpc.ServiceCall $call,
      $async.Future<$0.RefResolveRequest> $request) async {
    return resolve($call, await $request);
  }

  $async.Future<$0.RefResolution> resolve(
      $grpc.ServiceCall call, $0.RefResolveRequest request);
}
