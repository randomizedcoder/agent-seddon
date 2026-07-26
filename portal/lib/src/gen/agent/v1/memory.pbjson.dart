// This is a generated file - do not edit.
//
// Generated from agent/v1/memory.proto.

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

@$core.Deprecated('Use recallResponseDescriptor instead')
const RecallResponse$json = {
  '1': 'RecallResponse',
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

/// Descriptor for `RecallResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List recallResponseDescriptor = $convert.base64Decode(
    'Cg5SZWNhbGxSZXNwb25zZRIqCgVpdGVtcxgBIAMoCzIULmFnZW50LnYxLk1lbW9yeUl0ZW1SBW'
    'l0ZW1z');

@$core.Deprecated('Use appendResponseDescriptor instead')
const AppendResponse$json = {
  '1': 'AppendResponse',
};

/// Descriptor for `AppendResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List appendResponseDescriptor =
    $convert.base64Decode('Cg5BcHBlbmRSZXNwb25zZQ==');

@$core.Deprecated('Use distillRequestDescriptor instead')
const DistillRequest$json = {
  '1': 'DistillRequest',
};

/// Descriptor for `DistillRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List distillRequestDescriptor =
    $convert.base64Decode('Cg5EaXN0aWxsUmVxdWVzdA==');

@$core.Deprecated('Use distillResponseDescriptor instead')
const DistillResponse$json = {
  '1': 'DistillResponse',
  '2': [
    {'1': 'count', '3': 1, '4': 1, '5': 4, '10': 'count'},
  ],
};

/// Descriptor for `DistillResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List distillResponseDescriptor = $convert
    .base64Decode('Cg9EaXN0aWxsUmVzcG9uc2USFAoFY291bnQYASABKARSBWNvdW50');

@$core.Deprecated('Use recentRequestDescriptor instead')
const RecentRequest$json = {
  '1': 'RecentRequest',
  '2': [
    {'1': 'limit', '3': 1, '4': 1, '5': 4, '10': 'limit'},
  ],
};

/// Descriptor for `RecentRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List recentRequestDescriptor = $convert
    .base64Decode('Cg1SZWNlbnRSZXF1ZXN0EhQKBWxpbWl0GAEgASgEUgVsaW1pdA==');

@$core.Deprecated('Use recentResponseDescriptor instead')
const RecentResponse$json = {
  '1': 'RecentResponse',
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

/// Descriptor for `RecentResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List recentResponseDescriptor = $convert.base64Decode(
    'Cg5SZWNlbnRSZXNwb25zZRItCgZldmVudHMYASADKAsyFS5hZ2VudC52MS5NZW1vcnlFdmVudF'
    'IGZXZlbnRz');

@$core.Deprecated('Use semanticDistillRequestDescriptor instead')
const SemanticDistillRequest$json = {
  '1': 'SemanticDistillRequest',
  '2': [
    {
      '1': 'episodic',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.MemoryEvent',
      '10': 'episodic'
    },
  ],
};

/// Descriptor for `SemanticDistillRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List semanticDistillRequestDescriptor =
    $convert.base64Decode(
        'ChZTZW1hbnRpY0Rpc3RpbGxSZXF1ZXN0EjEKCGVwaXNvZGljGAEgAygLMhUuYWdlbnQudjEuTW'
        'Vtb3J5RXZlbnRSCGVwaXNvZGlj');
