// This is a generated file - do not edit.
//
// Generated from agent/v1/prompt.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

import 'prompt.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'prompt.pbenum.dart';

/// Identifies one prompt. `id` is a context.d filename (prepend/append), a TaskMode
/// name (mode lens), or empty (the singleton system prompt).
class PromptRef extends $pb.GeneratedMessage {
  factory PromptRef({
    PromptKind? kind,
    $core.String? id,
  }) {
    final result = create();
    if (kind != null) result.kind = kind;
    if (id != null) result.id = id;
    return result;
  }

  PromptRef._();

  factory PromptRef.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PromptRef.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PromptRef',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aE<PromptKind>(1, _omitFieldNames ? '' : 'kind',
        enumValues: PromptKind.values)
    ..aOS(2, _omitFieldNames ? '' : 'id')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromptRef clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromptRef copyWith(void Function(PromptRef) updates) =>
      super.copyWith((message) => updates(message as PromptRef)) as PromptRef;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PromptRef create() => PromptRef._();
  @$core.override
  PromptRef createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PromptRef getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<PromptRef>(create);
  static PromptRef? _defaultInstance;

  @$pb.TagNumber(1)
  PromptKind get kind => $_getN(0);
  @$pb.TagNumber(1)
  set kind(PromptKind value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasKind() => $_has(0);
  @$pb.TagNumber(1)
  void clearKind() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get id => $_getSZ(1);
  @$pb.TagNumber(2)
  set id($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasId() => $_has(1);
  @$pb.TagNumber(2)
  void clearId() => $_clearField(2);
}

/// One prompt entry. `builtin` is true when `content` is the compiled/config default
/// (no operator override file yet). `order` is the numeric `NNNN` filename prefix for
/// a context.d entry (0 otherwise). `read_only` is reserved (always false today).
class PromptEntry extends $pb.GeneratedMessage {
  factory PromptEntry({
    PromptKind? kind,
    $core.String? id,
    $core.String? content,
    $core.bool? builtin,
    $core.bool? readOnly,
    $core.int? order,
  }) {
    final result = create();
    if (kind != null) result.kind = kind;
    if (id != null) result.id = id;
    if (content != null) result.content = content;
    if (builtin != null) result.builtin = builtin;
    if (readOnly != null) result.readOnly = readOnly;
    if (order != null) result.order = order;
    return result;
  }

  PromptEntry._();

  factory PromptEntry.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PromptEntry.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PromptEntry',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aE<PromptKind>(1, _omitFieldNames ? '' : 'kind',
        enumValues: PromptKind.values)
    ..aOS(2, _omitFieldNames ? '' : 'id')
    ..aOS(3, _omitFieldNames ? '' : 'content')
    ..aOB(4, _omitFieldNames ? '' : 'builtin')
    ..aOB(5, _omitFieldNames ? '' : 'readOnly')
    ..aI(6, _omitFieldNames ? '' : 'order', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromptEntry clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromptEntry copyWith(void Function(PromptEntry) updates) =>
      super.copyWith((message) => updates(message as PromptEntry))
          as PromptEntry;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PromptEntry create() => PromptEntry._();
  @$core.override
  PromptEntry createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PromptEntry getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PromptEntry>(create);
  static PromptEntry? _defaultInstance;

  @$pb.TagNumber(1)
  PromptKind get kind => $_getN(0);
  @$pb.TagNumber(1)
  set kind(PromptKind value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasKind() => $_has(0);
  @$pb.TagNumber(1)
  void clearKind() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get id => $_getSZ(1);
  @$pb.TagNumber(2)
  set id($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasId() => $_has(1);
  @$pb.TagNumber(2)
  void clearId() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get content => $_getSZ(2);
  @$pb.TagNumber(3)
  set content($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasContent() => $_has(2);
  @$pb.TagNumber(3)
  void clearContent() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.bool get builtin => $_getBF(3);
  @$pb.TagNumber(4)
  set builtin($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasBuiltin() => $_has(3);
  @$pb.TagNumber(4)
  void clearBuiltin() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.bool get readOnly => $_getBF(4);
  @$pb.TagNumber(5)
  set readOnly($core.bool value) => $_setBool(4, value);
  @$pb.TagNumber(5)
  $core.bool hasReadOnly() => $_has(4);
  @$pb.TagNumber(5)
  void clearReadOnly() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.int get order => $_getIZ(5);
  @$pb.TagNumber(6)
  set order($core.int value) => $_setUnsignedInt32(5, value);
  @$pb.TagNumber(6)
  $core.bool hasOrder() => $_has(5);
  @$pb.TagNumber(6)
  void clearOrder() => $_clearField(6);
}

class PromptListRequest extends $pb.GeneratedMessage {
  factory PromptListRequest({
    PromptKind? kind,
  }) {
    final result = create();
    if (kind != null) result.kind = kind;
    return result;
  }

  PromptListRequest._();

  factory PromptListRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PromptListRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PromptListRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aE<PromptKind>(1, _omitFieldNames ? '' : 'kind',
        enumValues: PromptKind.values)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromptListRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromptListRequest copyWith(void Function(PromptListRequest) updates) =>
      super.copyWith((message) => updates(message as PromptListRequest))
          as PromptListRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PromptListRequest create() => PromptListRequest._();
  @$core.override
  PromptListRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PromptListRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PromptListRequest>(create);
  static PromptListRequest? _defaultInstance;

  @$pb.TagNumber(1)
  PromptKind get kind => $_getN(0);
  @$pb.TagNumber(1)
  set kind(PromptKind value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasKind() => $_has(0);
  @$pb.TagNumber(1)
  void clearKind() => $_clearField(1);
}

class PromptList extends $pb.GeneratedMessage {
  factory PromptList({
    $core.Iterable<PromptEntry>? entries,
  }) {
    final result = create();
    if (entries != null) result.entries.addAll(entries);
    return result;
  }

  PromptList._();

  factory PromptList.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PromptList.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PromptList',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<PromptEntry>(1, _omitFieldNames ? '' : 'entries',
        subBuilder: PromptEntry.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromptList clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromptList copyWith(void Function(PromptList) updates) =>
      super.copyWith((message) => updates(message as PromptList)) as PromptList;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PromptList create() => PromptList._();
  @$core.override
  PromptList createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PromptList getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PromptList>(create);
  static PromptList? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<PromptEntry> get entries => $_getList(0);
}

class DeleteReply extends $pb.GeneratedMessage {
  factory DeleteReply({
    $core.bool? deleted,
  }) {
    final result = create();
    if (deleted != null) result.deleted = deleted;
    return result;
  }

  DeleteReply._();

  factory DeleteReply.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DeleteReply.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DeleteReply',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'deleted')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DeleteReply clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DeleteReply copyWith(void Function(DeleteReply) updates) =>
      super.copyWith((message) => updates(message as DeleteReply))
          as DeleteReply;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DeleteReply create() => DeleteReply._();
  @$core.override
  DeleteReply createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DeleteReply getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DeleteReply>(create);
  static DeleteReply? _defaultInstance;

  @$pb.TagNumber(1)
  $core.bool get deleted => $_getBF(0);
  @$pb.TagNumber(1)
  set deleted($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasDeleted() => $_has(0);
  @$pb.TagNumber(1)
  void clearDeleted() => $_clearField(1);
}

/// Preview the assembled context for a goal. `mode` is informational — initial
/// assembly is mode-independent (the lens applies only at switch-compaction).
class PreviewRequest extends $pb.GeneratedMessage {
  factory PreviewRequest({
    $core.String? mode,
    $core.String? goal,
  }) {
    final result = create();
    if (mode != null) result.mode = mode;
    if (goal != null) result.goal = goal;
    return result;
  }

  PreviewRequest._();

  factory PreviewRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PreviewRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PreviewRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'mode')
    ..aOS(2, _omitFieldNames ? '' : 'goal')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PreviewRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PreviewRequest copyWith(void Function(PreviewRequest) updates) =>
      super.copyWith((message) => updates(message as PreviewRequest))
          as PreviewRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PreviewRequest create() => PreviewRequest._();
  @$core.override
  PreviewRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PreviewRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PreviewRequest>(create);
  static PreviewRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get mode => $_getSZ(0);
  @$pb.TagNumber(1)
  set mode($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasMode() => $_has(0);
  @$pb.TagNumber(1)
  void clearMode() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get goal => $_getSZ(1);
  @$pb.TagNumber(2)
  set goal($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasGoal() => $_has(1);
  @$pb.TagNumber(2)
  void clearGoal() => $_clearField(2);
}

class PreviewMessage extends $pb.GeneratedMessage {
  factory PreviewMessage({
    $core.String? role,
    $core.String? content,
  }) {
    final result = create();
    if (role != null) result.role = role;
    if (content != null) result.content = content;
    return result;
  }

  PreviewMessage._();

  factory PreviewMessage.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PreviewMessage.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PreviewMessage',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'role')
    ..aOS(2, _omitFieldNames ? '' : 'content')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PreviewMessage clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PreviewMessage copyWith(void Function(PreviewMessage) updates) =>
      super.copyWith((message) => updates(message as PreviewMessage))
          as PreviewMessage;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PreviewMessage create() => PreviewMessage._();
  @$core.override
  PreviewMessage createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PreviewMessage getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PreviewMessage>(create);
  static PreviewMessage? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get role => $_getSZ(0);
  @$pb.TagNumber(1)
  set role($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRole() => $_has(0);
  @$pb.TagNumber(1)
  void clearRole() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get content => $_getSZ(1);
  @$pb.TagNumber(2)
  set content($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasContent() => $_has(1);
  @$pb.TagNumber(2)
  void clearContent() => $_clearField(2);
}

class AssembledContext extends $pb.GeneratedMessage {
  factory AssembledContext({
    $core.Iterable<PreviewMessage>? messages,
  }) {
    final result = create();
    if (messages != null) result.messages.addAll(messages);
    return result;
  }

  AssembledContext._();

  factory AssembledContext.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory AssembledContext.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'AssembledContext',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<PreviewMessage>(1, _omitFieldNames ? '' : 'messages',
        subBuilder: PreviewMessage.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AssembledContext clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AssembledContext copyWith(void Function(AssembledContext) updates) =>
      super.copyWith((message) => updates(message as AssembledContext))
          as AssembledContext;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static AssembledContext create() => AssembledContext._();
  @$core.override
  AssembledContext createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static AssembledContext getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<AssembledContext>(create);
  static AssembledContext? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<PreviewMessage> get messages => $_getList(0);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
