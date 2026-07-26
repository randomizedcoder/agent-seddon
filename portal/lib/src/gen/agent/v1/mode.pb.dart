// This is a generated file - do not edit.
//
// Generated from agent/v1/mode.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

import 'common.pb.dart' as $1;
import 'mode.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'mode.pbenum.dart';

/// What a classifier saw the incoming work as. `confidence` is untrusted (a pool
/// vote's self-report) — clamp to 0..=1 on receipt.
class ModeVerdict extends $pb.GeneratedMessage {
  factory ModeVerdict({
    TaskMode? mode,
    $core.double? confidence,
    $core.String? reason,
  }) {
    final result = create();
    if (mode != null) result.mode = mode;
    if (confidence != null) result.confidence = confidence;
    if (reason != null) result.reason = reason;
    return result;
  }

  ModeVerdict._();

  factory ModeVerdict.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ModeVerdict.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ModeVerdict',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aE<TaskMode>(1, _omitFieldNames ? '' : 'mode',
        enumValues: TaskMode.values)
    ..aD(2, _omitFieldNames ? '' : 'confidence', fieldType: $pb.PbFieldType.OF)
    ..aOS(3, _omitFieldNames ? '' : 'reason')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ModeVerdict clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ModeVerdict copyWith(void Function(ModeVerdict) updates) =>
      super.copyWith((message) => updates(message as ModeVerdict))
          as ModeVerdict;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ModeVerdict create() => ModeVerdict._();
  @$core.override
  ModeVerdict createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ModeVerdict getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ModeVerdict>(create);
  static ModeVerdict? _defaultInstance;

  @$pb.TagNumber(1)
  TaskMode get mode => $_getN(0);
  @$pb.TagNumber(1)
  set mode(TaskMode value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasMode() => $_has(0);
  @$pb.TagNumber(1)
  void clearMode() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.double get confidence => $_getN(1);
  @$pb.TagNumber(2)
  set confidence($core.double value) => $_setFloat(1, value);
  @$pb.TagNumber(2)
  $core.bool hasConfidence() => $_has(1);
  @$pb.TagNumber(2)
  void clearConfidence() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get reason => $_getSZ(2);
  @$pb.TagNumber(3)
  set reason($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasReason() => $_has(2);
  @$pb.TagNumber(3)
  void clearReason() => $_clearField(3);
}

/// A decided change of task mode (recorded; drives compaction + memory later).
class ModeSwitch extends $pb.GeneratedMessage {
  factory ModeSwitch({
    TaskMode? from,
    TaskMode? to,
    $core.String? reason,
    $core.double? confidence,
  }) {
    final result = create();
    if (from != null) result.from = from;
    if (to != null) result.to = to;
    if (reason != null) result.reason = reason;
    if (confidence != null) result.confidence = confidence;
    return result;
  }

  ModeSwitch._();

  factory ModeSwitch.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ModeSwitch.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ModeSwitch',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aE<TaskMode>(1, _omitFieldNames ? '' : 'from',
        enumValues: TaskMode.values)
    ..aE<TaskMode>(2, _omitFieldNames ? '' : 'to', enumValues: TaskMode.values)
    ..aOS(3, _omitFieldNames ? '' : 'reason')
    ..aD(4, _omitFieldNames ? '' : 'confidence', fieldType: $pb.PbFieldType.OF)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ModeSwitch clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ModeSwitch copyWith(void Function(ModeSwitch) updates) =>
      super.copyWith((message) => updates(message as ModeSwitch)) as ModeSwitch;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ModeSwitch create() => ModeSwitch._();
  @$core.override
  ModeSwitch createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ModeSwitch getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ModeSwitch>(create);
  static ModeSwitch? _defaultInstance;

  @$pb.TagNumber(1)
  TaskMode get from => $_getN(0);
  @$pb.TagNumber(1)
  set from(TaskMode value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasFrom() => $_has(0);
  @$pb.TagNumber(1)
  void clearFrom() => $_clearField(1);

  @$pb.TagNumber(2)
  TaskMode get to => $_getN(1);
  @$pb.TagNumber(2)
  set to(TaskMode value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasTo() => $_has(1);
  @$pb.TagNumber(2)
  void clearTo() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get reason => $_getSZ(2);
  @$pb.TagNumber(3)
  set reason($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasReason() => $_has(2);
  @$pb.TagNumber(3)
  void clearReason() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.double get confidence => $_getN(3);
  @$pb.TagNumber(4)
  set confidence($core.double value) => $_setFloat(3, value);
  @$pb.TagNumber(4)
  $core.bool hasConfidence() => $_has(3);
  @$pb.TagNumber(4)
  void clearConfidence() => $_clearField(4);
}

/// The context a classifier judges. The prompt/history are bounded server-side and
/// never logged verbatim (a hash stands in for the prompt).
class ClassifyRequest extends $pb.GeneratedMessage {
  factory ClassifyRequest({
    $core.String? prompt,
    $core.Iterable<$1.Message>? history,
  }) {
    final result = create();
    if (prompt != null) result.prompt = prompt;
    if (history != null) result.history.addAll(history);
    return result;
  }

  ClassifyRequest._();

  factory ClassifyRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ClassifyRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ClassifyRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'prompt')
    ..pPM<$1.Message>(2, _omitFieldNames ? '' : 'history',
        subBuilder: $1.Message.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ClassifyRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ClassifyRequest copyWith(void Function(ClassifyRequest) updates) =>
      super.copyWith((message) => updates(message as ClassifyRequest))
          as ClassifyRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ClassifyRequest create() => ClassifyRequest._();
  @$core.override
  ClassifyRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ClassifyRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ClassifyRequest>(create);
  static ClassifyRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get prompt => $_getSZ(0);
  @$pb.TagNumber(1)
  set prompt($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPrompt() => $_has(0);
  @$pb.TagNumber(1)
  void clearPrompt() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<$1.Message> get history => $_getList(1);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
