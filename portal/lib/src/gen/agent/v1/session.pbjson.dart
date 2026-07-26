// This is a generated file - do not edit.
//
// Generated from agent/v1/session.proto.

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

@$core.Deprecated('Use sessionCheckpointRefDescriptor instead')
const SessionCheckpointRef$json = {
  '1': 'SessionCheckpointRef',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
  ],
};

/// Descriptor for `SessionCheckpointRef`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List sessionCheckpointRefDescriptor = $convert
    .base64Decode('ChRTZXNzaW9uQ2hlY2twb2ludFJlZhIOCgJpZBgBIAEoCVICaWQ=');

@$core.Deprecated('Use sessionRefDescriptor instead')
const SessionRef$json = {
  '1': 'SessionRef',
  '2': [
    {'1': 'session', '3': 1, '4': 1, '5': 9, '10': 'session'},
  ],
};

/// Descriptor for `SessionRef`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List sessionRefDescriptor = $convert
    .base64Decode('CgpTZXNzaW9uUmVmEhgKB3Nlc3Npb24YASABKAlSB3Nlc3Npb24=');

@$core.Deprecated('Use sessionCheckpointRequestDescriptor instead')
const SessionCheckpointRequest$json = {
  '1': 'SessionCheckpointRequest',
  '2': [
    {'1': 'session', '3': 1, '4': 1, '5': 9, '10': 'session'},
    {
      '1': 'working',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.WorkingSet',
      '10': 'working'
    },
    {'1': 'label', '3': 3, '4': 1, '5': 9, '10': 'label'},
  ],
};

/// Descriptor for `SessionCheckpointRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List sessionCheckpointRequestDescriptor = $convert.base64Decode(
    'ChhTZXNzaW9uQ2hlY2twb2ludFJlcXVlc3QSGAoHc2Vzc2lvbhgBIAEoCVIHc2Vzc2lvbhIuCg'
    'd3b3JraW5nGAIgASgLMhQuYWdlbnQudjEuV29ya2luZ1NldFIHd29ya2luZxIUCgVsYWJlbBgD'
    'IAEoCVIFbGFiZWw=');

@$core.Deprecated('Use sessionCheckpointMetaDescriptor instead')
const SessionCheckpointMeta$json = {
  '1': 'SessionCheckpointMeta',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'parent', '3': 2, '4': 1, '5': 9, '9': 0, '10': 'parent', '17': true},
    {'1': 'branch', '3': 3, '4': 1, '5': 9, '10': 'branch'},
    {'1': 'turn', '3': 4, '4': 1, '5': 13, '10': 'turn'},
    {'1': 'label', '3': 5, '4': 1, '5': 9, '10': 'label'},
    {'1': 'created_ms', '3': 6, '4': 1, '5': 4, '10': 'createdMs'},
  ],
  '8': [
    {'1': '_parent'},
  ],
};

/// Descriptor for `SessionCheckpointMeta`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List sessionCheckpointMetaDescriptor = $convert.base64Decode(
    'ChVTZXNzaW9uQ2hlY2twb2ludE1ldGESDgoCaWQYASABKAlSAmlkEhsKBnBhcmVudBgCIAEoCU'
    'gAUgZwYXJlbnSIAQESFgoGYnJhbmNoGAMgASgJUgZicmFuY2gSEgoEdHVybhgEIAEoDVIEdHVy'
    'bhIUCgVsYWJlbBgFIAEoCVIFbGFiZWwSHQoKY3JlYXRlZF9tcxgGIAEoBFIJY3JlYXRlZE1zQg'
    'kKB19wYXJlbnQ=');

@$core.Deprecated('Use sessionCheckpointListDescriptor instead')
const SessionCheckpointList$json = {
  '1': 'SessionCheckpointList',
  '2': [
    {
      '1': 'checkpoints',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.SessionCheckpointMeta',
      '10': 'checkpoints'
    },
  ],
};

/// Descriptor for `SessionCheckpointList`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List sessionCheckpointListDescriptor = $convert.base64Decode(
    'ChVTZXNzaW9uQ2hlY2twb2ludExpc3QSQQoLY2hlY2twb2ludHMYASADKAsyHy5hZ2VudC52MS'
    '5TZXNzaW9uQ2hlY2twb2ludE1ldGFSC2NoZWNrcG9pbnRz');

@$core.Deprecated('Use sessionBranchRequestDescriptor instead')
const SessionBranchRequest$json = {
  '1': 'SessionBranchRequest',
  '2': [
    {'1': 'session', '3': 1, '4': 1, '5': 9, '10': 'session'},
    {'1': 'from', '3': 2, '4': 1, '5': 9, '10': 'from'},
    {'1': 'name', '3': 3, '4': 1, '5': 9, '10': 'name'},
  ],
};

/// Descriptor for `SessionBranchRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List sessionBranchRequestDescriptor = $convert.base64Decode(
    'ChRTZXNzaW9uQnJhbmNoUmVxdWVzdBIYCgdzZXNzaW9uGAEgASgJUgdzZXNzaW9uEhIKBGZyb2'
    '0YAiABKAlSBGZyb20SEgoEbmFtZRgDIAEoCVIEbmFtZQ==');

@$core.Deprecated('Use sessionBranchResponseDescriptor instead')
const SessionBranchResponse$json = {
  '1': 'SessionBranchResponse',
};

/// Descriptor for `SessionBranchResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List sessionBranchResponseDescriptor =
    $convert.base64Decode('ChVTZXNzaW9uQnJhbmNoUmVzcG9uc2U=');

@$core.Deprecated('Use sessionUndoRequestDescriptor instead')
const SessionUndoRequest$json = {
  '1': 'SessionUndoRequest',
  '2': [
    {'1': 'session', '3': 1, '4': 1, '5': 9, '10': 'session'},
    {'1': 'n', '3': 2, '4': 1, '5': 13, '10': 'n'},
  ],
};

/// Descriptor for `SessionUndoRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List sessionUndoRequestDescriptor = $convert.base64Decode(
    'ChJTZXNzaW9uVW5kb1JlcXVlc3QSGAoHc2Vzc2lvbhgBIAEoCVIHc2Vzc2lvbhIMCgFuGAIgAS'
    'gNUgFu');

@$core.Deprecated('Use sessionDiffRequestDescriptor instead')
const SessionDiffRequest$json = {
  '1': 'SessionDiffRequest',
  '2': [
    {'1': 'a', '3': 1, '4': 1, '5': 9, '10': 'a'},
    {'1': 'b', '3': 2, '4': 1, '5': 9, '10': 'b'},
  ],
};

/// Descriptor for `SessionDiffRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List sessionDiffRequestDescriptor =
    $convert.base64Decode(
        'ChJTZXNzaW9uRGlmZlJlcXVlc3QSDAoBYRgBIAEoCVIBYRIMCgFiGAIgASgJUgFi');

@$core.Deprecated('Use sessionCheckpointDiffDescriptor instead')
const SessionCheckpointDiff$json = {
  '1': 'SessionCheckpointDiff',
  '2': [
    {'1': 'added', '3': 1, '4': 1, '5': 4, '10': 'added'},
    {'1': 'removed', '3': 2, '4': 1, '5': 4, '10': 'removed'},
  ],
};

/// Descriptor for `SessionCheckpointDiff`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List sessionCheckpointDiffDescriptor = $convert.base64Decode(
    'ChVTZXNzaW9uQ2hlY2twb2ludERpZmYSFAoFYWRkZWQYASABKARSBWFkZGVkEhgKB3JlbW92ZW'
    'QYAiABKARSB3JlbW92ZWQ=');

@$core.Deprecated('Use sessionPruneResponseDescriptor instead')
const SessionPruneResponse$json = {
  '1': 'SessionPruneResponse',
  '2': [
    {'1': 'reclaimed', '3': 1, '4': 1, '5': 4, '10': 'reclaimed'},
  ],
};

/// Descriptor for `SessionPruneResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List sessionPruneResponseDescriptor = $convert.base64Decode(
    'ChRTZXNzaW9uUHJ1bmVSZXNwb25zZRIcCglyZWNsYWltZWQYASABKARSCXJlY2xhaW1lZA==');
