// This is a generated file - do not edit.
//
// Generated from agent/v1/config.proto.

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

import 'config.pb.dart' as $0;

export 'config.pb.dart';

@$pb.GrpcServiceName('agent.v1.ConfigService')
class ConfigServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  ConfigServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.ConfigSchema> getSchema(
    $0.GetSchemaRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$getSchema, request, options: options);
  }

  $grpc.ResponseFuture<$0.ConfigValues> getValues(
    $0.GetValuesRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$getValues, request, options: options);
  }

  $grpc.ResponseFuture<$0.ValidateConfigResponse> validate(
    $0.ValidateConfigRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$validate, request, options: options);
  }

  $grpc.ResponseFuture<$0.PutConfigResponse> put(
    $0.PutConfigRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$put, request, options: options);
  }

  $grpc.ResponseFuture<$0.ConfigStatus> status(
    $0.ConfigStatusRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$status, request, options: options);
  }

  // method descriptors

  static final _$getSchema =
      $grpc.ClientMethod<$0.GetSchemaRequest, $0.ConfigSchema>(
          '/agent.v1.ConfigService/GetSchema',
          ($0.GetSchemaRequest value) => value.writeToBuffer(),
          $0.ConfigSchema.fromBuffer);
  static final _$getValues =
      $grpc.ClientMethod<$0.GetValuesRequest, $0.ConfigValues>(
          '/agent.v1.ConfigService/GetValues',
          ($0.GetValuesRequest value) => value.writeToBuffer(),
          $0.ConfigValues.fromBuffer);
  static final _$validate =
      $grpc.ClientMethod<$0.ValidateConfigRequest, $0.ValidateConfigResponse>(
          '/agent.v1.ConfigService/Validate',
          ($0.ValidateConfigRequest value) => value.writeToBuffer(),
          $0.ValidateConfigResponse.fromBuffer);
  static final _$put =
      $grpc.ClientMethod<$0.PutConfigRequest, $0.PutConfigResponse>(
          '/agent.v1.ConfigService/Put',
          ($0.PutConfigRequest value) => value.writeToBuffer(),
          $0.PutConfigResponse.fromBuffer);
  static final _$status =
      $grpc.ClientMethod<$0.ConfigStatusRequest, $0.ConfigStatus>(
          '/agent.v1.ConfigService/Status',
          ($0.ConfigStatusRequest value) => value.writeToBuffer(),
          $0.ConfigStatus.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.ConfigService')
abstract class ConfigServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.ConfigService';

  ConfigServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.GetSchemaRequest, $0.ConfigSchema>(
        'GetSchema',
        getSchema_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.GetSchemaRequest.fromBuffer(value),
        ($0.ConfigSchema value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.GetValuesRequest, $0.ConfigValues>(
        'GetValues',
        getValues_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.GetValuesRequest.fromBuffer(value),
        ($0.ConfigValues value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ValidateConfigRequest,
            $0.ValidateConfigResponse>(
        'Validate',
        validate_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.ValidateConfigRequest.fromBuffer(value),
        ($0.ValidateConfigResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PutConfigRequest, $0.PutConfigResponse>(
        'Put',
        put_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PutConfigRequest.fromBuffer(value),
        ($0.PutConfigResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ConfigStatusRequest, $0.ConfigStatus>(
        'Status',
        status_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.ConfigStatusRequest.fromBuffer(value),
        ($0.ConfigStatus value) => value.writeToBuffer()));
  }

  $async.Future<$0.ConfigSchema> getSchema_Pre($grpc.ServiceCall $call,
      $async.Future<$0.GetSchemaRequest> $request) async {
    return getSchema($call, await $request);
  }

  $async.Future<$0.ConfigSchema> getSchema(
      $grpc.ServiceCall call, $0.GetSchemaRequest request);

  $async.Future<$0.ConfigValues> getValues_Pre($grpc.ServiceCall $call,
      $async.Future<$0.GetValuesRequest> $request) async {
    return getValues($call, await $request);
  }

  $async.Future<$0.ConfigValues> getValues(
      $grpc.ServiceCall call, $0.GetValuesRequest request);

  $async.Future<$0.ValidateConfigResponse> validate_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ValidateConfigRequest> $request) async {
    return validate($call, await $request);
  }

  $async.Future<$0.ValidateConfigResponse> validate(
      $grpc.ServiceCall call, $0.ValidateConfigRequest request);

  $async.Future<$0.PutConfigResponse> put_Pre($grpc.ServiceCall $call,
      $async.Future<$0.PutConfigRequest> $request) async {
    return put($call, await $request);
  }

  $async.Future<$0.PutConfigResponse> put(
      $grpc.ServiceCall call, $0.PutConfigRequest request);

  $async.Future<$0.ConfigStatus> status_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ConfigStatusRequest> $request) async {
    return status($call, await $request);
  }

  $async.Future<$0.ConfigStatus> status(
      $grpc.ServiceCall call, $0.ConfigStatusRequest request);
}
