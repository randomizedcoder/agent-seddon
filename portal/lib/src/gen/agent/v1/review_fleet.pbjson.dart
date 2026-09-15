// This is a generated file - do not edit.
//
// Generated from agent/v1/review_fleet.proto.

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

@$core.Deprecated('Use fleetSessionDescriptor instead')
const FleetSession$json = {
  '1': 'FleetSession',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'user', '3': 2, '4': 1, '5': 9, '10': 'user'},
    {'1': 'repo', '3': 3, '4': 1, '5': 9, '10': 'repo'},
    {'1': 'backend', '3': 4, '4': 1, '5': 9, '10': 'backend'},
    {'1': 'base_url', '3': 5, '4': 1, '5': 9, '10': 'baseUrl'},
    {'1': 'token_ref', '3': 6, '4': 1, '5': 9, '10': 'tokenRef'},
    {'1': 'skill', '3': 7, '4': 1, '5': 9, '10': 'skill'},
    {
      '1': 'slack_trigger_channel',
      '3': 8,
      '4': 1,
      '5': 9,
      '10': 'slackTriggerChannel'
    },
    {
      '1': 'slack_progress_channel',
      '3': 9,
      '4': 1,
      '5': 9,
      '10': 'slackProgressChannel'
    },
    {'1': 'poll_secs', '3': 10, '4': 1, '5': 4, '10': 'pollSecs'},
    {'1': 'enabled', '3': 11, '4': 1, '5': 8, '10': 'enabled'},
    {'1': 'created_at', '3': 12, '4': 1, '5': 3, '10': 'createdAt'},
    {'1': 'updated_at', '3': 13, '4': 1, '5': 3, '10': 'updatedAt'},
    {'1': 'forge_id', '3': 14, '4': 1, '5': 9, '10': 'forgeId'},
    {'1': 'transport_id', '3': 15, '4': 1, '5': 9, '10': 'transportId'},
  ],
};

/// Descriptor for `FleetSession`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List fleetSessionDescriptor = $convert.base64Decode(
    'CgxGbGVldFNlc3Npb24SDgoCaWQYASABKAlSAmlkEhIKBHVzZXIYAiABKAlSBHVzZXISEgoEcm'
    'VwbxgDIAEoCVIEcmVwbxIYCgdiYWNrZW5kGAQgASgJUgdiYWNrZW5kEhkKCGJhc2VfdXJsGAUg'
    'ASgJUgdiYXNlVXJsEhsKCXRva2VuX3JlZhgGIAEoCVIIdG9rZW5SZWYSFAoFc2tpbGwYByABKA'
    'lSBXNraWxsEjIKFXNsYWNrX3RyaWdnZXJfY2hhbm5lbBgIIAEoCVITc2xhY2tUcmlnZ2VyQ2hh'
    'bm5lbBI0ChZzbGFja19wcm9ncmVzc19jaGFubmVsGAkgASgJUhRzbGFja1Byb2dyZXNzQ2hhbm'
    '5lbBIbCglwb2xsX3NlY3MYCiABKARSCHBvbGxTZWNzEhgKB2VuYWJsZWQYCyABKAhSB2VuYWJs'
    'ZWQSHQoKY3JlYXRlZF9hdBgMIAEoA1IJY3JlYXRlZEF0Eh0KCnVwZGF0ZWRfYXQYDSABKANSCX'
    'VwZGF0ZWRBdBIZCghmb3JnZV9pZBgOIAEoCVIHZm9yZ2VJZBIhCgx0cmFuc3BvcnRfaWQYDyAB'
    'KAlSC3RyYW5zcG9ydElk');

@$core.Deprecated('Use fleetListRequestDescriptor instead')
const FleetListRequest$json = {
  '1': 'FleetListRequest',
};

/// Descriptor for `FleetListRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List fleetListRequestDescriptor =
    $convert.base64Decode('ChBGbGVldExpc3RSZXF1ZXN0');

@$core.Deprecated('Use fleetSessionListDescriptor instead')
const FleetSessionList$json = {
  '1': 'FleetSessionList',
  '2': [
    {
      '1': 'sessions',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.FleetSession',
      '10': 'sessions'
    },
  ],
};

/// Descriptor for `FleetSessionList`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List fleetSessionListDescriptor = $convert.base64Decode(
    'ChBGbGVldFNlc3Npb25MaXN0EjIKCHNlc3Npb25zGAEgAygLMhYuYWdlbnQudjEuRmxlZXRTZX'
    'NzaW9uUghzZXNzaW9ucw==');

@$core.Deprecated('Use fleetSessionRefDescriptor instead')
const FleetSessionRef$json = {
  '1': 'FleetSessionRef',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
  ],
};

/// Descriptor for `FleetSessionRef`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List fleetSessionRefDescriptor =
    $convert.base64Decode('Cg9GbGVldFNlc3Npb25SZWYSDgoCaWQYASABKAlSAmlk');

@$core.Deprecated('Use fleetDeleteReplyDescriptor instead')
const FleetDeleteReply$json = {
  '1': 'FleetDeleteReply',
  '2': [
    {'1': 'deleted', '3': 1, '4': 1, '5': 8, '10': 'deleted'},
  ],
};

/// Descriptor for `FleetDeleteReply`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List fleetDeleteReplyDescriptor = $convert.base64Decode(
    'ChBGbGVldERlbGV0ZVJlcGx5EhgKB2RlbGV0ZWQYASABKAhSB2RlbGV0ZWQ=');

@$core.Deprecated('Use fleetSetEnabledRequestDescriptor instead')
const FleetSetEnabledRequest$json = {
  '1': 'FleetSetEnabledRequest',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'enabled', '3': 2, '4': 1, '5': 8, '10': 'enabled'},
  ],
};

/// Descriptor for `FleetSetEnabledRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List fleetSetEnabledRequestDescriptor =
    $convert.base64Decode(
        'ChZGbGVldFNldEVuYWJsZWRSZXF1ZXN0Eg4KAmlkGAEgASgJUgJpZBIYCgdlbmFibGVkGAIgAS'
        'gIUgdlbmFibGVk');

@$core.Deprecated('Use reviewNowRequestDescriptor instead')
const ReviewNowRequest$json = {
  '1': 'ReviewNowRequest',
  '2': [
    {'1': 'session_id', '3': 1, '4': 1, '5': 9, '10': 'sessionId'},
    {'1': 'pr_number', '3': 2, '4': 1, '5': 4, '10': 'prNumber'},
  ],
};

/// Descriptor for `ReviewNowRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewNowRequestDescriptor = $convert.base64Decode(
    'ChBSZXZpZXdOb3dSZXF1ZXN0Eh0KCnNlc3Npb25faWQYASABKAlSCXNlc3Npb25JZBIbCglwcl'
    '9udW1iZXIYAiABKARSCHByTnVtYmVy');

@$core.Deprecated('Use reviewNowReplyDescriptor instead')
const ReviewNowReply$json = {
  '1': 'ReviewNowReply',
  '2': [
    {'1': 'accepted', '3': 1, '4': 1, '5': 8, '10': 'accepted'},
  ],
};

/// Descriptor for `ReviewNowReply`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewNowReplyDescriptor = $convert.base64Decode(
    'Cg5SZXZpZXdOb3dSZXBseRIaCghhY2NlcHRlZBgBIAEoCFIIYWNjZXB0ZWQ=');

@$core.Deprecated('Use approveRequestDescriptor instead')
const ApproveRequest$json = {
  '1': 'ApproveRequest',
  '2': [
    {'1': 'review_id', '3': 1, '4': 1, '5': 9, '10': 'reviewId'},
  ],
};

/// Descriptor for `ApproveRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List approveRequestDescriptor = $convert.base64Decode(
    'Cg5BcHByb3ZlUmVxdWVzdBIbCglyZXZpZXdfaWQYASABKAlSCHJldmlld0lk');

@$core.Deprecated('Use approveReplyDescriptor instead')
const ApproveReply$json = {
  '1': 'ApproveReply',
  '2': [
    {'1': 'status', '3': 1, '4': 1, '5': 9, '10': 'status'},
    {'1': 'detail', '3': 2, '4': 1, '5': 9, '10': 'detail'},
  ],
};

/// Descriptor for `ApproveReply`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List approveReplyDescriptor = $convert.base64Decode(
    'CgxBcHByb3ZlUmVwbHkSFgoGc3RhdHVzGAEgASgJUgZzdGF0dXMSFgoGZGV0YWlsGAIgASgJUg'
    'ZkZXRhaWw=');

@$core.Deprecated('Use reviewSummaryDescriptor instead')
const ReviewSummary$json = {
  '1': 'ReviewSummary',
  '2': [
    {'1': 'review_id', '3': 1, '4': 1, '5': 9, '10': 'reviewId'},
    {'1': 'repo', '3': 2, '4': 1, '5': 9, '10': 'repo'},
    {'1': 'pr_number', '3': 3, '4': 1, '5': 4, '10': 'prNumber'},
    {'1': 'head_sha', '3': 4, '4': 1, '5': 9, '10': 'headSha'},
    {'1': 'risk_score', '3': 5, '4': 1, '5': 1, '10': 'riskScore'},
    {'1': 'gate_failed', '3': 6, '4': 1, '5': 8, '10': 'gateFailed'},
    {'1': 'n_findings', '3': 7, '4': 1, '5': 13, '10': 'nFindings'},
    {'1': 'files_changed', '3': 8, '4': 1, '5': 13, '10': 'filesChanged'},
    {'1': 'additions', '3': 9, '4': 1, '5': 13, '10': 'additions'},
    {'1': 'deletions', '3': 10, '4': 1, '5': 13, '10': 'deletions'},
    {'1': 'status', '3': 11, '4': 1, '5': 9, '10': 'status'},
  ],
};

/// Descriptor for `ReviewSummary`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewSummaryDescriptor = $convert.base64Decode(
    'Cg1SZXZpZXdTdW1tYXJ5EhsKCXJldmlld19pZBgBIAEoCVIIcmV2aWV3SWQSEgoEcmVwbxgCIA'
    'EoCVIEcmVwbxIbCglwcl9udW1iZXIYAyABKARSCHByTnVtYmVyEhkKCGhlYWRfc2hhGAQgASgJ'
    'UgdoZWFkU2hhEh0KCnJpc2tfc2NvcmUYBSABKAFSCXJpc2tTY29yZRIfCgtnYXRlX2ZhaWxlZB'
    'gGIAEoCFIKZ2F0ZUZhaWxlZBIdCgpuX2ZpbmRpbmdzGAcgASgNUgluRmluZGluZ3MSIwoNZmls'
    'ZXNfY2hhbmdlZBgIIAEoDVIMZmlsZXNDaGFuZ2VkEhwKCWFkZGl0aW9ucxgJIAEoDVIJYWRkaX'
    'Rpb25zEhwKCWRlbGV0aW9ucxgKIAEoDVIJZGVsZXRpb25zEhYKBnN0YXR1cxgLIAEoCVIGc3Rh'
    'dHVz');

@$core.Deprecated('Use listReviewsRequestDescriptor instead')
const ListReviewsRequest$json = {
  '1': 'ListReviewsRequest',
  '2': [
    {'1': 'repo', '3': 1, '4': 1, '5': 9, '10': 'repo'},
    {'1': 'session_id', '3': 2, '4': 1, '5': 9, '10': 'sessionId'},
    {'1': 'status', '3': 3, '4': 1, '5': 9, '10': 'status'},
    {'1': 'limit', '3': 4, '4': 1, '5': 13, '10': 'limit'},
  ],
};

/// Descriptor for `ListReviewsRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List listReviewsRequestDescriptor = $convert.base64Decode(
    'ChJMaXN0UmV2aWV3c1JlcXVlc3QSEgoEcmVwbxgBIAEoCVIEcmVwbxIdCgpzZXNzaW9uX2lkGA'
    'IgASgJUglzZXNzaW9uSWQSFgoGc3RhdHVzGAMgASgJUgZzdGF0dXMSFAoFbGltaXQYBCABKA1S'
    'BWxpbWl0');

@$core.Deprecated('Use listReviewsReplyDescriptor instead')
const ListReviewsReply$json = {
  '1': 'ListReviewsReply',
  '2': [
    {
      '1': 'reviews',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewSummary',
      '10': 'reviews'
    },
  ],
};

/// Descriptor for `ListReviewsReply`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List listReviewsReplyDescriptor = $convert.base64Decode(
    'ChBMaXN0UmV2aWV3c1JlcGx5EjEKB3Jldmlld3MYASADKAsyFy5hZ2VudC52MS5SZXZpZXdTdW'
    '1tYXJ5UgdyZXZpZXdz');

@$core.Deprecated('Use getReviewRequestDescriptor instead')
const GetReviewRequest$json = {
  '1': 'GetReviewRequest',
  '2': [
    {'1': 'review_id', '3': 1, '4': 1, '5': 9, '10': 'reviewId'},
  ],
};

/// Descriptor for `GetReviewRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List getReviewRequestDescriptor = $convert.base64Decode(
    'ChBHZXRSZXZpZXdSZXF1ZXN0EhsKCXJldmlld19pZBgBIAEoCVIIcmV2aWV3SWQ=');

@$core.Deprecated('Use getReviewReplyDescriptor instead')
const GetReviewReply$json = {
  '1': 'GetReviewReply',
  '2': [
    {
      '1': 'meta',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ReviewSummary',
      '10': 'meta'
    },
    {'1': 'body', '3': 2, '4': 1, '5': 9, '10': 'body'},
    {'1': 'truncated', '3': 3, '4': 1, '5': 8, '10': 'truncated'},
  ],
};

/// Descriptor for `GetReviewReply`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List getReviewReplyDescriptor = $convert.base64Decode(
    'Cg5HZXRSZXZpZXdSZXBseRIrCgRtZXRhGAEgASgLMhcuYWdlbnQudjEuUmV2aWV3U3VtbWFyeV'
    'IEbWV0YRISCgRib2R5GAIgASgJUgRib2R5EhwKCXRydW5jYXRlZBgDIAEoCFIJdHJ1bmNhdGVk');

@$core.Deprecated('Use updateReviewRequestDescriptor instead')
const UpdateReviewRequest$json = {
  '1': 'UpdateReviewRequest',
  '2': [
    {'1': 'review_id', '3': 1, '4': 1, '5': 9, '10': 'reviewId'},
    {'1': 'body', '3': 2, '4': 1, '5': 9, '10': 'body'},
  ],
};

/// Descriptor for `UpdateReviewRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List updateReviewRequestDescriptor = $convert.base64Decode(
    'ChNVcGRhdGVSZXZpZXdSZXF1ZXN0EhsKCXJldmlld19pZBgBIAEoCVIIcmV2aWV3SWQSEgoEYm'
    '9keRgCIAEoCVIEYm9keQ==');

@$core.Deprecated('Use updateReviewReplyDescriptor instead')
const UpdateReviewReply$json = {
  '1': 'UpdateReviewReply',
  '2': [
    {'1': 'status', '3': 1, '4': 1, '5': 9, '10': 'status'},
  ],
};

/// Descriptor for `UpdateReviewReply`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List updateReviewReplyDescriptor = $convert.base64Decode(
    'ChFVcGRhdGVSZXZpZXdSZXBseRIWCgZzdGF0dXMYASABKAlSBnN0YXR1cw==');

@$core.Deprecated('Use preflightRequestDescriptor instead')
const PreflightRequest$json = {
  '1': 'PreflightRequest',
};

/// Descriptor for `PreflightRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List preflightRequestDescriptor =
    $convert.base64Decode('ChBQcmVmbGlnaHRSZXF1ZXN0');

@$core.Deprecated('Use preflightProbeDescriptor instead')
const PreflightProbe$json = {
  '1': 'PreflightProbe',
  '2': [
    {'1': 'name', '3': 1, '4': 1, '5': 9, '10': 'name'},
    {'1': 'status', '3': 2, '4': 1, '5': 9, '10': 'status'},
    {'1': 'detail', '3': 3, '4': 1, '5': 9, '10': 'detail'},
    {'1': 'latency_ms', '3': 4, '4': 1, '5': 13, '10': 'latencyMs'},
  ],
};

/// Descriptor for `PreflightProbe`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List preflightProbeDescriptor = $convert.base64Decode(
    'Cg5QcmVmbGlnaHRQcm9iZRISCgRuYW1lGAEgASgJUgRuYW1lEhYKBnN0YXR1cxgCIAEoCVIGc3'
    'RhdHVzEhYKBmRldGFpbBgDIAEoCVIGZGV0YWlsEh0KCmxhdGVuY3lfbXMYBCABKA1SCWxhdGVu'
    'Y3lNcw==');

@$core.Deprecated('Use preflightReplyDescriptor instead')
const PreflightReply$json = {
  '1': 'PreflightReply',
  '2': [
    {'1': 'ok', '3': 1, '4': 1, '5': 8, '10': 'ok'},
    {
      '1': 'probes',
      '3': 2,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.PreflightProbe',
      '10': 'probes'
    },
  ],
};

/// Descriptor for `PreflightReply`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List preflightReplyDescriptor = $convert.base64Decode(
    'Cg5QcmVmbGlnaHRSZXBseRIOCgJvaxgBIAEoCFICb2sSMAoGcHJvYmVzGAIgAygLMhguYWdlbn'
    'QudjEuUHJlZmxpZ2h0UHJvYmVSBnByb2Jlcw==');
