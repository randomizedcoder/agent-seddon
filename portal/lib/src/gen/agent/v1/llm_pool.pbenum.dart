// This is a generated file - do not edit.
//
// Generated from agent/v1/llm_pool.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

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

/// Graded liveness of a member (GPU pool 03).
class PoolMemberState extends $pb.ProtobufEnum {
  static const PoolMemberState POOL_MEMBER_STATE_UNSPECIFIED =
      PoolMemberState._(
          0, _omitEnumNames ? '' : 'POOL_MEMBER_STATE_UNSPECIFIED');
  static const PoolMemberState POOL_MEMBER_STATE_HEALTHY =
      PoolMemberState._(1, _omitEnumNames ? '' : 'POOL_MEMBER_STATE_HEALTHY');
  static const PoolMemberState POOL_MEMBER_STATE_DEGRADED =
      PoolMemberState._(2, _omitEnumNames ? '' : 'POOL_MEMBER_STATE_DEGRADED');
  static const PoolMemberState POOL_MEMBER_STATE_DEAD =
      PoolMemberState._(3, _omitEnumNames ? '' : 'POOL_MEMBER_STATE_DEAD');

  static const $core.List<PoolMemberState> values = <PoolMemberState>[
    POOL_MEMBER_STATE_UNSPECIFIED,
    POOL_MEMBER_STATE_HEALTHY,
    POOL_MEMBER_STATE_DEGRADED,
    POOL_MEMBER_STATE_DEAD,
  ];

  static final $core.List<PoolMemberState?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 3);
  static PoolMemberState? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const PoolMemberState._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
