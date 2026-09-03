// This is a generated file - do not edit.
//
// Generated from agent/v1/common.proto.

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

@$core.Deprecated('Use roleDescriptor instead')
const Role$json = {
  '1': 'Role',
  '2': [
    {'1': 'ROLE_UNSPECIFIED', '2': 0},
    {'1': 'ROLE_SYSTEM', '2': 1},
    {'1': 'ROLE_USER', '2': 2},
    {'1': 'ROLE_ASSISTANT', '2': 3},
    {'1': 'ROLE_TOOL', '2': 4},
  ],
};

/// Descriptor for `Role`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List roleDescriptor = $convert.base64Decode(
    'CgRSb2xlEhQKEFJPTEVfVU5TUEVDSUZJRUQQABIPCgtST0xFX1NZU1RFTRABEg0KCVJPTEVfVV'
    'NFUhACEhIKDlJPTEVfQVNTSVNUQU5UEAMSDQoJUk9MRV9UT09MEAQ=');

@$core.Deprecated('Use nullValueDescriptor instead')
const NullValue$json = {
  '1': 'NullValue',
  '2': [
    {'1': 'NULL_VALUE', '2': 0},
  ],
};

/// Descriptor for `NullValue`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List nullValueDescriptor =
    $convert.base64Decode('CglOdWxsVmFsdWUSDgoKTlVMTF9WQUxVRRAA');

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

@$core.Deprecated('Use poolTierDescriptor instead')
const PoolTier$json = {
  '1': 'PoolTier',
  '2': [
    {'1': 'POOL_TIER_UNSPECIFIED', '2': 0},
    {'1': 'POOL_TIER_LIGHT', '2': 1},
    {'1': 'POOL_TIER_MEDIUM', '2': 2},
    {'1': 'POOL_TIER_HEAVY', '2': 3},
  ],
};

/// Descriptor for `PoolTier`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List poolTierDescriptor = $convert.base64Decode(
    'CghQb29sVGllchIZChVQT09MX1RJRVJfVU5TUEVDSUZJRUQQABITCg9QT09MX1RJRVJfTElHSF'
    'QQARIUChBQT09MX1RJRVJfTUVESVVNEAISEwoPUE9PTF9USUVSX0hFQVZZEAM=');

@$core.Deprecated('Use routeRoleDescriptor instead')
const RouteRole$json = {
  '1': 'RouteRole',
  '2': [
    {'1': 'ROUTE_ROLE_UNSPECIFIED', '2': 0},
    {'1': 'ROUTE_ROLE_MAIN', '2': 1},
    {'1': 'ROUTE_ROLE_JUDGE', '2': 2},
    {'1': 'ROUTE_ROLE_CLASSIFY', '2': 3},
    {'1': 'ROUTE_ROLE_SUMMARIZE', '2': 4},
    {'1': 'ROUTE_ROLE_VERIFY', '2': 5},
    {'1': 'ROUTE_ROLE_REVIEW', '2': 6},
  ],
};

/// Descriptor for `RouteRole`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List routeRoleDescriptor = $convert.base64Decode(
    'CglSb3V0ZVJvbGUSGgoWUk9VVEVfUk9MRV9VTlNQRUNJRklFRBAAEhMKD1JPVVRFX1JPTEVfTU'
    'FJThABEhQKEFJPVVRFX1JPTEVfSlVER0UQAhIXChNST1VURV9ST0xFX0NMQVNTSUZZEAMSGAoU'
    'Uk9VVEVfUk9MRV9TVU1NQVJJWkUQBBIVChFST1VURV9ST0xFX1ZFUklGWRAFEhUKEVJPVVRFX1'
    'JPTEVfUkVWSUVXEAY=');

@$core.Deprecated('Use jsonValueDescriptor instead')
const JsonValue$json = {
  '1': 'JsonValue',
  '2': [
    {
      '1': 'null_value',
      '3': 1,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.NullValue',
      '9': 0,
      '10': 'nullValue'
    },
    {'1': 'bool_value', '3': 2, '4': 1, '5': 8, '9': 0, '10': 'boolValue'},
    {'1': 'int_value', '3': 3, '4': 1, '5': 18, '9': 0, '10': 'intValue'},
    {'1': 'uint_value', '3': 4, '4': 1, '5': 4, '9': 0, '10': 'uintValue'},
    {'1': 'double_value', '3': 5, '4': 1, '5': 1, '9': 0, '10': 'doubleValue'},
    {'1': 'string_value', '3': 6, '4': 1, '5': 9, '9': 0, '10': 'stringValue'},
    {
      '1': 'array_value',
      '3': 7,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.JsonArray',
      '9': 0,
      '10': 'arrayValue'
    },
    {
      '1': 'object_value',
      '3': 8,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.JsonObject',
      '9': 0,
      '10': 'objectValue'
    },
    {'1': 'big_number', '3': 9, '4': 1, '5': 9, '9': 0, '10': 'bigNumber'},
  ],
  '8': [
    {'1': 'kind'},
  ],
};

/// Descriptor for `JsonValue`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List jsonValueDescriptor = $convert.base64Decode(
    'CglKc29uVmFsdWUSNAoKbnVsbF92YWx1ZRgBIAEoDjITLmFnZW50LnYxLk51bGxWYWx1ZUgAUg'
    'ludWxsVmFsdWUSHwoKYm9vbF92YWx1ZRgCIAEoCEgAUglib29sVmFsdWUSHQoJaW50X3ZhbHVl'
    'GAMgASgSSABSCGludFZhbHVlEh8KCnVpbnRfdmFsdWUYBCABKARIAFIJdWludFZhbHVlEiMKDG'
    'RvdWJsZV92YWx1ZRgFIAEoAUgAUgtkb3VibGVWYWx1ZRIjCgxzdHJpbmdfdmFsdWUYBiABKAlI'
    'AFILc3RyaW5nVmFsdWUSNgoLYXJyYXlfdmFsdWUYByABKAsyEy5hZ2VudC52MS5Kc29uQXJyYX'
    'lIAFIKYXJyYXlWYWx1ZRI5CgxvYmplY3RfdmFsdWUYCCABKAsyFC5hZ2VudC52MS5Kc29uT2Jq'
    'ZWN0SABSC29iamVjdFZhbHVlEh8KCmJpZ19udW1iZXIYCSABKAlIAFIJYmlnTnVtYmVyQgYKBG'
    'tpbmQ=');

@$core.Deprecated('Use jsonArrayDescriptor instead')
const JsonArray$json = {
  '1': 'JsonArray',
  '2': [
    {
      '1': 'values',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.JsonValue',
      '10': 'values'
    },
  ],
};

/// Descriptor for `JsonArray`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List jsonArrayDescriptor = $convert.base64Decode(
    'CglKc29uQXJyYXkSKwoGdmFsdWVzGAEgAygLMhMuYWdlbnQudjEuSnNvblZhbHVlUgZ2YWx1ZX'
    'M=');

@$core.Deprecated('Use jsonObjectDescriptor instead')
const JsonObject$json = {
  '1': 'JsonObject',
  '2': [
    {
      '1': 'fields',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.JsonObject.FieldsEntry',
      '10': 'fields'
    },
  ],
  '3': [JsonObject_FieldsEntry$json],
};

@$core.Deprecated('Use jsonObjectDescriptor instead')
const JsonObject_FieldsEntry$json = {
  '1': 'FieldsEntry',
  '2': [
    {'1': 'key', '3': 1, '4': 1, '5': 9, '10': 'key'},
    {
      '1': 'value',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.JsonValue',
      '10': 'value'
    },
  ],
  '7': {'7': true},
};

/// Descriptor for `JsonObject`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List jsonObjectDescriptor = $convert.base64Decode(
    'CgpKc29uT2JqZWN0EjgKBmZpZWxkcxgBIAMoCzIgLmFnZW50LnYxLkpzb25PYmplY3QuRmllbG'
    'RzRW50cnlSBmZpZWxkcxpOCgtGaWVsZHNFbnRyeRIQCgNrZXkYASABKAlSA2tleRIpCgV2YWx1'
    'ZRgCIAEoCzITLmFnZW50LnYxLkpzb25WYWx1ZVIFdmFsdWU6AjgB');

@$core.Deprecated('Use toolCallDescriptor instead')
const ToolCall$json = {
  '1': 'ToolCall',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'name', '3': 2, '4': 1, '5': 9, '10': 'name'},
    {
      '1': 'arguments',
      '3': 3,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.JsonValue',
      '10': 'arguments'
    },
  ],
};

/// Descriptor for `ToolCall`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List toolCallDescriptor = $convert.base64Decode(
    'CghUb29sQ2FsbBIOCgJpZBgBIAEoCVICaWQSEgoEbmFtZRgCIAEoCVIEbmFtZRIxCglhcmd1bW'
    'VudHMYAyABKAsyEy5hZ2VudC52MS5Kc29uVmFsdWVSCWFyZ3VtZW50cw==');

@$core.Deprecated('Use contentBlockDescriptor instead')
const ContentBlock$json = {
  '1': 'ContentBlock',
  '2': [
    {
      '1': 'text',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ContentBlock.Text',
      '9': 0,
      '10': 'text'
    },
    {
      '1': 'image',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ContentBlock.Image',
      '9': 0,
      '10': 'image'
    },
    {
      '1': 'document',
      '3': 3,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ContentBlock.Document',
      '9': 0,
      '10': 'document'
    },
  ],
  '3': [
    ContentBlock_Text$json,
    ContentBlock_Image$json,
    ContentBlock_Document$json
  ],
  '8': [
    {'1': 'kind'},
  ],
};

@$core.Deprecated('Use contentBlockDescriptor instead')
const ContentBlock_Text$json = {
  '1': 'Text',
  '2': [
    {'1': 'text', '3': 1, '4': 1, '5': 9, '10': 'text'},
  ],
};

@$core.Deprecated('Use contentBlockDescriptor instead')
const ContentBlock_Image$json = {
  '1': 'Image',
  '2': [
    {'1': 'media_type', '3': 1, '4': 1, '5': 9, '10': 'mediaType'},
    {'1': 'data', '3': 2, '4': 1, '5': 12, '10': 'data'},
  ],
};

@$core.Deprecated('Use contentBlockDescriptor instead')
const ContentBlock_Document$json = {
  '1': 'Document',
  '2': [
    {'1': 'media_type', '3': 1, '4': 1, '5': 9, '10': 'mediaType'},
    {'1': 'data', '3': 2, '4': 1, '5': 12, '10': 'data'},
    {'1': 'name', '3': 3, '4': 1, '5': 9, '9': 0, '10': 'name', '17': true},
  ],
  '8': [
    {'1': '_name'},
  ],
};

/// Descriptor for `ContentBlock`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List contentBlockDescriptor = $convert.base64Decode(
    'CgxDb250ZW50QmxvY2sSMQoEdGV4dBgBIAEoCzIbLmFnZW50LnYxLkNvbnRlbnRCbG9jay5UZX'
    'h0SABSBHRleHQSNAoFaW1hZ2UYAiABKAsyHC5hZ2VudC52MS5Db250ZW50QmxvY2suSW1hZ2VI'
    'AFIFaW1hZ2USPQoIZG9jdW1lbnQYAyABKAsyHy5hZ2VudC52MS5Db250ZW50QmxvY2suRG9jdW'
    '1lbnRIAFIIZG9jdW1lbnQaGgoEVGV4dBISCgR0ZXh0GAEgASgJUgR0ZXh0GjoKBUltYWdlEh0K'
    'Cm1lZGlhX3R5cGUYASABKAlSCW1lZGlhVHlwZRISCgRkYXRhGAIgASgMUgRkYXRhGl8KCERvY3'
    'VtZW50Eh0KCm1lZGlhX3R5cGUYASABKAlSCW1lZGlhVHlwZRISCgRkYXRhGAIgASgMUgRkYXRh'
    'EhcKBG5hbWUYAyABKAlIAFIEbmFtZYgBAUIHCgVfbmFtZUIGCgRraW5k');

@$core.Deprecated('Use messageDescriptor instead')
const Message$json = {
  '1': 'Message',
  '2': [
    {'1': 'role', '3': 1, '4': 1, '5': 14, '6': '.agent.v1.Role', '10': 'role'},
    {'1': 'content', '3': 2, '4': 1, '5': 9, '10': 'content'},
    {
      '1': 'tool_calls',
      '3': 3,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ToolCall',
      '10': 'toolCalls'
    },
    {
      '1': 'tool_call_id',
      '3': 4,
      '4': 1,
      '5': 9,
      '9': 0,
      '10': 'toolCallId',
      '17': true
    },
    {
      '1': 'blocks',
      '3': 5,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ContentBlock',
      '10': 'blocks'
    },
  ],
  '8': [
    {'1': '_tool_call_id'},
  ],
};

/// Descriptor for `Message`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List messageDescriptor = $convert.base64Decode(
    'CgdNZXNzYWdlEiIKBHJvbGUYASABKA4yDi5hZ2VudC52MS5Sb2xlUgRyb2xlEhgKB2NvbnRlbn'
    'QYAiABKAlSB2NvbnRlbnQSMQoKdG9vbF9jYWxscxgDIAMoCzISLmFnZW50LnYxLlRvb2xDYWxs'
    'Ugl0b29sQ2FsbHMSJQoMdG9vbF9jYWxsX2lkGAQgASgJSABSCnRvb2xDYWxsSWSIAQESLgoGYm'
    'xvY2tzGAUgAygLMhYuYWdlbnQudjEuQ29udGVudEJsb2NrUgZibG9ja3NCDwoNX3Rvb2xfY2Fs'
    'bF9pZA==');

@$core.Deprecated('Use toolSchemaDescriptor instead')
const ToolSchema$json = {
  '1': 'ToolSchema',
  '2': [
    {'1': 'name', '3': 1, '4': 1, '5': 9, '10': 'name'},
    {'1': 'description', '3': 2, '4': 1, '5': 9, '10': 'description'},
    {
      '1': 'parameters',
      '3': 3,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.JsonValue',
      '10': 'parameters'
    },
    {'1': 'parallel_safe', '3': 4, '4': 1, '5': 8, '10': 'parallelSafe'},
  ],
};

/// Descriptor for `ToolSchema`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List toolSchemaDescriptor = $convert.base64Decode(
    'CgpUb29sU2NoZW1hEhIKBG5hbWUYASABKAlSBG5hbWUSIAoLZGVzY3JpcHRpb24YAiABKAlSC2'
    'Rlc2NyaXB0aW9uEjMKCnBhcmFtZXRlcnMYAyABKAsyEy5hZ2VudC52MS5Kc29uVmFsdWVSCnBh'
    'cmFtZXRlcnMSIwoNcGFyYWxsZWxfc2FmZRgEIAEoCFIMcGFyYWxsZWxTYWZl');

@$core.Deprecated('Use observationDescriptor instead')
const Observation$json = {
  '1': 'Observation',
  '2': [
    {'1': 'content', '3': 1, '4': 1, '5': 9, '10': 'content'},
    {'1': 'is_error', '3': 2, '4': 1, '5': 8, '10': 'isError'},
    {
      '1': 'blocks',
      '3': 3,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ContentBlock',
      '10': 'blocks'
    },
  ],
};

/// Descriptor for `Observation`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List observationDescriptor = $convert.base64Decode(
    'CgtPYnNlcnZhdGlvbhIYCgdjb250ZW50GAEgASgJUgdjb250ZW50EhkKCGlzX2Vycm9yGAIgAS'
    'gIUgdpc0Vycm9yEi4KBmJsb2NrcxgDIAMoCzIWLmFnZW50LnYxLkNvbnRlbnRCbG9ja1IGYmxv'
    'Y2tz');

@$core.Deprecated('Use toolContextDescriptor instead')
const ToolContext$json = {
  '1': 'ToolContext',
  '2': [
    {'1': 'cwd', '3': 1, '4': 1, '5': 9, '10': 'cwd'},
  ],
};

/// Descriptor for `ToolContext`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List toolContextDescriptor =
    $convert.base64Decode('CgtUb29sQ29udGV4dBIQCgNjd2QYASABKAlSA2N3ZA==');

@$core.Deprecated('Use modelCapabilitiesDescriptor instead')
const ModelCapabilities$json = {
  '1': 'ModelCapabilities',
  '2': [
    {'1': 'supports_tools', '3': 1, '4': 1, '5': 8, '10': 'supportsTools'},
    {'1': 'context_window', '3': 2, '4': 1, '5': 13, '10': 'contextWindow'},
    {'1': 'supports_vision', '3': 3, '4': 1, '5': 8, '10': 'supportsVision'},
  ],
};

/// Descriptor for `ModelCapabilities`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List modelCapabilitiesDescriptor = $convert.base64Decode(
    'ChFNb2RlbENhcGFiaWxpdGllcxIlCg5zdXBwb3J0c190b29scxgBIAEoCFINc3VwcG9ydHNUb2'
    '9scxIlCg5jb250ZXh0X3dpbmRvdxgCIAEoDVINY29udGV4dFdpbmRvdxInCg9zdXBwb3J0c192'
    'aXNpb24YAyABKAhSDnN1cHBvcnRzVmlzaW9u');

@$core.Deprecated('Use usageDescriptor instead')
const Usage$json = {
  '1': 'Usage',
  '2': [
    {'1': 'prompt_tokens', '3': 1, '4': 1, '5': 13, '10': 'promptTokens'},
    {
      '1': 'completion_tokens',
      '3': 2,
      '4': 1,
      '5': 13,
      '10': 'completionTokens'
    },
    {'1': 'total_tokens', '3': 3, '4': 1, '5': 13, '10': 'totalTokens'},
  ],
};

/// Descriptor for `Usage`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List usageDescriptor = $convert.base64Decode(
    'CgVVc2FnZRIjCg1wcm9tcHRfdG9rZW5zGAEgASgNUgxwcm9tcHRUb2tlbnMSKwoRY29tcGxldG'
    'lvbl90b2tlbnMYAiABKA1SEGNvbXBsZXRpb25Ub2tlbnMSIQoMdG90YWxfdG9rZW5zGAMgASgN'
    'Ugt0b3RhbFRva2Vucw==');

@$core.Deprecated('Use routeHintDescriptor instead')
const RouteHint$json = {
  '1': 'RouteHint',
  '2': [
    {
      '1': 'task_mode',
      '3': 1,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.TaskMode',
      '10': 'taskMode'
    },
    {
      '1': 'role',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.RouteRole',
      '10': 'role'
    },
    {'1': 'min_context', '3': 3, '4': 1, '5': 13, '10': 'minContext'},
    {
      '1': 'max_cost',
      '3': 4,
      '4': 1,
      '5': 2,
      '9': 0,
      '10': 'maxCost',
      '17': true
    },
    {
      '1': 'tier',
      '3': 5,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.PoolTier',
      '10': 'tier'
    },
    {
      '1': 'override_upstream',
      '3': 6,
      '4': 1,
      '5': 9,
      '10': 'overrideUpstream'
    },
  ],
  '8': [
    {'1': '_max_cost'},
  ],
};

/// Descriptor for `RouteHint`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List routeHintDescriptor = $convert.base64Decode(
    'CglSb3V0ZUhpbnQSLwoJdGFza19tb2RlGAEgASgOMhIuYWdlbnQudjEuVGFza01vZGVSCHRhc2'
    'tNb2RlEicKBHJvbGUYAiABKA4yEy5hZ2VudC52MS5Sb3V0ZVJvbGVSBHJvbGUSHwoLbWluX2Nv'
    'bnRleHQYAyABKA1SCm1pbkNvbnRleHQSHgoIbWF4X2Nvc3QYBCABKAJIAFIHbWF4Q29zdIgBAR'
    'ImCgR0aWVyGAUgASgOMhIuYWdlbnQudjEuUG9vbFRpZXJSBHRpZXISKwoRb3ZlcnJpZGVfdXBz'
    'dHJlYW0YBiABKAlSEG92ZXJyaWRlVXBzdHJlYW1CCwoJX21heF9jb3N0');

@$core.Deprecated('Use completionRequestDescriptor instead')
const CompletionRequest$json = {
  '1': 'CompletionRequest',
  '2': [
    {
      '1': 'messages',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Message',
      '10': 'messages'
    },
    {
      '1': 'tools',
      '3': 2,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ToolSchema',
      '10': 'tools'
    },
    {'1': 'max_tokens', '3': 3, '4': 1, '5': 13, '10': 'maxTokens'},
    {'1': 'temperature', '3': 4, '4': 1, '5': 2, '10': 'temperature'},
    {
      '1': 'route_hint',
      '3': 5,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.RouteHint',
      '10': 'routeHint'
    },
  ],
};

/// Descriptor for `CompletionRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List completionRequestDescriptor = $convert.base64Decode(
    'ChFDb21wbGV0aW9uUmVxdWVzdBItCghtZXNzYWdlcxgBIAMoCzIRLmFnZW50LnYxLk1lc3NhZ2'
    'VSCG1lc3NhZ2VzEioKBXRvb2xzGAIgAygLMhQuYWdlbnQudjEuVG9vbFNjaGVtYVIFdG9vbHMS'
    'HQoKbWF4X3Rva2VucxgDIAEoDVIJbWF4VG9rZW5zEiAKC3RlbXBlcmF0dXJlGAQgASgCUgt0ZW'
    '1wZXJhdHVyZRIyCgpyb3V0ZV9oaW50GAUgASgLMhMuYWdlbnQudjEuUm91dGVIaW50Uglyb3V0'
    'ZUhpbnQ=');

@$core.Deprecated('Use completionResponseDescriptor instead')
const CompletionResponse$json = {
  '1': 'CompletionResponse',
  '2': [
    {
      '1': 'message',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.Message',
      '10': 'message'
    },
    {'1': 'finish_reason', '3': 2, '4': 1, '5': 9, '10': 'finishReason'},
    {
      '1': 'usage',
      '3': 3,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.Usage',
      '9': 0,
      '10': 'usage',
      '17': true
    },
  ],
  '8': [
    {'1': '_usage'},
  ],
};

/// Descriptor for `CompletionResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List completionResponseDescriptor = $convert.base64Decode(
    'ChJDb21wbGV0aW9uUmVzcG9uc2USKwoHbWVzc2FnZRgBIAEoCzIRLmFnZW50LnYxLk1lc3NhZ2'
    'VSB21lc3NhZ2USIwoNZmluaXNoX3JlYXNvbhgCIAEoCVIMZmluaXNoUmVhc29uEioKBXVzYWdl'
    'GAMgASgLMg8uYWdlbnQudjEuVXNhZ2VIAFIFdXNhZ2WIAQFCCAoGX3VzYWdl');

@$core.Deprecated('Use completionChunkDescriptor instead')
const CompletionChunk$json = {
  '1': 'CompletionChunk',
  '2': [
    {'1': 'delta_text', '3': 1, '4': 1, '5': 9, '10': 'deltaText'},
    {
      '1': 'tool_call',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ToolCall',
      '9': 0,
      '10': 'toolCall',
      '17': true
    },
    {
      '1': 'finish_reason',
      '3': 3,
      '4': 1,
      '5': 9,
      '9': 1,
      '10': 'finishReason',
      '17': true
    },
    {
      '1': 'usage',
      '3': 4,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.Usage',
      '9': 2,
      '10': 'usage',
      '17': true
    },
  ],
  '8': [
    {'1': '_tool_call'},
    {'1': '_finish_reason'},
    {'1': '_usage'},
  ],
};

/// Descriptor for `CompletionChunk`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List completionChunkDescriptor = $convert.base64Decode(
    'Cg9Db21wbGV0aW9uQ2h1bmsSHQoKZGVsdGFfdGV4dBgBIAEoCVIJZGVsdGFUZXh0EjQKCXRvb2'
    'xfY2FsbBgCIAEoCzISLmFnZW50LnYxLlRvb2xDYWxsSABSCHRvb2xDYWxsiAEBEigKDWZpbmlz'
    'aF9yZWFzb24YAyABKAlIAVIMZmluaXNoUmVhc29uiAEBEioKBXVzYWdlGAQgASgLMg8uYWdlbn'
    'QudjEuVXNhZ2VIAlIFdXNhZ2WIAQFCDAoKX3Rvb2xfY2FsbEIQCg5fZmluaXNoX3JlYXNvbkII'
    'CgZfdXNhZ2U=');

@$core.Deprecated('Use memoryItemDescriptor instead')
const MemoryItem$json = {
  '1': 'MemoryItem',
  '2': [
    {'1': 'source', '3': 1, '4': 1, '5': 9, '10': 'source'},
    {'1': 'content', '3': 2, '4': 1, '5': 9, '10': 'content'},
  ],
};

/// Descriptor for `MemoryItem`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List memoryItemDescriptor = $convert.base64Decode(
    'CgpNZW1vcnlJdGVtEhYKBnNvdXJjZRgBIAEoCVIGc291cmNlEhgKB2NvbnRlbnQYAiABKAlSB2'
    'NvbnRlbnQ=');

@$core.Deprecated('Use recallQueryDescriptor instead')
const RecallQuery$json = {
  '1': 'RecallQuery',
  '2': [
    {'1': 'text', '3': 1, '4': 1, '5': 9, '10': 'text'},
    {'1': 'limit', '3': 2, '4': 1, '5': 4, '10': 'limit'},
  ],
};

/// Descriptor for `RecallQuery`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List recallQueryDescriptor = $convert.base64Decode(
    'CgtSZWNhbGxRdWVyeRISCgR0ZXh0GAEgASgJUgR0ZXh0EhQKBWxpbWl0GAIgASgEUgVsaW1pdA'
    '==');

@$core.Deprecated('Use memoryEventDescriptor instead')
const MemoryEvent$json = {
  '1': 'MemoryEvent',
  '2': [
    {'1': 'kind', '3': 1, '4': 1, '5': 9, '10': 'kind'},
    {
      '1': 'message',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.Message',
      '10': 'message'
    },
    {'1': 'ts_ms', '3': 3, '4': 1, '5': 4, '10': 'tsMs'},
    {'1': 'session_id', '3': 4, '4': 1, '5': 9, '10': 'sessionId'},
    {
      '1': 'usage',
      '3': 5,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.Usage',
      '9': 0,
      '10': 'usage',
      '17': true
    },
    {'1': 'iter', '3': 6, '4': 1, '5': 13, '9': 1, '10': 'iter', '17': true},
  ],
  '8': [
    {'1': '_usage'},
    {'1': '_iter'},
  ],
};

/// Descriptor for `MemoryEvent`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List memoryEventDescriptor = $convert.base64Decode(
    'CgtNZW1vcnlFdmVudBISCgRraW5kGAEgASgJUgRraW5kEisKB21lc3NhZ2UYAiABKAsyES5hZ2'
    'VudC52MS5NZXNzYWdlUgdtZXNzYWdlEhMKBXRzX21zGAMgASgEUgR0c01zEh0KCnNlc3Npb25f'
    'aWQYBCABKAlSCXNlc3Npb25JZBIqCgV1c2FnZRgFIAEoCzIPLmFnZW50LnYxLlVzYWdlSABSBX'
    'VzYWdliAEBEhcKBGl0ZXIYBiABKA1IAVIEaXRlcogBAUIICgZfdXNhZ2VCBwoFX2l0ZXI=');

@$core.Deprecated('Use contextBlockDescriptor instead')
const ContextBlock$json = {
  '1': 'ContextBlock',
  '2': [
    {'1': 'source', '3': 1, '4': 1, '5': 9, '10': 'source'},
    {'1': 'content', '3': 2, '4': 1, '5': 9, '10': 'content'},
  ],
};

/// Descriptor for `ContextBlock`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List contextBlockDescriptor = $convert.base64Decode(
    'CgxDb250ZXh0QmxvY2sSFgoGc291cmNlGAEgASgJUgZzb3VyY2USGAoHY29udGVudBgCIAEoCV'
    'IHY29udGVudA==');

@$core.Deprecated('Use contextInputDescriptor instead')
const ContextInput$json = {
  '1': 'ContextInput',
  '2': [
    {'1': 'system_prompt', '3': 1, '4': 1, '5': 9, '10': 'systemPrompt'},
    {
      '1': 'prepend',
      '3': 2,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ContextBlock',
      '10': 'prepend'
    },
    {
      '1': 'recalled',
      '3': 3,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.MemoryItem',
      '10': 'recalled'
    },
    {'1': 'goal', '3': 4, '4': 1, '5': 9, '10': 'goal'},
    {
      '1': 'append',
      '3': 5,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ContextBlock',
      '10': 'append'
    },
  ],
};

/// Descriptor for `ContextInput`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List contextInputDescriptor = $convert.base64Decode(
    'CgxDb250ZXh0SW5wdXQSIwoNc3lzdGVtX3Byb21wdBgBIAEoCVIMc3lzdGVtUHJvbXB0EjAKB3'
    'ByZXBlbmQYAiADKAsyFi5hZ2VudC52MS5Db250ZXh0QmxvY2tSB3ByZXBlbmQSMAoIcmVjYWxs'
    'ZWQYAyADKAsyFC5hZ2VudC52MS5NZW1vcnlJdGVtUghyZWNhbGxlZBISCgRnb2FsGAQgASgJUg'
    'Rnb2FsEi4KBmFwcGVuZBgFIAMoCzIWLmFnZW50LnYxLkNvbnRleHRCbG9ja1IGYXBwZW5k');

@$core.Deprecated('Use workingSetDescriptor instead')
const WorkingSet$json = {
  '1': 'WorkingSet',
  '2': [
    {
      '1': 'messages',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Message',
      '10': 'messages'
    },
  ],
};

/// Descriptor for `WorkingSet`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List workingSetDescriptor = $convert.base64Decode(
    'CgpXb3JraW5nU2V0Ei0KCG1lc3NhZ2VzGAEgAygLMhEuYWdlbnQudjEuTWVzc2FnZVIIbWVzc2'
    'FnZXM=');

@$core.Deprecated('Use tokenBudgetDescriptor instead')
const TokenBudget$json = {
  '1': 'TokenBudget',
  '2': [
    {
      '1': 'max_context_tokens',
      '3': 1,
      '4': 1,
      '5': 13,
      '10': 'maxContextTokens'
    },
    {'1': 'reserve_output', '3': 2, '4': 1, '5': 13, '10': 'reserveOutput'},
  ],
};

/// Descriptor for `TokenBudget`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List tokenBudgetDescriptor = $convert.base64Decode(
    'CgtUb2tlbkJ1ZGdldBIsChJtYXhfY29udGV4dF90b2tlbnMYASABKA1SEG1heENvbnRleHRUb2'
    'tlbnMSJQoOcmVzZXJ2ZV9vdXRwdXQYAiABKA1SDXJlc2VydmVPdXRwdXQ=');

@$core.Deprecated('Use decisionDescriptor instead')
const Decision$json = {
  '1': 'Decision',
  '2': [
    {'1': 'allowed', '3': 1, '4': 1, '5': 8, '10': 'allowed'},
    {
      '1': 'deny_reason',
      '3': 2,
      '4': 1,
      '5': 9,
      '9': 0,
      '10': 'denyReason',
      '17': true
    },
  ],
  '8': [
    {'1': '_deny_reason'},
  ],
};

/// Descriptor for `Decision`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List decisionDescriptor = $convert.base64Decode(
    'CghEZWNpc2lvbhIYCgdhbGxvd2VkGAEgASgIUgdhbGxvd2VkEiQKC2RlbnlfcmVhc29uGAIgAS'
    'gJSABSCmRlbnlSZWFzb26IAQFCDgoMX2RlbnlfcmVhc29u');
