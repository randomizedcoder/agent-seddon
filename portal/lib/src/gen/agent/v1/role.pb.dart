// This is a generated file - do not edit.
//
// Generated from agent/v1/role.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

/// One `(action, resource_type)` grant — both validated strings (e.g.
/// `("write", "registry")`, `("approve", "fleet")`).
class RolePermission extends $pb.GeneratedMessage {
  factory RolePermission({
    $core.String? action,
    $core.String? resourceType,
  }) {
    final result = create();
    if (action != null) result.action = action;
    if (resourceType != null) result.resourceType = resourceType;
    return result;
  }

  RolePermission._();

  factory RolePermission.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RolePermission.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RolePermission',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'action')
    ..aOS(2, _omitFieldNames ? '' : 'resourceType')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RolePermission clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RolePermission copyWith(void Function(RolePermission) updates) =>
      super.copyWith((message) => updates(message as RolePermission))
          as RolePermission;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RolePermission create() => RolePermission._();
  @$core.override
  RolePermission createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RolePermission getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RolePermission>(create);
  static RolePermission? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get action => $_getSZ(0);
  @$pb.TagNumber(1)
  set action($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasAction() => $_has(0);
  @$pb.TagNumber(1)
  void clearAction() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get resourceType => $_getSZ(1);
  @$pb.TagNumber(2)
  set resourceType($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasResourceType() => $_has(1);
  @$pb.TagNumber(2)
  void clearResourceType() => $_clearField(2);
}

/// An operator-defined role card. Mirrors `agent_core::RoleCard`. The permission
/// shape is a flat, precedence-ordered trio (no oneof): `all` wins; else non-empty
/// `pairs`; else `actions_on_all` (possibly empty ⇒ grants nothing).
class RoleCard extends $pb.GeneratedMessage {
  factory RoleCard({
    $core.String? id,
    $core.bool? crossesTenants,
    $core.bool? all,
    $core.Iterable<$core.String>? actionsOnAll,
    $core.Iterable<RolePermission>? pairs,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (crossesTenants != null) result.crossesTenants = crossesTenants;
    if (all != null) result.all = all;
    if (actionsOnAll != null) result.actionsOnAll.addAll(actionsOnAll);
    if (pairs != null) result.pairs.addAll(pairs);
    return result;
  }

  RoleCard._();

  factory RoleCard.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RoleCard.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RoleCard',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aOB(2, _omitFieldNames ? '' : 'crossesTenants')
    ..aOB(3, _omitFieldNames ? '' : 'all')
    ..pPS(4, _omitFieldNames ? '' : 'actionsOnAll')
    ..pPM<RolePermission>(5, _omitFieldNames ? '' : 'pairs',
        subBuilder: RolePermission.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoleCard clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoleCard copyWith(void Function(RoleCard) updates) =>
      super.copyWith((message) => updates(message as RoleCard)) as RoleCard;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RoleCard create() => RoleCard._();
  @$core.override
  RoleCard createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RoleCard getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<RoleCard>(create);
  static RoleCard? _defaultInstance;

  /// Path-safe role name (server-validated; may not reuse a built-in id).
  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  /// Host-global role — may act in any tenant (like the built-in `operator`).
  @$pb.TagNumber(2)
  $core.bool get crossesTenants => $_getBF(1);
  @$pb.TagNumber(2)
  set crossesTenants($core.bool value) => $_setBool(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCrossesTenants() => $_has(1);
  @$pb.TagNumber(2)
  void clearCrossesTenants() => $_clearField(2);

  /// Every action on every resource type (an admin role). Takes precedence.
  @$pb.TagNumber(3)
  $core.bool get all => $_getBF(2);
  @$pb.TagNumber(3)
  set all($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasAll() => $_has(2);
  @$pb.TagNumber(3)
  void clearAll() => $_clearField(3);

  /// Actions granted on EVERY resource type (used when `all` is false and `pairs`
  /// is empty). An empty list here (with all=false, no pairs) grants nothing.
  @$pb.TagNumber(4)
  $pb.PbList<$core.String> get actionsOnAll => $_getList(3);

  /// Explicit grants (used when `all` is false and this list is non-empty).
  @$pb.TagNumber(5)
  $pb.PbList<RolePermission> get pairs => $_getList(4);
}

class RoleListRequest extends $pb.GeneratedMessage {
  factory RoleListRequest() => create();

  RoleListRequest._();

  factory RoleListRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RoleListRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RoleListRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoleListRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoleListRequest copyWith(void Function(RoleListRequest) updates) =>
      super.copyWith((message) => updates(message as RoleListRequest))
          as RoleListRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RoleListRequest create() => RoleListRequest._();
  @$core.override
  RoleListRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RoleListRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RoleListRequest>(create);
  static RoleListRequest? _defaultInstance;
}

class RoleList extends $pb.GeneratedMessage {
  factory RoleList({
    $core.Iterable<RoleCard>? roles,
  }) {
    final result = create();
    if (roles != null) result.roles.addAll(roles);
    return result;
  }

  RoleList._();

  factory RoleList.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RoleList.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RoleList',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<RoleCard>(1, _omitFieldNames ? '' : 'roles',
        subBuilder: RoleCard.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoleList clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoleList copyWith(void Function(RoleList) updates) =>
      super.copyWith((message) => updates(message as RoleList)) as RoleList;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RoleList create() => RoleList._();
  @$core.override
  RoleList createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RoleList getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<RoleList>(create);
  static RoleList? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<RoleCard> get roles => $_getList(0);
}

class RoleRef extends $pb.GeneratedMessage {
  factory RoleRef({
    $core.String? id,
  }) {
    final result = create();
    if (id != null) result.id = id;
    return result;
  }

  RoleRef._();

  factory RoleRef.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RoleRef.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RoleRef',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoleRef clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoleRef copyWith(void Function(RoleRef) updates) =>
      super.copyWith((message) => updates(message as RoleRef)) as RoleRef;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RoleRef create() => RoleRef._();
  @$core.override
  RoleRef createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RoleRef getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<RoleRef>(create);
  static RoleRef? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);
}

class RoleDeleteReply extends $pb.GeneratedMessage {
  factory RoleDeleteReply({
    $core.bool? deleted,
  }) {
    final result = create();
    if (deleted != null) result.deleted = deleted;
    return result;
  }

  RoleDeleteReply._();

  factory RoleDeleteReply.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RoleDeleteReply.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RoleDeleteReply',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'deleted')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoleDeleteReply clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoleDeleteReply copyWith(void Function(RoleDeleteReply) updates) =>
      super.copyWith((message) => updates(message as RoleDeleteReply))
          as RoleDeleteReply;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RoleDeleteReply create() => RoleDeleteReply._();
  @$core.override
  RoleDeleteReply createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RoleDeleteReply getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RoleDeleteReply>(create);
  static RoleDeleteReply? _defaultInstance;

  @$pb.TagNumber(1)
  $core.bool get deleted => $_getBF(0);
  @$pb.TagNumber(1)
  set deleted($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasDeleted() => $_has(0);
  @$pb.TagNumber(1)
  void clearDeleted() => $_clearField(1);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
