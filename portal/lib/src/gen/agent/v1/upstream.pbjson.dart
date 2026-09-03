// This is a generated file - do not edit.
//
// Generated from agent/v1/upstream.proto.

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

@$core.Deprecated('Use upstreamDescriptor instead')
const Upstream$json = {
  '1': 'Upstream',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'kind', '3': 2, '4': 1, '5': 9, '10': 'kind'},
    {'1': 'enabled', '3': 3, '4': 1, '5': 8, '10': 'enabled'},
    {'1': 'base_url', '3': 4, '4': 1, '5': 9, '10': 'baseUrl'},
    {'1': 'model', '3': 5, '4': 1, '5': 9, '10': 'model'},
    {'1': 'api_key_ref', '3': 6, '4': 1, '5': 9, '10': 'apiKeyRef'},
    {'1': 'insecure_tls', '3': 7, '4': 1, '5': 8, '10': 'insecureTls'},
    {'1': 'version', '3': 8, '4': 1, '5': 9, '10': 'version'},
    {'1': 'max_retries', '3': 9, '4': 1, '5': 13, '10': 'maxRetries'},
    {'1': 'context_window', '3': 10, '4': 1, '5': 13, '10': 'contextWindow'},
    {
      '1': 'max_output_tokens',
      '3': 11,
      '4': 1,
      '5': 13,
      '10': 'maxOutputTokens'
    },
    {'1': 'supports_tools', '3': 12, '4': 1, '5': 8, '10': 'supportsTools'},
    {'1': 'supports_vision', '3': 13, '4': 1, '5': 8, '10': 'supportsVision'},
    {
      '1': 'supports_response_format',
      '3': 14,
      '4': 1,
      '5': 8,
      '10': 'supportsResponseFormat'
    },
    {'1': 'tags', '3': 15, '4': 3, '5': 9, '10': 'tags'},
    {'1': 'input_cost', '3': 16, '4': 1, '5': 2, '10': 'inputCost'},
    {'1': 'output_cost', '3': 17, '4': 1, '5': 2, '10': 'outputCost'},
    {
      '1': 'tier',
      '3': 18,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.PoolTier',
      '10': 'tier'
    },
    {'1': 'weight', '3': 19, '4': 1, '5': 2, '10': 'weight'},
    {'1': 'max_concurrency', '3': 20, '4': 1, '5': 13, '10': 'maxConcurrency'},
  ],
};

/// Descriptor for `Upstream`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List upstreamDescriptor = $convert.base64Decode(
    'CghVcHN0cmVhbRIOCgJpZBgBIAEoCVICaWQSEgoEa2luZBgCIAEoCVIEa2luZBIYCgdlbmFibG'
    'VkGAMgASgIUgdlbmFibGVkEhkKCGJhc2VfdXJsGAQgASgJUgdiYXNlVXJsEhQKBW1vZGVsGAUg'
    'ASgJUgVtb2RlbBIeCgthcGlfa2V5X3JlZhgGIAEoCVIJYXBpS2V5UmVmEiEKDGluc2VjdXJlX3'
    'RscxgHIAEoCFILaW5zZWN1cmVUbHMSGAoHdmVyc2lvbhgIIAEoCVIHdmVyc2lvbhIfCgttYXhf'
    'cmV0cmllcxgJIAEoDVIKbWF4UmV0cmllcxIlCg5jb250ZXh0X3dpbmRvdxgKIAEoDVINY29udG'
    'V4dFdpbmRvdxIqChFtYXhfb3V0cHV0X3Rva2VucxgLIAEoDVIPbWF4T3V0cHV0VG9rZW5zEiUK'
    'DnN1cHBvcnRzX3Rvb2xzGAwgASgIUg1zdXBwb3J0c1Rvb2xzEicKD3N1cHBvcnRzX3Zpc2lvbh'
    'gNIAEoCFIOc3VwcG9ydHNWaXNpb24SOAoYc3VwcG9ydHNfcmVzcG9uc2VfZm9ybWF0GA4gASgI'
    'UhZzdXBwb3J0c1Jlc3BvbnNlRm9ybWF0EhIKBHRhZ3MYDyADKAlSBHRhZ3MSHQoKaW5wdXRfY2'
    '9zdBgQIAEoAlIJaW5wdXRDb3N0Eh8KC291dHB1dF9jb3N0GBEgASgCUgpvdXRwdXRDb3N0EiYK'
    'BHRpZXIYEiABKA4yEi5hZ2VudC52MS5Qb29sVGllclIEdGllchIWCgZ3ZWlnaHQYEyABKAJSBn'
    'dlaWdodBInCg9tYXhfY29uY3VycmVuY3kYFCABKA1SDm1heENvbmN1cnJlbmN5');

@$core.Deprecated('Use upstreamHealthDescriptor instead')
const UpstreamHealth$json = {
  '1': 'UpstreamHealth',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {
      '1': 'state',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.PoolMemberState',
      '10': 'state'
    },
    {'1': 'in_flight', '3': 3, '4': 1, '5': 13, '10': 'inFlight'},
    {'1': 'latency_ms_ewma', '3': 4, '4': 1, '5': 13, '10': 'latencyMsEwma'},
    {'1': 'saturated', '3': 5, '4': 1, '5': 8, '10': 'saturated'},
    {
      '1': 'consecutive_failures',
      '3': 6,
      '4': 1,
      '5': 13,
      '10': 'consecutiveFailures'
    },
  ],
};

/// Descriptor for `UpstreamHealth`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List upstreamHealthDescriptor = $convert.base64Decode(
    'Cg5VcHN0cmVhbUhlYWx0aBIOCgJpZBgBIAEoCVICaWQSLwoFc3RhdGUYAiABKA4yGS5hZ2VudC'
    '52MS5Qb29sTWVtYmVyU3RhdGVSBXN0YXRlEhsKCWluX2ZsaWdodBgDIAEoDVIIaW5GbGlnaHQS'
    'JgoPbGF0ZW5jeV9tc19ld21hGAQgASgNUg1sYXRlbmN5TXNFd21hEhwKCXNhdHVyYXRlZBgFIA'
    'EoCFIJc2F0dXJhdGVkEjEKFGNvbnNlY3V0aXZlX2ZhaWx1cmVzGAYgASgNUhNjb25zZWN1dGl2'
    'ZUZhaWx1cmVz');

@$core.Deprecated('Use routeMatchDescriptor instead')
const RouteMatch$json = {
  '1': 'RouteMatch',
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
  ],
};

/// Descriptor for `RouteMatch`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List routeMatchDescriptor = $convert.base64Decode(
    'CgpSb3V0ZU1hdGNoEi8KCXRhc2tfbW9kZRgBIAEoDjISLmFnZW50LnYxLlRhc2tNb2RlUgh0YX'
    'NrTW9kZRInCgRyb2xlGAIgASgOMhMuYWdlbnQudjEuUm91dGVSb2xlUgRyb2xlEh8KC21pbl9j'
    'b250ZXh0GAMgASgNUgptaW5Db250ZXh0');

@$core.Deprecated('Use routePreferDescriptor instead')
const RoutePrefer$json = {
  '1': 'RoutePrefer',
  '2': [
    {'1': 'tags', '3': 1, '4': 3, '5': 9, '10': 'tags'},
    {
      '1': 'tier',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.PoolTier',
      '10': 'tier'
    },
    {'1': 'upstreams', '3': 3, '4': 3, '5': 9, '10': 'upstreams'},
    {'1': 'policy', '3': 4, '4': 1, '5': 9, '10': 'policy'},
  ],
};

/// Descriptor for `RoutePrefer`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List routePreferDescriptor = $convert.base64Decode(
    'CgtSb3V0ZVByZWZlchISCgR0YWdzGAEgAygJUgR0YWdzEiYKBHRpZXIYAiABKA4yEi5hZ2VudC'
    '52MS5Qb29sVGllclIEdGllchIcCgl1cHN0cmVhbXMYAyADKAlSCXVwc3RyZWFtcxIWCgZwb2xp'
    'Y3kYBCABKAlSBnBvbGljeQ==');

@$core.Deprecated('Use routeRuleDescriptor instead')
const RouteRule$json = {
  '1': 'RouteRule',
  '2': [
    {
      '1': 'match',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.RouteMatch',
      '10': 'match'
    },
    {
      '1': 'prefer',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.RoutePrefer',
      '10': 'prefer'
    },
  ],
};

/// Descriptor for `RouteRule`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List routeRuleDescriptor = $convert.base64Decode(
    'CglSb3V0ZVJ1bGUSKgoFbWF0Y2gYASABKAsyFC5hZ2VudC52MS5Sb3V0ZU1hdGNoUgVtYXRjaB'
    'ItCgZwcmVmZXIYAiABKAsyFS5hZ2VudC52MS5Sb3V0ZVByZWZlclIGcHJlZmVy');

@$core.Deprecated('Use routePolicyDescriptor instead')
const RoutePolicy$json = {
  '1': 'RoutePolicy',
  '2': [
    {
      '1': 'rules',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.RouteRule',
      '10': 'rules'
    },
    {
      '1': 'default_prefer',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.RoutePrefer',
      '10': 'defaultPrefer'
    },
    {
      '1': 'failure_threshold',
      '3': 3,
      '4': 1,
      '5': 13,
      '10': 'failureThreshold'
    },
    {'1': 'cooldown_secs', '3': 4, '4': 1, '5': 13, '10': 'cooldownSecs'},
  ],
};

/// Descriptor for `RoutePolicy`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List routePolicyDescriptor = $convert.base64Decode(
    'CgtSb3V0ZVBvbGljeRIpCgVydWxlcxgBIAMoCzITLmFnZW50LnYxLlJvdXRlUnVsZVIFcnVsZX'
    'MSPAoOZGVmYXVsdF9wcmVmZXIYAiABKAsyFS5hZ2VudC52MS5Sb3V0ZVByZWZlclINZGVmYXVs'
    'dFByZWZlchIrChFmYWlsdXJlX3RocmVzaG9sZBgDIAEoDVIQZmFpbHVyZVRocmVzaG9sZBIjCg'
    '1jb29sZG93bl9zZWNzGAQgASgNUgxjb29sZG93blNlY3M=');

@$core.Deprecated('Use modelRouterConfigDescriptor instead')
const ModelRouterConfig$json = {
  '1': 'ModelRouterConfig',
  '2': [
    {
      '1': 'upstreams',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Upstream',
      '10': 'upstreams'
    },
    {
      '1': 'policy',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.RoutePolicy',
      '10': 'policy'
    },
  ],
};

/// Descriptor for `ModelRouterConfig`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List modelRouterConfigDescriptor = $convert.base64Decode(
    'ChFNb2RlbFJvdXRlckNvbmZpZxIwCgl1cHN0cmVhbXMYASADKAsyEi5hZ2VudC52MS5VcHN0cm'
    'VhbVIJdXBzdHJlYW1zEi0KBnBvbGljeRgCIAEoCzIVLmFnZW50LnYxLlJvdXRlUG9saWN5UgZw'
    'b2xpY3k=');

@$core.Deprecated('Use routeDecisionDescriptor instead')
const RouteDecision$json = {
  '1': 'RouteDecision',
  '2': [
    {'1': 'chosen', '3': 1, '4': 1, '5': 9, '10': 'chosen'},
    {'1': 'order', '3': 2, '4': 3, '5': 9, '10': 'order'},
    {'1': 'rule', '3': 3, '4': 1, '5': 9, '10': 'rule'},
    {'1': 'why', '3': 4, '4': 1, '5': 9, '10': 'why'},
  ],
};

/// Descriptor for `RouteDecision`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List routeDecisionDescriptor = $convert.base64Decode(
    'Cg1Sb3V0ZURlY2lzaW9uEhYKBmNob3NlbhgBIAEoCVIGY2hvc2VuEhQKBW9yZGVyGAIgAygJUg'
    'VvcmRlchISCgRydWxlGAMgASgJUgRydWxlEhAKA3doeRgEIAEoCVIDd2h5');

@$core.Deprecated('Use upstreamListRequestDescriptor instead')
const UpstreamListRequest$json = {
  '1': 'UpstreamListRequest',
};

/// Descriptor for `UpstreamListRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List upstreamListRequestDescriptor =
    $convert.base64Decode('ChNVcHN0cmVhbUxpc3RSZXF1ZXN0');

@$core.Deprecated('Use upstreamListDescriptor instead')
const UpstreamList$json = {
  '1': 'UpstreamList',
  '2': [
    {
      '1': 'upstreams',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Upstream',
      '10': 'upstreams'
    },
  ],
};

/// Descriptor for `UpstreamList`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List upstreamListDescriptor = $convert.base64Decode(
    'CgxVcHN0cmVhbUxpc3QSMAoJdXBzdHJlYW1zGAEgAygLMhIuYWdlbnQudjEuVXBzdHJlYW1SCX'
    'Vwc3RyZWFtcw==');

@$core.Deprecated('Use upstreamRefDescriptor instead')
const UpstreamRef$json = {
  '1': 'UpstreamRef',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
  ],
};

/// Descriptor for `UpstreamRef`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List upstreamRefDescriptor =
    $convert.base64Decode('CgtVcHN0cmVhbVJlZhIOCgJpZBgBIAEoCVICaWQ=');

@$core.Deprecated('Use upstreamDeleteReplyDescriptor instead')
const UpstreamDeleteReply$json = {
  '1': 'UpstreamDeleteReply',
  '2': [
    {'1': 'deleted', '3': 1, '4': 1, '5': 8, '10': 'deleted'},
  ],
};

/// Descriptor for `UpstreamDeleteReply`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List upstreamDeleteReplyDescriptor =
    $convert.base64Decode(
        'ChNVcHN0cmVhbURlbGV0ZVJlcGx5EhgKB2RlbGV0ZWQYASABKAhSB2RlbGV0ZWQ=');

@$core.Deprecated('Use upstreamEnableRequestDescriptor instead')
const UpstreamEnableRequest$json = {
  '1': 'UpstreamEnableRequest',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'enabled', '3': 2, '4': 1, '5': 8, '10': 'enabled'},
  ],
};

/// Descriptor for `UpstreamEnableRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List upstreamEnableRequestDescriptor = $convert.base64Decode(
    'ChVVcHN0cmVhbUVuYWJsZVJlcXVlc3QSDgoCaWQYASABKAlSAmlkEhgKB2VuYWJsZWQYAiABKA'
    'hSB2VuYWJsZWQ=');

@$core.Deprecated('Use routePolicyRefDescriptor instead')
const RoutePolicyRef$json = {
  '1': 'RoutePolicyRef',
};

/// Descriptor for `RoutePolicyRef`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List routePolicyRefDescriptor =
    $convert.base64Decode('Cg5Sb3V0ZVBvbGljeVJlZg==');

@$core.Deprecated('Use routeRequestDescriptor instead')
const RouteRequest$json = {
  '1': 'RouteRequest',
  '2': [
    {
      '1': 'hint',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.RouteHint',
      '10': 'hint'
    },
  ],
};

/// Descriptor for `RouteRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List routeRequestDescriptor = $convert.base64Decode(
    'CgxSb3V0ZVJlcXVlc3QSJwoEaGludBgBIAEoCzITLmFnZW50LnYxLlJvdXRlSGludFIEaGludA'
    '==');

@$core.Deprecated('Use upstreamHealthRequestDescriptor instead')
const UpstreamHealthRequest$json = {
  '1': 'UpstreamHealthRequest',
};

/// Descriptor for `UpstreamHealthRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List upstreamHealthRequestDescriptor =
    $convert.base64Decode('ChVVcHN0cmVhbUhlYWx0aFJlcXVlc3Q=');

@$core.Deprecated('Use upstreamHealthListDescriptor instead')
const UpstreamHealthList$json = {
  '1': 'UpstreamHealthList',
  '2': [
    {
      '1': 'entries',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.UpstreamHealth',
      '10': 'entries'
    },
  ],
};

/// Descriptor for `UpstreamHealthList`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List upstreamHealthListDescriptor = $convert.base64Decode(
    'ChJVcHN0cmVhbUhlYWx0aExpc3QSMgoHZW50cmllcxgBIAMoCzIYLmFnZW50LnYxLlVwc3RyZW'
    'FtSGVhbHRoUgdlbnRyaWVz');
