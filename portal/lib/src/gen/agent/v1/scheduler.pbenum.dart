// This is a generated file - do not edit.
//
// Generated from agent/v1/scheduler.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

class SchedRunOutcome extends $pb.ProtobufEnum {
  static const SchedRunOutcome SCHED_RUN_OUTCOME_COMPLETED =
      SchedRunOutcome._(0, _omitEnumNames ? '' : 'SCHED_RUN_OUTCOME_COMPLETED');
  static const SchedRunOutcome SCHED_RUN_OUTCOME_FAILED =
      SchedRunOutcome._(1, _omitEnumNames ? '' : 'SCHED_RUN_OUTCOME_FAILED');

  /// The previous run was still going — this fire was deliberately dropped
  /// rather than stacked.
  static const SchedRunOutcome SCHED_RUN_OUTCOME_SKIPPED =
      SchedRunOutcome._(2, _omitEnumNames ? '' : 'SCHED_RUN_OUTCOME_SKIPPED');

  static const $core.List<SchedRunOutcome> values = <SchedRunOutcome>[
    SCHED_RUN_OUTCOME_COMPLETED,
    SCHED_RUN_OUTCOME_FAILED,
    SCHED_RUN_OUTCOME_SKIPPED,
  ];

  static final $core.List<SchedRunOutcome?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 2);
  static SchedRunOutcome? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const SchedRunOutcome._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
