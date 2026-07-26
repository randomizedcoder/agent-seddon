// This is a generated file - do not edit.
//
// Generated from agent/v1/session.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

import 'common.pb.dart' as $1;

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

/// A content-addressed checkpoint id.
class SessionCheckpointRef extends $pb.GeneratedMessage {
  factory SessionCheckpointRef({
    $core.String? id,
  }) {
    final result = create();
    if (id != null) result.id = id;
    return result;
  }

  SessionCheckpointRef._();

  factory SessionCheckpointRef.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SessionCheckpointRef.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SessionCheckpointRef',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionCheckpointRef clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionCheckpointRef copyWith(void Function(SessionCheckpointRef) updates) =>
      super.copyWith((message) => updates(message as SessionCheckpointRef))
          as SessionCheckpointRef;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SessionCheckpointRef create() => SessionCheckpointRef._();
  @$core.override
  SessionCheckpointRef createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SessionCheckpointRef getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SessionCheckpointRef>(create);
  static SessionCheckpointRef? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);
}

/// A session id.
class SessionRef extends $pb.GeneratedMessage {
  factory SessionRef({
    $core.String? session,
  }) {
    final result = create();
    if (session != null) result.session = session;
    return result;
  }

  SessionRef._();

  factory SessionRef.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SessionRef.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SessionRef',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'session')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionRef clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionRef copyWith(void Function(SessionRef) updates) =>
      super.copyWith((message) => updates(message as SessionRef)) as SessionRef;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SessionRef create() => SessionRef._();
  @$core.override
  SessionRef createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SessionRef getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SessionRef>(create);
  static SessionRef? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get session => $_getSZ(0);
  @$pb.TagNumber(1)
  set session($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSession() => $_has(0);
  @$pb.TagNumber(1)
  void clearSession() => $_clearField(1);
}

class SessionCheckpointRequest extends $pb.GeneratedMessage {
  factory SessionCheckpointRequest({
    $core.String? session,
    $1.WorkingSet? working,
    $core.String? label,
  }) {
    final result = create();
    if (session != null) result.session = session;
    if (working != null) result.working = working;
    if (label != null) result.label = label;
    return result;
  }

  SessionCheckpointRequest._();

  factory SessionCheckpointRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SessionCheckpointRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SessionCheckpointRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'session')
    ..aOM<$1.WorkingSet>(2, _omitFieldNames ? '' : 'working',
        subBuilder: $1.WorkingSet.create)
    ..aOS(3, _omitFieldNames ? '' : 'label')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionCheckpointRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionCheckpointRequest copyWith(
          void Function(SessionCheckpointRequest) updates) =>
      super.copyWith((message) => updates(message as SessionCheckpointRequest))
          as SessionCheckpointRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SessionCheckpointRequest create() => SessionCheckpointRequest._();
  @$core.override
  SessionCheckpointRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SessionCheckpointRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SessionCheckpointRequest>(create);
  static SessionCheckpointRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get session => $_getSZ(0);
  @$pb.TagNumber(1)
  set session($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSession() => $_has(0);
  @$pb.TagNumber(1)
  void clearSession() => $_clearField(1);

  @$pb.TagNumber(2)
  $1.WorkingSet get working => $_getN(1);
  @$pb.TagNumber(2)
  set working($1.WorkingSet value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasWorking() => $_has(1);
  @$pb.TagNumber(2)
  void clearWorking() => $_clearField(2);
  @$pb.TagNumber(2)
  $1.WorkingSet ensureWorking() => $_ensure(1);

  @$pb.TagNumber(3)
  $core.String get label => $_getSZ(2);
  @$pb.TagNumber(3)
  set label($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasLabel() => $_has(2);
  @$pb.TagNumber(3)
  void clearLabel() => $_clearField(3);
}

/// One node in the branch tree.
class SessionCheckpointMeta extends $pb.GeneratedMessage {
  factory SessionCheckpointMeta({
    $core.String? id,
    $core.String? parent,
    $core.String? branch,
    $core.int? turn,
    $core.String? label,
    $fixnum.Int64? createdMs,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (parent != null) result.parent = parent;
    if (branch != null) result.branch = branch;
    if (turn != null) result.turn = turn;
    if (label != null) result.label = label;
    if (createdMs != null) result.createdMs = createdMs;
    return result;
  }

  SessionCheckpointMeta._();

  factory SessionCheckpointMeta.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SessionCheckpointMeta.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SessionCheckpointMeta',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aOS(2, _omitFieldNames ? '' : 'parent')
    ..aOS(3, _omitFieldNames ? '' : 'branch')
    ..aI(4, _omitFieldNames ? '' : 'turn', fieldType: $pb.PbFieldType.OU3)
    ..aOS(5, _omitFieldNames ? '' : 'label')
    ..a<$fixnum.Int64>(
        6, _omitFieldNames ? '' : 'createdMs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionCheckpointMeta clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionCheckpointMeta copyWith(
          void Function(SessionCheckpointMeta) updates) =>
      super.copyWith((message) => updates(message as SessionCheckpointMeta))
          as SessionCheckpointMeta;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SessionCheckpointMeta create() => SessionCheckpointMeta._();
  @$core.override
  SessionCheckpointMeta createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SessionCheckpointMeta getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SessionCheckpointMeta>(create);
  static SessionCheckpointMeta? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  /// Absent for a root checkpoint.
  @$pb.TagNumber(2)
  $core.String get parent => $_getSZ(1);
  @$pb.TagNumber(2)
  set parent($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasParent() => $_has(1);
  @$pb.TagNumber(2)
  void clearParent() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get branch => $_getSZ(2);
  @$pb.TagNumber(3)
  set branch($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasBranch() => $_has(2);
  @$pb.TagNumber(3)
  void clearBranch() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get turn => $_getIZ(3);
  @$pb.TagNumber(4)
  set turn($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasTurn() => $_has(3);
  @$pb.TagNumber(4)
  void clearTurn() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get label => $_getSZ(4);
  @$pb.TagNumber(5)
  set label($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasLabel() => $_has(4);
  @$pb.TagNumber(5)
  void clearLabel() => $_clearField(5);

  @$pb.TagNumber(6)
  $fixnum.Int64 get createdMs => $_getI64(5);
  @$pb.TagNumber(6)
  set createdMs($fixnum.Int64 value) => $_setInt64(5, value);
  @$pb.TagNumber(6)
  $core.bool hasCreatedMs() => $_has(5);
  @$pb.TagNumber(6)
  void clearCreatedMs() => $_clearField(6);
}

class SessionCheckpointList extends $pb.GeneratedMessage {
  factory SessionCheckpointList({
    $core.Iterable<SessionCheckpointMeta>? checkpoints,
  }) {
    final result = create();
    if (checkpoints != null) result.checkpoints.addAll(checkpoints);
    return result;
  }

  SessionCheckpointList._();

  factory SessionCheckpointList.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SessionCheckpointList.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SessionCheckpointList',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<SessionCheckpointMeta>(1, _omitFieldNames ? '' : 'checkpoints',
        subBuilder: SessionCheckpointMeta.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionCheckpointList clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionCheckpointList copyWith(
          void Function(SessionCheckpointList) updates) =>
      super.copyWith((message) => updates(message as SessionCheckpointList))
          as SessionCheckpointList;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SessionCheckpointList create() => SessionCheckpointList._();
  @$core.override
  SessionCheckpointList createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SessionCheckpointList getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SessionCheckpointList>(create);
  static SessionCheckpointList? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<SessionCheckpointMeta> get checkpoints => $_getList(0);
}

class SessionBranchRequest extends $pb.GeneratedMessage {
  factory SessionBranchRequest({
    $core.String? session,
    $core.String? from,
    $core.String? name,
  }) {
    final result = create();
    if (session != null) result.session = session;
    if (from != null) result.from = from;
    if (name != null) result.name = name;
    return result;
  }

  SessionBranchRequest._();

  factory SessionBranchRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SessionBranchRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SessionBranchRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'session')
    ..aOS(2, _omitFieldNames ? '' : 'from')
    ..aOS(3, _omitFieldNames ? '' : 'name')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionBranchRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionBranchRequest copyWith(void Function(SessionBranchRequest) updates) =>
      super.copyWith((message) => updates(message as SessionBranchRequest))
          as SessionBranchRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SessionBranchRequest create() => SessionBranchRequest._();
  @$core.override
  SessionBranchRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SessionBranchRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SessionBranchRequest>(create);
  static SessionBranchRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get session => $_getSZ(0);
  @$pb.TagNumber(1)
  set session($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSession() => $_has(0);
  @$pb.TagNumber(1)
  void clearSession() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get from => $_getSZ(1);
  @$pb.TagNumber(2)
  set from($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasFrom() => $_has(1);
  @$pb.TagNumber(2)
  void clearFrom() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get name => $_getSZ(2);
  @$pb.TagNumber(3)
  set name($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasName() => $_has(2);
  @$pb.TagNumber(3)
  void clearName() => $_clearField(3);
}

class SessionBranchResponse extends $pb.GeneratedMessage {
  factory SessionBranchResponse() => create();

  SessionBranchResponse._();

  factory SessionBranchResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SessionBranchResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SessionBranchResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionBranchResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionBranchResponse copyWith(
          void Function(SessionBranchResponse) updates) =>
      super.copyWith((message) => updates(message as SessionBranchResponse))
          as SessionBranchResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SessionBranchResponse create() => SessionBranchResponse._();
  @$core.override
  SessionBranchResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SessionBranchResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SessionBranchResponse>(create);
  static SessionBranchResponse? _defaultInstance;
}

class SessionUndoRequest extends $pb.GeneratedMessage {
  factory SessionUndoRequest({
    $core.String? session,
    $core.int? n,
  }) {
    final result = create();
    if (session != null) result.session = session;
    if (n != null) result.n = n;
    return result;
  }

  SessionUndoRequest._();

  factory SessionUndoRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SessionUndoRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SessionUndoRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'session')
    ..aI(2, _omitFieldNames ? '' : 'n', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionUndoRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionUndoRequest copyWith(void Function(SessionUndoRequest) updates) =>
      super.copyWith((message) => updates(message as SessionUndoRequest))
          as SessionUndoRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SessionUndoRequest create() => SessionUndoRequest._();
  @$core.override
  SessionUndoRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SessionUndoRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SessionUndoRequest>(create);
  static SessionUndoRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get session => $_getSZ(0);
  @$pb.TagNumber(1)
  set session($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSession() => $_has(0);
  @$pb.TagNumber(1)
  void clearSession() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get n => $_getIZ(1);
  @$pb.TagNumber(2)
  set n($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasN() => $_has(1);
  @$pb.TagNumber(2)
  void clearN() => $_clearField(2);
}

class SessionDiffRequest extends $pb.GeneratedMessage {
  factory SessionDiffRequest({
    $core.String? a,
    $core.String? b,
  }) {
    final result = create();
    if (a != null) result.a = a;
    if (b != null) result.b = b;
    return result;
  }

  SessionDiffRequest._();

  factory SessionDiffRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SessionDiffRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SessionDiffRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'a')
    ..aOS(2, _omitFieldNames ? '' : 'b')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionDiffRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionDiffRequest copyWith(void Function(SessionDiffRequest) updates) =>
      super.copyWith((message) => updates(message as SessionDiffRequest))
          as SessionDiffRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SessionDiffRequest create() => SessionDiffRequest._();
  @$core.override
  SessionDiffRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SessionDiffRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SessionDiffRequest>(create);
  static SessionDiffRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get a => $_getSZ(0);
  @$pb.TagNumber(1)
  set a($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasA() => $_has(0);
  @$pb.TagNumber(1)
  void clearA() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get b => $_getSZ(1);
  @$pb.TagNumber(2)
  set b($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasB() => $_has(1);
  @$pb.TagNumber(2)
  void clearB() => $_clearField(2);
}

/// The delta of `b` relative to `a`.
class SessionCheckpointDiff extends $pb.GeneratedMessage {
  factory SessionCheckpointDiff({
    $fixnum.Int64? added,
    $fixnum.Int64? removed,
  }) {
    final result = create();
    if (added != null) result.added = added;
    if (removed != null) result.removed = removed;
    return result;
  }

  SessionCheckpointDiff._();

  factory SessionCheckpointDiff.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SessionCheckpointDiff.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SessionCheckpointDiff',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..a<$fixnum.Int64>(1, _omitFieldNames ? '' : 'added', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..a<$fixnum.Int64>(2, _omitFieldNames ? '' : 'removed', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionCheckpointDiff clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionCheckpointDiff copyWith(
          void Function(SessionCheckpointDiff) updates) =>
      super.copyWith((message) => updates(message as SessionCheckpointDiff))
          as SessionCheckpointDiff;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SessionCheckpointDiff create() => SessionCheckpointDiff._();
  @$core.override
  SessionCheckpointDiff createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SessionCheckpointDiff getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SessionCheckpointDiff>(create);
  static SessionCheckpointDiff? _defaultInstance;

  @$pb.TagNumber(1)
  $fixnum.Int64 get added => $_getI64(0);
  @$pb.TagNumber(1)
  set added($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasAdded() => $_has(0);
  @$pb.TagNumber(1)
  void clearAdded() => $_clearField(1);

  @$pb.TagNumber(2)
  $fixnum.Int64 get removed => $_getI64(1);
  @$pb.TagNumber(2)
  set removed($fixnum.Int64 value) => $_setInt64(1, value);
  @$pb.TagNumber(2)
  $core.bool hasRemoved() => $_has(1);
  @$pb.TagNumber(2)
  void clearRemoved() => $_clearField(2);
}

class SessionPruneResponse extends $pb.GeneratedMessage {
  factory SessionPruneResponse({
    $fixnum.Int64? reclaimed,
  }) {
    final result = create();
    if (reclaimed != null) result.reclaimed = reclaimed;
    return result;
  }

  SessionPruneResponse._();

  factory SessionPruneResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SessionPruneResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SessionPruneResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..a<$fixnum.Int64>(
        1, _omitFieldNames ? '' : 'reclaimed', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionPruneResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SessionPruneResponse copyWith(void Function(SessionPruneResponse) updates) =>
      super.copyWith((message) => updates(message as SessionPruneResponse))
          as SessionPruneResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SessionPruneResponse create() => SessionPruneResponse._();
  @$core.override
  SessionPruneResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SessionPruneResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SessionPruneResponse>(create);
  static SessionPruneResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $fixnum.Int64 get reclaimed => $_getI64(0);
  @$pb.TagNumber(1)
  set reclaimed($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasReclaimed() => $_has(0);
  @$pb.TagNumber(1)
  void clearReclaimed() => $_clearField(1);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
