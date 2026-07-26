// This is a generated file - do not edit.
//
// Generated from agent/v1/forge.proto.

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

@$core.Deprecated('Use forgeReviewVerdictDescriptor instead')
const ForgeReviewVerdict$json = {
  '1': 'ForgeReviewVerdict',
  '2': [
    {'1': 'FORGE_REVIEW_VERDICT_COMMENT', '2': 0},
    {'1': 'FORGE_REVIEW_VERDICT_APPROVE', '2': 1},
    {'1': 'FORGE_REVIEW_VERDICT_REQUEST_CHANGES', '2': 2},
  ],
};

/// Descriptor for `ForgeReviewVerdict`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List forgeReviewVerdictDescriptor = $convert.base64Decode(
    'ChJGb3JnZVJldmlld1ZlcmRpY3QSIAocRk9SR0VfUkVWSUVXX1ZFUkRJQ1RfQ09NTUVOVBAAEi'
    'AKHEZPUkdFX1JFVklFV19WRVJESUNUX0FQUFJPVkUQARIoCiRGT1JHRV9SRVZJRVdfVkVSRElD'
    'VF9SRVFVRVNUX0NIQU5HRVMQAg==');

@$core.Deprecated('Use taskStatusDescriptor instead')
const TaskStatus$json = {
  '1': 'TaskStatus',
  '2': [
    {'1': 'TASK_STATUS_PENDING', '2': 0},
    {'1': 'TASK_STATUS_IN_PROGRESS', '2': 1},
    {'1': 'TASK_STATUS_COMPLETED', '2': 2},
    {'1': 'TASK_STATUS_CANCELLED', '2': 3},
  ],
};

/// Descriptor for `TaskStatus`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List taskStatusDescriptor = $convert.base64Decode(
    'CgpUYXNrU3RhdHVzEhcKE1RBU0tfU1RBVFVTX1BFTkRJTkcQABIbChdUQVNLX1NUQVRVU19JTl'
    '9QUk9HUkVTUxABEhkKFVRBU0tfU1RBVFVTX0NPTVBMRVRFRBACEhkKFVRBU0tfU1RBVFVTX0NB'
    'TkNFTExFRBAD');

@$core.Deprecated('Use taskPriorityDescriptor instead')
const TaskPriority$json = {
  '1': 'TaskPriority',
  '2': [
    {'1': 'TASK_PRIORITY_MEDIUM', '2': 0},
    {'1': 'TASK_PRIORITY_HIGH', '2': 1},
    {'1': 'TASK_PRIORITY_LOW', '2': 2},
  ],
};

/// Descriptor for `TaskPriority`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List taskPriorityDescriptor = $convert.base64Decode(
    'CgxUYXNrUHJpb3JpdHkSGAoUVEFTS19QUklPUklUWV9NRURJVU0QABIWChJUQVNLX1BSSU9SSV'
    'RZX0hJR0gQARIVChFUQVNLX1BSSU9SSVRZX0xPVxAC');

@$core.Deprecated('Use forgeNumberDescriptor instead')
const ForgeNumber$json = {
  '1': 'ForgeNumber',
  '2': [
    {'1': 'number', '3': 1, '4': 1, '5': 4, '10': 'number'},
  ],
};

/// Descriptor for `ForgeNumber`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List forgeNumberDescriptor = $convert
    .base64Decode('CgtGb3JnZU51bWJlchIWCgZudW1iZXIYASABKARSBm51bWJlcg==');

@$core.Deprecated('Use forgePageDescriptor instead')
const ForgePage$json = {
  '1': 'ForgePage',
  '2': [
    {'1': 'page', '3': 1, '4': 1, '5': 13, '10': 'page'},
  ],
};

/// Descriptor for `ForgePage`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List forgePageDescriptor =
    $convert.base64Decode('CglGb3JnZVBhZ2USEgoEcGFnZRgBIAEoDVIEcGFnZQ==');

@$core.Deprecated('Use forgePullRequestDescriptor instead')
const ForgePullRequest$json = {
  '1': 'ForgePullRequest',
  '2': [
    {'1': 'number', '3': 1, '4': 1, '5': 4, '10': 'number'},
    {'1': 'title', '3': 2, '4': 1, '5': 9, '10': 'title'},
    {'1': 'body', '3': 3, '4': 1, '5': 9, '10': 'body'},
    {'1': 'state', '3': 4, '4': 1, '5': 9, '10': 'state'},
    {'1': 'author', '3': 5, '4': 1, '5': 9, '10': 'author'},
    {'1': 'url', '3': 6, '4': 1, '5': 9, '10': 'url'},
    {'1': 'source_branch', '3': 7, '4': 1, '5': 9, '10': 'sourceBranch'},
    {'1': 'target_branch', '3': 8, '4': 1, '5': 9, '10': 'targetBranch'},
    {'1': 'draft', '3': 9, '4': 1, '5': 8, '10': 'draft'},
  ],
};

/// Descriptor for `ForgePullRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List forgePullRequestDescriptor = $convert.base64Decode(
    'ChBGb3JnZVB1bGxSZXF1ZXN0EhYKBm51bWJlchgBIAEoBFIGbnVtYmVyEhQKBXRpdGxlGAIgAS'
    'gJUgV0aXRsZRISCgRib2R5GAMgASgJUgRib2R5EhQKBXN0YXRlGAQgASgJUgVzdGF0ZRIWCgZh'
    'dXRob3IYBSABKAlSBmF1dGhvchIQCgN1cmwYBiABKAlSA3VybBIjCg1zb3VyY2VfYnJhbmNoGA'
    'cgASgJUgxzb3VyY2VCcmFuY2gSIwoNdGFyZ2V0X2JyYW5jaBgIIAEoCVIMdGFyZ2V0QnJhbmNo'
    'EhQKBWRyYWZ0GAkgASgIUgVkcmFmdA==');

@$core.Deprecated('Use forgeCommentDescriptor instead')
const ForgeComment$json = {
  '1': 'ForgeComment',
  '2': [
    {'1': 'author', '3': 1, '4': 1, '5': 9, '10': 'author'},
    {'1': 'body', '3': 2, '4': 1, '5': 9, '10': 'body'},
    {'1': 'url', '3': 3, '4': 1, '5': 9, '10': 'url'},
  ],
};

/// Descriptor for `ForgeComment`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List forgeCommentDescriptor = $convert.base64Decode(
    'CgxGb3JnZUNvbW1lbnQSFgoGYXV0aG9yGAEgASgJUgZhdXRob3ISEgoEYm9keRgCIAEoCVIEYm'
    '9keRIQCgN1cmwYAyABKAlSA3VybA==');

@$core.Deprecated('Use forgeIssueDescriptor instead')
const ForgeIssue$json = {
  '1': 'ForgeIssue',
  '2': [
    {'1': 'number', '3': 1, '4': 1, '5': 4, '10': 'number'},
    {'1': 'title', '3': 2, '4': 1, '5': 9, '10': 'title'},
    {'1': 'body', '3': 3, '4': 1, '5': 9, '10': 'body'},
    {'1': 'state', '3': 4, '4': 1, '5': 9, '10': 'state'},
    {'1': 'author', '3': 5, '4': 1, '5': 9, '10': 'author'},
    {'1': 'url', '3': 6, '4': 1, '5': 9, '10': 'url'},
    {'1': 'labels', '3': 7, '4': 3, '5': 9, '10': 'labels'},
    {
      '1': 'comments',
      '3': 8,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ForgeComment',
      '10': 'comments'
    },
  ],
};

/// Descriptor for `ForgeIssue`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List forgeIssueDescriptor = $convert.base64Decode(
    'CgpGb3JnZUlzc3VlEhYKBm51bWJlchgBIAEoBFIGbnVtYmVyEhQKBXRpdGxlGAIgASgJUgV0aX'
    'RsZRISCgRib2R5GAMgASgJUgRib2R5EhQKBXN0YXRlGAQgASgJUgVzdGF0ZRIWCgZhdXRob3IY'
    'BSABKAlSBmF1dGhvchIQCgN1cmwYBiABKAlSA3VybBIWCgZsYWJlbHMYByADKAlSBmxhYmVscx'
    'IyCghjb21tZW50cxgIIAMoCzIWLmFnZW50LnYxLkZvcmdlQ29tbWVudFIIY29tbWVudHM=');

@$core.Deprecated('Use forgePrPageDescriptor instead')
const ForgePrPage$json = {
  '1': 'ForgePrPage',
  '2': [
    {
      '1': 'items',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ForgePullRequest',
      '10': 'items'
    },
    {
      '1': 'next_page',
      '3': 2,
      '4': 1,
      '5': 13,
      '9': 0,
      '10': 'nextPage',
      '17': true
    },
  ],
  '8': [
    {'1': '_next_page'},
  ],
};

/// Descriptor for `ForgePrPage`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List forgePrPageDescriptor = $convert.base64Decode(
    'CgtGb3JnZVByUGFnZRIwCgVpdGVtcxgBIAMoCzIaLmFnZW50LnYxLkZvcmdlUHVsbFJlcXVlc3'
    'RSBWl0ZW1zEiAKCW5leHRfcGFnZRgCIAEoDUgAUghuZXh0UGFnZYgBAUIMCgpfbmV4dF9wYWdl');

@$core.Deprecated('Use forgeIssuePageDescriptor instead')
const ForgeIssuePage$json = {
  '1': 'ForgeIssuePage',
  '2': [
    {
      '1': 'items',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ForgeIssue',
      '10': 'items'
    },
    {
      '1': 'next_page',
      '3': 2,
      '4': 1,
      '5': 13,
      '9': 0,
      '10': 'nextPage',
      '17': true
    },
  ],
  '8': [
    {'1': '_next_page'},
  ],
};

/// Descriptor for `ForgeIssuePage`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List forgeIssuePageDescriptor = $convert.base64Decode(
    'Cg5Gb3JnZUlzc3VlUGFnZRIqCgVpdGVtcxgBIAMoCzIULmFnZW50LnYxLkZvcmdlSXNzdWVSBW'
    'l0ZW1zEiAKCW5leHRfcGFnZRgCIAEoDUgAUghuZXh0UGFnZYgBAUIMCgpfbmV4dF9wYWdl');

@$core.Deprecated('Use forgeCreatePrRequestDescriptor instead')
const ForgeCreatePrRequest$json = {
  '1': 'ForgeCreatePrRequest',
  '2': [
    {'1': 'title', '3': 1, '4': 1, '5': 9, '10': 'title'},
    {'1': 'body', '3': 2, '4': 1, '5': 9, '10': 'body'},
    {'1': 'source_branch', '3': 3, '4': 1, '5': 9, '10': 'sourceBranch'},
    {'1': 'target_branch', '3': 4, '4': 1, '5': 9, '10': 'targetBranch'},
    {'1': 'draft', '3': 5, '4': 1, '5': 8, '10': 'draft'},
  ],
};

/// Descriptor for `ForgeCreatePrRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List forgeCreatePrRequestDescriptor = $convert.base64Decode(
    'ChRGb3JnZUNyZWF0ZVByUmVxdWVzdBIUCgV0aXRsZRgBIAEoCVIFdGl0bGUSEgoEYm9keRgCIA'
    'EoCVIEYm9keRIjCg1zb3VyY2VfYnJhbmNoGAMgASgJUgxzb3VyY2VCcmFuY2gSIwoNdGFyZ2V0'
    'X2JyYW5jaBgEIAEoCVIMdGFyZ2V0QnJhbmNoEhQKBWRyYWZ0GAUgASgIUgVkcmFmdA==');

@$core.Deprecated('Use forgeCommentRequestDescriptor instead')
const ForgeCommentRequest$json = {
  '1': 'ForgeCommentRequest',
  '2': [
    {'1': 'number', '3': 1, '4': 1, '5': 4, '10': 'number'},
    {'1': 'body', '3': 2, '4': 1, '5': 9, '10': 'body'},
  ],
};

/// Descriptor for `ForgeCommentRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List forgeCommentRequestDescriptor = $convert.base64Decode(
    'ChNGb3JnZUNvbW1lbnRSZXF1ZXN0EhYKBm51bWJlchgBIAEoBFIGbnVtYmVyEhIKBGJvZHkYAi'
    'ABKAlSBGJvZHk=');

@$core.Deprecated('Use forgeReviewRequestDescriptor instead')
const ForgeReviewRequest$json = {
  '1': 'ForgeReviewRequest',
  '2': [
    {'1': 'number', '3': 1, '4': 1, '5': 4, '10': 'number'},
    {
      '1': 'verdict',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ForgeReviewVerdict',
      '10': 'verdict'
    },
    {'1': 'body', '3': 3, '4': 1, '5': 9, '10': 'body'},
  ],
};

/// Descriptor for `ForgeReviewRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List forgeReviewRequestDescriptor = $convert.base64Decode(
    'ChJGb3JnZVJldmlld1JlcXVlc3QSFgoGbnVtYmVyGAEgASgEUgZudW1iZXISNgoHdmVyZGljdB'
    'gCIAEoDjIcLmFnZW50LnYxLkZvcmdlUmV2aWV3VmVyZGljdFIHdmVyZGljdBISCgRib2R5GAMg'
    'ASgJUgRib2R5');

@$core.Deprecated('Use taskDescriptor instead')
const Task$json = {
  '1': 'Task',
  '2': [
    {'1': 'content', '3': 1, '4': 1, '5': 9, '10': 'content'},
    {
      '1': 'status',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.TaskStatus',
      '10': 'status'
    },
    {
      '1': 'priority',
      '3': 3,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.TaskPriority',
      '10': 'priority'
    },
  ],
};

/// Descriptor for `Task`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List taskDescriptor = $convert.base64Decode(
    'CgRUYXNrEhgKB2NvbnRlbnQYASABKAlSB2NvbnRlbnQSLAoGc3RhdHVzGAIgASgOMhQuYWdlbn'
    'QudjEuVGFza1N0YXR1c1IGc3RhdHVzEjIKCHByaW9yaXR5GAMgASgOMhYuYWdlbnQudjEuVGFz'
    'a1ByaW9yaXR5Ughwcmlvcml0eQ==');

@$core.Deprecated('Use taskWriteRequestDescriptor instead')
const TaskWriteRequest$json = {
  '1': 'TaskWriteRequest',
  '2': [
    {
      '1': 'todos',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Task',
      '10': 'todos'
    },
  ],
};

/// Descriptor for `TaskWriteRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List taskWriteRequestDescriptor = $convert.base64Decode(
    'ChBUYXNrV3JpdGVSZXF1ZXN0EiQKBXRvZG9zGAEgAygLMg4uYWdlbnQudjEuVGFza1IFdG9kb3'
    'M=');

@$core.Deprecated('Use taskUpdateRequestDescriptor instead')
const TaskUpdateRequest$json = {
  '1': 'TaskUpdateRequest',
  '2': [
    {'1': 'content', '3': 1, '4': 1, '5': 9, '10': 'content'},
    {
      '1': 'status',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.TaskStatus',
      '9': 0,
      '10': 'status',
      '17': true
    },
    {
      '1': 'priority',
      '3': 3,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.TaskPriority',
      '9': 1,
      '10': 'priority',
      '17': true
    },
  ],
  '8': [
    {'1': '_status'},
    {'1': '_priority'},
  ],
};

/// Descriptor for `TaskUpdateRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List taskUpdateRequestDescriptor = $convert.base64Decode(
    'ChFUYXNrVXBkYXRlUmVxdWVzdBIYCgdjb250ZW50GAEgASgJUgdjb250ZW50EjEKBnN0YXR1cx'
    'gCIAEoDjIULmFnZW50LnYxLlRhc2tTdGF0dXNIAFIGc3RhdHVziAEBEjcKCHByaW9yaXR5GAMg'
    'ASgOMhYuYWdlbnQudjEuVGFza1ByaW9yaXR5SAFSCHByaW9yaXR5iAEBQgkKB19zdGF0dXNCCw'
    'oJX3ByaW9yaXR5');

@$core.Deprecated('Use taskListRequestDescriptor instead')
const TaskListRequest$json = {
  '1': 'TaskListRequest',
};

/// Descriptor for `TaskListRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List taskListRequestDescriptor =
    $convert.base64Decode('Cg9UYXNrTGlzdFJlcXVlc3Q=');

@$core.Deprecated('Use taskClearRequestDescriptor instead')
const TaskClearRequest$json = {
  '1': 'TaskClearRequest',
};

/// Descriptor for `TaskClearRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List taskClearRequestDescriptor =
    $convert.base64Decode('ChBUYXNrQ2xlYXJSZXF1ZXN0');

@$core.Deprecated('Use taskClearResponseDescriptor instead')
const TaskClearResponse$json = {
  '1': 'TaskClearResponse',
};

/// Descriptor for `TaskClearResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List taskClearResponseDescriptor =
    $convert.base64Decode('ChFUYXNrQ2xlYXJSZXNwb25zZQ==');

@$core.Deprecated('Use taskListDescriptor instead')
const TaskList$json = {
  '1': 'TaskList',
  '2': [
    {
      '1': 'todos',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Task',
      '10': 'todos'
    },
  ],
};

/// Descriptor for `TaskList`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List taskListDescriptor = $convert.base64Decode(
    'CghUYXNrTGlzdBIkCgV0b2RvcxgBIAMoCzIOLmFnZW50LnYxLlRhc2tSBXRvZG9z');
