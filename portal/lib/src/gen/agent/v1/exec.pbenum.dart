// This is a generated file - do not edit.
//
// Generated from agent/v1/exec.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

class ExecNetworkPolicy extends $pb.ProtobufEnum {
  static const ExecNetworkPolicy EXEC_NETWORK_POLICY_ON =
      ExecNetworkPolicy._(0, _omitEnumNames ? '' : 'EXEC_NETWORK_POLICY_ON');
  static const ExecNetworkPolicy EXEC_NETWORK_POLICY_OFF =
      ExecNetworkPolicy._(1, _omitEnumNames ? '' : 'EXEC_NETWORK_POLICY_OFF');
  static const ExecNetworkPolicy EXEC_NETWORK_POLICY_LOOPBACK =
      ExecNetworkPolicy._(
          2, _omitEnumNames ? '' : 'EXEC_NETWORK_POLICY_LOOPBACK');

  static const $core.List<ExecNetworkPolicy> values = <ExecNetworkPolicy>[
    EXEC_NETWORK_POLICY_ON,
    EXEC_NETWORK_POLICY_OFF,
    EXEC_NETWORK_POLICY_LOOPBACK,
  ];

  static final $core.List<ExecNetworkPolicy?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 2);
  static ExecNetworkPolicy? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const ExecNetworkPolicy._(super.value, super.name);
}

class ExecEnvPolicy extends $pb.ProtobufEnum {
  static const ExecEnvPolicy EXEC_ENV_POLICY_INHERIT =
      ExecEnvPolicy._(0, _omitEnumNames ? '' : 'EXEC_ENV_POLICY_INHERIT');
  static const ExecEnvPolicy EXEC_ENV_POLICY_SCRUB =
      ExecEnvPolicy._(1, _omitEnumNames ? '' : 'EXEC_ENV_POLICY_SCRUB');

  static const $core.List<ExecEnvPolicy> values = <ExecEnvPolicy>[
    EXEC_ENV_POLICY_INHERIT,
    EXEC_ENV_POLICY_SCRUB,
  ];

  static final $core.List<ExecEnvPolicy?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 1);
  static ExecEnvPolicy? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const ExecEnvPolicy._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
