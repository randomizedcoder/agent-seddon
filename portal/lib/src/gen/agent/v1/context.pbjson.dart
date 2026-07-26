// This is a generated file - do not edit.
//
// Generated from agent/v1/context.proto.

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

@$core.Deprecated('Use assembleResponseDescriptor instead')
const AssembleResponse$json = {
  '1': 'AssembleResponse',
  '2': [
    {
      '1': 'messages',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Message',
      '10': 'messages'
    },
  ],
};

/// Descriptor for `AssembleResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List assembleResponseDescriptor = $convert.base64Decode(
    'ChBBc3NlbWJsZVJlc3BvbnNlEi0KCG1lc3NhZ2VzGAEgAygLMhEuYWdlbnQudjEuTWVzc2FnZV'
    'IIbWVzc2FnZXM=');

@$core.Deprecated('Use compactRequestDescriptor instead')
const CompactRequest$json = {
  '1': 'CompactRequest',
  '2': [
    {
      '1': 'working',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.WorkingSet',
      '10': 'working'
    },
    {
      '1': 'budget',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.TokenBudget',
      '10': 'budget'
    },
    {
      '1': 'from_mode',
      '3': 3,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.TaskMode',
      '10': 'fromMode'
    },
    {
      '1': 'to_mode',
      '3': 4,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.TaskMode',
      '10': 'toMode'
    },
  ],
};

/// Descriptor for `CompactRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List compactRequestDescriptor = $convert.base64Decode(
    'Cg5Db21wYWN0UmVxdWVzdBIuCgd3b3JraW5nGAEgASgLMhQuYWdlbnQudjEuV29ya2luZ1NldF'
    'IHd29ya2luZxItCgZidWRnZXQYAiABKAsyFS5hZ2VudC52MS5Ub2tlbkJ1ZGdldFIGYnVkZ2V0'
    'Ei8KCWZyb21fbW9kZRgDIAEoDjISLmFnZW50LnYxLlRhc2tNb2RlUghmcm9tTW9kZRIrCgd0b1'
    '9tb2RlGAQgASgOMhIuYWdlbnQudjEuVGFza01vZGVSBnRvTW9kZQ==');

@$core.Deprecated('Use compactStatsDescriptor instead')
const CompactStats$json = {
  '1': 'CompactStats',
  '2': [
    {'1': 'kept_tokens', '3': 1, '4': 1, '5': 13, '10': 'keptTokens'},
    {'1': 'shed_tokens', '3': 2, '4': 1, '5': 13, '10': 'shedTokens'},
    {'1': 'action', '3': 3, '4': 1, '5': 9, '10': 'action'},
  ],
};

/// Descriptor for `CompactStats`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List compactStatsDescriptor = $convert.base64Decode(
    'CgxDb21wYWN0U3RhdHMSHwoLa2VwdF90b2tlbnMYASABKA1SCmtlcHRUb2tlbnMSHwoLc2hlZF'
    '90b2tlbnMYAiABKA1SCnNoZWRUb2tlbnMSFgoGYWN0aW9uGAMgASgJUgZhY3Rpb24=');

@$core.Deprecated('Use compactResponseDescriptor instead')
const CompactResponse$json = {
  '1': 'CompactResponse',
  '2': [
    {
      '1': 'working',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.WorkingSet',
      '10': 'working'
    },
    {
      '1': 'stats',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.CompactStats',
      '10': 'stats'
    },
  ],
};

/// Descriptor for `CompactResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List compactResponseDescriptor = $convert.base64Decode(
    'Cg9Db21wYWN0UmVzcG9uc2USLgoHd29ya2luZxgBIAEoCzIULmFnZW50LnYxLldvcmtpbmdTZX'
    'RSB3dvcmtpbmcSLAoFc3RhdHMYAiABKAsyFi5hZ2VudC52MS5Db21wYWN0U3RhdHNSBXN0YXRz');
