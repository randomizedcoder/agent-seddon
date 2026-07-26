// This is a generated file - do not edit.
//
// Generated from agent/v1/embed.proto.

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

@$core.Deprecated('Use embCapabilitiesRequestDescriptor instead')
const EmbCapabilitiesRequest$json = {
  '1': 'EmbCapabilitiesRequest',
};

/// Descriptor for `EmbCapabilitiesRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List embCapabilitiesRequestDescriptor =
    $convert.base64Decode('ChZFbWJDYXBhYmlsaXRpZXNSZXF1ZXN0');

@$core.Deprecated('Use embCapabilitiesDescriptor instead')
const EmbCapabilities$json = {
  '1': 'EmbCapabilities',
  '2': [
    {'1': 'dimensions', '3': 1, '4': 1, '5': 13, '10': 'dimensions'},
    {'1': 'max_batch', '3': 2, '4': 1, '5': 13, '10': 'maxBatch'},
  ],
};

/// Descriptor for `EmbCapabilities`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List embCapabilitiesDescriptor = $convert.base64Decode(
    'Cg9FbWJDYXBhYmlsaXRpZXMSHgoKZGltZW5zaW9ucxgBIAEoDVIKZGltZW5zaW9ucxIbCgltYX'
    'hfYmF0Y2gYAiABKA1SCG1heEJhdGNo');

@$core.Deprecated('Use embQueryRequestDescriptor instead')
const EmbQueryRequest$json = {
  '1': 'EmbQueryRequest',
  '2': [
    {'1': 'text', '3': 1, '4': 1, '5': 9, '10': 'text'},
  ],
};

/// Descriptor for `EmbQueryRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List embQueryRequestDescriptor = $convert
    .base64Decode('Cg9FbWJRdWVyeVJlcXVlc3QSEgoEdGV4dBgBIAEoCVIEdGV4dA==');

@$core.Deprecated('Use embDocsRequestDescriptor instead')
const EmbDocsRequest$json = {
  '1': 'EmbDocsRequest',
  '2': [
    {'1': 'texts', '3': 1, '4': 3, '5': 9, '10': 'texts'},
  ],
};

/// Descriptor for `EmbDocsRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List embDocsRequestDescriptor = $convert
    .base64Decode('Cg5FbWJEb2NzUmVxdWVzdBIUCgV0ZXh0cxgBIAMoCVIFdGV4dHM=');

@$core.Deprecated('Use embVectorDescriptor instead')
const EmbVector$json = {
  '1': 'EmbVector',
  '2': [
    {'1': 'values', '3': 1, '4': 3, '5': 2, '10': 'values'},
  ],
};

/// Descriptor for `EmbVector`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List embVectorDescriptor =
    $convert.base64Decode('CglFbWJWZWN0b3ISFgoGdmFsdWVzGAEgAygCUgZ2YWx1ZXM=');

@$core.Deprecated('Use embVectorsDescriptor instead')
const EmbVectors$json = {
  '1': 'EmbVectors',
  '2': [
    {
      '1': 'vectors',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.EmbVector',
      '10': 'vectors'
    },
  ],
};

/// Descriptor for `EmbVectors`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List embVectorsDescriptor = $convert.base64Decode(
    'CgpFbWJWZWN0b3JzEi0KB3ZlY3RvcnMYASADKAsyEy5hZ2VudC52MS5FbWJWZWN0b3JSB3ZlY3'
    'RvcnM=');
