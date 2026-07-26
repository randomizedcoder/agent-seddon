// This is a generated file - do not edit.
//
// Generated from agent/v1/reference.proto.

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

@$core.Deprecated('Use refResolveRequestDescriptor instead')
const RefResolveRequest$json = {
  '1': 'RefResolveRequest',
  '2': [
    {'1': 'prompt', '3': 1, '4': 1, '5': 9, '10': 'prompt'},
    {'1': 'budget_tokens', '3': 2, '4': 1, '5': 4, '10': 'budgetTokens'},
  ],
};

/// Descriptor for `RefResolveRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List refResolveRequestDescriptor = $convert.base64Decode(
    'ChFSZWZSZXNvbHZlUmVxdWVzdBIWCgZwcm9tcHQYASABKAlSBnByb21wdBIjCg1idWRnZXRfdG'
    '9rZW5zGAIgASgEUgxidWRnZXRUb2tlbnM=');

@$core.Deprecated('Use refResolutionDescriptor instead')
const RefResolution$json = {
  '1': 'RefResolution',
  '2': [
    {
      '1': 'blocks',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ContextBlock',
      '10': 'blocks'
    },
    {'1': 'warnings', '3': 2, '4': 3, '5': 9, '10': 'warnings'},
    {'1': 'blocked', '3': 3, '4': 1, '5': 8, '10': 'blocked'},
  ],
};

/// Descriptor for `RefResolution`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List refResolutionDescriptor = $convert.base64Decode(
    'Cg1SZWZSZXNvbHV0aW9uEi4KBmJsb2NrcxgBIAMoCzIWLmFnZW50LnYxLkNvbnRleHRCbG9ja1'
    'IGYmxvY2tzEhoKCHdhcm5pbmdzGAIgAygJUgh3YXJuaW5ncxIYCgdibG9ja2VkGAMgASgIUgdi'
    'bG9ja2Vk');
