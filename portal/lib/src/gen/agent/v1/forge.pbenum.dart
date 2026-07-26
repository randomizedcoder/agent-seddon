// This is a generated file - do not edit.
//
// Generated from agent/v1/forge.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

class ForgeReviewVerdict extends $pb.ProtobufEnum {
  static const ForgeReviewVerdict FORGE_REVIEW_VERDICT_COMMENT =
      ForgeReviewVerdict._(
          0, _omitEnumNames ? '' : 'FORGE_REVIEW_VERDICT_COMMENT');
  static const ForgeReviewVerdict FORGE_REVIEW_VERDICT_APPROVE =
      ForgeReviewVerdict._(
          1, _omitEnumNames ? '' : 'FORGE_REVIEW_VERDICT_APPROVE');
  static const ForgeReviewVerdict FORGE_REVIEW_VERDICT_REQUEST_CHANGES =
      ForgeReviewVerdict._(
          2, _omitEnumNames ? '' : 'FORGE_REVIEW_VERDICT_REQUEST_CHANGES');

  static const $core.List<ForgeReviewVerdict> values = <ForgeReviewVerdict>[
    FORGE_REVIEW_VERDICT_COMMENT,
    FORGE_REVIEW_VERDICT_APPROVE,
    FORGE_REVIEW_VERDICT_REQUEST_CHANGES,
  ];

  static final $core.List<ForgeReviewVerdict?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 2);
  static ForgeReviewVerdict? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const ForgeReviewVerdict._(super.value, super.name);
}

class TaskStatus extends $pb.ProtobufEnum {
  static const TaskStatus TASK_STATUS_PENDING =
      TaskStatus._(0, _omitEnumNames ? '' : 'TASK_STATUS_PENDING');
  static const TaskStatus TASK_STATUS_IN_PROGRESS =
      TaskStatus._(1, _omitEnumNames ? '' : 'TASK_STATUS_IN_PROGRESS');
  static const TaskStatus TASK_STATUS_COMPLETED =
      TaskStatus._(2, _omitEnumNames ? '' : 'TASK_STATUS_COMPLETED');
  static const TaskStatus TASK_STATUS_CANCELLED =
      TaskStatus._(3, _omitEnumNames ? '' : 'TASK_STATUS_CANCELLED');

  static const $core.List<TaskStatus> values = <TaskStatus>[
    TASK_STATUS_PENDING,
    TASK_STATUS_IN_PROGRESS,
    TASK_STATUS_COMPLETED,
    TASK_STATUS_CANCELLED,
  ];

  static final $core.List<TaskStatus?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 3);
  static TaskStatus? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const TaskStatus._(super.value, super.name);
}

class TaskPriority extends $pb.ProtobufEnum {
  static const TaskPriority TASK_PRIORITY_MEDIUM =
      TaskPriority._(0, _omitEnumNames ? '' : 'TASK_PRIORITY_MEDIUM');
  static const TaskPriority TASK_PRIORITY_HIGH =
      TaskPriority._(1, _omitEnumNames ? '' : 'TASK_PRIORITY_HIGH');
  static const TaskPriority TASK_PRIORITY_LOW =
      TaskPriority._(2, _omitEnumNames ? '' : 'TASK_PRIORITY_LOW');

  static const $core.List<TaskPriority> values = <TaskPriority>[
    TASK_PRIORITY_MEDIUM,
    TASK_PRIORITY_HIGH,
    TASK_PRIORITY_LOW,
  ];

  static final $core.List<TaskPriority?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 2);
  static TaskPriority? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const TaskPriority._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
