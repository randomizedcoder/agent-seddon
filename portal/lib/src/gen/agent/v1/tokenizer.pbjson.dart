// This is a generated file - do not edit.
//
// Generated from agent/v1/tokenizer.proto.

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

@$core.Deprecated('Use tokCountRequestDescriptor instead')
const TokCountRequest$json = {
  '1': 'TokCountRequest',
  '2': [
    {'1': 'text', '3': 1, '4': 1, '5': 9, '10': 'text'},
    {'1': 'model', '3': 2, '4': 1, '5': 9, '10': 'model'},
  ],
};

/// Descriptor for `TokCountRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List tokCountRequestDescriptor = $convert.base64Decode(
    'Cg9Ub2tDb3VudFJlcXVlc3QSEgoEdGV4dBgBIAEoCVIEdGV4dBIUCgVtb2RlbBgCIAEoCVIFbW'
    '9kZWw=');

@$core.Deprecated('Use tokCountMessagesRequestDescriptor instead')
const TokCountMessagesRequest$json = {
  '1': 'TokCountMessagesRequest',
  '2': [
    {
      '1': 'messages',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Message',
      '10': 'messages'
    },
    {'1': 'model', '3': 2, '4': 1, '5': 9, '10': 'model'},
  ],
};

/// Descriptor for `TokCountMessagesRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List tokCountMessagesRequestDescriptor =
    $convert.base64Decode(
        'ChdUb2tDb3VudE1lc3NhZ2VzUmVxdWVzdBItCghtZXNzYWdlcxgBIAMoCzIRLmFnZW50LnYxLk'
        '1lc3NhZ2VSCG1lc3NhZ2VzEhQKBW1vZGVsGAIgASgJUgVtb2RlbA==');

@$core.Deprecated('Use tokCountDescriptor instead')
const TokCount$json = {
  '1': 'TokCount',
  '2': [
    {'1': 'tokens', '3': 1, '4': 1, '5': 13, '10': 'tokens'},
  ],
};

/// Descriptor for `TokCount`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List tokCountDescriptor =
    $convert.base64Decode('CghUb2tDb3VudBIWCgZ0b2tlbnMYASABKA1SBnRva2Vucw==');
