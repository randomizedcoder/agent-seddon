// This is a generated file - do not edit.
//
// Generated from agent/v1/exec.proto.

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

@$core.Deprecated('Use execNetworkPolicyDescriptor instead')
const ExecNetworkPolicy$json = {
  '1': 'ExecNetworkPolicy',
  '2': [
    {'1': 'EXEC_NETWORK_POLICY_ON', '2': 0},
    {'1': 'EXEC_NETWORK_POLICY_OFF', '2': 1},
    {'1': 'EXEC_NETWORK_POLICY_LOOPBACK', '2': 2},
  ],
};

/// Descriptor for `ExecNetworkPolicy`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List execNetworkPolicyDescriptor = $convert.base64Decode(
    'ChFFeGVjTmV0d29ya1BvbGljeRIaChZFWEVDX05FVFdPUktfUE9MSUNZX09OEAASGwoXRVhFQ1'
    '9ORVRXT1JLX1BPTElDWV9PRkYQARIgChxFWEVDX05FVFdPUktfUE9MSUNZX0xPT1BCQUNLEAI=');

@$core.Deprecated('Use execEnvPolicyDescriptor instead')
const ExecEnvPolicy$json = {
  '1': 'ExecEnvPolicy',
  '2': [
    {'1': 'EXEC_ENV_POLICY_INHERIT', '2': 0},
    {'1': 'EXEC_ENV_POLICY_SCRUB', '2': 1},
  ],
};

/// Descriptor for `ExecEnvPolicy`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List execEnvPolicyDescriptor = $convert.base64Decode(
    'Cg1FeGVjRW52UG9saWN5EhsKF0VYRUNfRU5WX1BPTElDWV9JTkhFUklUEAASGQoVRVhFQ19FTl'
    'ZfUE9MSUNZX1NDUlVCEAE=');

@$core.Deprecated('Use execRequestDescriptor instead')
const ExecRequest$json = {
  '1': 'ExecRequest',
  '2': [
    {'1': 'command', '3': 1, '4': 1, '5': 9, '10': 'command'},
    {'1': 'cwd', '3': 2, '4': 1, '5': 9, '10': 'cwd'},
    {
      '1': 'network',
      '3': 3,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ExecNetworkPolicy',
      '10': 'network'
    },
    {
      '1': 'env',
      '3': 4,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ExecEnvPolicy',
      '10': 'env'
    },
    {'1': 'timeout_secs', '3': 5, '4': 1, '5': 4, '10': 'timeoutSecs'},
  ],
};

/// Descriptor for `ExecRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List execRequestDescriptor = $convert.base64Decode(
    'CgtFeGVjUmVxdWVzdBIYCgdjb21tYW5kGAEgASgJUgdjb21tYW5kEhAKA2N3ZBgCIAEoCVIDY3'
    'dkEjUKB25ldHdvcmsYAyABKA4yGy5hZ2VudC52MS5FeGVjTmV0d29ya1BvbGljeVIHbmV0d29y'
    'axIpCgNlbnYYBCABKA4yFy5hZ2VudC52MS5FeGVjRW52UG9saWN5UgNlbnYSIQoMdGltZW91dF'
    '9zZWNzGAUgASgEUgt0aW1lb3V0U2Vjcw==');

@$core.Deprecated('Use execResultDescriptor instead')
const ExecResult$json = {
  '1': 'ExecResult',
  '2': [
    {'1': 'stdout', '3': 1, '4': 1, '5': 9, '10': 'stdout'},
    {'1': 'stderr', '3': 2, '4': 1, '5': 9, '10': 'stderr'},
    {'1': 'exit_code', '3': 3, '4': 1, '5': 5, '10': 'exitCode'},
    {'1': 'timed_out', '3': 4, '4': 1, '5': 8, '10': 'timedOut'},
  ],
};

/// Descriptor for `ExecResult`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List execResultDescriptor = $convert.base64Decode(
    'CgpFeGVjUmVzdWx0EhYKBnN0ZG91dBgBIAEoCVIGc3Rkb3V0EhYKBnN0ZGVychgCIAEoCVIGc3'
    'RkZXJyEhsKCWV4aXRfY29kZRgDIAEoBVIIZXhpdENvZGUSGwoJdGltZWRfb3V0GAQgASgIUgh0'
    'aW1lZE91dA==');

@$core.Deprecated('Use execCapabilitiesRequestDescriptor instead')
const ExecCapabilitiesRequest$json = {
  '1': 'ExecCapabilitiesRequest',
};

/// Descriptor for `ExecCapabilitiesRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List execCapabilitiesRequestDescriptor =
    $convert.base64Decode('ChdFeGVjQ2FwYWJpbGl0aWVzUmVxdWVzdA==');

@$core.Deprecated('Use execCapabilitiesDescriptor instead')
const ExecCapabilities$json = {
  '1': 'ExecCapabilities',
  '2': [
    {'1': 'backend', '3': 1, '4': 1, '5': 9, '10': 'backend'},
    {'1': 'available', '3': 2, '4': 1, '5': 8, '10': 'available'},
    {'1': 'network_off', '3': 3, '4': 1, '5': 8, '10': 'networkOff'},
    {'1': 'private_tmp', '3': 4, '4': 1, '5': 8, '10': 'privateTmp'},
    {
      '1': 'content_addressed',
      '3': 5,
      '4': 1,
      '5': 8,
      '10': 'contentAddressed'
    },
  ],
};

/// Descriptor for `ExecCapabilities`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List execCapabilitiesDescriptor = $convert.base64Decode(
    'ChBFeGVjQ2FwYWJpbGl0aWVzEhgKB2JhY2tlbmQYASABKAlSB2JhY2tlbmQSHAoJYXZhaWxhYm'
    'xlGAIgASgIUglhdmFpbGFibGUSHwoLbmV0d29ya19vZmYYAyABKAhSCm5ldHdvcmtPZmYSHwoL'
    'cHJpdmF0ZV90bXAYBCABKAhSCnByaXZhdGVUbXASKwoRY29udGVudF9hZGRyZXNzZWQYBSABKA'
    'hSEGNvbnRlbnRBZGRyZXNzZWQ=');

@$core.Deprecated('Use ptySessionRefDescriptor instead')
const PtySessionRef$json = {
  '1': 'PtySessionRef',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
  ],
};

/// Descriptor for `PtySessionRef`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List ptySessionRefDescriptor =
    $convert.base64Decode('Cg1QdHlTZXNzaW9uUmVmEg4KAmlkGAEgASgJUgJpZA==');

@$core.Deprecated('Use ptyOpenRequestDescriptor instead')
const PtyOpenRequest$json = {
  '1': 'PtyOpenRequest',
  '2': [
    {'1': 'command', '3': 1, '4': 1, '5': 9, '10': 'command'},
    {'1': 'args', '3': 2, '4': 3, '5': 9, '10': 'args'},
    {'1': 'cols', '3': 3, '4': 1, '5': 13, '10': 'cols'},
    {'1': 'rows', '3': 4, '4': 1, '5': 13, '10': 'rows'},
    {'1': 'cwd', '3': 5, '4': 1, '5': 9, '10': 'cwd'},
  ],
};

/// Descriptor for `PtyOpenRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List ptyOpenRequestDescriptor = $convert.base64Decode(
    'Cg5QdHlPcGVuUmVxdWVzdBIYCgdjb21tYW5kGAEgASgJUgdjb21tYW5kEhIKBGFyZ3MYAiADKA'
    'lSBGFyZ3MSEgoEY29scxgDIAEoDVIEY29scxISCgRyb3dzGAQgASgNUgRyb3dzEhAKA2N3ZBgF'
    'IAEoCVIDY3dk');

@$core.Deprecated('Use ptyWriteRequestDescriptor instead')
const PtyWriteRequest$json = {
  '1': 'PtyWriteRequest',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'input', '3': 2, '4': 1, '5': 12, '10': 'input'},
  ],
};

/// Descriptor for `PtyWriteRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List ptyWriteRequestDescriptor = $convert.base64Decode(
    'Cg9QdHlXcml0ZVJlcXVlc3QSDgoCaWQYASABKAlSAmlkEhQKBWlucHV0GAIgASgMUgVpbnB1dA'
    '==');

@$core.Deprecated('Use ptyWriteResponseDescriptor instead')
const PtyWriteResponse$json = {
  '1': 'PtyWriteResponse',
};

/// Descriptor for `PtyWriteResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List ptyWriteResponseDescriptor =
    $convert.base64Decode('ChBQdHlXcml0ZVJlc3BvbnNl');

@$core.Deprecated('Use ptyReadRequestDescriptor instead')
const PtyReadRequest$json = {
  '1': 'PtyReadRequest',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'cursor', '3': 2, '4': 1, '5': 4, '9': 0, '10': 'cursor', '17': true},
  ],
  '8': [
    {'1': '_cursor'},
  ],
};

/// Descriptor for `PtyReadRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List ptyReadRequestDescriptor = $convert.base64Decode(
    'Cg5QdHlSZWFkUmVxdWVzdBIOCgJpZBgBIAEoCVICaWQSGwoGY3Vyc29yGAIgASgESABSBmN1cn'
    'NvcogBAUIJCgdfY3Vyc29y');

@$core.Deprecated('Use ptyStateMsgDescriptor instead')
const PtyStateMsg$json = {
  '1': 'PtyStateMsg',
  '2': [
    {'1': 'running', '3': 1, '4': 1, '5': 8, '10': 'running'},
    {'1': 'closed', '3': 2, '4': 1, '5': 8, '10': 'closed'},
    {
      '1': 'exit_code',
      '3': 3,
      '4': 1,
      '5': 5,
      '9': 0,
      '10': 'exitCode',
      '17': true
    },
  ],
  '8': [
    {'1': '_exit_code'},
  ],
};

/// Descriptor for `PtyStateMsg`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List ptyStateMsgDescriptor = $convert.base64Decode(
    'CgtQdHlTdGF0ZU1zZxIYCgdydW5uaW5nGAEgASgIUgdydW5uaW5nEhYKBmNsb3NlZBgCIAEoCF'
    'IGY2xvc2VkEiAKCWV4aXRfY29kZRgDIAEoBUgAUghleGl0Q29kZYgBAUIMCgpfZXhpdF9jb2Rl');

@$core.Deprecated('Use ptyReadResponseDescriptor instead')
const PtyReadResponse$json = {
  '1': 'PtyReadResponse',
  '2': [
    {'1': 'data', '3': 1, '4': 1, '5': 12, '10': 'data'},
    {'1': 'next_cursor', '3': 2, '4': 1, '5': 4, '10': 'nextCursor'},
    {'1': 'dropped', '3': 3, '4': 1, '5': 4, '10': 'dropped'},
    {
      '1': 'state',
      '3': 4,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.PtyStateMsg',
      '10': 'state'
    },
  ],
};

/// Descriptor for `PtyReadResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List ptyReadResponseDescriptor = $convert.base64Decode(
    'Cg9QdHlSZWFkUmVzcG9uc2USEgoEZGF0YRgBIAEoDFIEZGF0YRIfCgtuZXh0X2N1cnNvchgCIA'
    'EoBFIKbmV4dEN1cnNvchIYCgdkcm9wcGVkGAMgASgEUgdkcm9wcGVkEisKBXN0YXRlGAQgASgL'
    'MhUuYWdlbnQudjEuUHR5U3RhdGVNc2dSBXN0YXRl');

@$core.Deprecated('Use ptyResizeRequestDescriptor instead')
const PtyResizeRequest$json = {
  '1': 'PtyResizeRequest',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'cols', '3': 2, '4': 1, '5': 13, '10': 'cols'},
    {'1': 'rows', '3': 3, '4': 1, '5': 13, '10': 'rows'},
  ],
};

/// Descriptor for `PtyResizeRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List ptyResizeRequestDescriptor = $convert.base64Decode(
    'ChBQdHlSZXNpemVSZXF1ZXN0Eg4KAmlkGAEgASgJUgJpZBISCgRjb2xzGAIgASgNUgRjb2xzEh'
    'IKBHJvd3MYAyABKA1SBHJvd3M=');

@$core.Deprecated('Use ptyResizeResponseDescriptor instead')
const PtyResizeResponse$json = {
  '1': 'PtyResizeResponse',
};

/// Descriptor for `PtyResizeResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List ptyResizeResponseDescriptor =
    $convert.base64Decode('ChFQdHlSZXNpemVSZXNwb25zZQ==');

@$core.Deprecated('Use ptyCloseResponseDescriptor instead')
const PtyCloseResponse$json = {
  '1': 'PtyCloseResponse',
  '2': [
    {'1': 'closed', '3': 1, '4': 1, '5': 8, '10': 'closed'},
  ],
};

/// Descriptor for `PtyCloseResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List ptyCloseResponseDescriptor = $convert
    .base64Decode('ChBQdHlDbG9zZVJlc3BvbnNlEhYKBmNsb3NlZBgBIAEoCFIGY2xvc2Vk');

@$core.Deprecated('Use ptyListRequestDescriptor instead')
const PtyListRequest$json = {
  '1': 'PtyListRequest',
};

/// Descriptor for `PtyListRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List ptyListRequestDescriptor =
    $convert.base64Decode('Cg5QdHlMaXN0UmVxdWVzdA==');

@$core.Deprecated('Use ptySessionInfoDescriptor instead')
const PtySessionInfo$json = {
  '1': 'PtySessionInfo',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'command', '3': 2, '4': 1, '5': 9, '10': 'command'},
    {
      '1': 'state',
      '3': 3,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.PtyStateMsg',
      '10': 'state'
    },
    {'1': 'cols', '3': 4, '4': 1, '5': 13, '10': 'cols'},
    {'1': 'rows', '3': 5, '4': 1, '5': 13, '10': 'rows'},
    {'1': 'bytes_out', '3': 6, '4': 1, '5': 4, '10': 'bytesOut'},
    {'1': 'first_retained', '3': 7, '4': 1, '5': 4, '10': 'firstRetained'},
    {'1': 'next_cursor', '3': 8, '4': 1, '5': 4, '10': 'nextCursor'},
  ],
};

/// Descriptor for `PtySessionInfo`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List ptySessionInfoDescriptor = $convert.base64Decode(
    'Cg5QdHlTZXNzaW9uSW5mbxIOCgJpZBgBIAEoCVICaWQSGAoHY29tbWFuZBgCIAEoCVIHY29tbW'
    'FuZBIrCgVzdGF0ZRgDIAEoCzIVLmFnZW50LnYxLlB0eVN0YXRlTXNnUgVzdGF0ZRISCgRjb2xz'
    'GAQgASgNUgRjb2xzEhIKBHJvd3MYBSABKA1SBHJvd3MSGwoJYnl0ZXNfb3V0GAYgASgEUghieX'
    'Rlc091dBIlCg5maXJzdF9yZXRhaW5lZBgHIAEoBFINZmlyc3RSZXRhaW5lZBIfCgtuZXh0X2N1'
    'cnNvchgIIAEoBFIKbmV4dEN1cnNvcg==');

@$core.Deprecated('Use ptySessionListDescriptor instead')
const PtySessionList$json = {
  '1': 'PtySessionList',
  '2': [
    {
      '1': 'sessions',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.PtySessionInfo',
      '10': 'sessions'
    },
  ],
};

/// Descriptor for `PtySessionList`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List ptySessionListDescriptor = $convert.base64Decode(
    'Cg5QdHlTZXNzaW9uTGlzdBI0CghzZXNzaW9ucxgBIAMoCzIYLmFnZW50LnYxLlB0eVNlc3Npb2'
    '5JbmZvUghzZXNzaW9ucw==');
