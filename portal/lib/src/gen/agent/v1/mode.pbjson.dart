// This is a generated file - do not edit.
//
// Generated from agent/v1/mode.proto.

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

@$core.Deprecated('Use taskModeDescriptor instead')
const TaskMode$json = {
  '1': 'TaskMode',
  '2': [
    {'1': 'TASK_MODE_UNSPECIFIED', '2': 0},
    {'1': 'TASK_MODE_REVIEW', '2': 1},
    {'1': 'TASK_MODE_IMPLEMENT', '2': 2},
    {'1': 'TASK_MODE_DESIGN', '2': 3},
    {'1': 'TASK_MODE_DEBUG', '2': 4},
    {'1': 'TASK_MODE_EXPLAIN', '2': 5},
    {'1': 'TASK_MODE_OTHER', '2': 6},
  ],
};

/// Descriptor for `TaskMode`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List taskModeDescriptor = $convert.base64Decode(
    'CghUYXNrTW9kZRIZChVUQVNLX01PREVfVU5TUEVDSUZJRUQQABIUChBUQVNLX01PREVfUkVWSU'
    'VXEAESFwoTVEFTS19NT0RFX0lNUExFTUVOVBACEhQKEFRBU0tfTU9ERV9ERVNJR04QAxITCg9U'
    'QVNLX01PREVfREVCVUcQBBIVChFUQVNLX01PREVfRVhQTEFJThAFEhMKD1RBU0tfTU9ERV9PVE'
    'hFUhAG');

@$core.Deprecated('Use modeVerdictDescriptor instead')
const ModeVerdict$json = {
  '1': 'ModeVerdict',
  '2': [
    {
      '1': 'mode',
      '3': 1,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.TaskMode',
      '10': 'mode'
    },
    {'1': 'confidence', '3': 2, '4': 1, '5': 2, '10': 'confidence'},
    {'1': 'reason', '3': 3, '4': 1, '5': 9, '10': 'reason'},
  ],
};

/// Descriptor for `ModeVerdict`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List modeVerdictDescriptor = $convert.base64Decode(
    'CgtNb2RlVmVyZGljdBImCgRtb2RlGAEgASgOMhIuYWdlbnQudjEuVGFza01vZGVSBG1vZGUSHg'
    'oKY29uZmlkZW5jZRgCIAEoAlIKY29uZmlkZW5jZRIWCgZyZWFzb24YAyABKAlSBnJlYXNvbg==');

@$core.Deprecated('Use modeSwitchDescriptor instead')
const ModeSwitch$json = {
  '1': 'ModeSwitch',
  '2': [
    {
      '1': 'from',
      '3': 1,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.TaskMode',
      '10': 'from'
    },
    {'1': 'to', '3': 2, '4': 1, '5': 14, '6': '.agent.v1.TaskMode', '10': 'to'},
    {'1': 'reason', '3': 3, '4': 1, '5': 9, '10': 'reason'},
    {'1': 'confidence', '3': 4, '4': 1, '5': 2, '10': 'confidence'},
  ],
};

/// Descriptor for `ModeSwitch`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List modeSwitchDescriptor = $convert.base64Decode(
    'CgpNb2RlU3dpdGNoEiYKBGZyb20YASABKA4yEi5hZ2VudC52MS5UYXNrTW9kZVIEZnJvbRIiCg'
    'J0bxgCIAEoDjISLmFnZW50LnYxLlRhc2tNb2RlUgJ0bxIWCgZyZWFzb24YAyABKAlSBnJlYXNv'
    'bhIeCgpjb25maWRlbmNlGAQgASgCUgpjb25maWRlbmNl');

@$core.Deprecated('Use classifyRequestDescriptor instead')
const ClassifyRequest$json = {
  '1': 'ClassifyRequest',
  '2': [
    {'1': 'prompt', '3': 1, '4': 1, '5': 9, '10': 'prompt'},
    {
      '1': 'history',
      '3': 2,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Message',
      '10': 'history'
    },
  ],
};

/// Descriptor for `ClassifyRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List classifyRequestDescriptor = $convert.base64Decode(
    'Cg9DbGFzc2lmeVJlcXVlc3QSFgoGcHJvbXB0GAEgASgJUgZwcm9tcHQSKwoHaGlzdG9yeRgCIA'
    'MoCzIRLmFnZW50LnYxLk1lc3NhZ2VSB2hpc3Rvcnk=');
