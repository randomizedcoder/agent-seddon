// This is a generated file - do not edit.
//
// Generated from agent/v1/graph.proto.

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

@$core.Deprecated('Use graphNodeDescriptor instead')
const GraphNode$json = {
  '1': 'GraphNode',
  '2': [
    {'1': 'type', '3': 1, '4': 1, '5': 9, '10': 'type'},
    {'1': 'type_version', '3': 2, '4': 1, '5': 13, '10': 'typeVersion'},
    {
      '1': 'params',
      '3': 3,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.JsonValue',
      '10': 'params'
    },
  ],
};

/// Descriptor for `GraphNode`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List graphNodeDescriptor = $convert.base64Decode(
    'CglHcmFwaE5vZGUSEgoEdHlwZRgBIAEoCVIEdHlwZRIhCgx0eXBlX3ZlcnNpb24YAiABKA1SC3'
    'R5cGVWZXJzaW9uEisKBnBhcmFtcxgDIAEoCzITLmFnZW50LnYxLkpzb25WYWx1ZVIGcGFyYW1z');

@$core.Deprecated('Use graphEdgeDescriptor instead')
const GraphEdge$json = {
  '1': 'GraphEdge',
  '2': [
    {'1': 'from', '3': 1, '4': 1, '5': 9, '10': 'from'},
    {'1': 'to', '3': 2, '4': 1, '5': 9, '10': 'to'},
    {
      '1': 'kind',
      '3': 3,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.GraphEdge.Kind',
      '10': 'kind'
    },
  ],
  '4': [GraphEdge_Kind$json],
};

@$core.Deprecated('Use graphEdgeDescriptor instead')
const GraphEdge_Kind$json = {
  '1': 'Kind',
  '2': [
    {'1': 'KIND_UNSPECIFIED', '2': 0},
    {'1': 'KIND_MAIN', '2': 1},
    {'1': 'KIND_BACKGROUND', '2': 2},
    {'1': 'KIND_CAPABILITY', '2': 3},
  ],
};

/// Descriptor for `GraphEdge`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List graphEdgeDescriptor = $convert.base64Decode(
    'CglHcmFwaEVkZ2USEgoEZnJvbRgBIAEoCVIEZnJvbRIOCgJ0bxgCIAEoCVICdG8SLAoEa2luZB'
    'gDIAEoDjIYLmFnZW50LnYxLkdyYXBoRWRnZS5LaW5kUgRraW5kIlUKBEtpbmQSFAoQS0lORF9V'
    'TlNQRUNJRklFRBAAEg0KCUtJTkRfTUFJThABEhMKD0tJTkRfQkFDS0dST1VORBACEhMKD0tJTk'
    'RfQ0FQQUJJTElUWRAD');

@$core.Deprecated('Use cognitionGraphDescriptor instead')
const CognitionGraph$json = {
  '1': 'CognitionGraph',
  '2': [
    {'1': 'version', '3': 1, '4': 1, '5': 13, '10': 'version'},
    {
      '1': 'nodes',
      '3': 2,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.CognitionGraph.NodesEntry',
      '10': 'nodes'
    },
    {
      '1': 'edges',
      '3': 3,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.GraphEdge',
      '10': 'edges'
    },
  ],
  '3': [CognitionGraph_NodesEntry$json],
};

@$core.Deprecated('Use cognitionGraphDescriptor instead')
const CognitionGraph_NodesEntry$json = {
  '1': 'NodesEntry',
  '2': [
    {'1': 'key', '3': 1, '4': 1, '5': 9, '10': 'key'},
    {
      '1': 'value',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.GraphNode',
      '10': 'value'
    },
  ],
  '7': {'7': true},
};

/// Descriptor for `CognitionGraph`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List cognitionGraphDescriptor = $convert.base64Decode(
    'Cg5Db2duaXRpb25HcmFwaBIYCgd2ZXJzaW9uGAEgASgNUgd2ZXJzaW9uEjkKBW5vZGVzGAIgAy'
    'gLMiMuYWdlbnQudjEuQ29nbml0aW9uR3JhcGguTm9kZXNFbnRyeVIFbm9kZXMSKQoFZWRnZXMY'
    'AyADKAsyEy5hZ2VudC52MS5HcmFwaEVkZ2VSBWVkZ2VzGk0KCk5vZGVzRW50cnkSEAoDa2V5GA'
    'EgASgJUgNrZXkSKQoFdmFsdWUYAiABKAsyEy5hZ2VudC52MS5HcmFwaE5vZGVSBXZhbHVlOgI4'
    'AQ==');

@$core.Deprecated('Use nodePortDescriptor instead')
const NodePort$json = {
  '1': 'NodePort',
  '2': [
    {'1': 'name', '3': 1, '4': 1, '5': 9, '10': 'name'},
    {'1': 'kind', '3': 2, '4': 1, '5': 9, '10': 'kind'},
  ],
};

/// Descriptor for `NodePort`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List nodePortDescriptor = $convert.base64Decode(
    'CghOb2RlUG9ydBISCgRuYW1lGAEgASgJUgRuYW1lEhIKBGtpbmQYAiABKAlSBGtpbmQ=');

@$core.Deprecated('Use nodeTypeSchemaDescriptor instead')
const NodeTypeSchema$json = {
  '1': 'NodeTypeSchema',
  '2': [
    {'1': 'type', '3': 1, '4': 1, '5': 9, '10': 'type'},
    {'1': 'type_version', '3': 2, '4': 1, '5': 13, '10': 'typeVersion'},
    {'1': 'title', '3': 3, '4': 1, '5': 9, '10': 'title'},
    {'1': 'doc', '3': 4, '4': 1, '5': 9, '10': 'doc'},
    {
      '1': 'inputs',
      '3': 5,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.NodePort',
      '10': 'inputs'
    },
    {
      '1': 'outputs',
      '3': 6,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.NodePort',
      '10': 'outputs'
    },
    {
      '1': 'params_schema',
      '3': 7,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.JsonValue',
      '10': 'paramsSchema'
    },
  ],
};

/// Descriptor for `NodeTypeSchema`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List nodeTypeSchemaDescriptor = $convert.base64Decode(
    'Cg5Ob2RlVHlwZVNjaGVtYRISCgR0eXBlGAEgASgJUgR0eXBlEiEKDHR5cGVfdmVyc2lvbhgCIA'
    'EoDVILdHlwZVZlcnNpb24SFAoFdGl0bGUYAyABKAlSBXRpdGxlEhAKA2RvYxgEIAEoCVIDZG9j'
    'EioKBmlucHV0cxgFIAMoCzISLmFnZW50LnYxLk5vZGVQb3J0UgZpbnB1dHMSLAoHb3V0cHV0cx'
    'gGIAMoCzISLmFnZW50LnYxLk5vZGVQb3J0UgdvdXRwdXRzEjgKDXBhcmFtc19zY2hlbWEYByAB'
    'KAsyEy5hZ2VudC52MS5Kc29uVmFsdWVSDHBhcmFtc1NjaGVtYQ==');

@$core.Deprecated('Use graphIssueDescriptor instead')
const GraphIssue$json = {
  '1': 'GraphIssue',
  '2': [
    {'1': 'node', '3': 1, '4': 1, '5': 9, '10': 'node'},
    {'1': 'code', '3': 2, '4': 1, '5': 9, '10': 'code'},
    {'1': 'detail', '3': 3, '4': 1, '5': 9, '10': 'detail'},
  ],
};

/// Descriptor for `GraphIssue`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List graphIssueDescriptor = $convert.base64Decode(
    'CgpHcmFwaElzc3VlEhIKBG5vZGUYASABKAlSBG5vZGUSEgoEY29kZRgCIAEoCVIEY29kZRIWCg'
    'ZkZXRhaWwYAyABKAlSBmRldGFpbA==');

@$core.Deprecated('Use getGraphRequestDescriptor instead')
const GetGraphRequest$json = {
  '1': 'GetGraphRequest',
};

/// Descriptor for `GetGraphRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List getGraphRequestDescriptor =
    $convert.base64Decode('Cg9HZXRHcmFwaFJlcXVlc3Q=');

@$core.Deprecated('Use getGraphResponseDescriptor instead')
const GetGraphResponse$json = {
  '1': 'GetGraphResponse',
  '2': [
    {
      '1': 'graph',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.CognitionGraph',
      '10': 'graph'
    },
  ],
};

/// Descriptor for `GetGraphResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List getGraphResponseDescriptor = $convert.base64Decode(
    'ChBHZXRHcmFwaFJlc3BvbnNlEi4KBWdyYXBoGAEgASgLMhguYWdlbnQudjEuQ29nbml0aW9uR3'
    'JhcGhSBWdyYXBo');

@$core.Deprecated('Use putGraphRequestDescriptor instead')
const PutGraphRequest$json = {
  '1': 'PutGraphRequest',
  '2': [
    {
      '1': 'graph',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.CognitionGraph',
      '10': 'graph'
    },
  ],
};

/// Descriptor for `PutGraphRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List putGraphRequestDescriptor = $convert.base64Decode(
    'Cg9QdXRHcmFwaFJlcXVlc3QSLgoFZ3JhcGgYASABKAsyGC5hZ2VudC52MS5Db2duaXRpb25Hcm'
    'FwaFIFZ3JhcGg=');

@$core.Deprecated('Use putGraphResponseDescriptor instead')
const PutGraphResponse$json = {
  '1': 'PutGraphResponse',
};

/// Descriptor for `PutGraphResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List putGraphResponseDescriptor =
    $convert.base64Decode('ChBQdXRHcmFwaFJlc3BvbnNl');

@$core.Deprecated('Use validateGraphRequestDescriptor instead')
const ValidateGraphRequest$json = {
  '1': 'ValidateGraphRequest',
  '2': [
    {
      '1': 'graph',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.CognitionGraph',
      '10': 'graph'
    },
  ],
};

/// Descriptor for `ValidateGraphRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List validateGraphRequestDescriptor = $convert.base64Decode(
    'ChRWYWxpZGF0ZUdyYXBoUmVxdWVzdBIuCgVncmFwaBgBIAEoCzIYLmFnZW50LnYxLkNvZ25pdG'
    'lvbkdyYXBoUgVncmFwaA==');

@$core.Deprecated('Use validateGraphResponseDescriptor instead')
const ValidateGraphResponse$json = {
  '1': 'ValidateGraphResponse',
  '2': [
    {
      '1': 'issues',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.GraphIssue',
      '10': 'issues'
    },
  ],
};

/// Descriptor for `ValidateGraphResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List validateGraphResponseDescriptor = $convert.base64Decode(
    'ChVWYWxpZGF0ZUdyYXBoUmVzcG9uc2USLAoGaXNzdWVzGAEgAygLMhQuYWdlbnQudjEuR3JhcG'
    'hJc3N1ZVIGaXNzdWVz');

@$core.Deprecated('Use describeNodeTypesRequestDescriptor instead')
const DescribeNodeTypesRequest$json = {
  '1': 'DescribeNodeTypesRequest',
};

/// Descriptor for `DescribeNodeTypesRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List describeNodeTypesRequestDescriptor =
    $convert.base64Decode('ChhEZXNjcmliZU5vZGVUeXBlc1JlcXVlc3Q=');

@$core.Deprecated('Use describeNodeTypesResponseDescriptor instead')
const DescribeNodeTypesResponse$json = {
  '1': 'DescribeNodeTypesResponse',
  '2': [
    {
      '1': 'node_types',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.NodeTypeSchema',
      '10': 'nodeTypes'
    },
  ],
};

/// Descriptor for `DescribeNodeTypesResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List describeNodeTypesResponseDescriptor =
    $convert.base64Decode(
        'ChlEZXNjcmliZU5vZGVUeXBlc1Jlc3BvbnNlEjcKCm5vZGVfdHlwZXMYASADKAsyGC5hZ2VudC'
        '52MS5Ob2RlVHlwZVNjaGVtYVIJbm9kZVR5cGVz');
