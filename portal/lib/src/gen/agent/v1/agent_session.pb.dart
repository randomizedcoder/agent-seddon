// This is a generated file - do not edit.
//
// Generated from agent/v1/agent_session.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

class SubscribeRequest extends $pb.GeneratedMessage {
  factory SubscribeRequest() => create();

  SubscribeRequest._();

  factory SubscribeRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SubscribeRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SubscribeRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SubscribeRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SubscribeRequest copyWith(void Function(SubscribeRequest) updates) =>
      super.copyWith((message) => updates(message as SubscribeRequest))
          as SubscribeRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SubscribeRequest create() => SubscribeRequest._();
  @$core.override
  SubscribeRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SubscribeRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SubscribeRequest>(create);
  static SubscribeRequest? _defaultInstance;
}

class SnapshotRequest extends $pb.GeneratedMessage {
  factory SnapshotRequest() => create();

  SnapshotRequest._();

  factory SnapshotRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SnapshotRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SnapshotRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SnapshotRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SnapshotRequest copyWith(void Function(SnapshotRequest) updates) =>
      super.copyWith((message) => updates(message as SnapshotRequest))
          as SnapshotRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SnapshotRequest create() => SnapshotRequest._();
  @$core.override
  SnapshotRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SnapshotRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SnapshotRequest>(create);
  static SnapshotRequest? _defaultInstance;
}

class RunStarted extends $pb.GeneratedMessage {
  factory RunStarted({
    $core.String? goal,
  }) {
    final result = create();
    if (goal != null) result.goal = goal;
    return result;
  }

  RunStarted._();

  factory RunStarted.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RunStarted.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RunStarted',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'goal')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RunStarted clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RunStarted copyWith(void Function(RunStarted) updates) =>
      super.copyWith((message) => updates(message as RunStarted)) as RunStarted;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RunStarted create() => RunStarted._();
  @$core.override
  RunStarted createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RunStarted getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RunStarted>(create);
  static RunStarted? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get goal => $_getSZ(0);
  @$pb.TagNumber(1)
  set goal($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasGoal() => $_has(0);
  @$pb.TagNumber(1)
  void clearGoal() => $_clearField(1);
}

class IterationStart extends $pb.GeneratedMessage {
  factory IterationStart({
    $core.int? iter,
  }) {
    final result = create();
    if (iter != null) result.iter = iter;
    return result;
  }

  IterationStart._();

  factory IterationStart.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory IterationStart.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'IterationStart',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aI(1, _omitFieldNames ? '' : 'iter', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  IterationStart clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  IterationStart copyWith(void Function(IterationStart) updates) =>
      super.copyWith((message) => updates(message as IterationStart))
          as IterationStart;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static IterationStart create() => IterationStart._();
  @$core.override
  IterationStart createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static IterationStart getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<IterationStart>(create);
  static IterationStart? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get iter => $_getIZ(0);
  @$pb.TagNumber(1)
  set iter($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasIter() => $_has(0);
  @$pb.TagNumber(1)
  void clearIter() => $_clearField(1);
}

class TokenDelta extends $pb.GeneratedMessage {
  factory TokenDelta({
    $core.String? text,
  }) {
    final result = create();
    if (text != null) result.text = text;
    return result;
  }

  TokenDelta._();

  factory TokenDelta.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory TokenDelta.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'TokenDelta',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'text')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TokenDelta clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TokenDelta copyWith(void Function(TokenDelta) updates) =>
      super.copyWith((message) => updates(message as TokenDelta)) as TokenDelta;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static TokenDelta create() => TokenDelta._();
  @$core.override
  TokenDelta createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static TokenDelta getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<TokenDelta>(create);
  static TokenDelta? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get text => $_getSZ(0);
  @$pb.TagNumber(1)
  set text($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasText() => $_has(0);
  @$pb.TagNumber(1)
  void clearText() => $_clearField(1);
}

class ToolCallStart extends $pb.GeneratedMessage {
  factory ToolCallStart({
    $core.String? name,
    $core.String? args,
  }) {
    final result = create();
    if (name != null) result.name = name;
    if (args != null) result.args = args;
    return result;
  }

  ToolCallStart._();

  factory ToolCallStart.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ToolCallStart.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ToolCallStart',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'name')
    ..aOS(2, _omitFieldNames ? '' : 'args')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ToolCallStart clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ToolCallStart copyWith(void Function(ToolCallStart) updates) =>
      super.copyWith((message) => updates(message as ToolCallStart))
          as ToolCallStart;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ToolCallStart create() => ToolCallStart._();
  @$core.override
  ToolCallStart createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ToolCallStart getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ToolCallStart>(create);
  static ToolCallStart? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get name => $_getSZ(0);
  @$pb.TagNumber(1)
  set name($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasName() => $_has(0);
  @$pb.TagNumber(1)
  void clearName() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get args => $_getSZ(1);
  @$pb.TagNumber(2)
  set args($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasArgs() => $_has(1);
  @$pb.TagNumber(2)
  void clearArgs() => $_clearField(2);
}

class ToolCallResult extends $pb.GeneratedMessage {
  factory ToolCallResult({
    $core.String? name,
    $core.bool? ok,
    $fixnum.Int64? durationMs,
  }) {
    final result = create();
    if (name != null) result.name = name;
    if (ok != null) result.ok = ok;
    if (durationMs != null) result.durationMs = durationMs;
    return result;
  }

  ToolCallResult._();

  factory ToolCallResult.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ToolCallResult.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ToolCallResult',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'name')
    ..aOB(2, _omitFieldNames ? '' : 'ok')
    ..a<$fixnum.Int64>(
        3, _omitFieldNames ? '' : 'durationMs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ToolCallResult clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ToolCallResult copyWith(void Function(ToolCallResult) updates) =>
      super.copyWith((message) => updates(message as ToolCallResult))
          as ToolCallResult;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ToolCallResult create() => ToolCallResult._();
  @$core.override
  ToolCallResult createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ToolCallResult getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ToolCallResult>(create);
  static ToolCallResult? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get name => $_getSZ(0);
  @$pb.TagNumber(1)
  set name($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasName() => $_has(0);
  @$pb.TagNumber(1)
  void clearName() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.bool get ok => $_getBF(1);
  @$pb.TagNumber(2)
  set ok($core.bool value) => $_setBool(1, value);
  @$pb.TagNumber(2)
  $core.bool hasOk() => $_has(1);
  @$pb.TagNumber(2)
  void clearOk() => $_clearField(2);

  @$pb.TagNumber(3)
  $fixnum.Int64 get durationMs => $_getI64(2);
  @$pb.TagNumber(3)
  set durationMs($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasDurationMs() => $_has(2);
  @$pb.TagNumber(3)
  void clearDurationMs() => $_clearField(3);
}

/// Named `Session`-prefixed: the flat `agent.v1` package already has a `ModeSwitch`
/// in mode.proto (see the same note in session.proto about global message names).
class SessionModeSwitch extends $pb.GeneratedMessage {
  factory SessionModeSwitch({
    $core.String? from,
    $core.String? to,
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

  SessionModeSwitch._();

  factory SessionModeSwitch.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SessionModeSwitch.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SessionModeSwitch',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'from')
    ..aOS(2, _omitFieldNames ? '' : 'to')
    ..aOS(3, _omitFieldNames ? '' : 'reason')
    ..aD(4, _omitFieldNames ? '' : 'confidence', fieldType: $pb.PbFieldType.OF)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionModeSwitch clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionModeSwitch copyWith(void Function(SessionModeSwitch) updates) =>
      super.copyWith((message) => updates(message as SessionModeSwitch))
          as SessionModeSwitch;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SessionModeSwitch create() => SessionModeSwitch._();
  @$core.override
  SessionModeSwitch createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SessionModeSwitch getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SessionModeSwitch>(create);
  static SessionModeSwitch? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get from => $_getSZ(0);
  @$pb.TagNumber(1)
  set from($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasFrom() => $_has(0);
  @$pb.TagNumber(1)
  void clearFrom() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get to => $_getSZ(1);
  @$pb.TagNumber(2)
  set to($core.String value) => $_setString(1, value);
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

class ContextUpdate extends $pb.GeneratedMessage {
  factory ContextUpdate({
    $core.int? promptTokens,
    $core.int? contextWindow,
    $core.int? messages,
  }) {
    final result = create();
    if (promptTokens != null) result.promptTokens = promptTokens;
    if (contextWindow != null) result.contextWindow = contextWindow;
    if (messages != null) result.messages = messages;
    return result;
  }

  ContextUpdate._();

  factory ContextUpdate.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ContextUpdate.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ContextUpdate',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aI(1, _omitFieldNames ? '' : 'promptTokens',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(2, _omitFieldNames ? '' : 'contextWindow',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(3, _omitFieldNames ? '' : 'messages', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ContextUpdate clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ContextUpdate copyWith(void Function(ContextUpdate) updates) =>
      super.copyWith((message) => updates(message as ContextUpdate))
          as ContextUpdate;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ContextUpdate create() => ContextUpdate._();
  @$core.override
  ContextUpdate createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ContextUpdate getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ContextUpdate>(create);
  static ContextUpdate? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get promptTokens => $_getIZ(0);
  @$pb.TagNumber(1)
  set promptTokens($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPromptTokens() => $_has(0);
  @$pb.TagNumber(1)
  void clearPromptTokens() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get contextWindow => $_getIZ(1);
  @$pb.TagNumber(2)
  set contextWindow($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasContextWindow() => $_has(1);
  @$pb.TagNumber(2)
  void clearContextWindow() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get messages => $_getIZ(2);
  @$pb.TagNumber(3)
  set messages($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasMessages() => $_has(2);
  @$pb.TagNumber(3)
  void clearMessages() => $_clearField(3);
}

class RunFinished extends $pb.GeneratedMessage {
  factory RunFinished({
    $core.bool? ok,
  }) {
    final result = create();
    if (ok != null) result.ok = ok;
    return result;
  }

  RunFinished._();

  factory RunFinished.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RunFinished.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RunFinished',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'ok')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RunFinished clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RunFinished copyWith(void Function(RunFinished) updates) =>
      super.copyWith((message) => updates(message as RunFinished))
          as RunFinished;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RunFinished create() => RunFinished._();
  @$core.override
  RunFinished createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RunFinished getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RunFinished>(create);
  static RunFinished? _defaultInstance;

  @$pb.TagNumber(1)
  $core.bool get ok => $_getBF(0);
  @$pb.TagNumber(1)
  set ok($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasOk() => $_has(0);
  @$pb.TagNumber(1)
  void clearOk() => $_clearField(1);
}

class StatusSnapshot extends $pb.GeneratedMessage {
  factory StatusSnapshot({
    $core.String? currentMode,
    $core.int? contextTokens,
    $core.int? contextWindow,
    $core.int? contextMessages,
    $core.bool? active,
  }) {
    final result = create();
    if (currentMode != null) result.currentMode = currentMode;
    if (contextTokens != null) result.contextTokens = contextTokens;
    if (contextWindow != null) result.contextWindow = contextWindow;
    if (contextMessages != null) result.contextMessages = contextMessages;
    if (active != null) result.active = active;
    return result;
  }

  StatusSnapshot._();

  factory StatusSnapshot.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory StatusSnapshot.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'StatusSnapshot',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'currentMode')
    ..aI(2, _omitFieldNames ? '' : 'contextTokens',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(3, _omitFieldNames ? '' : 'contextWindow',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(4, _omitFieldNames ? '' : 'contextMessages',
        fieldType: $pb.PbFieldType.OU3)
    ..aOB(5, _omitFieldNames ? '' : 'active')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  StatusSnapshot clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  StatusSnapshot copyWith(void Function(StatusSnapshot) updates) =>
      super.copyWith((message) => updates(message as StatusSnapshot))
          as StatusSnapshot;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static StatusSnapshot create() => StatusSnapshot._();
  @$core.override
  StatusSnapshot createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static StatusSnapshot getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<StatusSnapshot>(create);
  static StatusSnapshot? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get currentMode => $_getSZ(0);
  @$pb.TagNumber(1)
  set currentMode($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasCurrentMode() => $_has(0);
  @$pb.TagNumber(1)
  void clearCurrentMode() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get contextTokens => $_getIZ(1);
  @$pb.TagNumber(2)
  set contextTokens($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasContextTokens() => $_has(1);
  @$pb.TagNumber(2)
  void clearContextTokens() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get contextWindow => $_getIZ(2);
  @$pb.TagNumber(3)
  set contextWindow($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasContextWindow() => $_has(2);
  @$pb.TagNumber(3)
  void clearContextWindow() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get contextMessages => $_getIZ(3);
  @$pb.TagNumber(4)
  set contextMessages($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasContextMessages() => $_has(3);
  @$pb.TagNumber(4)
  void clearContextMessages() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.bool get active => $_getBF(4);
  @$pb.TagNumber(5)
  set active($core.bool value) => $_setBool(4, value);
  @$pb.TagNumber(5)
  $core.bool hasActive() => $_has(4);
  @$pb.TagNumber(5)
  void clearActive() => $_clearField(5);
}

enum SessionEvent_Kind {
  runStarted,
  iteration,
  token,
  toolStart,
  toolResult,
  modeSwitch,
  contextUpdate,
  runFinished,
  statusSnapshot,
  notSet
}

class SessionEvent extends $pb.GeneratedMessage {
  factory SessionEvent({
    RunStarted? runStarted,
    IterationStart? iteration,
    TokenDelta? token,
    ToolCallStart? toolStart,
    ToolCallResult? toolResult,
    SessionModeSwitch? modeSwitch,
    ContextUpdate? contextUpdate,
    RunFinished? runFinished,
    StatusSnapshot? statusSnapshot,
  }) {
    final result = create();
    if (runStarted != null) result.runStarted = runStarted;
    if (iteration != null) result.iteration = iteration;
    if (token != null) result.token = token;
    if (toolStart != null) result.toolStart = toolStart;
    if (toolResult != null) result.toolResult = toolResult;
    if (modeSwitch != null) result.modeSwitch = modeSwitch;
    if (contextUpdate != null) result.contextUpdate = contextUpdate;
    if (runFinished != null) result.runFinished = runFinished;
    if (statusSnapshot != null) result.statusSnapshot = statusSnapshot;
    return result;
  }

  SessionEvent._();

  factory SessionEvent.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SessionEvent.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static const $core.Map<$core.int, SessionEvent_Kind> _SessionEvent_KindByTag =
      {
    1: SessionEvent_Kind.runStarted,
    2: SessionEvent_Kind.iteration,
    3: SessionEvent_Kind.token,
    4: SessionEvent_Kind.toolStart,
    5: SessionEvent_Kind.toolResult,
    6: SessionEvent_Kind.modeSwitch,
    7: SessionEvent_Kind.contextUpdate,
    8: SessionEvent_Kind.runFinished,
    9: SessionEvent_Kind.statusSnapshot,
    0: SessionEvent_Kind.notSet
  };
  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SessionEvent',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..oo(0, [1, 2, 3, 4, 5, 6, 7, 8, 9])
    ..aOM<RunStarted>(1, _omitFieldNames ? '' : 'runStarted',
        subBuilder: RunStarted.create)
    ..aOM<IterationStart>(2, _omitFieldNames ? '' : 'iteration',
        subBuilder: IterationStart.create)
    ..aOM<TokenDelta>(3, _omitFieldNames ? '' : 'token',
        subBuilder: TokenDelta.create)
    ..aOM<ToolCallStart>(4, _omitFieldNames ? '' : 'toolStart',
        subBuilder: ToolCallStart.create)
    ..aOM<ToolCallResult>(5, _omitFieldNames ? '' : 'toolResult',
        subBuilder: ToolCallResult.create)
    ..aOM<SessionModeSwitch>(6, _omitFieldNames ? '' : 'modeSwitch',
        subBuilder: SessionModeSwitch.create)
    ..aOM<ContextUpdate>(7, _omitFieldNames ? '' : 'contextUpdate',
        subBuilder: ContextUpdate.create)
    ..aOM<RunFinished>(8, _omitFieldNames ? '' : 'runFinished',
        subBuilder: RunFinished.create)
    ..aOM<StatusSnapshot>(9, _omitFieldNames ? '' : 'statusSnapshot',
        subBuilder: StatusSnapshot.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionEvent clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionEvent copyWith(void Function(SessionEvent) updates) =>
      super.copyWith((message) => updates(message as SessionEvent))
          as SessionEvent;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SessionEvent create() => SessionEvent._();
  @$core.override
  SessionEvent createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SessionEvent getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SessionEvent>(create);
  static SessionEvent? _defaultInstance;

  @$pb.TagNumber(1)
  @$pb.TagNumber(2)
  @$pb.TagNumber(3)
  @$pb.TagNumber(4)
  @$pb.TagNumber(5)
  @$pb.TagNumber(6)
  @$pb.TagNumber(7)
  @$pb.TagNumber(8)
  @$pb.TagNumber(9)
  SessionEvent_Kind whichKind() => _SessionEvent_KindByTag[$_whichOneof(0)]!;
  @$pb.TagNumber(1)
  @$pb.TagNumber(2)
  @$pb.TagNumber(3)
  @$pb.TagNumber(4)
  @$pb.TagNumber(5)
  @$pb.TagNumber(6)
  @$pb.TagNumber(7)
  @$pb.TagNumber(8)
  @$pb.TagNumber(9)
  void clearKind() => $_clearField($_whichOneof(0));

  @$pb.TagNumber(1)
  RunStarted get runStarted => $_getN(0);
  @$pb.TagNumber(1)
  set runStarted(RunStarted value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasRunStarted() => $_has(0);
  @$pb.TagNumber(1)
  void clearRunStarted() => $_clearField(1);
  @$pb.TagNumber(1)
  RunStarted ensureRunStarted() => $_ensure(0);

  @$pb.TagNumber(2)
  IterationStart get iteration => $_getN(1);
  @$pb.TagNumber(2)
  set iteration(IterationStart value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasIteration() => $_has(1);
  @$pb.TagNumber(2)
  void clearIteration() => $_clearField(2);
  @$pb.TagNumber(2)
  IterationStart ensureIteration() => $_ensure(1);

  @$pb.TagNumber(3)
  TokenDelta get token => $_getN(2);
  @$pb.TagNumber(3)
  set token(TokenDelta value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasToken() => $_has(2);
  @$pb.TagNumber(3)
  void clearToken() => $_clearField(3);
  @$pb.TagNumber(3)
  TokenDelta ensureToken() => $_ensure(2);

  @$pb.TagNumber(4)
  ToolCallStart get toolStart => $_getN(3);
  @$pb.TagNumber(4)
  set toolStart(ToolCallStart value) => $_setField(4, value);
  @$pb.TagNumber(4)
  $core.bool hasToolStart() => $_has(3);
  @$pb.TagNumber(4)
  void clearToolStart() => $_clearField(4);
  @$pb.TagNumber(4)
  ToolCallStart ensureToolStart() => $_ensure(3);

  @$pb.TagNumber(5)
  ToolCallResult get toolResult => $_getN(4);
  @$pb.TagNumber(5)
  set toolResult(ToolCallResult value) => $_setField(5, value);
  @$pb.TagNumber(5)
  $core.bool hasToolResult() => $_has(4);
  @$pb.TagNumber(5)
  void clearToolResult() => $_clearField(5);
  @$pb.TagNumber(5)
  ToolCallResult ensureToolResult() => $_ensure(4);

  @$pb.TagNumber(6)
  SessionModeSwitch get modeSwitch => $_getN(5);
  @$pb.TagNumber(6)
  set modeSwitch(SessionModeSwitch value) => $_setField(6, value);
  @$pb.TagNumber(6)
  $core.bool hasModeSwitch() => $_has(5);
  @$pb.TagNumber(6)
  void clearModeSwitch() => $_clearField(6);
  @$pb.TagNumber(6)
  SessionModeSwitch ensureModeSwitch() => $_ensure(5);

  @$pb.TagNumber(7)
  ContextUpdate get contextUpdate => $_getN(6);
  @$pb.TagNumber(7)
  set contextUpdate(ContextUpdate value) => $_setField(7, value);
  @$pb.TagNumber(7)
  $core.bool hasContextUpdate() => $_has(6);
  @$pb.TagNumber(7)
  void clearContextUpdate() => $_clearField(7);
  @$pb.TagNumber(7)
  ContextUpdate ensureContextUpdate() => $_ensure(6);

  @$pb.TagNumber(8)
  RunFinished get runFinished => $_getN(7);
  @$pb.TagNumber(8)
  set runFinished(RunFinished value) => $_setField(8, value);
  @$pb.TagNumber(8)
  $core.bool hasRunFinished() => $_has(7);
  @$pb.TagNumber(8)
  void clearRunFinished() => $_clearField(8);
  @$pb.TagNumber(8)
  RunFinished ensureRunFinished() => $_ensure(7);

  /// The initial snapshot delivered first on Subscribe, so a late joiner is
  /// consistent before the live tail arrives.
  @$pb.TagNumber(9)
  StatusSnapshot get statusSnapshot => $_getN(8);
  @$pb.TagNumber(9)
  set statusSnapshot(StatusSnapshot value) => $_setField(9, value);
  @$pb.TagNumber(9)
  $core.bool hasStatusSnapshot() => $_has(8);
  @$pb.TagNumber(9)
  void clearStatusSnapshot() => $_clearField(9);
  @$pb.TagNumber(9)
  StatusSnapshot ensureStatusSnapshot() => $_ensure(8);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
