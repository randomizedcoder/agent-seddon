// This is a generated file - do not edit.
//
// Generated from agent/v1/config.proto.

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

@$core.Deprecated('Use getSchemaRequestDescriptor instead')
const GetSchemaRequest$json = {
  '1': 'GetSchemaRequest',
};

/// Descriptor for `GetSchemaRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List getSchemaRequestDescriptor =
    $convert.base64Decode('ChBHZXRTY2hlbWFSZXF1ZXN0');

@$core.Deprecated('Use configSchemaDescriptor instead')
const ConfigSchema$json = {
  '1': 'ConfigSchema',
  '2': [
    {
      '1': 'schema',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.JsonValue',
      '10': 'schema'
    },
  ],
};

/// Descriptor for `ConfigSchema`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List configSchemaDescriptor = $convert.base64Decode(
    'CgxDb25maWdTY2hlbWESKwoGc2NoZW1hGAEgASgLMhMuYWdlbnQudjEuSnNvblZhbHVlUgZzY2'
    'hlbWE=');

@$core.Deprecated('Use getValuesRequestDescriptor instead')
const GetValuesRequest$json = {
  '1': 'GetValuesRequest',
};

/// Descriptor for `GetValuesRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List getValuesRequestDescriptor =
    $convert.base64Decode('ChBHZXRWYWx1ZXNSZXF1ZXN0');

@$core.Deprecated('Use configValuesDescriptor instead')
const ConfigValues$json = {
  '1': 'ConfigValues',
  '2': [
    {
      '1': 'values',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.JsonValue',
      '10': 'values'
    },
  ],
};

/// Descriptor for `ConfigValues`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List configValuesDescriptor = $convert.base64Decode(
    'CgxDb25maWdWYWx1ZXMSKwoGdmFsdWVzGAEgASgLMhMuYWdlbnQudjEuSnNvblZhbHVlUgZ2YW'
    'x1ZXM=');

@$core.Deprecated('Use configEditDescriptor instead')
const ConfigEdit$json = {
  '1': 'ConfigEdit',
  '2': [
    {'1': 'path', '3': 1, '4': 1, '5': 9, '10': 'path'},
    {
      '1': 'value',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.JsonValue',
      '10': 'value'
    },
  ],
};

/// Descriptor for `ConfigEdit`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List configEditDescriptor = $convert.base64Decode(
    'CgpDb25maWdFZGl0EhIKBHBhdGgYASABKAlSBHBhdGgSKQoFdmFsdWUYAiABKAsyEy5hZ2VudC'
    '52MS5Kc29uVmFsdWVSBXZhbHVl');

@$core.Deprecated('Use configIssueDescriptor instead')
const ConfigIssue$json = {
  '1': 'ConfigIssue',
  '2': [
    {'1': 'path', '3': 1, '4': 1, '5': 9, '10': 'path'},
    {'1': 'code', '3': 2, '4': 1, '5': 9, '10': 'code'},
    {'1': 'detail', '3': 3, '4': 1, '5': 9, '10': 'detail'},
  ],
};

/// Descriptor for `ConfigIssue`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List configIssueDescriptor = $convert.base64Decode(
    'CgtDb25maWdJc3N1ZRISCgRwYXRoGAEgASgJUgRwYXRoEhIKBGNvZGUYAiABKAlSBGNvZGUSFg'
    'oGZGV0YWlsGAMgASgJUgZkZXRhaWw=');

@$core.Deprecated('Use validateConfigRequestDescriptor instead')
const ValidateConfigRequest$json = {
  '1': 'ValidateConfigRequest',
  '2': [
    {
      '1': 'edits',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ConfigEdit',
      '10': 'edits'
    },
  ],
};

/// Descriptor for `ValidateConfigRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List validateConfigRequestDescriptor = $convert.base64Decode(
    'ChVWYWxpZGF0ZUNvbmZpZ1JlcXVlc3QSKgoFZWRpdHMYASADKAsyFC5hZ2VudC52MS5Db25maW'
    'dFZGl0UgVlZGl0cw==');

@$core.Deprecated('Use validateConfigResponseDescriptor instead')
const ValidateConfigResponse$json = {
  '1': 'ValidateConfigResponse',
  '2': [
    {
      '1': 'issues',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ConfigIssue',
      '10': 'issues'
    },
  ],
};

/// Descriptor for `ValidateConfigResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List validateConfigResponseDescriptor =
    $convert.base64Decode(
        'ChZWYWxpZGF0ZUNvbmZpZ1Jlc3BvbnNlEi0KBmlzc3VlcxgBIAMoCzIVLmFnZW50LnYxLkNvbm'
        'ZpZ0lzc3VlUgZpc3N1ZXM=');

@$core.Deprecated('Use putConfigRequestDescriptor instead')
const PutConfigRequest$json = {
  '1': 'PutConfigRequest',
  '2': [
    {
      '1': 'edits',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ConfigEdit',
      '10': 'edits'
    },
  ],
};

/// Descriptor for `PutConfigRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List putConfigRequestDescriptor = $convert.base64Decode(
    'ChBQdXRDb25maWdSZXF1ZXN0EioKBWVkaXRzGAEgAygLMhQuYWdlbnQudjEuQ29uZmlnRWRpdF'
    'IFZWRpdHM=');

@$core.Deprecated('Use putConfigResponseDescriptor instead')
const PutConfigResponse$json = {
  '1': 'PutConfigResponse',
  '2': [
    {'1': 'restart_required', '3': 1, '4': 1, '5': 8, '10': 'restartRequired'},
    {
      '1': 'issues',
      '3': 2,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ConfigIssue',
      '10': 'issues'
    },
  ],
};

/// Descriptor for `PutConfigResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List putConfigResponseDescriptor = $convert.base64Decode(
    'ChFQdXRDb25maWdSZXNwb25zZRIpChByZXN0YXJ0X3JlcXVpcmVkGAEgASgIUg9yZXN0YXJ0Um'
    'VxdWlyZWQSLQoGaXNzdWVzGAIgAygLMhUuYWdlbnQudjEuQ29uZmlnSXNzdWVSBmlzc3Vlcw==');

@$core.Deprecated('Use configStatusRequestDescriptor instead')
const ConfigStatusRequest$json = {
  '1': 'ConfigStatusRequest',
};

/// Descriptor for `ConfigStatusRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List configStatusRequestDescriptor =
    $convert.base64Decode('ChNDb25maWdTdGF0dXNSZXF1ZXN0');

@$core.Deprecated('Use configStatusDescriptor instead')
const ConfigStatus$json = {
  '1': 'ConfigStatus',
  '2': [
    {'1': 'restart_required', '3': 1, '4': 1, '5': 8, '10': 'restartRequired'},
    {
      '1': 'pending',
      '3': 2,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ConfigEdit',
      '10': 'pending'
    },
    {'1': 'loaded_hash', '3': 3, '4': 1, '5': 9, '10': 'loadedHash'},
    {'1': 'ondisk_hash', '3': 4, '4': 1, '5': 9, '10': 'ondiskHash'},
  ],
};

/// Descriptor for `ConfigStatus`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List configStatusDescriptor = $convert.base64Decode(
    'CgxDb25maWdTdGF0dXMSKQoQcmVzdGFydF9yZXF1aXJlZBgBIAEoCFIPcmVzdGFydFJlcXVpcm'
    'VkEi4KB3BlbmRpbmcYAiADKAsyFC5hZ2VudC52MS5Db25maWdFZGl0UgdwZW5kaW5nEh8KC2xv'
    'YWRlZF9oYXNoGAMgASgJUgpsb2FkZWRIYXNoEh8KC29uZGlza19oYXNoGAQgASgJUgpvbmRpc2'
    'tIYXNo');
