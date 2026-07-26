// This is a generated file - do not edit.
//
// Generated from agent/v1/web.proto.

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

@$core.Deprecated('Use webFormatDescriptor instead')
const WebFormat$json = {
  '1': 'WebFormat',
  '2': [
    {'1': 'WEB_FORMAT_MARKDOWN', '2': 0},
    {'1': 'WEB_FORMAT_TEXT', '2': 1},
    {'1': 'WEB_FORMAT_HTML', '2': 2},
  ],
};

/// Descriptor for `WebFormat`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List webFormatDescriptor = $convert.base64Decode(
    'CglXZWJGb3JtYXQSFwoTV0VCX0ZPUk1BVF9NQVJLRE9XThAAEhMKD1dFQl9GT1JNQVRfVEVYVB'
    'ABEhMKD1dFQl9GT1JNQVRfSFRNTBAC');

@$core.Deprecated('Use webCacheStateDescriptor instead')
const WebCacheState$json = {
  '1': 'WebCacheState',
  '2': [
    {'1': 'WEB_CACHE_STATE_MISSING', '2': 0},
    {'1': 'WEB_CACHE_STATE_FRESH', '2': 1},
    {'1': 'WEB_CACHE_STATE_STALE', '2': 2},
  ],
};

/// Descriptor for `WebCacheState`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List webCacheStateDescriptor = $convert.base64Decode(
    'Cg1XZWJDYWNoZVN0YXRlEhsKF1dFQl9DQUNIRV9TVEFURV9NSVNTSU5HEAASGQoVV0VCX0NBQ0'
    'hFX1NUQVRFX0ZSRVNIEAESGQoVV0VCX0NBQ0hFX1NUQVRFX1NUQUxFEAI=');

@$core.Deprecated('Use webFetchRequestDescriptor instead')
const WebFetchRequest$json = {
  '1': 'WebFetchRequest',
  '2': [
    {'1': 'url', '3': 1, '4': 1, '5': 9, '10': 'url'},
    {
      '1': 'format',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.WebFormat',
      '10': 'format'
    },
    {'1': 'timeout_secs', '3': 3, '4': 1, '5': 4, '10': 'timeoutSecs'},
    {'1': 'max_bytes', '3': 4, '4': 1, '5': 4, '10': 'maxBytes'},
    {'1': 'max_redirects', '3': 5, '4': 1, '5': 13, '10': 'maxRedirects'},
  ],
};

/// Descriptor for `WebFetchRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List webFetchRequestDescriptor = $convert.base64Decode(
    'Cg9XZWJGZXRjaFJlcXVlc3QSEAoDdXJsGAEgASgJUgN1cmwSKwoGZm9ybWF0GAIgASgOMhMuYW'
    'dlbnQudjEuV2ViRm9ybWF0UgZmb3JtYXQSIQoMdGltZW91dF9zZWNzGAMgASgEUgt0aW1lb3V0'
    'U2VjcxIbCgltYXhfYnl0ZXMYBCABKARSCG1heEJ5dGVzEiMKDW1heF9yZWRpcmVjdHMYBSABKA'
    '1SDG1heFJlZGlyZWN0cw==');

@$core.Deprecated('Use webFetchResponseDescriptor instead')
const WebFetchResponse$json = {
  '1': 'WebFetchResponse',
  '2': [
    {'1': 'final_url', '3': 1, '4': 1, '5': 9, '10': 'finalUrl'},
    {'1': 'status', '3': 2, '4': 1, '5': 13, '10': 'status'},
    {'1': 'content_type', '3': 3, '4': 1, '5': 9, '10': 'contentType'},
    {
      '1': 'format',
      '3': 4,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.WebFormat',
      '10': 'format'
    },
    {'1': 'body', '3': 5, '4': 1, '5': 9, '10': 'body'},
    {'1': 'bytes', '3': 6, '4': 1, '5': 4, '10': 'bytes'},
  ],
};

/// Descriptor for `WebFetchResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List webFetchResponseDescriptor = $convert.base64Decode(
    'ChBXZWJGZXRjaFJlc3BvbnNlEhsKCWZpbmFsX3VybBgBIAEoCVIIZmluYWxVcmwSFgoGc3RhdH'
    'VzGAIgASgNUgZzdGF0dXMSIQoMY29udGVudF90eXBlGAMgASgJUgtjb250ZW50VHlwZRIrCgZm'
    'b3JtYXQYBCABKA4yEy5hZ2VudC52MS5XZWJGb3JtYXRSBmZvcm1hdBISCgRib2R5GAUgASgJUg'
    'Rib2R5EhQKBWJ5dGVzGAYgASgEUgVieXRlcw==');

@$core.Deprecated('Use webSearchRequestDescriptor instead')
const WebSearchRequest$json = {
  '1': 'WebSearchRequest',
  '2': [
    {'1': 'text', '3': 1, '4': 1, '5': 9, '10': 'text'},
    {'1': 'limit', '3': 2, '4': 1, '5': 13, '10': 'limit'},
    {'1': 'freshness_days', '3': 3, '4': 1, '5': 13, '10': 'freshnessDays'},
    {
      '1': 'backend',
      '3': 4,
      '4': 1,
      '5': 9,
      '9': 0,
      '10': 'backend',
      '17': true
    },
  ],
  '8': [
    {'1': '_backend'},
  ],
};

/// Descriptor for `WebSearchRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List webSearchRequestDescriptor = $convert.base64Decode(
    'ChBXZWJTZWFyY2hSZXF1ZXN0EhIKBHRleHQYASABKAlSBHRleHQSFAoFbGltaXQYAiABKA1SBW'
    'xpbWl0EiUKDmZyZXNobmVzc19kYXlzGAMgASgNUg1mcmVzaG5lc3NEYXlzEh0KB2JhY2tlbmQY'
    'BCABKAlIAFIHYmFja2VuZIgBAUIKCghfYmFja2VuZA==');

@$core.Deprecated('Use webSearchResultDescriptor instead')
const WebSearchResult$json = {
  '1': 'WebSearchResult',
  '2': [
    {'1': 'url', '3': 1, '4': 1, '5': 9, '10': 'url'},
    {'1': 'title', '3': 2, '4': 1, '5': 9, '10': 'title'},
    {'1': 'snippet', '3': 3, '4': 1, '5': 9, '10': 'snippet'},
    {'1': 'score', '3': 4, '4': 1, '5': 2, '10': 'score'},
    {
      '1': 'published_ms',
      '3': 5,
      '4': 1,
      '5': 4,
      '9': 0,
      '10': 'publishedMs',
      '17': true
    },
  ],
  '8': [
    {'1': '_published_ms'},
  ],
};

/// Descriptor for `WebSearchResult`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List webSearchResultDescriptor = $convert.base64Decode(
    'Cg9XZWJTZWFyY2hSZXN1bHQSEAoDdXJsGAEgASgJUgN1cmwSFAoFdGl0bGUYAiABKAlSBXRpdG'
    'xlEhgKB3NuaXBwZXQYAyABKAlSB3NuaXBwZXQSFAoFc2NvcmUYBCABKAJSBXNjb3JlEiYKDHB1'
    'Ymxpc2hlZF9tcxgFIAEoBEgAUgtwdWJsaXNoZWRNc4gBAUIPCg1fcHVibGlzaGVkX21z');

@$core.Deprecated('Use webSearchResponseDescriptor instead')
const WebSearchResponse$json = {
  '1': 'WebSearchResponse',
  '2': [
    {
      '1': 'results',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.WebSearchResult',
      '10': 'results'
    },
  ],
};

/// Descriptor for `WebSearchResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List webSearchResponseDescriptor = $convert.base64Decode(
    'ChFXZWJTZWFyY2hSZXNwb25zZRIzCgdyZXN1bHRzGAEgAygLMhkuYWdlbnQudjEuV2ViU2Vhcm'
    'NoUmVzdWx0UgdyZXN1bHRz');

@$core.Deprecated('Use webCacheStatusDescriptor instead')
const WebCacheStatus$json = {
  '1': 'WebCacheStatus',
  '2': [
    {
      '1': 'state',
      '3': 1,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.WebCacheState',
      '10': 'state'
    },
  ],
};

/// Descriptor for `WebCacheStatus`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List webCacheStatusDescriptor = $convert.base64Decode(
    'Cg5XZWJDYWNoZVN0YXR1cxItCgVzdGF0ZRgBIAEoDjIXLmFnZW50LnYxLldlYkNhY2hlU3RhdG'
    'VSBXN0YXRl');

@$core.Deprecated('Use webSearchCapabilitiesRequestDescriptor instead')
const WebSearchCapabilitiesRequest$json = {
  '1': 'WebSearchCapabilitiesRequest',
};

/// Descriptor for `WebSearchCapabilitiesRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List webSearchCapabilitiesRequestDescriptor =
    $convert.base64Decode('ChxXZWJTZWFyY2hDYXBhYmlsaXRpZXNSZXF1ZXN0');

@$core.Deprecated('Use webSearchCapabilitiesDescriptor instead')
const WebSearchCapabilities$json = {
  '1': 'WebSearchCapabilities',
  '2': [
    {'1': 'backend', '3': 1, '4': 1, '5': 9, '10': 'backend'},
    {'1': 'scored', '3': 2, '4': 1, '5': 8, '10': 'scored'},
    {'1': 'freshness', '3': 3, '4': 1, '5': 8, '10': 'freshness'},
    {'1': 'max_results', '3': 4, '4': 1, '5': 13, '10': 'maxResults'},
  ],
};

/// Descriptor for `WebSearchCapabilities`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List webSearchCapabilitiesDescriptor = $convert.base64Decode(
    'ChVXZWJTZWFyY2hDYXBhYmlsaXRpZXMSGAoHYmFja2VuZBgBIAEoCVIHYmFja2VuZBIWCgZzY2'
    '9yZWQYAiABKAhSBnNjb3JlZBIcCglmcmVzaG5lc3MYAyABKAhSCWZyZXNobmVzcxIfCgttYXhf'
    'cmVzdWx0cxgEIAEoDVIKbWF4UmVzdWx0cw==');
