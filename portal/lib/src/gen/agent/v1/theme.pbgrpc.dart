// This is a generated file - do not edit.
//
// Generated from agent/v1/theme.proto.

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

import 'theme.pb.dart' as $0;

export 'theme.pb.dart';

/// User themes and the caller's active theme (docs/design/visual/03, 04), gated by
/// `(read|write|delete|approve, theme)`. A theme the caller may not see is reported
/// as missing, never as forbidden.
@$pb.GrpcServiceName('agent.v1.ThemeService')
class ThemeServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  ThemeServiceClient(super.channel, {super.options, super.interceptors});

  /// Built-ins, the tenant's shared themes and the caller's own private ones.
  $grpc.ResponseFuture<$0.ListThemesResponse> list(
    $0.ListThemesRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$list, request, options: options);
  }

  $grpc.ResponseFuture<$0.Theme> get(
    $0.GetThemeRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$get, request, options: options);
  }

  /// Create or update a user theme; returns it as stored. `version` must equal the
  /// stored version (0 to create), else ABORTED. Built-in ids are refused.
  $grpc.ResponseFuture<$0.Theme> put(
    $0.Theme request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$put, request, options: options);
  }

  $grpc.ResponseFuture<$0.DeleteThemeResponse> delete(
    $0.DeleteThemeRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$delete, request, options: options);
  }

  /// The caller's theme, resolved: their pick, else the server default, else
  /// `prototype`.
  $grpc.ResponseFuture<$0.ActiveTheme> getActive(
    $0.GetActiveThemeRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$getActive, request, options: options);
  }

  $grpc.ResponseFuture<$0.ActiveTheme> setActive(
    $0.SetActiveThemeRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$setActive, request, options: options);
  }

  /// Parse a textproto theme (the file form) for the editor. Stores nothing.
  /// Text that does not parse is INVALID_ARGUMENT; a theme that parses but would
  /// not be accepted by Put comes back with `problem` set, so it can be fixed.
  $grpc.ResponseFuture<$0.ImportThemeResponse> import(
    $0.ImportThemeRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$import, request, options: options);
  }

  /// A theme as textproto, for sharing or committing to config/themes/. Stores
  /// nothing; the server-set fields are left out.
  $grpc.ResponseFuture<$0.ExportThemeResponse> export(
    $0.ExportThemeRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$export, request, options: options);
  }

  // method descriptors

  static final _$list =
      $grpc.ClientMethod<$0.ListThemesRequest, $0.ListThemesResponse>(
          '/agent.v1.ThemeService/List',
          ($0.ListThemesRequest value) => value.writeToBuffer(),
          $0.ListThemesResponse.fromBuffer);
  static final _$get = $grpc.ClientMethod<$0.GetThemeRequest, $0.Theme>(
      '/agent.v1.ThemeService/Get',
      ($0.GetThemeRequest value) => value.writeToBuffer(),
      $0.Theme.fromBuffer);
  static final _$put = $grpc.ClientMethod<$0.Theme, $0.Theme>(
      '/agent.v1.ThemeService/Put',
      ($0.Theme value) => value.writeToBuffer(),
      $0.Theme.fromBuffer);
  static final _$delete =
      $grpc.ClientMethod<$0.DeleteThemeRequest, $0.DeleteThemeResponse>(
          '/agent.v1.ThemeService/Delete',
          ($0.DeleteThemeRequest value) => value.writeToBuffer(),
          $0.DeleteThemeResponse.fromBuffer);
  static final _$getActive =
      $grpc.ClientMethod<$0.GetActiveThemeRequest, $0.ActiveTheme>(
          '/agent.v1.ThemeService/GetActive',
          ($0.GetActiveThemeRequest value) => value.writeToBuffer(),
          $0.ActiveTheme.fromBuffer);
  static final _$setActive =
      $grpc.ClientMethod<$0.SetActiveThemeRequest, $0.ActiveTheme>(
          '/agent.v1.ThemeService/SetActive',
          ($0.SetActiveThemeRequest value) => value.writeToBuffer(),
          $0.ActiveTheme.fromBuffer);
  static final _$import =
      $grpc.ClientMethod<$0.ImportThemeRequest, $0.ImportThemeResponse>(
          '/agent.v1.ThemeService/Import',
          ($0.ImportThemeRequest value) => value.writeToBuffer(),
          $0.ImportThemeResponse.fromBuffer);
  static final _$export =
      $grpc.ClientMethod<$0.ExportThemeRequest, $0.ExportThemeResponse>(
          '/agent.v1.ThemeService/Export',
          ($0.ExportThemeRequest value) => value.writeToBuffer(),
          $0.ExportThemeResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.ThemeService')
abstract class ThemeServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.ThemeService';

  ThemeServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.ListThemesRequest, $0.ListThemesResponse>(
        'List',
        list_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ListThemesRequest.fromBuffer(value),
        ($0.ListThemesResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.GetThemeRequest, $0.Theme>(
        'Get',
        get_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.GetThemeRequest.fromBuffer(value),
        ($0.Theme value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.Theme, $0.Theme>(
        'Put',
        put_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.Theme.fromBuffer(value),
        ($0.Theme value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.DeleteThemeRequest, $0.DeleteThemeResponse>(
            'Delete',
            delete_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.DeleteThemeRequest.fromBuffer(value),
            ($0.DeleteThemeResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.GetActiveThemeRequest, $0.ActiveTheme>(
        'GetActive',
        getActive_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.GetActiveThemeRequest.fromBuffer(value),
        ($0.ActiveTheme value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.SetActiveThemeRequest, $0.ActiveTheme>(
        'SetActive',
        setActive_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.SetActiveThemeRequest.fromBuffer(value),
        ($0.ActiveTheme value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.ImportThemeRequest, $0.ImportThemeResponse>(
            'Import',
            import_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.ImportThemeRequest.fromBuffer(value),
            ($0.ImportThemeResponse value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.ExportThemeRequest, $0.ExportThemeResponse>(
            'Export',
            export_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.ExportThemeRequest.fromBuffer(value),
            ($0.ExportThemeResponse value) => value.writeToBuffer()));
  }

  $async.Future<$0.ListThemesResponse> list_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ListThemesRequest> $request) async {
    return list($call, await $request);
  }

  $async.Future<$0.ListThemesResponse> list(
      $grpc.ServiceCall call, $0.ListThemesRequest request);

  $async.Future<$0.Theme> get_Pre($grpc.ServiceCall $call,
      $async.Future<$0.GetThemeRequest> $request) async {
    return get($call, await $request);
  }

  $async.Future<$0.Theme> get(
      $grpc.ServiceCall call, $0.GetThemeRequest request);

  $async.Future<$0.Theme> put_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.Theme> $request) async {
    return put($call, await $request);
  }

  $async.Future<$0.Theme> put($grpc.ServiceCall call, $0.Theme request);

  $async.Future<$0.DeleteThemeResponse> delete_Pre($grpc.ServiceCall $call,
      $async.Future<$0.DeleteThemeRequest> $request) async {
    return delete($call, await $request);
  }

  $async.Future<$0.DeleteThemeResponse> delete(
      $grpc.ServiceCall call, $0.DeleteThemeRequest request);

  $async.Future<$0.ActiveTheme> getActive_Pre($grpc.ServiceCall $call,
      $async.Future<$0.GetActiveThemeRequest> $request) async {
    return getActive($call, await $request);
  }

  $async.Future<$0.ActiveTheme> getActive(
      $grpc.ServiceCall call, $0.GetActiveThemeRequest request);

  $async.Future<$0.ActiveTheme> setActive_Pre($grpc.ServiceCall $call,
      $async.Future<$0.SetActiveThemeRequest> $request) async {
    return setActive($call, await $request);
  }

  $async.Future<$0.ActiveTheme> setActive(
      $grpc.ServiceCall call, $0.SetActiveThemeRequest request);

  $async.Future<$0.ImportThemeResponse> import_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ImportThemeRequest> $request) async {
    return import($call, await $request);
  }

  $async.Future<$0.ImportThemeResponse> import(
      $grpc.ServiceCall call, $0.ImportThemeRequest request);

  $async.Future<$0.ExportThemeResponse> export_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ExportThemeRequest> $request) async {
    return export($call, await $request);
  }

  $async.Future<$0.ExportThemeResponse> export(
      $grpc.ServiceCall call, $0.ExportThemeRequest request);
}
