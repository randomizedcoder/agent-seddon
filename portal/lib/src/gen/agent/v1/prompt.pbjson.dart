// This is a generated file - do not edit.
//
// Generated from agent/v1/prompt.proto.

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

@$core.Deprecated('Use promptKindDescriptor instead')
const PromptKind$json = {
  '1': 'PromptKind',
  '2': [
    {'1': 'PROMPT_KIND_UNSPECIFIED', '2': 0},
    {'1': 'PROMPT_KIND_SYSTEM', '2': 1},
    {'1': 'PROMPT_KIND_PREPEND', '2': 2},
    {'1': 'PROMPT_KIND_APPEND', '2': 3},
    {'1': 'PROMPT_KIND_MODE_LENS', '2': 4},
  ],
};

/// Descriptor for `PromptKind`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List promptKindDescriptor = $convert.base64Decode(
    'CgpQcm9tcHRLaW5kEhsKF1BST01QVF9LSU5EX1VOU1BFQ0lGSUVEEAASFgoSUFJPTVBUX0tJTk'
    'RfU1lTVEVNEAESFwoTUFJPTVBUX0tJTkRfUFJFUEVORBACEhYKElBST01QVF9LSU5EX0FQUEVO'
    'RBADEhkKFVBST01QVF9LSU5EX01PREVfTEVOUxAE');

@$core.Deprecated('Use promptRefDescriptor instead')
const PromptRef$json = {
  '1': 'PromptRef',
  '2': [
    {
      '1': 'kind',
      '3': 1,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.PromptKind',
      '10': 'kind'
    },
    {'1': 'id', '3': 2, '4': 1, '5': 9, '10': 'id'},
  ],
};

/// Descriptor for `PromptRef`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List promptRefDescriptor = $convert.base64Decode(
    'CglQcm9tcHRSZWYSKAoEa2luZBgBIAEoDjIULmFnZW50LnYxLlByb21wdEtpbmRSBGtpbmQSDg'
    'oCaWQYAiABKAlSAmlk');

@$core.Deprecated('Use promptEntryDescriptor instead')
const PromptEntry$json = {
  '1': 'PromptEntry',
  '2': [
    {
      '1': 'kind',
      '3': 1,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.PromptKind',
      '10': 'kind'
    },
    {'1': 'id', '3': 2, '4': 1, '5': 9, '10': 'id'},
    {'1': 'content', '3': 3, '4': 1, '5': 9, '10': 'content'},
    {'1': 'builtin', '3': 4, '4': 1, '5': 8, '10': 'builtin'},
    {'1': 'read_only', '3': 5, '4': 1, '5': 8, '10': 'readOnly'},
    {'1': 'order', '3': 6, '4': 1, '5': 13, '10': 'order'},
  ],
};

/// Descriptor for `PromptEntry`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List promptEntryDescriptor = $convert.base64Decode(
    'CgtQcm9tcHRFbnRyeRIoCgRraW5kGAEgASgOMhQuYWdlbnQudjEuUHJvbXB0S2luZFIEa2luZB'
    'IOCgJpZBgCIAEoCVICaWQSGAoHY29udGVudBgDIAEoCVIHY29udGVudBIYCgdidWlsdGluGAQg'
    'ASgIUgdidWlsdGluEhsKCXJlYWRfb25seRgFIAEoCFIIcmVhZE9ubHkSFAoFb3JkZXIYBiABKA'
    '1SBW9yZGVy');

@$core.Deprecated('Use promptListRequestDescriptor instead')
const PromptListRequest$json = {
  '1': 'PromptListRequest',
  '2': [
    {
      '1': 'kind',
      '3': 1,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.PromptKind',
      '10': 'kind'
    },
  ],
};

/// Descriptor for `PromptListRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List promptListRequestDescriptor = $convert.base64Decode(
    'ChFQcm9tcHRMaXN0UmVxdWVzdBIoCgRraW5kGAEgASgOMhQuYWdlbnQudjEuUHJvbXB0S2luZF'
    'IEa2luZA==');

@$core.Deprecated('Use promptListDescriptor instead')
const PromptList$json = {
  '1': 'PromptList',
  '2': [
    {
      '1': 'entries',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.PromptEntry',
      '10': 'entries'
    },
  ],
};

/// Descriptor for `PromptList`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List promptListDescriptor = $convert.base64Decode(
    'CgpQcm9tcHRMaXN0Ei8KB2VudHJpZXMYASADKAsyFS5hZ2VudC52MS5Qcm9tcHRFbnRyeVIHZW'
    '50cmllcw==');

@$core.Deprecated('Use deleteReplyDescriptor instead')
const DeleteReply$json = {
  '1': 'DeleteReply',
  '2': [
    {'1': 'deleted', '3': 1, '4': 1, '5': 8, '10': 'deleted'},
  ],
};

/// Descriptor for `DeleteReply`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List deleteReplyDescriptor = $convert
    .base64Decode('CgtEZWxldGVSZXBseRIYCgdkZWxldGVkGAEgASgIUgdkZWxldGVk');

@$core.Deprecated('Use previewRequestDescriptor instead')
const PreviewRequest$json = {
  '1': 'PreviewRequest',
  '2': [
    {'1': 'mode', '3': 1, '4': 1, '5': 9, '10': 'mode'},
    {'1': 'goal', '3': 2, '4': 1, '5': 9, '10': 'goal'},
  ],
};

/// Descriptor for `PreviewRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List previewRequestDescriptor = $convert.base64Decode(
    'Cg5QcmV2aWV3UmVxdWVzdBISCgRtb2RlGAEgASgJUgRtb2RlEhIKBGdvYWwYAiABKAlSBGdvYW'
    'w=');

@$core.Deprecated('Use previewMessageDescriptor instead')
const PreviewMessage$json = {
  '1': 'PreviewMessage',
  '2': [
    {'1': 'role', '3': 1, '4': 1, '5': 9, '10': 'role'},
    {'1': 'content', '3': 2, '4': 1, '5': 9, '10': 'content'},
  ],
};

/// Descriptor for `PreviewMessage`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List previewMessageDescriptor = $convert.base64Decode(
    'Cg5QcmV2aWV3TWVzc2FnZRISCgRyb2xlGAEgASgJUgRyb2xlEhgKB2NvbnRlbnQYAiABKAlSB2'
    'NvbnRlbnQ=');

@$core.Deprecated('Use assembledContextDescriptor instead')
const AssembledContext$json = {
  '1': 'AssembledContext',
  '2': [
    {
      '1': 'messages',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.PreviewMessage',
      '10': 'messages'
    },
  ],
};

/// Descriptor for `AssembledContext`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List assembledContextDescriptor = $convert.base64Decode(
    'ChBBc3NlbWJsZWRDb250ZXh0EjQKCG1lc3NhZ2VzGAEgAygLMhguYWdlbnQudjEuUHJldmlld0'
    '1lc3NhZ2VSCG1lc3NhZ2Vz');
