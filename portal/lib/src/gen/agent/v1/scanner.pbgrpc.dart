// This is a generated file - do not edit.
//
// Generated from agent/v1/scanner.proto.

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

import 'scanner.pb.dart' as $0;

export 'scanner.pb.dart';

@$pb.GrpcServiceName('agent.v1.ScannerService')
class ScannerServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  ScannerServiceClient(super.channel, {super.options, super.interceptors});

  /// Findings in `content`, in the order the rules ran.
  ///
  /// NOTE the seam's contract, mirrored here: this RPC reports *detections*, and
  /// an empty result means "nothing found". A backend that cannot run must
  /// return an empty result rather than an error — the `Scanner` trait is
  /// deliberately fail-open on infrastructure failure so an unreachable advisory
  /// database can never block a tool call.
  $grpc.ResponseFuture<$0.ScanResponse> scan(
    $0.ScanRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$scan, request, options: options);
  }

  // method descriptors

  static final _$scan = $grpc.ClientMethod<$0.ScanRequest, $0.ScanResponse>(
      '/agent.v1.ScannerService/Scan',
      ($0.ScanRequest value) => value.writeToBuffer(),
      $0.ScanResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.ScannerService')
abstract class ScannerServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.ScannerService';

  ScannerServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.ScanRequest, $0.ScanResponse>(
        'Scan',
        scan_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ScanRequest.fromBuffer(value),
        ($0.ScanResponse value) => value.writeToBuffer()));
  }

  $async.Future<$0.ScanResponse> scan_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.ScanRequest> $request) async {
    return scan($call, await $request);
  }

  $async.Future<$0.ScanResponse> scan(
      $grpc.ServiceCall call, $0.ScanRequest request);
}
