// This is a generated file - do not edit.
//
// Generated from agent/v1/role.proto.

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

@$core.Deprecated('Use rolePermissionDescriptor instead')
const RolePermission$json = {
  '1': 'RolePermission',
  '2': [
    {'1': 'action', '3': 1, '4': 1, '5': 9, '10': 'action'},
    {'1': 'resource_type', '3': 2, '4': 1, '5': 9, '10': 'resourceType'},
  ],
};

/// Descriptor for `RolePermission`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List rolePermissionDescriptor = $convert.base64Decode(
    'Cg5Sb2xlUGVybWlzc2lvbhIWCgZhY3Rpb24YASABKAlSBmFjdGlvbhIjCg1yZXNvdXJjZV90eX'
    'BlGAIgASgJUgxyZXNvdXJjZVR5cGU=');

@$core.Deprecated('Use roleCardDescriptor instead')
const RoleCard$json = {
  '1': 'RoleCard',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'crosses_tenants', '3': 2, '4': 1, '5': 8, '10': 'crossesTenants'},
    {'1': 'all', '3': 3, '4': 1, '5': 8, '10': 'all'},
    {'1': 'actions_on_all', '3': 4, '4': 3, '5': 9, '10': 'actionsOnAll'},
    {
      '1': 'pairs',
      '3': 5,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.RolePermission',
      '10': 'pairs'
    },
  ],
};

/// Descriptor for `RoleCard`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List roleCardDescriptor = $convert.base64Decode(
    'CghSb2xlQ2FyZBIOCgJpZBgBIAEoCVICaWQSJwoPY3Jvc3Nlc190ZW5hbnRzGAIgASgIUg5jcm'
    '9zc2VzVGVuYW50cxIQCgNhbGwYAyABKAhSA2FsbBIkCg5hY3Rpb25zX29uX2FsbBgEIAMoCVIM'
    'YWN0aW9uc09uQWxsEi4KBXBhaXJzGAUgAygLMhguYWdlbnQudjEuUm9sZVBlcm1pc3Npb25SBX'
    'BhaXJz');

@$core.Deprecated('Use roleListRequestDescriptor instead')
const RoleListRequest$json = {
  '1': 'RoleListRequest',
};

/// Descriptor for `RoleListRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List roleListRequestDescriptor =
    $convert.base64Decode('Cg9Sb2xlTGlzdFJlcXVlc3Q=');

@$core.Deprecated('Use roleListDescriptor instead')
const RoleList$json = {
  '1': 'RoleList',
  '2': [
    {
      '1': 'roles',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.RoleCard',
      '10': 'roles'
    },
  ],
};

/// Descriptor for `RoleList`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List roleListDescriptor = $convert.base64Decode(
    'CghSb2xlTGlzdBIoCgVyb2xlcxgBIAMoCzISLmFnZW50LnYxLlJvbGVDYXJkUgVyb2xlcw==');

@$core.Deprecated('Use roleRefDescriptor instead')
const RoleRef$json = {
  '1': 'RoleRef',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
  ],
};

/// Descriptor for `RoleRef`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List roleRefDescriptor =
    $convert.base64Decode('CgdSb2xlUmVmEg4KAmlkGAEgASgJUgJpZA==');

@$core.Deprecated('Use roleDeleteReplyDescriptor instead')
const RoleDeleteReply$json = {
  '1': 'RoleDeleteReply',
  '2': [
    {'1': 'deleted', '3': 1, '4': 1, '5': 8, '10': 'deleted'},
  ],
};

/// Descriptor for `RoleDeleteReply`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List roleDeleteReplyDescriptor = $convert.base64Decode(
    'Cg9Sb2xlRGVsZXRlUmVwbHkSGAoHZGVsZXRlZBgBIAEoCFIHZGVsZXRlZA==');
