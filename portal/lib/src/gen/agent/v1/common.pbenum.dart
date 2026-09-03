// This is a generated file - do not edit.
//
// Generated from agent/v1/common.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

/// Conversation role. `ROLE_UNSPECIFIED` is the proto3 zero value and is never a
/// valid agent-core `Role`; conversions reject it.
class Role extends $pb.ProtobufEnum {
  static const Role ROLE_UNSPECIFIED =
      Role._(0, _omitEnumNames ? '' : 'ROLE_UNSPECIFIED');
  static const Role ROLE_SYSTEM =
      Role._(1, _omitEnumNames ? '' : 'ROLE_SYSTEM');
  static const Role ROLE_USER = Role._(2, _omitEnumNames ? '' : 'ROLE_USER');
  static const Role ROLE_ASSISTANT =
      Role._(3, _omitEnumNames ? '' : 'ROLE_ASSISTANT');
  static const Role ROLE_TOOL = Role._(4, _omitEnumNames ? '' : 'ROLE_TOOL');

  static const $core.List<Role> values = <Role>[
    ROLE_UNSPECIFIED,
    ROLE_SYSTEM,
    ROLE_USER,
    ROLE_ASSISTANT,
    ROLE_TOOL,
  ];

  static final $core.List<Role?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 4);
  static Role? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const Role._(super.value, super.name);
}

/// The sole inhabitant of the JSON `null` type (proto enums need a zero value).
class NullValue extends $pb.ProtobufEnum {
  static const NullValue NULL_VALUE =
      NullValue._(0, _omitEnumNames ? '' : 'NULL_VALUE');

  static const $core.List<NullValue> values = <NullValue>[
    NULL_VALUE,
  ];

  static final $core.List<NullValue?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 0);
  static NullValue? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const NullValue._(super.value, super.name);
}

/// The coarse task mode. Mirrors `agent_core::TaskMode`. Lives here (not
/// mode.proto) so `RouteHint` below can carry it without an import cycle —
/// a same-package move, wire/JSON-identical (`breaking: WIRE_JSON`).
class TaskMode extends $pb.ProtobufEnum {
  static const TaskMode TASK_MODE_UNSPECIFIED =
      TaskMode._(0, _omitEnumNames ? '' : 'TASK_MODE_UNSPECIFIED');
  static const TaskMode TASK_MODE_REVIEW =
      TaskMode._(1, _omitEnumNames ? '' : 'TASK_MODE_REVIEW');
  static const TaskMode TASK_MODE_IMPLEMENT =
      TaskMode._(2, _omitEnumNames ? '' : 'TASK_MODE_IMPLEMENT');
  static const TaskMode TASK_MODE_DESIGN =
      TaskMode._(3, _omitEnumNames ? '' : 'TASK_MODE_DESIGN');
  static const TaskMode TASK_MODE_DEBUG =
      TaskMode._(4, _omitEnumNames ? '' : 'TASK_MODE_DEBUG');
  static const TaskMode TASK_MODE_EXPLAIN =
      TaskMode._(5, _omitEnumNames ? '' : 'TASK_MODE_EXPLAIN');
  static const TaskMode TASK_MODE_OTHER =
      TaskMode._(6, _omitEnumNames ? '' : 'TASK_MODE_OTHER');

  static const $core.List<TaskMode> values = <TaskMode>[
    TASK_MODE_UNSPECIFIED,
    TASK_MODE_REVIEW,
    TASK_MODE_IMPLEMENT,
    TASK_MODE_DESIGN,
    TASK_MODE_DEBUG,
    TASK_MODE_EXPLAIN,
    TASK_MODE_OTHER,
  ];

  static final $core.List<TaskMode?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 6);
  static TaskMode? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const TaskMode._(super.value, super.name);
}

/// Pool member weight class. Mirrors `agent_core::PoolTier`. Moved here from
/// llm_pool.proto (same package, wire/JSON-identical) so `RouteHint` can carry a
/// tier floor without an import cycle.
class PoolTier extends $pb.ProtobufEnum {
  static const PoolTier POOL_TIER_UNSPECIFIED =
      PoolTier._(0, _omitEnumNames ? '' : 'POOL_TIER_UNSPECIFIED');
  static const PoolTier POOL_TIER_LIGHT =
      PoolTier._(1, _omitEnumNames ? '' : 'POOL_TIER_LIGHT');
  static const PoolTier POOL_TIER_MEDIUM =
      PoolTier._(2, _omitEnumNames ? '' : 'POOL_TIER_MEDIUM');
  static const PoolTier POOL_TIER_HEAVY =
      PoolTier._(3, _omitEnumNames ? '' : 'POOL_TIER_HEAVY');

  static const $core.List<PoolTier> values = <PoolTier>[
    POOL_TIER_UNSPECIFIED,
    POOL_TIER_LIGHT,
    POOL_TIER_MEDIUM,
    POOL_TIER_HEAVY,
  ];

  static final $core.List<PoolTier?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 3);
  static PoolTier? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const PoolTier._(super.value, super.name);
}

/// The internal call-site a request originates from. Mirrors
/// `agent_core::RouteRole` (model-router 02b); UNSPECIFIED ⇒ `None` (the routing
/// provider applies its own default).
class RouteRole extends $pb.ProtobufEnum {
  static const RouteRole ROUTE_ROLE_UNSPECIFIED =
      RouteRole._(0, _omitEnumNames ? '' : 'ROUTE_ROLE_UNSPECIFIED');
  static const RouteRole ROUTE_ROLE_MAIN =
      RouteRole._(1, _omitEnumNames ? '' : 'ROUTE_ROLE_MAIN');
  static const RouteRole ROUTE_ROLE_JUDGE =
      RouteRole._(2, _omitEnumNames ? '' : 'ROUTE_ROLE_JUDGE');
  static const RouteRole ROUTE_ROLE_CLASSIFY =
      RouteRole._(3, _omitEnumNames ? '' : 'ROUTE_ROLE_CLASSIFY');
  static const RouteRole ROUTE_ROLE_SUMMARIZE =
      RouteRole._(4, _omitEnumNames ? '' : 'ROUTE_ROLE_SUMMARIZE');
  static const RouteRole ROUTE_ROLE_VERIFY =
      RouteRole._(5, _omitEnumNames ? '' : 'ROUTE_ROLE_VERIFY');
  static const RouteRole ROUTE_ROLE_REVIEW =
      RouteRole._(6, _omitEnumNames ? '' : 'ROUTE_ROLE_REVIEW');

  static const $core.List<RouteRole> values = <RouteRole>[
    ROUTE_ROLE_UNSPECIFIED,
    ROUTE_ROLE_MAIN,
    ROUTE_ROLE_JUDGE,
    ROUTE_ROLE_CLASSIFY,
    ROUTE_ROLE_SUMMARIZE,
    ROUTE_ROLE_VERIFY,
    ROUTE_ROLE_REVIEW,
  ];

  static final $core.List<RouteRole?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 6);
  static RouteRole? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const RouteRole._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
