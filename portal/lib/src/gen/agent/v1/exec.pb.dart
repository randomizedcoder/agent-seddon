// This is a generated file - do not edit.
//
// Generated from agent/v1/exec.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

import 'exec.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'exec.pbenum.dart';

class ExecRequest extends $pb.GeneratedMessage {
  factory ExecRequest({
    $core.String? command,
    $core.String? cwd,
    ExecNetworkPolicy? network,
    ExecEnvPolicy? env,
    $fixnum.Int64? timeoutSecs,
  }) {
    final result = create();
    if (command != null) result.command = command;
    if (cwd != null) result.cwd = cwd;
    if (network != null) result.network = network;
    if (env != null) result.env = env;
    if (timeoutSecs != null) result.timeoutSecs = timeoutSecs;
    return result;
  }

  ExecRequest._();

  factory ExecRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ExecRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ExecRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'command')
    ..aOS(2, _omitFieldNames ? '' : 'cwd')
    ..aE<ExecNetworkPolicy>(3, _omitFieldNames ? '' : 'network',
        enumValues: ExecNetworkPolicy.values)
    ..aE<ExecEnvPolicy>(4, _omitFieldNames ? '' : 'env',
        enumValues: ExecEnvPolicy.values)
    ..a<$fixnum.Int64>(
        5, _omitFieldNames ? '' : 'timeoutSecs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ExecRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ExecRequest copyWith(void Function(ExecRequest) updates) =>
      super.copyWith((message) => updates(message as ExecRequest))
          as ExecRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ExecRequest create() => ExecRequest._();
  @$core.override
  ExecRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ExecRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ExecRequest>(create);
  static ExecRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get command => $_getSZ(0);
  @$pb.TagNumber(1)
  set command($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasCommand() => $_has(0);
  @$pb.TagNumber(1)
  void clearCommand() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get cwd => $_getSZ(1);
  @$pb.TagNumber(2)
  set cwd($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCwd() => $_has(1);
  @$pb.TagNumber(2)
  void clearCwd() => $_clearField(2);

  @$pb.TagNumber(3)
  ExecNetworkPolicy get network => $_getN(2);
  @$pb.TagNumber(3)
  set network(ExecNetworkPolicy value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasNetwork() => $_has(2);
  @$pb.TagNumber(3)
  void clearNetwork() => $_clearField(3);

  @$pb.TagNumber(4)
  ExecEnvPolicy get env => $_getN(3);
  @$pb.TagNumber(4)
  set env(ExecEnvPolicy value) => $_setField(4, value);
  @$pb.TagNumber(4)
  $core.bool hasEnv() => $_has(3);
  @$pb.TagNumber(4)
  void clearEnv() => $_clearField(4);

  @$pb.TagNumber(5)
  $fixnum.Int64 get timeoutSecs => $_getI64(4);
  @$pb.TagNumber(5)
  set timeoutSecs($fixnum.Int64 value) => $_setInt64(4, value);
  @$pb.TagNumber(5)
  $core.bool hasTimeoutSecs() => $_has(4);
  @$pb.TagNumber(5)
  void clearTimeoutSecs() => $_clearField(5);
}

class ExecResult extends $pb.GeneratedMessage {
  factory ExecResult({
    $core.String? stdout,
    $core.String? stderr,
    $core.int? exitCode,
    $core.bool? timedOut,
  }) {
    final result = create();
    if (stdout != null) result.stdout = stdout;
    if (stderr != null) result.stderr = stderr;
    if (exitCode != null) result.exitCode = exitCode;
    if (timedOut != null) result.timedOut = timedOut;
    return result;
  }

  ExecResult._();

  factory ExecResult.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ExecResult.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ExecResult',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'stdout')
    ..aOS(2, _omitFieldNames ? '' : 'stderr')
    ..aI(3, _omitFieldNames ? '' : 'exitCode')
    ..aOB(4, _omitFieldNames ? '' : 'timedOut')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ExecResult clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ExecResult copyWith(void Function(ExecResult) updates) =>
      super.copyWith((message) => updates(message as ExecResult)) as ExecResult;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ExecResult create() => ExecResult._();
  @$core.override
  ExecResult createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ExecResult getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ExecResult>(create);
  static ExecResult? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get stdout => $_getSZ(0);
  @$pb.TagNumber(1)
  set stdout($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasStdout() => $_has(0);
  @$pb.TagNumber(1)
  void clearStdout() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get stderr => $_getSZ(1);
  @$pb.TagNumber(2)
  set stderr($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasStderr() => $_has(1);
  @$pb.TagNumber(2)
  void clearStderr() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get exitCode => $_getIZ(2);
  @$pb.TagNumber(3)
  set exitCode($core.int value) => $_setSignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasExitCode() => $_has(2);
  @$pb.TagNumber(3)
  void clearExitCode() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.bool get timedOut => $_getBF(3);
  @$pb.TagNumber(4)
  set timedOut($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasTimedOut() => $_has(3);
  @$pb.TagNumber(4)
  void clearTimedOut() => $_clearField(4);
}

class ExecCapabilitiesRequest extends $pb.GeneratedMessage {
  factory ExecCapabilitiesRequest() => create();

  ExecCapabilitiesRequest._();

  factory ExecCapabilitiesRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ExecCapabilitiesRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ExecCapabilitiesRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ExecCapabilitiesRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ExecCapabilitiesRequest copyWith(
          void Function(ExecCapabilitiesRequest) updates) =>
      super.copyWith((message) => updates(message as ExecCapabilitiesRequest))
          as ExecCapabilitiesRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ExecCapabilitiesRequest create() => ExecCapabilitiesRequest._();
  @$core.override
  ExecCapabilitiesRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ExecCapabilitiesRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ExecCapabilitiesRequest>(create);
  static ExecCapabilitiesRequest? _defaultInstance;
}

class ExecCapabilities extends $pb.GeneratedMessage {
  factory ExecCapabilities({
    $core.String? backend,
    $core.bool? available,
    $core.bool? networkOff,
    $core.bool? privateTmp,
    $core.bool? contentAddressed,
  }) {
    final result = create();
    if (backend != null) result.backend = backend;
    if (available != null) result.available = available;
    if (networkOff != null) result.networkOff = networkOff;
    if (privateTmp != null) result.privateTmp = privateTmp;
    if (contentAddressed != null) result.contentAddressed = contentAddressed;
    return result;
  }

  ExecCapabilities._();

  factory ExecCapabilities.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ExecCapabilities.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ExecCapabilities',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'backend')
    ..aOB(2, _omitFieldNames ? '' : 'available')
    ..aOB(3, _omitFieldNames ? '' : 'networkOff')
    ..aOB(4, _omitFieldNames ? '' : 'privateTmp')
    ..aOB(5, _omitFieldNames ? '' : 'contentAddressed')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ExecCapabilities clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ExecCapabilities copyWith(void Function(ExecCapabilities) updates) =>
      super.copyWith((message) => updates(message as ExecCapabilities))
          as ExecCapabilities;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ExecCapabilities create() => ExecCapabilities._();
  @$core.override
  ExecCapabilities createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ExecCapabilities getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ExecCapabilities>(create);
  static ExecCapabilities? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get backend => $_getSZ(0);
  @$pb.TagNumber(1)
  set backend($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasBackend() => $_has(0);
  @$pb.TagNumber(1)
  void clearBackend() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.bool get available => $_getBF(1);
  @$pb.TagNumber(2)
  set available($core.bool value) => $_setBool(1, value);
  @$pb.TagNumber(2)
  $core.bool hasAvailable() => $_has(1);
  @$pb.TagNumber(2)
  void clearAvailable() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.bool get networkOff => $_getBF(2);
  @$pb.TagNumber(3)
  set networkOff($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasNetworkOff() => $_has(2);
  @$pb.TagNumber(3)
  void clearNetworkOff() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.bool get privateTmp => $_getBF(3);
  @$pb.TagNumber(4)
  set privateTmp($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasPrivateTmp() => $_has(3);
  @$pb.TagNumber(4)
  void clearPrivateTmp() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.bool get contentAddressed => $_getBF(4);
  @$pb.TagNumber(5)
  set contentAddressed($core.bool value) => $_setBool(4, value);
  @$pb.TagNumber(5)
  $core.bool hasContentAddressed() => $_has(4);
  @$pb.TagNumber(5)
  void clearContentAddressed() => $_clearField(5);
}

class PtySessionRef extends $pb.GeneratedMessage {
  factory PtySessionRef({
    $core.String? id,
  }) {
    final result = create();
    if (id != null) result.id = id;
    return result;
  }

  PtySessionRef._();

  factory PtySessionRef.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PtySessionRef.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PtySessionRef',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtySessionRef clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtySessionRef copyWith(void Function(PtySessionRef) updates) =>
      super.copyWith((message) => updates(message as PtySessionRef))
          as PtySessionRef;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PtySessionRef create() => PtySessionRef._();
  @$core.override
  PtySessionRef createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PtySessionRef getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PtySessionRef>(create);
  static PtySessionRef? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);
}

class PtyOpenRequest extends $pb.GeneratedMessage {
  factory PtyOpenRequest({
    $core.String? command,
    $core.Iterable<$core.String>? args,
    $core.int? cols,
    $core.int? rows,
    $core.String? cwd,
  }) {
    final result = create();
    if (command != null) result.command = command;
    if (args != null) result.args.addAll(args);
    if (cols != null) result.cols = cols;
    if (rows != null) result.rows = rows;
    if (cwd != null) result.cwd = cwd;
    return result;
  }

  PtyOpenRequest._();

  factory PtyOpenRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PtyOpenRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PtyOpenRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'command')
    ..pPS(2, _omitFieldNames ? '' : 'args')
    ..aI(3, _omitFieldNames ? '' : 'cols', fieldType: $pb.PbFieldType.OU3)
    ..aI(4, _omitFieldNames ? '' : 'rows', fieldType: $pb.PbFieldType.OU3)
    ..aOS(5, _omitFieldNames ? '' : 'cwd')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyOpenRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyOpenRequest copyWith(void Function(PtyOpenRequest) updates) =>
      super.copyWith((message) => updates(message as PtyOpenRequest))
          as PtyOpenRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PtyOpenRequest create() => PtyOpenRequest._();
  @$core.override
  PtyOpenRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PtyOpenRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PtyOpenRequest>(create);
  static PtyOpenRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get command => $_getSZ(0);
  @$pb.TagNumber(1)
  set command($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasCommand() => $_has(0);
  @$pb.TagNumber(1)
  void clearCommand() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<$core.String> get args => $_getList(1);

  @$pb.TagNumber(3)
  $core.int get cols => $_getIZ(2);
  @$pb.TagNumber(3)
  set cols($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasCols() => $_has(2);
  @$pb.TagNumber(3)
  void clearCols() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get rows => $_getIZ(3);
  @$pb.TagNumber(4)
  set rows($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasRows() => $_has(3);
  @$pb.TagNumber(4)
  void clearRows() => $_clearField(4);

  /// Empty ⇒ the server's working directory.
  @$pb.TagNumber(5)
  $core.String get cwd => $_getSZ(4);
  @$pb.TagNumber(5)
  set cwd($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasCwd() => $_has(4);
  @$pb.TagNumber(5)
  void clearCwd() => $_clearField(5);
}

class PtyWriteRequest extends $pb.GeneratedMessage {
  factory PtyWriteRequest({
    $core.String? id,
    $core.List<$core.int>? input,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (input != null) result.input = input;
    return result;
  }

  PtyWriteRequest._();

  factory PtyWriteRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PtyWriteRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PtyWriteRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..a<$core.List<$core.int>>(
        2, _omitFieldNames ? '' : 'input', $pb.PbFieldType.OY)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyWriteRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyWriteRequest copyWith(void Function(PtyWriteRequest) updates) =>
      super.copyWith((message) => updates(message as PtyWriteRequest))
          as PtyWriteRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PtyWriteRequest create() => PtyWriteRequest._();
  @$core.override
  PtyWriteRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PtyWriteRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PtyWriteRequest>(create);
  static PtyWriteRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.List<$core.int> get input => $_getN(1);
  @$pb.TagNumber(2)
  set input($core.List<$core.int> value) => $_setBytes(1, value);
  @$pb.TagNumber(2)
  $core.bool hasInput() => $_has(1);
  @$pb.TagNumber(2)
  void clearInput() => $_clearField(2);
}

class PtyWriteResponse extends $pb.GeneratedMessage {
  factory PtyWriteResponse() => create();

  PtyWriteResponse._();

  factory PtyWriteResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PtyWriteResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PtyWriteResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyWriteResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyWriteResponse copyWith(void Function(PtyWriteResponse) updates) =>
      super.copyWith((message) => updates(message as PtyWriteResponse))
          as PtyWriteResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PtyWriteResponse create() => PtyWriteResponse._();
  @$core.override
  PtyWriteResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PtyWriteResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PtyWriteResponse>(create);
  static PtyWriteResponse? _defaultInstance;
}

class PtyReadRequest extends $pb.GeneratedMessage {
  factory PtyReadRequest({
    $core.String? id,
    $fixnum.Int64? cursor,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (cursor != null) result.cursor = cursor;
    return result;
  }

  PtyReadRequest._();

  factory PtyReadRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PtyReadRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PtyReadRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..a<$fixnum.Int64>(2, _omitFieldNames ? '' : 'cursor', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyReadRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyReadRequest copyWith(void Function(PtyReadRequest) updates) =>
      super.copyWith((message) => updates(message as PtyReadRequest))
          as PtyReadRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PtyReadRequest create() => PtyReadRequest._();
  @$core.override
  PtyReadRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PtyReadRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PtyReadRequest>(create);
  static PtyReadRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  /// Absent ⇒ from the oldest retained byte.
  @$pb.TagNumber(2)
  $fixnum.Int64 get cursor => $_getI64(1);
  @$pb.TagNumber(2)
  set cursor($fixnum.Int64 value) => $_setInt64(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCursor() => $_has(1);
  @$pb.TagNumber(2)
  void clearCursor() => $_clearField(2);
}

class PtyStateMsg extends $pb.GeneratedMessage {
  factory PtyStateMsg({
    $core.bool? running,
    $core.bool? closed,
    $core.int? exitCode,
  }) {
    final result = create();
    if (running != null) result.running = running;
    if (closed != null) result.closed = closed;
    if (exitCode != null) result.exitCode = exitCode;
    return result;
  }

  PtyStateMsg._();

  factory PtyStateMsg.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PtyStateMsg.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PtyStateMsg',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'running')
    ..aOB(2, _omitFieldNames ? '' : 'closed')
    ..aI(3, _omitFieldNames ? '' : 'exitCode')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyStateMsg clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyStateMsg copyWith(void Function(PtyStateMsg) updates) =>
      super.copyWith((message) => updates(message as PtyStateMsg))
          as PtyStateMsg;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PtyStateMsg create() => PtyStateMsg._();
  @$core.override
  PtyStateMsg createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PtyStateMsg getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PtyStateMsg>(create);
  static PtyStateMsg? _defaultInstance;

  /// Running / Closed carry no code; Exited does.
  @$pb.TagNumber(1)
  $core.bool get running => $_getBF(0);
  @$pb.TagNumber(1)
  set running($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRunning() => $_has(0);
  @$pb.TagNumber(1)
  void clearRunning() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.bool get closed => $_getBF(1);
  @$pb.TagNumber(2)
  set closed($core.bool value) => $_setBool(1, value);
  @$pb.TagNumber(2)
  $core.bool hasClosed() => $_has(1);
  @$pb.TagNumber(2)
  void clearClosed() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get exitCode => $_getIZ(2);
  @$pb.TagNumber(3)
  set exitCode($core.int value) => $_setSignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasExitCode() => $_has(2);
  @$pb.TagNumber(3)
  void clearExitCode() => $_clearField(3);
}

class PtyReadResponse extends $pb.GeneratedMessage {
  factory PtyReadResponse({
    $core.List<$core.int>? data,
    $fixnum.Int64? nextCursor,
    $fixnum.Int64? dropped,
    PtyStateMsg? state,
  }) {
    final result = create();
    if (data != null) result.data = data;
    if (nextCursor != null) result.nextCursor = nextCursor;
    if (dropped != null) result.dropped = dropped;
    if (state != null) result.state = state;
    return result;
  }

  PtyReadResponse._();

  factory PtyReadResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PtyReadResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PtyReadResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..a<$core.List<$core.int>>(
        1, _omitFieldNames ? '' : 'data', $pb.PbFieldType.OY)
    ..a<$fixnum.Int64>(
        2, _omitFieldNames ? '' : 'nextCursor', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..a<$fixnum.Int64>(3, _omitFieldNames ? '' : 'dropped', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOM<PtyStateMsg>(4, _omitFieldNames ? '' : 'state',
        subBuilder: PtyStateMsg.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyReadResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyReadResponse copyWith(void Function(PtyReadResponse) updates) =>
      super.copyWith((message) => updates(message as PtyReadResponse))
          as PtyReadResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PtyReadResponse create() => PtyReadResponse._();
  @$core.override
  PtyReadResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PtyReadResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PtyReadResponse>(create);
  static PtyReadResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $core.List<$core.int> get data => $_getN(0);
  @$pb.TagNumber(1)
  set data($core.List<$core.int> value) => $_setBytes(0, value);
  @$pb.TagNumber(1)
  $core.bool hasData() => $_has(0);
  @$pb.TagNumber(1)
  void clearData() => $_clearField(1);

  @$pb.TagNumber(2)
  $fixnum.Int64 get nextCursor => $_getI64(1);
  @$pb.TagNumber(2)
  set nextCursor($fixnum.Int64 value) => $_setInt64(1, value);
  @$pb.TagNumber(2)
  $core.bool hasNextCursor() => $_has(1);
  @$pb.TagNumber(2)
  void clearNextCursor() => $_clearField(2);

  /// Bytes the rolling buffer had already dropped — surfaced rather than
  /// silently omitted, so a caller knows it lost data.
  @$pb.TagNumber(3)
  $fixnum.Int64 get dropped => $_getI64(2);
  @$pb.TagNumber(3)
  set dropped($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasDropped() => $_has(2);
  @$pb.TagNumber(3)
  void clearDropped() => $_clearField(3);

  @$pb.TagNumber(4)
  PtyStateMsg get state => $_getN(3);
  @$pb.TagNumber(4)
  set state(PtyStateMsg value) => $_setField(4, value);
  @$pb.TagNumber(4)
  $core.bool hasState() => $_has(3);
  @$pb.TagNumber(4)
  void clearState() => $_clearField(4);
  @$pb.TagNumber(4)
  PtyStateMsg ensureState() => $_ensure(3);
}

class PtyResizeRequest extends $pb.GeneratedMessage {
  factory PtyResizeRequest({
    $core.String? id,
    $core.int? cols,
    $core.int? rows,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (cols != null) result.cols = cols;
    if (rows != null) result.rows = rows;
    return result;
  }

  PtyResizeRequest._();

  factory PtyResizeRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PtyResizeRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PtyResizeRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aI(2, _omitFieldNames ? '' : 'cols', fieldType: $pb.PbFieldType.OU3)
    ..aI(3, _omitFieldNames ? '' : 'rows', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyResizeRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyResizeRequest copyWith(void Function(PtyResizeRequest) updates) =>
      super.copyWith((message) => updates(message as PtyResizeRequest))
          as PtyResizeRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PtyResizeRequest create() => PtyResizeRequest._();
  @$core.override
  PtyResizeRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PtyResizeRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PtyResizeRequest>(create);
  static PtyResizeRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get cols => $_getIZ(1);
  @$pb.TagNumber(2)
  set cols($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCols() => $_has(1);
  @$pb.TagNumber(2)
  void clearCols() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get rows => $_getIZ(2);
  @$pb.TagNumber(3)
  set rows($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasRows() => $_has(2);
  @$pb.TagNumber(3)
  void clearRows() => $_clearField(3);
}

class PtyResizeResponse extends $pb.GeneratedMessage {
  factory PtyResizeResponse() => create();

  PtyResizeResponse._();

  factory PtyResizeResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PtyResizeResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PtyResizeResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyResizeResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyResizeResponse copyWith(void Function(PtyResizeResponse) updates) =>
      super.copyWith((message) => updates(message as PtyResizeResponse))
          as PtyResizeResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PtyResizeResponse create() => PtyResizeResponse._();
  @$core.override
  PtyResizeResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PtyResizeResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PtyResizeResponse>(create);
  static PtyResizeResponse? _defaultInstance;
}

class PtyCloseResponse extends $pb.GeneratedMessage {
  factory PtyCloseResponse({
    $core.bool? closed,
  }) {
    final result = create();
    if (closed != null) result.closed = closed;
    return result;
  }

  PtyCloseResponse._();

  factory PtyCloseResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PtyCloseResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PtyCloseResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'closed')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyCloseResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyCloseResponse copyWith(void Function(PtyCloseResponse) updates) =>
      super.copyWith((message) => updates(message as PtyCloseResponse))
          as PtyCloseResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PtyCloseResponse create() => PtyCloseResponse._();
  @$core.override
  PtyCloseResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PtyCloseResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PtyCloseResponse>(create);
  static PtyCloseResponse? _defaultInstance;

  /// False when there was no such session — not an error.
  @$pb.TagNumber(1)
  $core.bool get closed => $_getBF(0);
  @$pb.TagNumber(1)
  set closed($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasClosed() => $_has(0);
  @$pb.TagNumber(1)
  void clearClosed() => $_clearField(1);
}

class PtyListRequest extends $pb.GeneratedMessage {
  factory PtyListRequest() => create();

  PtyListRequest._();

  factory PtyListRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PtyListRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PtyListRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyListRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtyListRequest copyWith(void Function(PtyListRequest) updates) =>
      super.copyWith((message) => updates(message as PtyListRequest))
          as PtyListRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PtyListRequest create() => PtyListRequest._();
  @$core.override
  PtyListRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PtyListRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PtyListRequest>(create);
  static PtyListRequest? _defaultInstance;
}

class PtySessionInfo extends $pb.GeneratedMessage {
  factory PtySessionInfo({
    $core.String? id,
    $core.String? command,
    PtyStateMsg? state,
    $core.int? cols,
    $core.int? rows,
    $fixnum.Int64? bytesOut,
    $fixnum.Int64? firstRetained,
    $fixnum.Int64? nextCursor,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (command != null) result.command = command;
    if (state != null) result.state = state;
    if (cols != null) result.cols = cols;
    if (rows != null) result.rows = rows;
    if (bytesOut != null) result.bytesOut = bytesOut;
    if (firstRetained != null) result.firstRetained = firstRetained;
    if (nextCursor != null) result.nextCursor = nextCursor;
    return result;
  }

  PtySessionInfo._();

  factory PtySessionInfo.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PtySessionInfo.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PtySessionInfo',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aOS(2, _omitFieldNames ? '' : 'command')
    ..aOM<PtyStateMsg>(3, _omitFieldNames ? '' : 'state',
        subBuilder: PtyStateMsg.create)
    ..aI(4, _omitFieldNames ? '' : 'cols', fieldType: $pb.PbFieldType.OU3)
    ..aI(5, _omitFieldNames ? '' : 'rows', fieldType: $pb.PbFieldType.OU3)
    ..a<$fixnum.Int64>(
        6, _omitFieldNames ? '' : 'bytesOut', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..a<$fixnum.Int64>(
        7, _omitFieldNames ? '' : 'firstRetained', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..a<$fixnum.Int64>(
        8, _omitFieldNames ? '' : 'nextCursor', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtySessionInfo clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtySessionInfo copyWith(void Function(PtySessionInfo) updates) =>
      super.copyWith((message) => updates(message as PtySessionInfo))
          as PtySessionInfo;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PtySessionInfo create() => PtySessionInfo._();
  @$core.override
  PtySessionInfo createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PtySessionInfo getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PtySessionInfo>(create);
  static PtySessionInfo? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get command => $_getSZ(1);
  @$pb.TagNumber(2)
  set command($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCommand() => $_has(1);
  @$pb.TagNumber(2)
  void clearCommand() => $_clearField(2);

  @$pb.TagNumber(3)
  PtyStateMsg get state => $_getN(2);
  @$pb.TagNumber(3)
  set state(PtyStateMsg value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasState() => $_has(2);
  @$pb.TagNumber(3)
  void clearState() => $_clearField(3);
  @$pb.TagNumber(3)
  PtyStateMsg ensureState() => $_ensure(2);

  @$pb.TagNumber(4)
  $core.int get cols => $_getIZ(3);
  @$pb.TagNumber(4)
  set cols($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasCols() => $_has(3);
  @$pb.TagNumber(4)
  void clearCols() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.int get rows => $_getIZ(4);
  @$pb.TagNumber(5)
  set rows($core.int value) => $_setUnsignedInt32(4, value);
  @$pb.TagNumber(5)
  $core.bool hasRows() => $_has(4);
  @$pb.TagNumber(5)
  void clearRows() => $_clearField(5);

  @$pb.TagNumber(6)
  $fixnum.Int64 get bytesOut => $_getI64(5);
  @$pb.TagNumber(6)
  set bytesOut($fixnum.Int64 value) => $_setInt64(5, value);
  @$pb.TagNumber(6)
  $core.bool hasBytesOut() => $_has(5);
  @$pb.TagNumber(6)
  void clearBytesOut() => $_clearField(6);

  @$pb.TagNumber(7)
  $fixnum.Int64 get firstRetained => $_getI64(6);
  @$pb.TagNumber(7)
  set firstRetained($fixnum.Int64 value) => $_setInt64(6, value);
  @$pb.TagNumber(7)
  $core.bool hasFirstRetained() => $_has(6);
  @$pb.TagNumber(7)
  void clearFirstRetained() => $_clearField(7);

  @$pb.TagNumber(8)
  $fixnum.Int64 get nextCursor => $_getI64(7);
  @$pb.TagNumber(8)
  set nextCursor($fixnum.Int64 value) => $_setInt64(7, value);
  @$pb.TagNumber(8)
  $core.bool hasNextCursor() => $_has(7);
  @$pb.TagNumber(8)
  void clearNextCursor() => $_clearField(8);
}

class PtySessionList extends $pb.GeneratedMessage {
  factory PtySessionList({
    $core.Iterable<PtySessionInfo>? sessions,
  }) {
    final result = create();
    if (sessions != null) result.sessions.addAll(sessions);
    return result;
  }

  PtySessionList._();

  factory PtySessionList.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PtySessionList.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PtySessionList',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<PtySessionInfo>(1, _omitFieldNames ? '' : 'sessions',
        subBuilder: PtySessionInfo.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtySessionList clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PtySessionList copyWith(void Function(PtySessionList) updates) =>
      super.copyWith((message) => updates(message as PtySessionList))
          as PtySessionList;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PtySessionList create() => PtySessionList._();
  @$core.override
  PtySessionList createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PtySessionList getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PtySessionList>(create);
  static PtySessionList? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<PtySessionInfo> get sessions => $_getList(0);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
