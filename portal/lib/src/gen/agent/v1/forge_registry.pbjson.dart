// This is a generated file - do not edit.
//
// Generated from agent/v1/forge_registry.proto.

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

@$core.Deprecated('Use forgeCardDescriptor instead')
const ForgeCard$json = {
  '1': 'ForgeCard',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'kind', '3': 2, '4': 1, '5': 9, '10': 'kind'},
    {'1': 'enabled', '3': 3, '4': 1, '5': 8, '10': 'enabled'},
    {'1': 'base_url', '3': 4, '4': 1, '5': 9, '10': 'baseUrl'},
    {'1': 'token_ref', '3': 5, '4': 1, '5': 9, '10': 'tokenRef'},
    {'1': 'repo_encoding', '3': 6, '4': 1, '5': 9, '10': 'repoEncoding'},
    {'1': 'timeout_secs', '3': 7, '4': 1, '5': 13, '10': 'timeoutSecs'},
    {'1': 'max_retries', '3': 8, '4': 1, '5': 13, '10': 'maxRetries'},
  ],
};

/// Descriptor for `ForgeCard`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List forgeCardDescriptor = $convert.base64Decode(
    'CglGb3JnZUNhcmQSDgoCaWQYASABKAlSAmlkEhIKBGtpbmQYAiABKAlSBGtpbmQSGAoHZW5hYm'
    'xlZBgDIAEoCFIHZW5hYmxlZBIZCghiYXNlX3VybBgEIAEoCVIHYmFzZVVybBIbCgl0b2tlbl9y'
    'ZWYYBSABKAlSCHRva2VuUmVmEiMKDXJlcG9fZW5jb2RpbmcYBiABKAlSDHJlcG9FbmNvZGluZx'
    'IhCgx0aW1lb3V0X3NlY3MYByABKA1SC3RpbWVvdXRTZWNzEh8KC21heF9yZXRyaWVzGAggASgN'
    'UgptYXhSZXRyaWVz');

@$core.Deprecated('Use forgeListRequestDescriptor instead')
const ForgeListRequest$json = {
  '1': 'ForgeListRequest',
};

/// Descriptor for `ForgeListRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List forgeListRequestDescriptor =
    $convert.base64Decode('ChBGb3JnZUxpc3RSZXF1ZXN0');

@$core.Deprecated('Use forgeListDescriptor instead')
const ForgeList$json = {
  '1': 'ForgeList',
  '2': [
    {
      '1': 'forges',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ForgeCard',
      '10': 'forges'
    },
  ],
};

/// Descriptor for `ForgeList`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List forgeListDescriptor = $convert.base64Decode(
    'CglGb3JnZUxpc3QSKwoGZm9yZ2VzGAEgAygLMhMuYWdlbnQudjEuRm9yZ2VDYXJkUgZmb3JnZX'
    'M=');

@$core.Deprecated('Use forgeRefDescriptor instead')
const ForgeRef$json = {
  '1': 'ForgeRef',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
  ],
};

/// Descriptor for `ForgeRef`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List forgeRefDescriptor =
    $convert.base64Decode('CghGb3JnZVJlZhIOCgJpZBgBIAEoCVICaWQ=');

@$core.Deprecated('Use forgeDeleteReplyDescriptor instead')
const ForgeDeleteReply$json = {
  '1': 'ForgeDeleteReply',
  '2': [
    {'1': 'deleted', '3': 1, '4': 1, '5': 8, '10': 'deleted'},
  ],
};

/// Descriptor for `ForgeDeleteReply`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List forgeDeleteReplyDescriptor = $convert.base64Decode(
    'ChBGb3JnZURlbGV0ZVJlcGx5EhgKB2RlbGV0ZWQYASABKAhSB2RlbGV0ZWQ=');
