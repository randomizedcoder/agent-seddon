// This is a generated file - do not edit.
//
// Generated from agent/v1/config.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

import 'common.pb.dart' as $1;

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

class GetSchemaRequest extends $pb.GeneratedMessage {
  factory GetSchemaRequest() => create();

  GetSchemaRequest._();

  factory GetSchemaRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GetSchemaRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GetSchemaRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetSchemaRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetSchemaRequest copyWith(void Function(GetSchemaRequest) updates) =>
      super.copyWith((message) => updates(message as GetSchemaRequest))
          as GetSchemaRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GetSchemaRequest create() => GetSchemaRequest._();
  @$core.override
  GetSchemaRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GetSchemaRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GetSchemaRequest>(create);
  static GetSchemaRequest? _defaultInstance;
}

/// A JSON-Schema (draft 2020-12) describing every config field: type, enum
/// choices, description (from the struct's doc-comments), default, and an
/// `x-secret` flag on secret fields.
class ConfigSchema extends $pb.GeneratedMessage {
  factory ConfigSchema({
    $1.JsonValue? schema,
  }) {
    final result = create();
    if (schema != null) result.schema = schema;
    return result;
  }

  ConfigSchema._();

  factory ConfigSchema.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ConfigSchema.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ConfigSchema',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<$1.JsonValue>(1, _omitFieldNames ? '' : 'schema',
        subBuilder: $1.JsonValue.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ConfigSchema clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ConfigSchema copyWith(void Function(ConfigSchema) updates) =>
      super.copyWith((message) => updates(message as ConfigSchema))
          as ConfigSchema;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ConfigSchema create() => ConfigSchema._();
  @$core.override
  ConfigSchema createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ConfigSchema getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ConfigSchema>(create);
  static ConfigSchema? _defaultInstance;

  @$pb.TagNumber(1)
  $1.JsonValue get schema => $_getN(0);
  @$pb.TagNumber(1)
  set schema($1.JsonValue value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasSchema() => $_has(0);
  @$pb.TagNumber(1)
  void clearSchema() => $_clearField(1);
  @$pb.TagNumber(1)
  $1.JsonValue ensureSchema() => $_ensure(0);
}

class GetValuesRequest extends $pb.GeneratedMessage {
  factory GetValuesRequest() => create();

  GetValuesRequest._();

  factory GetValuesRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GetValuesRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GetValuesRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetValuesRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetValuesRequest copyWith(void Function(GetValuesRequest) updates) =>
      super.copyWith((message) => updates(message as GetValuesRequest))
          as GetValuesRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GetValuesRequest create() => GetValuesRequest._();
  @$core.override
  GetValuesRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GetValuesRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GetValuesRequest>(create);
  static GetValuesRequest? _defaultInstance;
}

/// The effective config (defaults filled) as a JSON object, secrets masked.
class ConfigValues extends $pb.GeneratedMessage {
  factory ConfigValues({
    $1.JsonValue? values,
  }) {
    final result = create();
    if (values != null) result.values = values;
    return result;
  }

  ConfigValues._();

  factory ConfigValues.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ConfigValues.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ConfigValues',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<$1.JsonValue>(1, _omitFieldNames ? '' : 'values',
        subBuilder: $1.JsonValue.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ConfigValues clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ConfigValues copyWith(void Function(ConfigValues) updates) =>
      super.copyWith((message) => updates(message as ConfigValues))
          as ConfigValues;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ConfigValues create() => ConfigValues._();
  @$core.override
  ConfigValues createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ConfigValues getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ConfigValues>(create);
  static ConfigValues? _defaultInstance;

  @$pb.TagNumber(1)
  $1.JsonValue get values => $_getN(0);
  @$pb.TagNumber(1)
  set values($1.JsonValue value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasValues() => $_has(0);
  @$pb.TagNumber(1)
  void clearValues() => $_clearField(1);
  @$pb.TagNumber(1)
  $1.JsonValue ensureValues() => $_ensure(0);
}

/// One edit: a dotted key-path (e.g. `agent.context`, `pool.members`) and its new
/// value. A null/absent `value` means delete the key — revert it to the default.
class ConfigEdit extends $pb.GeneratedMessage {
  factory ConfigEdit({
    $core.String? path,
    $1.JsonValue? value,
  }) {
    final result = create();
    if (path != null) result.path = path;
    if (value != null) result.value = value;
    return result;
  }

  ConfigEdit._();

  factory ConfigEdit.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ConfigEdit.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ConfigEdit',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'path')
    ..aOM<$1.JsonValue>(2, _omitFieldNames ? '' : 'value',
        subBuilder: $1.JsonValue.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ConfigEdit clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ConfigEdit copyWith(void Function(ConfigEdit) updates) =>
      super.copyWith((message) => updates(message as ConfigEdit)) as ConfigEdit;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ConfigEdit create() => ConfigEdit._();
  @$core.override
  ConfigEdit createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ConfigEdit getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ConfigEdit>(create);
  static ConfigEdit? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get path => $_getSZ(0);
  @$pb.TagNumber(1)
  set path($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPath() => $_has(0);
  @$pb.TagNumber(1)
  void clearPath() => $_clearField(1);

  @$pb.TagNumber(2)
  $1.JsonValue get value => $_getN(1);
  @$pb.TagNumber(2)
  set value($1.JsonValue value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasValue() => $_has(1);
  @$pb.TagNumber(2)
  void clearValue() => $_clearField(2);
  @$pb.TagNumber(2)
  $1.JsonValue ensureValue() => $_ensure(1);
}

/// One validation finding: the offending path (`""` for a whole-document
/// finding), a typed `code` (the closed `ConfigIssueCode` set, snake_case), and a
/// human detail line.
class ConfigIssue extends $pb.GeneratedMessage {
  factory ConfigIssue({
    $core.String? path,
    $core.String? code,
    $core.String? detail,
  }) {
    final result = create();
    if (path != null) result.path = path;
    if (code != null) result.code = code;
    if (detail != null) result.detail = detail;
    return result;
  }

  ConfigIssue._();

  factory ConfigIssue.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ConfigIssue.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ConfigIssue',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'path')
    ..aOS(2, _omitFieldNames ? '' : 'code')
    ..aOS(3, _omitFieldNames ? '' : 'detail')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ConfigIssue clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ConfigIssue copyWith(void Function(ConfigIssue) updates) =>
      super.copyWith((message) => updates(message as ConfigIssue))
          as ConfigIssue;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ConfigIssue create() => ConfigIssue._();
  @$core.override
  ConfigIssue createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ConfigIssue getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ConfigIssue>(create);
  static ConfigIssue? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get path => $_getSZ(0);
  @$pb.TagNumber(1)
  set path($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPath() => $_has(0);
  @$pb.TagNumber(1)
  void clearPath() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get code => $_getSZ(1);
  @$pb.TagNumber(2)
  set code($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCode() => $_has(1);
  @$pb.TagNumber(2)
  void clearCode() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get detail => $_getSZ(2);
  @$pb.TagNumber(3)
  set detail($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasDetail() => $_has(2);
  @$pb.TagNumber(3)
  void clearDetail() => $_clearField(3);
}

class ValidateConfigRequest extends $pb.GeneratedMessage {
  factory ValidateConfigRequest({
    $core.Iterable<ConfigEdit>? edits,
  }) {
    final result = create();
    if (edits != null) result.edits.addAll(edits);
    return result;
  }

  ValidateConfigRequest._();

  factory ValidateConfigRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ValidateConfigRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ValidateConfigRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<ConfigEdit>(1, _omitFieldNames ? '' : 'edits',
        subBuilder: ConfigEdit.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ValidateConfigRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ValidateConfigRequest copyWith(
          void Function(ValidateConfigRequest) updates) =>
      super.copyWith((message) => updates(message as ValidateConfigRequest))
          as ValidateConfigRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ValidateConfigRequest create() => ValidateConfigRequest._();
  @$core.override
  ValidateConfigRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ValidateConfigRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ValidateConfigRequest>(create);
  static ValidateConfigRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<ConfigEdit> get edits => $_getList(0);
}

class ValidateConfigResponse extends $pb.GeneratedMessage {
  factory ValidateConfigResponse({
    $core.Iterable<ConfigIssue>? issues,
  }) {
    final result = create();
    if (issues != null) result.issues.addAll(issues);
    return result;
  }

  ValidateConfigResponse._();

  factory ValidateConfigResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ValidateConfigResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ValidateConfigResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<ConfigIssue>(1, _omitFieldNames ? '' : 'issues',
        subBuilder: ConfigIssue.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ValidateConfigResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ValidateConfigResponse copyWith(
          void Function(ValidateConfigResponse) updates) =>
      super.copyWith((message) => updates(message as ValidateConfigResponse))
          as ValidateConfigResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ValidateConfigResponse create() => ValidateConfigResponse._();
  @$core.override
  ValidateConfigResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ValidateConfigResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ValidateConfigResponse>(create);
  static ValidateConfigResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<ConfigIssue> get issues => $_getList(0);
}

class PutConfigRequest extends $pb.GeneratedMessage {
  factory PutConfigRequest({
    $core.Iterable<ConfigEdit>? edits,
  }) {
    final result = create();
    if (edits != null) result.edits.addAll(edits);
    return result;
  }

  PutConfigRequest._();

  factory PutConfigRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PutConfigRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PutConfigRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<ConfigEdit>(1, _omitFieldNames ? '' : 'edits',
        subBuilder: ConfigEdit.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutConfigRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutConfigRequest copyWith(void Function(PutConfigRequest) updates) =>
      super.copyWith((message) => updates(message as PutConfigRequest))
          as PutConfigRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PutConfigRequest create() => PutConfigRequest._();
  @$core.override
  PutConfigRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PutConfigRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PutConfigRequest>(create);
  static PutConfigRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<ConfigEdit> get edits => $_getList(0);
}

/// Non-empty `issues` means the patch was rejected and **nothing was written**.
class PutConfigResponse extends $pb.GeneratedMessage {
  factory PutConfigResponse({
    $core.bool? restartRequired,
    $core.Iterable<ConfigIssue>? issues,
  }) {
    final result = create();
    if (restartRequired != null) result.restartRequired = restartRequired;
    if (issues != null) result.issues.addAll(issues);
    return result;
  }

  PutConfigResponse._();

  factory PutConfigResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PutConfigResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PutConfigResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'restartRequired')
    ..pPM<ConfigIssue>(2, _omitFieldNames ? '' : 'issues',
        subBuilder: ConfigIssue.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutConfigResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutConfigResponse copyWith(void Function(PutConfigResponse) updates) =>
      super.copyWith((message) => updates(message as PutConfigResponse))
          as PutConfigResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PutConfigResponse create() => PutConfigResponse._();
  @$core.override
  PutConfigResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PutConfigResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PutConfigResponse>(create);
  static PutConfigResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $core.bool get restartRequired => $_getBF(0);
  @$pb.TagNumber(1)
  set restartRequired($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRestartRequired() => $_has(0);
  @$pb.TagNumber(1)
  void clearRestartRequired() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<ConfigIssue> get issues => $_getList(1);
}

class ConfigStatusRequest extends $pb.GeneratedMessage {
  factory ConfigStatusRequest() => create();

  ConfigStatusRequest._();

  factory ConfigStatusRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ConfigStatusRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ConfigStatusRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ConfigStatusRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ConfigStatusRequest copyWith(void Function(ConfigStatusRequest) updates) =>
      super.copyWith((message) => updates(message as ConfigStatusRequest))
          as ConfigStatusRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ConfigStatusRequest create() => ConfigStatusRequest._();
  @$core.override
  ConfigStatusRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ConfigStatusRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ConfigStatusRequest>(create);
  static ConfigStatusRequest? _defaultInstance;
}

/// Drift between the on-disk file and the running snapshot: `pending` is the set
/// of leaf paths that differ, `restart_required` is `!pending.is_empty()`.
class ConfigStatus extends $pb.GeneratedMessage {
  factory ConfigStatus({
    $core.bool? restartRequired,
    $core.Iterable<ConfigEdit>? pending,
    $core.String? loadedHash,
    $core.String? ondiskHash,
  }) {
    final result = create();
    if (restartRequired != null) result.restartRequired = restartRequired;
    if (pending != null) result.pending.addAll(pending);
    if (loadedHash != null) result.loadedHash = loadedHash;
    if (ondiskHash != null) result.ondiskHash = ondiskHash;
    return result;
  }

  ConfigStatus._();

  factory ConfigStatus.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ConfigStatus.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ConfigStatus',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'restartRequired')
    ..pPM<ConfigEdit>(2, _omitFieldNames ? '' : 'pending',
        subBuilder: ConfigEdit.create)
    ..aOS(3, _omitFieldNames ? '' : 'loadedHash')
    ..aOS(4, _omitFieldNames ? '' : 'ondiskHash')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ConfigStatus clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ConfigStatus copyWith(void Function(ConfigStatus) updates) =>
      super.copyWith((message) => updates(message as ConfigStatus))
          as ConfigStatus;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ConfigStatus create() => ConfigStatus._();
  @$core.override
  ConfigStatus createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ConfigStatus getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ConfigStatus>(create);
  static ConfigStatus? _defaultInstance;

  @$pb.TagNumber(1)
  $core.bool get restartRequired => $_getBF(0);
  @$pb.TagNumber(1)
  set restartRequired($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRestartRequired() => $_has(0);
  @$pb.TagNumber(1)
  void clearRestartRequired() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<ConfigEdit> get pending => $_getList(1);

  @$pb.TagNumber(3)
  $core.String get loadedHash => $_getSZ(2);
  @$pb.TagNumber(3)
  set loadedHash($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasLoadedHash() => $_has(2);
  @$pb.TagNumber(3)
  void clearLoadedHash() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get ondiskHash => $_getSZ(3);
  @$pb.TagNumber(4)
  set ondiskHash($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasOndiskHash() => $_has(3);
  @$pb.TagNumber(4)
  void clearOndiskHash() => $_clearField(4);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
