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

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
