// This is a generated file - do not edit.
//
// Generated from agent/v1/llm_pool.proto.

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

@$core.Deprecated('Use poolMemberStateDescriptor instead')
const PoolMemberState$json = {
  '1': 'PoolMemberState',
  '2': [
    {'1': 'POOL_MEMBER_STATE_UNSPECIFIED', '2': 0},
    {'1': 'POOL_MEMBER_STATE_HEALTHY', '2': 1},
    {'1': 'POOL_MEMBER_STATE_DEGRADED', '2': 2},
    {'1': 'POOL_MEMBER_STATE_DEAD', '2': 3},
  ],
};

/// Descriptor for `PoolMemberState`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List poolMemberStateDescriptor = $convert.base64Decode(
    'Cg9Qb29sTWVtYmVyU3RhdGUSIQodUE9PTF9NRU1CRVJfU1RBVEVfVU5TUEVDSUZJRUQQABIdCh'
    'lQT09MX01FTUJFUl9TVEFURV9IRUFMVEhZEAESHgoaUE9PTF9NRU1CRVJfU1RBVEVfREVHUkFE'
    'RUQQAhIaChZQT09MX01FTUJFUl9TVEFURV9ERUFEEAM=');

@$core.Deprecated('Use poolHealthRequestDescriptor instead')
const PoolHealthRequest$json = {
  '1': 'PoolHealthRequest',
};

/// Descriptor for `PoolHealthRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List poolHealthRequestDescriptor =
    $convert.base64Decode('ChFQb29sSGVhbHRoUmVxdWVzdA==');

@$core.Deprecated('Use poolMemberHealthDescriptor instead')
const PoolMemberHealth$json = {
  '1': 'PoolMemberHealth',
  '2': [
    {'1': 'name', '3': 1, '4': 1, '5': 9, '10': 'name'},
    {
      '1': 'tier',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.PoolTier',
      '10': 'tier'
    },
    {'1': 'alive', '3': 3, '4': 1, '5': 8, '10': 'alive'},
    {
      '1': 'consecutive_failures',
      '3': 4,
      '4': 1,
      '5': 13,
      '10': 'consecutiveFailures'
    },
    {'1': 'last_probe_ms', '3': 5, '4': 1, '5': 13, '10': 'lastProbeMs'},
    {'1': 'in_flight', '3': 6, '4': 1, '5': 13, '10': 'inFlight'},
    {'1': 'weight', '3': 7, '4': 1, '5': 2, '10': 'weight'},
    {'1': 'max_concurrency', '3': 8, '4': 1, '5': 13, '10': 'maxConcurrency'},
    {'1': 'saturated', '3': 9, '4': 1, '5': 8, '10': 'saturated'},
    {
      '1': 'state',
      '3': 10,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.PoolMemberState',
      '10': 'state'
    },
    {'1': 'latency_ms_ewma', '3': 11, '4': 1, '5': 13, '10': 'latencyMsEwma'},
  ],
};

/// Descriptor for `PoolMemberHealth`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List poolMemberHealthDescriptor = $convert.base64Decode(
    'ChBQb29sTWVtYmVySGVhbHRoEhIKBG5hbWUYASABKAlSBG5hbWUSJgoEdGllchgCIAEoDjISLm'
    'FnZW50LnYxLlBvb2xUaWVyUgR0aWVyEhQKBWFsaXZlGAMgASgIUgVhbGl2ZRIxChRjb25zZWN1'
    'dGl2ZV9mYWlsdXJlcxgEIAEoDVITY29uc2VjdXRpdmVGYWlsdXJlcxIiCg1sYXN0X3Byb2JlX2'
    '1zGAUgASgNUgtsYXN0UHJvYmVNcxIbCglpbl9mbGlnaHQYBiABKA1SCGluRmxpZ2h0EhYKBndl'
    'aWdodBgHIAEoAlIGd2VpZ2h0EicKD21heF9jb25jdXJyZW5jeRgIIAEoDVIObWF4Q29uY3Vycm'
    'VuY3kSHAoJc2F0dXJhdGVkGAkgASgIUglzYXR1cmF0ZWQSLwoFc3RhdGUYCiABKA4yGS5hZ2Vu'
    'dC52MS5Qb29sTWVtYmVyU3RhdGVSBXN0YXRlEiYKD2xhdGVuY3lfbXNfZXdtYRgLIAEoDVINbG'
    'F0ZW5jeU1zRXdtYQ==');

@$core.Deprecated('Use poolHealthReportDescriptor instead')
const PoolHealthReport$json = {
  '1': 'PoolHealthReport',
  '2': [
    {
      '1': 'members',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.PoolMemberHealth',
      '10': 'members'
    },
  ],
};

/// Descriptor for `PoolHealthReport`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List poolHealthReportDescriptor = $convert.base64Decode(
    'ChBQb29sSGVhbHRoUmVwb3J0EjQKB21lbWJlcnMYASADKAsyGi5hZ2VudC52MS5Qb29sTWVtYm'
    'VySGVhbHRoUgdtZW1iZXJz');

@$core.Deprecated('Use poolCompleteRequestDescriptor instead')
const PoolCompleteRequest$json = {
  '1': 'PoolCompleteRequest',
  '2': [
    {
      '1': 'req',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.CompletionRequest',
      '10': 'req'
    },
    {
      '1': 'tier',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.PoolTier',
      '10': 'tier'
    },
    {'1': 'fanout', '3': 3, '4': 1, '5': 13, '10': 'fanout'},
  ],
};

/// Descriptor for `PoolCompleteRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List poolCompleteRequestDescriptor = $convert.base64Decode(
    'ChNQb29sQ29tcGxldGVSZXF1ZXN0Ei0KA3JlcRgBIAEoCzIbLmFnZW50LnYxLkNvbXBsZXRpb2'
    '5SZXF1ZXN0UgNyZXESJgoEdGllchgCIAEoDjISLmFnZW50LnYxLlBvb2xUaWVyUgR0aWVyEhYK'
    'BmZhbm91dBgDIAEoDVIGZmFub3V0');

@$core.Deprecated('Use poolMemberResultDescriptor instead')
const PoolMemberResult$json = {
  '1': 'PoolMemberResult',
  '2': [
    {'1': 'member', '3': 1, '4': 1, '5': 9, '10': 'member'},
    {'1': 'ok', '3': 2, '4': 1, '5': 8, '10': 'ok'},
    {'1': 'error', '3': 3, '4': 1, '5': 9, '10': 'error'},
    {'1': 'duration_ms', '3': 4, '4': 1, '5': 13, '10': 'durationMs'},
    {
      '1': 'response',
      '3': 5,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.CompletionResponse',
      '10': 'response'
    },
  ],
};

/// Descriptor for `PoolMemberResult`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List poolMemberResultDescriptor = $convert.base64Decode(
    'ChBQb29sTWVtYmVyUmVzdWx0EhYKBm1lbWJlchgBIAEoCVIGbWVtYmVyEg4KAm9rGAIgASgIUg'
    'JvaxIUCgVlcnJvchgDIAEoCVIFZXJyb3ISHwoLZHVyYXRpb25fbXMYBCABKA1SCmR1cmF0aW9u'
    'TXMSOAoIcmVzcG9uc2UYBSABKAsyHC5hZ2VudC52MS5Db21wbGV0aW9uUmVzcG9uc2VSCHJlc3'
    'BvbnNl');

@$core.Deprecated('Use poolCompleteResponseDescriptor instead')
const PoolCompleteResponse$json = {
  '1': 'PoolCompleteResponse',
  '2': [
    {
      '1': 'results',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.PoolMemberResult',
      '10': 'results'
    },
  ],
};

/// Descriptor for `PoolCompleteResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List poolCompleteResponseDescriptor = $convert.base64Decode(
    'ChRQb29sQ29tcGxldGVSZXNwb25zZRI0CgdyZXN1bHRzGAEgAygLMhouYWdlbnQudjEuUG9vbE'
    '1lbWJlclJlc3VsdFIHcmVzdWx0cw==');
