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

/// The coarse task mode. Mirrors `agent_core::TaskMode`.
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

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
