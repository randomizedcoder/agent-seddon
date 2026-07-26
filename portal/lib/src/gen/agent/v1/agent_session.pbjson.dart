// This is a generated file - do not edit.
//
// Generated from agent/v1/agent_session.proto.

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

@$core.Deprecated('Use subscribeRequestDescriptor instead')
const SubscribeRequest$json = {
  '1': 'SubscribeRequest',
};

/// Descriptor for `SubscribeRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List subscribeRequestDescriptor =
    $convert.base64Decode('ChBTdWJzY3JpYmVSZXF1ZXN0');

@$core.Deprecated('Use snapshotRequestDescriptor instead')
const SnapshotRequest$json = {
  '1': 'SnapshotRequest',
};

/// Descriptor for `SnapshotRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List snapshotRequestDescriptor =
    $convert.base64Decode('Cg9TbmFwc2hvdFJlcXVlc3Q=');

@$core.Deprecated('Use runStartedDescriptor instead')
const RunStarted$json = {
  '1': 'RunStarted',
  '2': [
    {'1': 'goal', '3': 1, '4': 1, '5': 9, '10': 'goal'},
  ],
};

/// Descriptor for `RunStarted`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List runStartedDescriptor =
    $convert.base64Decode('CgpSdW5TdGFydGVkEhIKBGdvYWwYASABKAlSBGdvYWw=');

@$core.Deprecated('Use iterationStartDescriptor instead')
const IterationStart$json = {
  '1': 'IterationStart',
  '2': [
    {'1': 'iter', '3': 1, '4': 1, '5': 13, '10': 'iter'},
  ],
};

/// Descriptor for `IterationStart`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List iterationStartDescriptor =
    $convert.base64Decode('Cg5JdGVyYXRpb25TdGFydBISCgRpdGVyGAEgASgNUgRpdGVy');

@$core.Deprecated('Use tokenDeltaDescriptor instead')
const TokenDelta$json = {
  '1': 'TokenDelta',
  '2': [
    {'1': 'text', '3': 1, '4': 1, '5': 9, '10': 'text'},
  ],
};

/// Descriptor for `TokenDelta`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List tokenDeltaDescriptor =
    $convert.base64Decode('CgpUb2tlbkRlbHRhEhIKBHRleHQYASABKAlSBHRleHQ=');

@$core.Deprecated('Use toolCallStartDescriptor instead')
const ToolCallStart$json = {
  '1': 'ToolCallStart',
  '2': [
    {'1': 'name', '3': 1, '4': 1, '5': 9, '10': 'name'},
    {'1': 'args', '3': 2, '4': 1, '5': 9, '10': 'args'},
  ],
};

/// Descriptor for `ToolCallStart`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List toolCallStartDescriptor = $convert.base64Decode(
    'Cg1Ub29sQ2FsbFN0YXJ0EhIKBG5hbWUYASABKAlSBG5hbWUSEgoEYXJncxgCIAEoCVIEYXJncw'
    '==');

@$core.Deprecated('Use toolCallResultDescriptor instead')
const ToolCallResult$json = {
  '1': 'ToolCallResult',
  '2': [
    {'1': 'name', '3': 1, '4': 1, '5': 9, '10': 'name'},
    {'1': 'ok', '3': 2, '4': 1, '5': 8, '10': 'ok'},
    {'1': 'duration_ms', '3': 3, '4': 1, '5': 4, '10': 'durationMs'},
  ],
};

/// Descriptor for `ToolCallResult`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List toolCallResultDescriptor = $convert.base64Decode(
    'Cg5Ub29sQ2FsbFJlc3VsdBISCgRuYW1lGAEgASgJUgRuYW1lEg4KAm9rGAIgASgIUgJvaxIfCg'
    'tkdXJhdGlvbl9tcxgDIAEoBFIKZHVyYXRpb25Ncw==');

@$core.Deprecated('Use sessionModeSwitchDescriptor instead')
const SessionModeSwitch$json = {
  '1': 'SessionModeSwitch',
  '2': [
    {'1': 'from', '3': 1, '4': 1, '5': 9, '10': 'from'},
    {'1': 'to', '3': 2, '4': 1, '5': 9, '10': 'to'},
    {'1': 'reason', '3': 3, '4': 1, '5': 9, '10': 'reason'},
    {'1': 'confidence', '3': 4, '4': 1, '5': 2, '10': 'confidence'},
  ],
};

/// Descriptor for `SessionModeSwitch`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List sessionModeSwitchDescriptor = $convert.base64Decode(
    'ChFTZXNzaW9uTW9kZVN3aXRjaBISCgRmcm9tGAEgASgJUgRmcm9tEg4KAnRvGAIgASgJUgJ0bx'
    'IWCgZyZWFzb24YAyABKAlSBnJlYXNvbhIeCgpjb25maWRlbmNlGAQgASgCUgpjb25maWRlbmNl');

@$core.Deprecated('Use contextUpdateDescriptor instead')
const ContextUpdate$json = {
  '1': 'ContextUpdate',
  '2': [
    {'1': 'prompt_tokens', '3': 1, '4': 1, '5': 13, '10': 'promptTokens'},
    {'1': 'context_window', '3': 2, '4': 1, '5': 13, '10': 'contextWindow'},
    {'1': 'messages', '3': 3, '4': 1, '5': 13, '10': 'messages'},
  ],
};

/// Descriptor for `ContextUpdate`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List contextUpdateDescriptor = $convert.base64Decode(
    'Cg1Db250ZXh0VXBkYXRlEiMKDXByb21wdF90b2tlbnMYASABKA1SDHByb21wdFRva2VucxIlCg'
    '5jb250ZXh0X3dpbmRvdxgCIAEoDVINY29udGV4dFdpbmRvdxIaCghtZXNzYWdlcxgDIAEoDVII'
    'bWVzc2FnZXM=');

@$core.Deprecated('Use runFinishedDescriptor instead')
const RunFinished$json = {
  '1': 'RunFinished',
  '2': [
    {'1': 'ok', '3': 1, '4': 1, '5': 8, '10': 'ok'},
  ],
};

/// Descriptor for `RunFinished`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List runFinishedDescriptor =
    $convert.base64Decode('CgtSdW5GaW5pc2hlZBIOCgJvaxgBIAEoCFICb2s=');

@$core.Deprecated('Use statusSnapshotDescriptor instead')
const StatusSnapshot$json = {
  '1': 'StatusSnapshot',
  '2': [
    {'1': 'current_mode', '3': 1, '4': 1, '5': 9, '10': 'currentMode'},
    {'1': 'context_tokens', '3': 2, '4': 1, '5': 13, '10': 'contextTokens'},
    {'1': 'context_window', '3': 3, '4': 1, '5': 13, '10': 'contextWindow'},
    {'1': 'context_messages', '3': 4, '4': 1, '5': 13, '10': 'contextMessages'},
    {'1': 'active', '3': 5, '4': 1, '5': 8, '10': 'active'},
  ],
};

/// Descriptor for `StatusSnapshot`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List statusSnapshotDescriptor = $convert.base64Decode(
    'Cg5TdGF0dXNTbmFwc2hvdBIhCgxjdXJyZW50X21vZGUYASABKAlSC2N1cnJlbnRNb2RlEiUKDm'
    'NvbnRleHRfdG9rZW5zGAIgASgNUg1jb250ZXh0VG9rZW5zEiUKDmNvbnRleHRfd2luZG93GAMg'
    'ASgNUg1jb250ZXh0V2luZG93EikKEGNvbnRleHRfbWVzc2FnZXMYBCABKA1SD2NvbnRleHRNZX'
    'NzYWdlcxIWCgZhY3RpdmUYBSABKAhSBmFjdGl2ZQ==');

@$core.Deprecated('Use sessionEventDescriptor instead')
const SessionEvent$json = {
  '1': 'SessionEvent',
  '2': [
    {
      '1': 'run_started',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.RunStarted',
      '9': 0,
      '10': 'runStarted'
    },
    {
      '1': 'iteration',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.IterationStart',
      '9': 0,
      '10': 'iteration'
    },
    {
      '1': 'token',
      '3': 3,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.TokenDelta',
      '9': 0,
      '10': 'token'
    },
    {
      '1': 'tool_start',
      '3': 4,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ToolCallStart',
      '9': 0,
      '10': 'toolStart'
    },
    {
      '1': 'tool_result',
      '3': 5,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ToolCallResult',
      '9': 0,
      '10': 'toolResult'
    },
    {
      '1': 'mode_switch',
      '3': 6,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.SessionModeSwitch',
      '9': 0,
      '10': 'modeSwitch'
    },
    {
      '1': 'context_update',
      '3': 7,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ContextUpdate',
      '9': 0,
      '10': 'contextUpdate'
    },
    {
      '1': 'run_finished',
      '3': 8,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.RunFinished',
      '9': 0,
      '10': 'runFinished'
    },
    {
      '1': 'status_snapshot',
      '3': 9,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.StatusSnapshot',
      '9': 0,
      '10': 'statusSnapshot'
    },
  ],
  '8': [
    {'1': 'kind'},
  ],
};

/// Descriptor for `SessionEvent`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List sessionEventDescriptor = $convert.base64Decode(
    'CgxTZXNzaW9uRXZlbnQSNwoLcnVuX3N0YXJ0ZWQYASABKAsyFC5hZ2VudC52MS5SdW5TdGFydG'
    'VkSABSCnJ1blN0YXJ0ZWQSOAoJaXRlcmF0aW9uGAIgASgLMhguYWdlbnQudjEuSXRlcmF0aW9u'
    'U3RhcnRIAFIJaXRlcmF0aW9uEiwKBXRva2VuGAMgASgLMhQuYWdlbnQudjEuVG9rZW5EZWx0YU'
    'gAUgV0b2tlbhI4Cgp0b29sX3N0YXJ0GAQgASgLMhcuYWdlbnQudjEuVG9vbENhbGxTdGFydEgA'
    'Ugl0b29sU3RhcnQSOwoLdG9vbF9yZXN1bHQYBSABKAsyGC5hZ2VudC52MS5Ub29sQ2FsbFJlc3'
    'VsdEgAUgp0b29sUmVzdWx0Ej4KC21vZGVfc3dpdGNoGAYgASgLMhsuYWdlbnQudjEuU2Vzc2lv'
    'bk1vZGVTd2l0Y2hIAFIKbW9kZVN3aXRjaBJACg5jb250ZXh0X3VwZGF0ZRgHIAEoCzIXLmFnZW'
    '50LnYxLkNvbnRleHRVcGRhdGVIAFINY29udGV4dFVwZGF0ZRI6CgxydW5fZmluaXNoZWQYCCAB'
    'KAsyFS5hZ2VudC52MS5SdW5GaW5pc2hlZEgAUgtydW5GaW5pc2hlZBJDCg9zdGF0dXNfc25hcH'
    'Nob3QYCSABKAsyGC5hZ2VudC52MS5TdGF0dXNTbmFwc2hvdEgAUg5zdGF0dXNTbmFwc2hvdEIG'
    'CgRraW5k');
