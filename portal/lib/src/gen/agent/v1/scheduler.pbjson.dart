// This is a generated file - do not edit.
//
// Generated from agent/v1/scheduler.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports
// ignore_for_file: unused_import

import 'dart:convert' as $convert;
import 'dart:core' as $core;
import 'dart:typed_data' as $typed_data;

@$core.Deprecated('Use schedRunOutcomeDescriptor instead')
const SchedRunOutcome$json = {
  '1': 'SchedRunOutcome',
  '2': [
    {'1': 'SCHED_RUN_OUTCOME_COMPLETED', '2': 0},
    {'1': 'SCHED_RUN_OUTCOME_FAILED', '2': 1},
    {'1': 'SCHED_RUN_OUTCOME_SKIPPED', '2': 2},
  ],
};

/// Descriptor for `SchedRunOutcome`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List schedRunOutcomeDescriptor = $convert.base64Decode(
    'Cg9TY2hlZFJ1bk91dGNvbWUSHwobU0NIRURfUlVOX09VVENPTUVfQ09NUExFVEVEEAASHAoYU0'
    'NIRURfUlVOX09VVENPTUVfRkFJTEVEEAESHQoZU0NIRURfUlVOX09VVENPTUVfU0tJUFBFRBAC');

@$core.Deprecated('Use schedJobRefDescriptor instead')
const SchedJobRef$json = {
  '1': 'SchedJobRef',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
  ],
};

/// Descriptor for `SchedJobRef`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List schedJobRefDescriptor =
    $convert.base64Decode('CgtTY2hlZEpvYlJlZhIOCgJpZBgBIAEoCVICaWQ=');

@$core.Deprecated('Use schedListRequestDescriptor instead')
const SchedListRequest$json = {
  '1': 'SchedListRequest',
};

/// Descriptor for `SchedListRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List schedListRequestDescriptor =
    $convert.base64Decode('ChBTY2hlZExpc3RSZXF1ZXN0');

@$core.Deprecated('Use schedScheduleRequestDescriptor instead')
const SchedScheduleRequest$json = {
  '1': 'SchedScheduleRequest',
  '2': [
    {'1': 'spec', '3': 1, '4': 1, '5': 9, '10': 'spec'},
    {'1': 'goal', '3': 2, '4': 1, '5': 9, '10': 'goal'},
  ],
};

/// Descriptor for `SchedScheduleRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List schedScheduleRequestDescriptor = $convert.base64Decode(
    'ChRTY2hlZFNjaGVkdWxlUmVxdWVzdBISCgRzcGVjGAEgASgJUgRzcGVjEhIKBGdvYWwYAiABKA'
    'lSBGdvYWw=');

@$core.Deprecated('Use schedScheduleDescriptor instead')
const SchedSchedule$json = {
  '1': 'SchedSchedule',
  '2': [
    {
      '1': 'interval_secs',
      '3': 1,
      '4': 1,
      '5': 4,
      '9': 0,
      '10': 'intervalSecs'
    },
    {'1': 'cron_expr', '3': 2, '4': 1, '5': 9, '9': 0, '10': 'cronExpr'},
    {'1': 'once_at_ms', '3': 3, '4': 1, '5': 4, '9': 0, '10': 'onceAtMs'},
  ],
  '8': [
    {'1': 'kind'},
  ],
};

/// Descriptor for `SchedSchedule`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List schedScheduleDescriptor = $convert.base64Decode(
    'Cg1TY2hlZFNjaGVkdWxlEiUKDWludGVydmFsX3NlY3MYASABKARIAFIMaW50ZXJ2YWxTZWNzEh'
    '0KCWNyb25fZXhwchgCIAEoCUgAUghjcm9uRXhwchIeCgpvbmNlX2F0X21zGAMgASgESABSCG9u'
    'Y2VBdE1zQgYKBGtpbmQ=');

@$core.Deprecated('Use schedJobDescriptor instead')
const SchedJob$json = {
  '1': 'SchedJob',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'spec', '3': 2, '4': 1, '5': 9, '10': 'spec'},
    {
      '1': 'schedule',
      '3': 3,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.SchedSchedule',
      '10': 'schedule'
    },
    {'1': 'goal', '3': 4, '4': 1, '5': 9, '10': 'goal'},
    {
      '1': 'next_fire_ms',
      '3': 5,
      '4': 1,
      '5': 4,
      '9': 0,
      '10': 'nextFireMs',
      '17': true
    },
    {'1': 'enabled', '3': 6, '4': 1, '5': 8, '10': 'enabled'},
  ],
  '8': [
    {'1': '_next_fire_ms'},
  ],
};

/// Descriptor for `SchedJob`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List schedJobDescriptor = $convert.base64Decode(
    'CghTY2hlZEpvYhIOCgJpZBgBIAEoCVICaWQSEgoEc3BlYxgCIAEoCVIEc3BlYxIzCghzY2hlZH'
    'VsZRgDIAEoCzIXLmFnZW50LnYxLlNjaGVkU2NoZWR1bGVSCHNjaGVkdWxlEhIKBGdvYWwYBCAB'
    'KAlSBGdvYWwSJQoMbmV4dF9maXJlX21zGAUgASgESABSCm5leHRGaXJlTXOIAQESGAoHZW5hYm'
    'xlZBgGIAEoCFIHZW5hYmxlZEIPCg1fbmV4dF9maXJlX21z');

@$core.Deprecated('Use schedJobListDescriptor instead')
const SchedJobList$json = {
  '1': 'SchedJobList',
  '2': [
    {
      '1': 'jobs',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.SchedJob',
      '10': 'jobs'
    },
  ],
};

/// Descriptor for `SchedJobList`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List schedJobListDescriptor = $convert.base64Decode(
    'CgxTY2hlZEpvYkxpc3QSJgoEam9icxgBIAMoCzISLmFnZW50LnYxLlNjaGVkSm9iUgRqb2Jz');

@$core.Deprecated('Use schedRunDescriptor instead')
const SchedRun$json = {
  '1': 'SchedRun',
  '2': [
    {'1': 'job_id', '3': 1, '4': 1, '5': 9, '10': 'jobId'},
    {'1': 'started_ms', '3': 2, '4': 1, '5': 4, '10': 'startedMs'},
    {'1': 'finished_ms', '3': 3, '4': 1, '5': 4, '10': 'finishedMs'},
    {
      '1': 'outcome',
      '3': 4,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.SchedRunOutcome',
      '10': 'outcome'
    },
    {'1': 'detail', '3': 5, '4': 1, '5': 9, '10': 'detail'},
  ],
};

/// Descriptor for `SchedRun`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List schedRunDescriptor = $convert.base64Decode(
    'CghTY2hlZFJ1bhIVCgZqb2JfaWQYASABKAlSBWpvYklkEh0KCnN0YXJ0ZWRfbXMYAiABKARSCX'
    'N0YXJ0ZWRNcxIfCgtmaW5pc2hlZF9tcxgDIAEoBFIKZmluaXNoZWRNcxIzCgdvdXRjb21lGAQg'
    'ASgOMhkuYWdlbnQudjEuU2NoZWRSdW5PdXRjb21lUgdvdXRjb21lEhYKBmRldGFpbBgFIAEoCV'
    'IGZGV0YWls');

@$core.Deprecated('Use schedRunListDescriptor instead')
const SchedRunList$json = {
  '1': 'SchedRunList',
  '2': [
    {
      '1': 'runs',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.SchedRun',
      '10': 'runs'
    },
  ],
};

/// Descriptor for `SchedRunList`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List schedRunListDescriptor = $convert.base64Decode(
    'CgxTY2hlZFJ1bkxpc3QSJgoEcnVucxgBIAMoCzISLmFnZW50LnYxLlNjaGVkUnVuUgRydW5z');

@$core.Deprecated('Use schedCancelResponseDescriptor instead')
const SchedCancelResponse$json = {
  '1': 'SchedCancelResponse',
  '2': [
    {'1': 'cancelled', '3': 1, '4': 1, '5': 8, '10': 'cancelled'},
  ],
};

/// Descriptor for `SchedCancelResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List schedCancelResponseDescriptor =
    $convert.base64Decode(
        'ChNTY2hlZENhbmNlbFJlc3BvbnNlEhwKCWNhbmNlbGxlZBgBIAEoCFIJY2FuY2VsbGVk');
