// This is a generated file - do not edit.
//
// Generated from agent/v1/dimension.proto.

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

@$core.Deprecated('Use dimensionSummaryDescriptor instead')
const DimensionSummary$json = {
  '1': 'DimensionSummary',
  '2': [
    {'1': 'dimension', '3': 1, '4': 1, '5': 9, '10': 'dimension'},
    {'1': 'summary', '3': 2, '4': 1, '5': 9, '10': 'summary'},
    {'1': 'is_new', '3': 3, '4': 1, '5': 8, '10': 'isNew'},
  ],
};

/// Descriptor for `DimensionSummary`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List dimensionSummaryDescriptor = $convert.base64Decode(
    'ChBEaW1lbnNpb25TdW1tYXJ5EhwKCWRpbWVuc2lvbhgBIAEoCVIJZGltZW5zaW9uEhgKB3N1bW'
    '1hcnkYAiABKAlSB3N1bW1hcnkSFQoGaXNfbmV3GAMgASgIUgVpc05ldw==');

@$core.Deprecated('Use dimensionStepDescriptor instead')
const DimensionStep$json = {
  '1': 'DimensionStep',
  '2': [
    {
      '1': 'summaries',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.DimensionSummary',
      '10': 'summaries'
    },
  ],
};

/// Descriptor for `DimensionStep`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List dimensionStepDescriptor = $convert.base64Decode(
    'Cg1EaW1lbnNpb25TdGVwEjgKCXN1bW1hcmllcxgBIAMoCzIaLmFnZW50LnYxLkRpbWVuc2lvbl'
    'N1bW1hcnlSCXN1bW1hcmllcw==');

@$core.Deprecated('Use summarizeRequestDescriptor instead')
const SummarizeRequest$json = {
  '1': 'SummarizeRequest',
  '2': [
    {
      '1': 'events',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.MemoryEvent',
      '10': 'events'
    },
  ],
};

/// Descriptor for `SummarizeRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List summarizeRequestDescriptor = $convert.base64Decode(
    'ChBTdW1tYXJpemVSZXF1ZXN0Ei0KBmV2ZW50cxgBIAMoCzIVLmFnZW50LnYxLk1lbW9yeUV2ZW'
    '50UgZldmVudHM=');

@$core.Deprecated('Use dimensionRecallRequestDescriptor instead')
const DimensionRecallRequest$json = {
  '1': 'DimensionRecallRequest',
  '2': [
    {'1': 'dimension', '3': 1, '4': 1, '5': 9, '10': 'dimension'},
    {'1': 'limit', '3': 2, '4': 1, '5': 13, '10': 'limit'},
  ],
};

/// Descriptor for `DimensionRecallRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List dimensionRecallRequestDescriptor =
    $convert.base64Decode(
        'ChZEaW1lbnNpb25SZWNhbGxSZXF1ZXN0EhwKCWRpbWVuc2lvbhgBIAEoCVIJZGltZW5zaW9uEh'
        'QKBWxpbWl0GAIgASgNUgVsaW1pdA==');

@$core.Deprecated('Use dimensionRecallResponseDescriptor instead')
const DimensionRecallResponse$json = {
  '1': 'DimensionRecallResponse',
  '2': [
    {
      '1': 'items',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.MemoryItem',
      '10': 'items'
    },
  ],
};

/// Descriptor for `DimensionRecallResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List dimensionRecallResponseDescriptor =
    $convert.base64Decode(
        'ChdEaW1lbnNpb25SZWNhbGxSZXNwb25zZRIqCgVpdGVtcxgBIAMoCzIULmFnZW50LnYxLk1lbW'
        '9yeUl0ZW1SBWl0ZW1z');
