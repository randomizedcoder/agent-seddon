// This is a generated file - do not edit.
//
// Generated from agent/v1/tool.proto.

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

@$core.Deprecated('Use describeAllRequestDescriptor instead')
const DescribeAllRequest$json = {
  '1': 'DescribeAllRequest',
};

/// Descriptor for `DescribeAllRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List describeAllRequestDescriptor =
    $convert.base64Decode('ChJEZXNjcmliZUFsbFJlcXVlc3Q=');

@$core.Deprecated('Use describeAllResponseDescriptor instead')
const DescribeAllResponse$json = {
  '1': 'DescribeAllResponse',
  '2': [
    {
      '1': 'tools',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ToolSchema',
      '10': 'tools'
    },
  ],
};

/// Descriptor for `DescribeAllResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List describeAllResponseDescriptor = $convert.base64Decode(
    'ChNEZXNjcmliZUFsbFJlc3BvbnNlEioKBXRvb2xzGAEgAygLMhQuYWdlbnQudjEuVG9vbFNjaG'
    'VtYVIFdG9vbHM=');

@$core.Deprecated('Use executeRequestDescriptor instead')
const ExecuteRequest$json = {
  '1': 'ExecuteRequest',
  '2': [
    {'1': 'name', '3': 1, '4': 1, '5': 9, '10': 'name'},
    {
      '1': 'arguments',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.JsonValue',
      '10': 'arguments'
    },
    {
      '1': 'context',
      '3': 3,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ToolContext',
      '10': 'context'
    },
  ],
};

/// Descriptor for `ExecuteRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List executeRequestDescriptor = $convert.base64Decode(
    'Cg5FeGVjdXRlUmVxdWVzdBISCgRuYW1lGAEgASgJUgRuYW1lEjEKCWFyZ3VtZW50cxgCIAEoCz'
    'ITLmFnZW50LnYxLkpzb25WYWx1ZVIJYXJndW1lbnRzEi8KB2NvbnRleHQYAyABKAsyFS5hZ2Vu'
    'dC52MS5Ub29sQ29udGV4dFIHY29udGV4dA==');
