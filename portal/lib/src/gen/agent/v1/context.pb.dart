// This is a generated file - do not edit.
//
// Generated from agent/v1/context.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

import 'common.pb.dart' as $0;
import 'mode.pbenum.dart' as $2;

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

class AssembleResponse extends $pb.GeneratedMessage {
  factory AssembleResponse({
    $core.Iterable<$0.Message>? messages,
  }) {
    final result = create();
    if (messages != null) result.messages.addAll(messages);
    return result;
  }

  AssembleResponse._();

  factory AssembleResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory AssembleResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'AssembleResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<$0.Message>(1, _omitFieldNames ? '' : 'messages',
        subBuilder: $0.Message.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AssembleResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AssembleResponse copyWith(void Function(AssembleResponse) updates) =>
      super.copyWith((message) => updates(message as AssembleResponse))
          as AssembleResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static AssembleResponse create() => AssembleResponse._();
  @$core.override
  AssembleResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static AssembleResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<AssembleResponse>(create);
  static AssembleResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<$0.Message> get messages => $_getList(0);
}

class CompactRequest extends $pb.GeneratedMessage {
  factory CompactRequest({
    $0.WorkingSet? working,
    $0.TokenBudget? budget,
    $2.TaskMode? fromMode,
    $2.TaskMode? toMode,
  }) {
    final result = create();
    if (working != null) result.working = working;
    if (budget != null) result.budget = budget;
    if (fromMode != null) result.fromMode = fromMode;
    if (toMode != null) result.toMode = toMode;
    return result;
  }

  CompactRequest._();

  factory CompactRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CompactRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CompactRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<$0.WorkingSet>(1, _omitFieldNames ? '' : 'working',
        subBuilder: $0.WorkingSet.create)
    ..aOM<$0.TokenBudget>(2, _omitFieldNames ? '' : 'budget',
        subBuilder: $0.TokenBudget.create)
    ..aE<$2.TaskMode>(3, _omitFieldNames ? '' : 'fromMode',
        enumValues: $2.TaskMode.values)
    ..aE<$2.TaskMode>(4, _omitFieldNames ? '' : 'toMode',
        enumValues: $2.TaskMode.values)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CompactRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CompactRequest copyWith(void Function(CompactRequest) updates) =>
      super.copyWith((message) => updates(message as CompactRequest))
          as CompactRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CompactRequest create() => CompactRequest._();
  @$core.override
  CompactRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CompactRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<CompactRequest>(create);
  static CompactRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $0.WorkingSet get working => $_getN(0);
  @$pb.TagNumber(1)
  set working($0.WorkingSet value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasWorking() => $_has(0);
  @$pb.TagNumber(1)
  void clearWorking() => $_clearField(1);
  @$pb.TagNumber(1)
  $0.WorkingSet ensureWorking() => $_ensure(0);

  @$pb.TagNumber(2)
  $0.TokenBudget get budget => $_getN(1);
  @$pb.TagNumber(2)
  set budget($0.TokenBudget value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasBudget() => $_has(1);
  @$pb.TagNumber(2)
  void clearBudget() => $_clearField(2);
  @$pb.TagNumber(2)
  $0.TokenBudget ensureBudget() => $_ensure(1);

  /// Mode-aware compaction (adaptive-cognition 02), additive. UNSPECIFIED (the
  /// default for an old client) ⇒ an ordinary budget-triggered compaction. A
  /// switch reshape is requested when `to_mode` is set and differs from `from_mode`.
  @$pb.TagNumber(3)
  $2.TaskMode get fromMode => $_getN(2);
  @$pb.TagNumber(3)
  set fromMode($2.TaskMode value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasFromMode() => $_has(2);
  @$pb.TagNumber(3)
  void clearFromMode() => $_clearField(3);

  @$pb.TagNumber(4)
  $2.TaskMode get toMode => $_getN(3);
  @$pb.TagNumber(4)
  set toMode($2.TaskMode value) => $_setField(4, value);
  @$pb.TagNumber(4)
  $core.bool hasToMode() => $_has(3);
  @$pb.TagNumber(4)
  void clearToMode() => $_clearField(4);
}

/// How a compaction behaved, for the caller's accounting. `action` mirrors
/// `agent_core::CompactAction`: "budget" | "switch" | "fallback-generic" |
/// "fallback-drop". A compaction never errors on a dead summarizer — it returns a
/// fallback result so the caller can see the degradation.
class CompactStats extends $pb.GeneratedMessage {
  factory CompactStats({
    $core.int? keptTokens,
    $core.int? shedTokens,
    $core.String? action,
  }) {
    final result = create();
    if (keptTokens != null) result.keptTokens = keptTokens;
    if (shedTokens != null) result.shedTokens = shedTokens;
    if (action != null) result.action = action;
    return result;
  }

  CompactStats._();

  factory CompactStats.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CompactStats.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CompactStats',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aI(1, _omitFieldNames ? '' : 'keptTokens', fieldType: $pb.PbFieldType.OU3)
    ..aI(2, _omitFieldNames ? '' : 'shedTokens', fieldType: $pb.PbFieldType.OU3)
    ..aOS(3, _omitFieldNames ? '' : 'action')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CompactStats clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CompactStats copyWith(void Function(CompactStats) updates) =>
      super.copyWith((message) => updates(message as CompactStats))
          as CompactStats;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CompactStats create() => CompactStats._();
  @$core.override
  CompactStats createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CompactStats getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<CompactStats>(create);
  static CompactStats? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get keptTokens => $_getIZ(0);
  @$pb.TagNumber(1)
  set keptTokens($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasKeptTokens() => $_has(0);
  @$pb.TagNumber(1)
  void clearKeptTokens() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get shedTokens => $_getIZ(1);
  @$pb.TagNumber(2)
  set shedTokens($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasShedTokens() => $_has(1);
  @$pb.TagNumber(2)
  void clearShedTokens() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get action => $_getSZ(2);
  @$pb.TagNumber(3)
  set action($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasAction() => $_has(2);
  @$pb.TagNumber(3)
  void clearAction() => $_clearField(3);
}

class CompactResponse extends $pb.GeneratedMessage {
  factory CompactResponse({
    $0.WorkingSet? working,
    CompactStats? stats,
  }) {
    final result = create();
    if (working != null) result.working = working;
    if (stats != null) result.stats = stats;
    return result;
  }

  CompactResponse._();

  factory CompactResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CompactResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CompactResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<$0.WorkingSet>(1, _omitFieldNames ? '' : 'working',
        subBuilder: $0.WorkingSet.create)
    ..aOM<CompactStats>(2, _omitFieldNames ? '' : 'stats',
        subBuilder: CompactStats.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CompactResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CompactResponse copyWith(void Function(CompactResponse) updates) =>
      super.copyWith((message) => updates(message as CompactResponse))
          as CompactResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CompactResponse create() => CompactResponse._();
  @$core.override
  CompactResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CompactResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<CompactResponse>(create);
  static CompactResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $0.WorkingSet get working => $_getN(0);
  @$pb.TagNumber(1)
  set working($0.WorkingSet value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasWorking() => $_has(0);
  @$pb.TagNumber(1)
  void clearWorking() => $_clearField(1);
  @$pb.TagNumber(1)
  $0.WorkingSet ensureWorking() => $_ensure(0);

  @$pb.TagNumber(2)
  CompactStats get stats => $_getN(1);
  @$pb.TagNumber(2)
  set stats(CompactStats value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasStats() => $_has(1);
  @$pb.TagNumber(2)
  void clearStats() => $_clearField(2);
  @$pb.TagNumber(2)
  CompactStats ensureStats() => $_ensure(1);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
