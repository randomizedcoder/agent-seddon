// This is a generated file - do not edit.
//
// Generated from agent/v1/review.proto.

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

@$core.Deprecated('Use reviewCollectStatusDescriptor instead')
const ReviewCollectStatus$json = {
  '1': 'ReviewCollectStatus',
  '2': [
    {'1': 'REVIEW_COLLECT_STATUS_UNSPECIFIED', '2': 0},
    {'1': 'REVIEW_COLLECT_STATUS_OK', '2': 1},
    {'1': 'REVIEW_COLLECT_STATUS_PARTIAL', '2': 2},
    {'1': 'REVIEW_COLLECT_STATUS_SKIPPED', '2': 3},
    {'1': 'REVIEW_COLLECT_STATUS_FAILED', '2': 4},
  ],
};

/// Descriptor for `ReviewCollectStatus`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List reviewCollectStatusDescriptor = $convert.base64Decode(
    'ChNSZXZpZXdDb2xsZWN0U3RhdHVzEiUKIVJFVklFV19DT0xMRUNUX1NUQVRVU19VTlNQRUNJRk'
    'lFRBAAEhwKGFJFVklFV19DT0xMRUNUX1NUQVRVU19PSxABEiEKHVJFVklFV19DT0xMRUNUX1NU'
    'QVRVU19QQVJUSUFMEAISIQodUkVWSUVXX0NPTExFQ1RfU1RBVFVTX1NLSVBQRUQQAxIgChxSRV'
    'ZJRVdfQ09MTEVDVF9TVEFUVVNfRkFJTEVEEAQ=');

@$core.Deprecated('Use reviewForgeHostDescriptor instead')
const ReviewForgeHost$json = {
  '1': 'ReviewForgeHost',
  '2': [
    {'1': 'REVIEW_FORGE_HOST_UNSPECIFIED', '2': 0},
    {'1': 'REVIEW_FORGE_HOST_GITHUB', '2': 1},
    {'1': 'REVIEW_FORGE_HOST_GITLAB', '2': 2},
    {'1': 'REVIEW_FORGE_HOST_OTHER', '2': 3},
    {'1': 'REVIEW_FORGE_HOST_NONE', '2': 4},
  ],
};

/// Descriptor for `ReviewForgeHost`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List reviewForgeHostDescriptor = $convert.base64Decode(
    'Cg9SZXZpZXdGb3JnZUhvc3QSIQodUkVWSUVXX0ZPUkdFX0hPU1RfVU5TUEVDSUZJRUQQABIcCh'
    'hSRVZJRVdfRk9SR0VfSE9TVF9HSVRIVUIQARIcChhSRVZJRVdfRk9SR0VfSE9TVF9HSVRMQUIQ'
    'AhIbChdSRVZJRVdfRk9SR0VfSE9TVF9PVEhFUhADEhoKFlJFVklFV19GT1JHRV9IT1NUX05PTk'
    'UQBA==');

@$core.Deprecated('Use reviewRepoRelationDescriptor instead')
const ReviewRepoRelation$json = {
  '1': 'ReviewRepoRelation',
  '2': [
    {'1': 'REVIEW_REPO_RELATION_UNSPECIFIED', '2': 0},
    {'1': 'REVIEW_REPO_RELATION_CLONE', '2': 1},
    {'1': 'REVIEW_REPO_RELATION_FORK', '2': 2},
    {'1': 'REVIEW_REPO_RELATION_UNKNOWN', '2': 3},
  ],
};

/// Descriptor for `ReviewRepoRelation`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List reviewRepoRelationDescriptor = $convert.base64Decode(
    'ChJSZXZpZXdSZXBvUmVsYXRpb24SJAogUkVWSUVXX1JFUE9fUkVMQVRJT05fVU5TUEVDSUZJRU'
    'QQABIeChpSRVZJRVdfUkVQT19SRUxBVElPTl9DTE9ORRABEh0KGVJFVklFV19SRVBPX1JFTEFU'
    'SU9OX0ZPUksQAhIgChxSRVZJRVdfUkVQT19SRUxBVElPTl9VTktOT1dOEAM=');

@$core.Deprecated('Use reviewRepoLanguageDescriptor instead')
const ReviewRepoLanguage$json = {
  '1': 'ReviewRepoLanguage',
  '2': [
    {'1': 'REVIEW_REPO_LANGUAGE_UNSPECIFIED', '2': 0},
    {'1': 'REVIEW_REPO_LANGUAGE_GO', '2': 1},
    {'1': 'REVIEW_REPO_LANGUAGE_RUST', '2': 2},
    {'1': 'REVIEW_REPO_LANGUAGE_MIXED', '2': 3},
    {'1': 'REVIEW_REPO_LANGUAGE_UNKNOWN', '2': 4},
  ],
};

/// Descriptor for `ReviewRepoLanguage`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List reviewRepoLanguageDescriptor = $convert.base64Decode(
    'ChJSZXZpZXdSZXBvTGFuZ3VhZ2USJAogUkVWSUVXX1JFUE9fTEFOR1VBR0VfVU5TUEVDSUZJRU'
    'QQABIbChdSRVZJRVdfUkVQT19MQU5HVUFHRV9HTxABEh0KGVJFVklFV19SRVBPX0xBTkdVQUdF'
    'X1JVU1QQAhIeChpSRVZJRVdfUkVQT19MQU5HVUFHRV9NSVhFRBADEiAKHFJFVklFV19SRVBPX0'
    'xBTkdVQUdFX1VOS05PV04QBA==');

@$core.Deprecated('Use reviewCollectRequestDescriptor instead')
const ReviewCollectRequest$json = {
  '1': 'ReviewCollectRequest',
  '2': [
    {'1': 'target', '3': 1, '4': 1, '5': 9, '10': 'target'},
  ],
};

/// Descriptor for `ReviewCollectRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewCollectRequestDescriptor =
    $convert.base64Decode(
        'ChRSZXZpZXdDb2xsZWN0UmVxdWVzdBIWCgZ0YXJnZXQYASABKAlSBnRhcmdldA==');

@$core.Deprecated('Use reviewCollectorStatusDescriptor instead')
const ReviewCollectorStatus$json = {
  '1': 'ReviewCollectorStatus',
  '2': [
    {'1': 'collector', '3': 1, '4': 1, '5': 9, '10': 'collector'},
    {
      '1': 'status',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ReviewCollectStatus',
      '10': 'status'
    },
    {'1': 'reason', '3': 3, '4': 1, '5': 9, '10': 'reason'},
    {'1': 'duration_ms', '3': 4, '4': 1, '5': 13, '10': 'durationMs'},
  ],
};

/// Descriptor for `ReviewCollectorStatus`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewCollectorStatusDescriptor = $convert.base64Decode(
    'ChVSZXZpZXdDb2xsZWN0b3JTdGF0dXMSHAoJY29sbGVjdG9yGAEgASgJUgljb2xsZWN0b3ISNQ'
    'oGc3RhdHVzGAIgASgOMh0uYWdlbnQudjEuUmV2aWV3Q29sbGVjdFN0YXR1c1IGc3RhdHVzEhYK'
    'BnJlYXNvbhgDIAEoCVIGcmVhc29uEh8KC2R1cmF0aW9uX21zGAQgASgNUgpkdXJhdGlvbk1z');

@$core.Deprecated('Use reviewMetaDescriptor instead')
const ReviewMeta$json = {
  '1': 'ReviewMeta',
  '2': [
    {'1': 'repo_hash', '3': 1, '4': 1, '5': 9, '10': 'repoHash'},
    {'1': 'base_rev', '3': 2, '4': 1, '5': 9, '10': 'baseRev'},
    {'1': 'head_rev', '3': 3, '4': 1, '5': 9, '10': 'headRev'},
    {'1': 'total_ms', '3': 4, '4': 1, '5': 13, '10': 'totalMs'},
    {
      '1': 'collectors',
      '3': 5,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewCollectorStatus',
      '10': 'collectors'
    },
  ],
};

/// Descriptor for `ReviewMeta`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewMetaDescriptor = $convert.base64Decode(
    'CgpSZXZpZXdNZXRhEhsKCXJlcG9faGFzaBgBIAEoCVIIcmVwb0hhc2gSGQoIYmFzZV9yZXYYAi'
    'ABKAlSB2Jhc2VSZXYSGQoIaGVhZF9yZXYYAyABKAlSB2hlYWRSZXYSGQoIdG90YWxfbXMYBCAB'
    'KA1SB3RvdGFsTXMSPwoKY29sbGVjdG9ycxgFIAMoCzIfLmFnZW50LnYxLlJldmlld0NvbGxlY3'
    'RvclN0YXR1c1IKY29sbGVjdG9ycw==');

@$core.Deprecated('Use reviewChangedFileDescriptor instead')
const ReviewChangedFile$json = {
  '1': 'ReviewChangedFile',
  '2': [
    {'1': 'path', '3': 1, '4': 1, '5': 9, '10': 'path'},
    {
      '1': 'change',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ChangeKind',
      '10': 'change'
    },
    {'1': 'additions', '3': 3, '4': 1, '5': 13, '10': 'additions'},
    {'1': 'deletions', '3': 4, '4': 1, '5': 13, '10': 'deletions'},
    {'1': 'is_binary', '3': 5, '4': 1, '5': 8, '10': 'isBinary'},
    {'1': 'lang', '3': 6, '4': 1, '5': 9, '10': 'lang'},
    {'1': 'patch', '3': 7, '4': 1, '5': 9, '10': 'patch'},
  ],
};

/// Descriptor for `ReviewChangedFile`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewChangedFileDescriptor = $convert.base64Decode(
    'ChFSZXZpZXdDaGFuZ2VkRmlsZRISCgRwYXRoGAEgASgJUgRwYXRoEiwKBmNoYW5nZRgCIAEoDj'
    'IULmFnZW50LnYxLkNoYW5nZUtpbmRSBmNoYW5nZRIcCglhZGRpdGlvbnMYAyABKA1SCWFkZGl0'
    'aW9ucxIcCglkZWxldGlvbnMYBCABKA1SCWRlbGV0aW9ucxIbCglpc19iaW5hcnkYBSABKAhSCG'
    'lzQmluYXJ5EhIKBGxhbmcYBiABKAlSBGxhbmcSFAoFcGF0Y2gYByABKAlSBXBhdGNo');

@$core.Deprecated('Use reviewCommitDescriptor instead')
const ReviewCommit$json = {
  '1': 'ReviewCommit',
  '2': [
    {'1': 'short', '3': 1, '4': 1, '5': 9, '10': 'short'},
    {'1': 'summary', '3': 2, '4': 1, '5': 9, '10': 'summary'},
    {'1': 'body', '3': 3, '4': 1, '5': 9, '10': 'body'},
    {'1': 'author', '3': 4, '4': 1, '5': 9, '10': 'author'},
    {'1': 'age_days', '3': 5, '4': 1, '5': 13, '10': 'ageDays'},
  ],
};

/// Descriptor for `ReviewCommit`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewCommitDescriptor = $convert.base64Decode(
    'CgxSZXZpZXdDb21taXQSFAoFc2hvcnQYASABKAlSBXNob3J0EhgKB3N1bW1hcnkYAiABKAlSB3'
    'N1bW1hcnkSEgoEYm9keRgDIAEoCVIEYm9keRIWCgZhdXRob3IYBCABKAlSBmF1dGhvchIZCghh'
    'Z2VfZGF5cxgFIAEoDVIHYWdlRGF5cw==');

@$core.Deprecated('Use reviewChangeSetDescriptor instead')
const ReviewChangeSet$json = {
  '1': 'ReviewChangeSet',
  '2': [
    {'1': 'base_rev', '3': 1, '4': 1, '5': 9, '10': 'baseRev'},
    {'1': 'head_rev', '3': 2, '4': 1, '5': 9, '10': 'headRev'},
    {
      '1': 'files',
      '3': 3,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewChangedFile',
      '10': 'files'
    },
    {'1': 'repo_file_count', '3': 4, '4': 1, '5': 13, '10': 'repoFileCount'},
    {
      '1': 'commits',
      '3': 5,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewCommit',
      '10': 'commits'
    },
  ],
};

/// Descriptor for `ReviewChangeSet`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewChangeSetDescriptor = $convert.base64Decode(
    'Cg9SZXZpZXdDaGFuZ2VTZXQSGQoIYmFzZV9yZXYYASABKAlSB2Jhc2VSZXYSGQoIaGVhZF9yZX'
    'YYAiABKAlSB2hlYWRSZXYSMQoFZmlsZXMYAyADKAsyGy5hZ2VudC52MS5SZXZpZXdDaGFuZ2Vk'
    'RmlsZVIFZmlsZXMSJgoPcmVwb19maWxlX2NvdW50GAQgASgNUg1yZXBvRmlsZUNvdW50EjAKB2'
    'NvbW1pdHMYBSADKAsyFi5hZ2VudC52MS5SZXZpZXdDb21taXRSB2NvbW1pdHM=');

@$core.Deprecated('Use reviewGitStateDescriptor instead')
const ReviewGitState$json = {
  '1': 'ReviewGitState',
  '2': [
    {'1': 'remote_url_hash', '3': 1, '4': 1, '5': 9, '10': 'remoteUrlHash'},
    {
      '1': 'host',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ReviewForgeHost',
      '10': 'host'
    },
    {
      '1': 'relationship',
      '3': 3,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ReviewRepoRelation',
      '10': 'relationship'
    },
    {'1': 'default_branch', '3': 4, '4': 1, '5': 9, '10': 'defaultBranch'},
    {
      '1': 'project',
      '3': 5,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ReviewRepoLanguage',
      '10': 'project'
    },
  ],
};

/// Descriptor for `ReviewGitState`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewGitStateDescriptor = $convert.base64Decode(
    'Cg5SZXZpZXdHaXRTdGF0ZRImCg9yZW1vdGVfdXJsX2hhc2gYASABKAlSDXJlbW90ZVVybEhhc2'
    'gSLQoEaG9zdBgCIAEoDjIZLmFnZW50LnYxLlJldmlld0ZvcmdlSG9zdFIEaG9zdBJACgxyZWxh'
    'dGlvbnNoaXAYAyABKA4yHC5hZ2VudC52MS5SZXZpZXdSZXBvUmVsYXRpb25SDHJlbGF0aW9uc2'
    'hpcBIlCg5kZWZhdWx0X2JyYW5jaBgEIAEoCVINZGVmYXVsdEJyYW5jaBI2Cgdwcm9qZWN0GAUg'
    'ASgOMhwuYWdlbnQudjEuUmV2aWV3UmVwb0xhbmd1YWdlUgdwcm9qZWN0');

@$core.Deprecated('Use reviewAnalysisFindingDescriptor instead')
const ReviewAnalysisFinding$json = {
  '1': 'ReviewAnalysisFinding',
  '2': [
    {'1': 'tool', '3': 1, '4': 1, '5': 9, '10': 'tool'},
    {'1': 'rule', '3': 2, '4': 1, '5': 9, '10': 'rule'},
    {'1': 'severity', '3': 3, '4': 1, '5': 9, '10': 'severity'},
    {'1': 'file', '3': 4, '4': 1, '5': 9, '10': 'file'},
    {'1': 'line', '3': 5, '4': 1, '5': 13, '10': 'line'},
    {'1': 'message', '3': 6, '4': 1, '5': 9, '10': 'message'},
    {'1': 'in_change', '3': 7, '4': 1, '5': 8, '10': 'inChange'},
  ],
};

/// Descriptor for `ReviewAnalysisFinding`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewAnalysisFindingDescriptor = $convert.base64Decode(
    'ChVSZXZpZXdBbmFseXNpc0ZpbmRpbmcSEgoEdG9vbBgBIAEoCVIEdG9vbBISCgRydWxlGAIgAS'
    'gJUgRydWxlEhoKCHNldmVyaXR5GAMgASgJUghzZXZlcml0eRISCgRmaWxlGAQgASgJUgRmaWxl'
    'EhIKBGxpbmUYBSABKA1SBGxpbmUSGAoHbWVzc2FnZRgGIAEoCVIHbWVzc2FnZRIbCglpbl9jaG'
    'FuZ2UYByABKAhSCGluQ2hhbmdl');

@$core.Deprecated('Use reviewAnalyzerRunDescriptor instead')
const ReviewAnalyzerRun$json = {
  '1': 'ReviewAnalyzerRun',
  '2': [
    {'1': 'tool', '3': 1, '4': 1, '5': 9, '10': 'tool'},
    {'1': 'status', '3': 2, '4': 1, '5': 9, '10': 'status'},
    {'1': 'reason', '3': 3, '4': 1, '5': 9, '10': 'reason'},
    {'1': 'duration_ms', '3': 4, '4': 1, '5': 13, '10': 'durationMs'},
    {'1': 'finding_count', '3': 5, '4': 1, '5': 13, '10': 'findingCount'},
  ],
};

/// Descriptor for `ReviewAnalyzerRun`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewAnalyzerRunDescriptor = $convert.base64Decode(
    'ChFSZXZpZXdBbmFseXplclJ1bhISCgR0b29sGAEgASgJUgR0b29sEhYKBnN0YXR1cxgCIAEoCV'
    'IGc3RhdHVzEhYKBnJlYXNvbhgDIAEoCVIGcmVhc29uEh8KC2R1cmF0aW9uX21zGAQgASgNUgpk'
    'dXJhdGlvbk1zEiMKDWZpbmRpbmdfY291bnQYBSABKA1SDGZpbmRpbmdDb3VudA==');

@$core.Deprecated('Use reviewAnalysisReportDescriptor instead')
const ReviewAnalysisReport$json = {
  '1': 'ReviewAnalysisReport',
  '2': [
    {'1': 'language', '3': 1, '4': 1, '5': 9, '10': 'language'},
    {
      '1': 'runs',
      '3': 2,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewAnalyzerRun',
      '10': 'runs'
    },
    {
      '1': 'findings',
      '3': 3,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewAnalysisFinding',
      '10': 'findings'
    },
  ],
};

/// Descriptor for `ReviewAnalysisReport`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewAnalysisReportDescriptor = $convert.base64Decode(
    'ChRSZXZpZXdBbmFseXNpc1JlcG9ydBIaCghsYW5ndWFnZRgBIAEoCVIIbGFuZ3VhZ2USLwoEcn'
    'VucxgCIAMoCzIbLmFnZW50LnYxLlJldmlld0FuYWx5emVyUnVuUgRydW5zEjsKCGZpbmRpbmdz'
    'GAMgAygLMh8uYWdlbnQudjEuUmV2aWV3QW5hbHlzaXNGaW5kaW5nUghmaW5kaW5ncw==');

@$core.Deprecated('Use reviewSignatureChangeDescriptor instead')
const ReviewSignatureChange$json = {
  '1': 'ReviewSignatureChange',
  '2': [
    {'1': 'file', '3': 1, '4': 1, '5': 9, '10': 'file'},
    {'1': 'lang', '3': 2, '4': 1, '5': 9, '10': 'lang'},
    {'1': 'kind', '3': 3, '4': 1, '5': 9, '10': 'kind'},
    {'1': 'name', '3': 4, '4': 1, '5': 9, '10': 'name'},
    {'1': 'before', '3': 5, '4': 1, '5': 9, '10': 'before'},
    {'1': 'after', '3': 6, '4': 1, '5': 9, '10': 'after'},
  ],
};

/// Descriptor for `ReviewSignatureChange`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewSignatureChangeDescriptor = $convert.base64Decode(
    'ChVSZXZpZXdTaWduYXR1cmVDaGFuZ2USEgoEZmlsZRgBIAEoCVIEZmlsZRISCgRsYW5nGAIgAS'
    'gJUgRsYW5nEhIKBGtpbmQYAyABKAlSBGtpbmQSEgoEbmFtZRgEIAEoCVIEbmFtZRIWCgZiZWZv'
    'cmUYBSABKAlSBmJlZm9yZRIUCgVhZnRlchgGIAEoCVIFYWZ0ZXI=');

@$core.Deprecated('Use reviewSignatureReportDescriptor instead')
const ReviewSignatureReport$json = {
  '1': 'ReviewSignatureReport',
  '2': [
    {
      '1': 'changes',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewSignatureChange',
      '10': 'changes'
    },
    {'1': 'files_scanned', '3': 2, '4': 1, '5': 13, '10': 'filesScanned'},
    {'1': 'truncated', '3': 3, '4': 1, '5': 8, '10': 'truncated'},
  ],
};

/// Descriptor for `ReviewSignatureReport`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewSignatureReportDescriptor = $convert.base64Decode(
    'ChVSZXZpZXdTaWduYXR1cmVSZXBvcnQSOQoHY2hhbmdlcxgBIAMoCzIfLmFnZW50LnYxLlJldm'
    'lld1NpZ25hdHVyZUNoYW5nZVIHY2hhbmdlcxIjCg1maWxlc19zY2FubmVkGAIgASgNUgxmaWxl'
    'c1NjYW5uZWQSHAoJdHJ1bmNhdGVkGAMgASgIUgl0cnVuY2F0ZWQ=');

@$core.Deprecated('Use reviewCallGraphNodeDescriptor instead')
const ReviewCallGraphNode$json = {
  '1': 'ReviewCallGraphNode',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 13, '10': 'id'},
    {'1': 'package', '3': 2, '4': 1, '5': 9, '10': 'package'},
    {'1': 'name', '3': 3, '4': 1, '5': 9, '10': 'name'},
    {'1': 'exported', '3': 4, '4': 1, '5': 8, '10': 'exported'},
    {'1': 'file', '3': 5, '4': 1, '5': 9, '10': 'file'},
    {'1': 'line', '3': 6, '4': 1, '5': 13, '10': 'line'},
    {'1': 'centrality', '3': 7, '4': 1, '5': 1, '10': 'centrality'},
  ],
};

/// Descriptor for `ReviewCallGraphNode`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewCallGraphNodeDescriptor = $convert.base64Decode(
    'ChNSZXZpZXdDYWxsR3JhcGhOb2RlEg4KAmlkGAEgASgNUgJpZBIYCgdwYWNrYWdlGAIgASgJUg'
    'dwYWNrYWdlEhIKBG5hbWUYAyABKAlSBG5hbWUSGgoIZXhwb3J0ZWQYBCABKAhSCGV4cG9ydGVk'
    'EhIKBGZpbGUYBSABKAlSBGZpbGUSEgoEbGluZRgGIAEoDVIEbGluZRIeCgpjZW50cmFsaXR5GA'
    'cgASgBUgpjZW50cmFsaXR5');

@$core.Deprecated('Use reviewCallEdgeDescriptor instead')
const ReviewCallEdge$json = {
  '1': 'ReviewCallEdge',
  '2': [
    {'1': 'caller_id', '3': 1, '4': 1, '5': 13, '10': 'callerId'},
    {'1': 'callee_id', '3': 2, '4': 1, '5': 13, '10': 'calleeId'},
  ],
};

/// Descriptor for `ReviewCallEdge`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewCallEdgeDescriptor = $convert.base64Decode(
    'Cg5SZXZpZXdDYWxsRWRnZRIbCgljYWxsZXJfaWQYASABKA1SCGNhbGxlcklkEhsKCWNhbGxlZV'
    '9pZBgCIAEoDVIIY2FsbGVlSWQ=');

@$core.Deprecated('Use reviewPackageShapeDescriptor instead')
const ReviewPackageShape$json = {
  '1': 'ReviewPackageShape',
  '2': [
    {'1': 'package', '3': 1, '4': 1, '5': 9, '10': 'package'},
    {'1': 'files', '3': 2, '4': 1, '5': 13, '10': 'files'},
    {'1': 'exported_fns', '3': 3, '4': 1, '5': 13, '10': 'exportedFns'},
    {'1': 'types', '3': 4, '4': 1, '5': 13, '10': 'types'},
  ],
};

/// Descriptor for `ReviewPackageShape`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewPackageShapeDescriptor = $convert.base64Decode(
    'ChJSZXZpZXdQYWNrYWdlU2hhcGUSGAoHcGFja2FnZRgBIAEoCVIHcGFja2FnZRIUCgVmaWxlcx'
    'gCIAEoDVIFZmlsZXMSIQoMZXhwb3J0ZWRfZm5zGAMgASgNUgtleHBvcnRlZEZucxIUCgV0eXBl'
    'cxgEIAEoDVIFdHlwZXM=');

@$core.Deprecated('Use reviewCallGraphDescriptor instead')
const ReviewCallGraph$json = {
  '1': 'ReviewCallGraph',
  '2': [
    {
      '1': 'nodes',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewCallGraphNode',
      '10': 'nodes'
    },
    {
      '1': 'edges',
      '3': 2,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewCallEdge',
      '10': 'edges'
    },
    {'1': 'changed_fns', '3': 3, '4': 3, '5': 13, '10': 'changedFns'},
    {
      '1': 'packages',
      '3': 4,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewPackageShape',
      '10': 'packages'
    },
    {'1': 'truncated', '3': 5, '4': 1, '5': 8, '10': 'truncated'},
  ],
};

/// Descriptor for `ReviewCallGraph`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewCallGraphDescriptor = $convert.base64Decode(
    'Cg9SZXZpZXdDYWxsR3JhcGgSMwoFbm9kZXMYASADKAsyHS5hZ2VudC52MS5SZXZpZXdDYWxsR3'
    'JhcGhOb2RlUgVub2RlcxIuCgVlZGdlcxgCIAMoCzIYLmFnZW50LnYxLlJldmlld0NhbGxFZGdl'
    'UgVlZGdlcxIfCgtjaGFuZ2VkX2ZucxgDIAMoDVIKY2hhbmdlZEZucxI4CghwYWNrYWdlcxgEIA'
    'MoCzIcLmFnZW50LnYxLlJldmlld1BhY2thZ2VTaGFwZVIIcGFja2FnZXMSHAoJdHJ1bmNhdGVk'
    'GAUgASgIUgl0cnVuY2F0ZWQ=');

@$core.Deprecated('Use reviewNamingFactsDescriptor instead')
const ReviewNamingFacts$json = {
  '1': 'ReviewNamingFacts',
  '2': [
    {'1': 'functions', '3': 1, '4': 1, '5': 9, '10': 'functions'},
    {'1': 'variables', '3': 2, '4': 1, '5': 9, '10': 'variables'},
    {'1': 'constants', '3': 3, '4': 1, '5': 9, '10': 'constants'},
    {'1': 'exported_ratio', '3': 4, '4': 1, '5': 2, '10': 'exportedRatio'},
  ],
};

/// Descriptor for `ReviewNamingFacts`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewNamingFactsDescriptor = $convert.base64Decode(
    'ChFSZXZpZXdOYW1pbmdGYWN0cxIcCglmdW5jdGlvbnMYASABKAlSCWZ1bmN0aW9ucxIcCgl2YX'
    'JpYWJsZXMYAiABKAlSCXZhcmlhYmxlcxIcCgljb25zdGFudHMYAyABKAlSCWNvbnN0YW50cxIl'
    'Cg5leHBvcnRlZF9yYXRpbxgEIAEoAlINZXhwb3J0ZWRSYXRpbw==');

@$core.Deprecated('Use reviewCommitStyleFactsDescriptor instead')
const ReviewCommitStyleFacts$json = {
  '1': 'ReviewCommitStyleFacts',
  '2': [
    {
      '1': 'conventional_ratio',
      '3': 1,
      '4': 1,
      '5': 2,
      '10': 'conventionalRatio'
    },
    {'1': 'subject_len_p50', '3': 2, '4': 1, '5': 13, '10': 'subjectLenP50'},
    {'1': 'subject_len_p95', '3': 3, '4': 1, '5': 13, '10': 'subjectLenP95'},
    {
      '1': 'body_present_ratio',
      '3': 4,
      '4': 1,
      '5': 2,
      '10': 'bodyPresentRatio'
    },
    {'1': 'sampled_commits', '3': 5, '4': 1, '5': 13, '10': 'sampledCommits'},
  ],
};

/// Descriptor for `ReviewCommitStyleFacts`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewCommitStyleFactsDescriptor = $convert.base64Decode(
    'ChZSZXZpZXdDb21taXRTdHlsZUZhY3RzEi0KEmNvbnZlbnRpb25hbF9yYXRpbxgBIAEoAlIRY2'
    '9udmVudGlvbmFsUmF0aW8SJgoPc3ViamVjdF9sZW5fcDUwGAIgASgNUg1zdWJqZWN0TGVuUDUw'
    'EiYKD3N1YmplY3RfbGVuX3A5NRgDIAEoDVINc3ViamVjdExlblA5NRIsChJib2R5X3ByZXNlbn'
    'RfcmF0aW8YBCABKAJSEGJvZHlQcmVzZW50UmF0aW8SJwoPc2FtcGxlZF9jb21taXRzGAUgASgN'
    'Ug5zYW1wbGVkQ29tbWl0cw==');

@$core.Deprecated('Use reviewStyleFactsDescriptor instead')
const ReviewStyleFacts$json = {
  '1': 'ReviewStyleFacts',
  '2': [
    {'1': 'comment_density', '3': 1, '4': 1, '5': 2, '10': 'commentDensity'},
    {'1': 'doccomment_ratio', '3': 2, '4': 1, '5': 2, '10': 'doccommentRatio'},
    {'1': 'indent_tabs', '3': 3, '4': 1, '5': 8, '10': 'indentTabs'},
    {'1': 'line_len_p95', '3': 4, '4': 1, '5': 13, '10': 'lineLenP95'},
    {'1': 'fn_len_median', '3': 5, '4': 1, '5': 13, '10': 'fnLenMedian'},
    {
      '1': 'naming',
      '3': 6,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ReviewNamingFacts',
      '10': 'naming'
    },
    {
      '1': 'commits',
      '3': 7,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ReviewCommitStyleFacts',
      '10': 'commits'
    },
    {
      '1': 'diff_matches_style',
      '3': 8,
      '4': 1,
      '5': 8,
      '10': 'diffMatchesStyle'
    },
    {'1': 'files_scanned', '3': 9, '4': 1, '5': 13, '10': 'filesScanned'},
  ],
};

/// Descriptor for `ReviewStyleFacts`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewStyleFactsDescriptor = $convert.base64Decode(
    'ChBSZXZpZXdTdHlsZUZhY3RzEicKD2NvbW1lbnRfZGVuc2l0eRgBIAEoAlIOY29tbWVudERlbn'
    'NpdHkSKQoQZG9jY29tbWVudF9yYXRpbxgCIAEoAlIPZG9jY29tbWVudFJhdGlvEh8KC2luZGVu'
    'dF90YWJzGAMgASgIUgppbmRlbnRUYWJzEiAKDGxpbmVfbGVuX3A5NRgEIAEoDVIKbGluZUxlbl'
    'A5NRIiCg1mbl9sZW5fbWVkaWFuGAUgASgNUgtmbkxlbk1lZGlhbhIzCgZuYW1pbmcYBiABKAsy'
    'Gy5hZ2VudC52MS5SZXZpZXdOYW1pbmdGYWN0c1IGbmFtaW5nEjoKB2NvbW1pdHMYByABKAsyIC'
    '5hZ2VudC52MS5SZXZpZXdDb21taXRTdHlsZUZhY3RzUgdjb21taXRzEiwKEmRpZmZfbWF0Y2hl'
    'c19zdHlsZRgIIAEoCFIQZGlmZk1hdGNoZXNTdHlsZRIjCg1maWxlc19zY2FubmVkGAkgASgNUg'
    'xmaWxlc1NjYW5uZWQ=');

@$core.Deprecated('Use reviewFunctionSummaryDescriptor instead')
const ReviewFunctionSummary$json = {
  '1': 'ReviewFunctionSummary',
  '2': [
    {'1': 'name', '3': 1, '4': 1, '5': 9, '10': 'name'},
    {'1': 'file', '3': 2, '4': 1, '5': 9, '10': 'file'},
    {'1': 'kind', '3': 3, '4': 1, '5': 9, '10': 'kind'},
    {'1': 'summary', '3': 4, '4': 1, '5': 9, '10': 'summary'},
    {'1': 'model', '3': 5, '4': 1, '5': 9, '10': 'model'},
    {'1': 'duration_ms', '3': 6, '4': 1, '5': 13, '10': 'durationMs'},
  ],
};

/// Descriptor for `ReviewFunctionSummary`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewFunctionSummaryDescriptor = $convert.base64Decode(
    'ChVSZXZpZXdGdW5jdGlvblN1bW1hcnkSEgoEbmFtZRgBIAEoCVIEbmFtZRISCgRmaWxlGAIgAS'
    'gJUgRmaWxlEhIKBGtpbmQYAyABKAlSBGtpbmQSGAoHc3VtbWFyeRgEIAEoCVIHc3VtbWFyeRIU'
    'CgVtb2RlbBgFIAEoCVIFbW9kZWwSHwoLZHVyYXRpb25fbXMYBiABKA1SCmR1cmF0aW9uTXM=');

@$core.Deprecated('Use reviewSummaryReportDescriptor instead')
const ReviewSummaryReport$json = {
  '1': 'ReviewSummaryReport',
  '2': [
    {
      '1': 'summaries',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewFunctionSummary',
      '10': 'summaries'
    },
    {'1': 'requested', '3': 2, '4': 1, '5': 13, '10': 'requested'},
    {'1': 'produced', '3': 3, '4': 1, '5': 13, '10': 'produced'},
    {'1': 'omitted', '3': 4, '4': 1, '5': 13, '10': 'omitted'},
  ],
};

/// Descriptor for `ReviewSummaryReport`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewSummaryReportDescriptor = $convert.base64Decode(
    'ChNSZXZpZXdTdW1tYXJ5UmVwb3J0Ej0KCXN1bW1hcmllcxgBIAMoCzIfLmFnZW50LnYxLlJldm'
    'lld0Z1bmN0aW9uU3VtbWFyeVIJc3VtbWFyaWVzEhwKCXJlcXVlc3RlZBgCIAEoDVIJcmVxdWVz'
    'dGVkEhoKCHByb2R1Y2VkGAMgASgNUghwcm9kdWNlZBIYCgdvbWl0dGVkGAQgASgNUgdvbWl0dG'
    'Vk');

@$core.Deprecated('Use reviewCoChangePartnerDescriptor instead')
const ReviewCoChangePartner$json = {
  '1': 'ReviewCoChangePartner',
  '2': [
    {'1': 'path', '3': 1, '4': 1, '5': 9, '10': 'path'},
    {'1': 'confidence', '3': 2, '4': 1, '5': 1, '10': 'confidence'},
    {'1': 'co_occurrences', '3': 3, '4': 1, '5': 13, '10': 'coOccurrences'},
    {'1': 'in_diff', '3': 4, '4': 1, '5': 8, '10': 'inDiff'},
  ],
};

/// Descriptor for `ReviewCoChangePartner`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewCoChangePartnerDescriptor = $convert.base64Decode(
    'ChVSZXZpZXdDb0NoYW5nZVBhcnRuZXISEgoEcGF0aBgBIAEoCVIEcGF0aBIeCgpjb25maWRlbm'
    'NlGAIgASgBUgpjb25maWRlbmNlEiUKDmNvX29jY3VycmVuY2VzGAMgASgNUg1jb09jY3VycmVu'
    'Y2VzEhcKB2luX2RpZmYYBCABKAhSBmluRGlmZg==');

@$core.Deprecated('Use reviewCoChangeEntryDescriptor instead')
const ReviewCoChangeEntry$json = {
  '1': 'ReviewCoChangeEntry',
  '2': [
    {'1': 'path', '3': 1, '4': 1, '5': 9, '10': 'path'},
    {
      '1': 'partners',
      '3': 2,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewCoChangePartner',
      '10': 'partners'
    },
  ],
};

/// Descriptor for `ReviewCoChangeEntry`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewCoChangeEntryDescriptor = $convert.base64Decode(
    'ChNSZXZpZXdDb0NoYW5nZUVudHJ5EhIKBHBhdGgYASABKAlSBHBhdGgSOwoIcGFydG5lcnMYAi'
    'ADKAsyHy5hZ2VudC52MS5SZXZpZXdDb0NoYW5nZVBhcnRuZXJSCHBhcnRuZXJz');

@$core.Deprecated('Use reviewCoChangeReportDescriptor instead')
const ReviewCoChangeReport$json = {
  '1': 'ReviewCoChangeReport',
  '2': [
    {'1': 'commits_scanned', '3': 1, '4': 1, '5': 13, '10': 'commitsScanned'},
    {'1': 'truncated', '3': 2, '4': 1, '5': 8, '10': 'truncated'},
    {
      '1': 'entries',
      '3': 3,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewCoChangeEntry',
      '10': 'entries'
    },
    {'1': 'missing_partners', '3': 4, '4': 1, '5': 13, '10': 'missingPartners'},
  ],
};

/// Descriptor for `ReviewCoChangeReport`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewCoChangeReportDescriptor = $convert.base64Decode(
    'ChRSZXZpZXdDb0NoYW5nZVJlcG9ydBInCg9jb21taXRzX3NjYW5uZWQYASABKA1SDmNvbW1pdH'
    'NTY2FubmVkEhwKCXRydW5jYXRlZBgCIAEoCFIJdHJ1bmNhdGVkEjcKB2VudHJpZXMYAyADKAsy'
    'HS5hZ2VudC52MS5SZXZpZXdDb0NoYW5nZUVudHJ5UgdlbnRyaWVzEikKEG1pc3NpbmdfcGFydG'
    '5lcnMYBCABKA1SD21pc3NpbmdQYXJ0bmVycw==');

@$core.Deprecated('Use reviewFileChurnDescriptor instead')
const ReviewFileChurn$json = {
  '1': 'ReviewFileChurn',
  '2': [
    {'1': 'path', '3': 1, '4': 1, '5': 9, '10': 'path'},
    {'1': 'commits', '3': 2, '4': 1, '5': 13, '10': 'commits'},
    {'1': 'unique_authors', '3': 3, '4': 1, '5': 13, '10': 'uniqueAuthors'},
    {'1': 'bus_factor', '3': 4, '4': 1, '5': 13, '10': 'busFactor'},
    {'1': 'top_author_share', '3': 5, '4': 1, '5': 1, '10': 'topAuthorShare'},
    {'1': 'churn_trend', '3': 6, '4': 1, '5': 9, '10': 'churnTrend'},
    {'1': 'churn_slope', '3': 7, '4': 1, '5': 1, '10': 'churnSlope'},
    {'1': 'total_churn', '3': 8, '4': 1, '5': 4, '10': 'totalChurn'},
  ],
};

/// Descriptor for `ReviewFileChurn`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewFileChurnDescriptor = $convert.base64Decode(
    'Cg9SZXZpZXdGaWxlQ2h1cm4SEgoEcGF0aBgBIAEoCVIEcGF0aBIYCgdjb21taXRzGAIgASgNUg'
    'djb21taXRzEiUKDnVuaXF1ZV9hdXRob3JzGAMgASgNUg11bmlxdWVBdXRob3JzEh0KCmJ1c19m'
    'YWN0b3IYBCABKA1SCWJ1c0ZhY3RvchIoChB0b3BfYXV0aG9yX3NoYXJlGAUgASgBUg50b3BBdX'
    'Rob3JTaGFyZRIfCgtjaHVybl90cmVuZBgGIAEoCVIKY2h1cm5UcmVuZBIfCgtjaHVybl9zbG9w'
    'ZRgHIAEoAVIKY2h1cm5TbG9wZRIfCgt0b3RhbF9jaHVybhgIIAEoBFIKdG90YWxDaHVybg==');

@$core.Deprecated('Use reviewChurnReportDescriptor instead')
const ReviewChurnReport$json = {
  '1': 'ReviewChurnReport',
  '2': [
    {'1': 'commits_scanned', '3': 1, '4': 1, '5': 13, '10': 'commitsScanned'},
    {
      '1': 'files',
      '3': 2,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewFileChurn',
      '10': 'files'
    },
  ],
};

/// Descriptor for `ReviewChurnReport`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewChurnReportDescriptor = $convert.base64Decode(
    'ChFSZXZpZXdDaHVyblJlcG9ydBInCg9jb21taXRzX3NjYW5uZWQYASABKA1SDmNvbW1pdHNTY2'
    'FubmVkEi8KBWZpbGVzGAIgAygLMhkuYWdlbnQudjEuUmV2aWV3RmlsZUNodXJuUgVmaWxlcw==');

@$core.Deprecated('Use reviewFileSalienceDescriptor instead')
const ReviewFileSalience$json = {
  '1': 'ReviewFileSalience',
  '2': [
    {'1': 'file', '3': 1, '4': 1, '5': 9, '10': 'file'},
    {'1': 'centrality', '3': 2, '4': 1, '5': 1, '10': 'centrality'},
    {'1': 'bus_factor', '3': 3, '4': 1, '5': 13, '10': 'busFactor'},
    {'1': 'churn_increasing', '3': 4, '4': 1, '5': 8, '10': 'churnIncreasing'},
    {'1': 'class', '3': 5, '4': 1, '5': 9, '10': 'class'},
  ],
};

/// Descriptor for `ReviewFileSalience`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewFileSalienceDescriptor = $convert.base64Decode(
    'ChJSZXZpZXdGaWxlU2FsaWVuY2USEgoEZmlsZRgBIAEoCVIEZmlsZRIeCgpjZW50cmFsaXR5GA'
    'IgASgBUgpjZW50cmFsaXR5Eh0KCmJ1c19mYWN0b3IYAyABKA1SCWJ1c0ZhY3RvchIpChBjaHVy'
    'bl9pbmNyZWFzaW5nGAQgASgIUg9jaHVybkluY3JlYXNpbmcSFAoFY2xhc3MYBSABKAlSBWNsYX'
    'Nz');

@$core.Deprecated('Use reviewSalienceReportDescriptor instead')
const ReviewSalienceReport$json = {
  '1': 'ReviewSalienceReport',
  '2': [
    {
      '1': 'files',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewFileSalience',
      '10': 'files'
    },
  ],
};

/// Descriptor for `ReviewSalienceReport`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewSalienceReportDescriptor = $convert.base64Decode(
    'ChRSZXZpZXdTYWxpZW5jZVJlcG9ydBIyCgVmaWxlcxgBIAMoCzIcLmFnZW50LnYxLlJldmlld0'
    'ZpbGVTYWxpZW5jZVIFZmlsZXM=');

@$core.Deprecated('Use reviewRiskReasonDescriptor instead')
const ReviewRiskReason$json = {
  '1': 'ReviewRiskReason',
  '2': [
    {'1': 'kind', '3': 1, '4': 1, '5': 9, '10': 'kind'},
    {'1': 'weight', '3': 2, '4': 1, '5': 1, '10': 'weight'},
    {'1': 'detail', '3': 3, '4': 1, '5': 9, '10': 'detail'},
  ],
};

/// Descriptor for `ReviewRiskReason`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewRiskReasonDescriptor = $convert.base64Decode(
    'ChBSZXZpZXdSaXNrUmVhc29uEhIKBGtpbmQYASABKAlSBGtpbmQSFgoGd2VpZ2h0GAIgASgBUg'
    'Z3ZWlnaHQSFgoGZGV0YWlsGAMgASgJUgZkZXRhaWw=');

@$core.Deprecated('Use reviewFileRiskDescriptor instead')
const ReviewFileRisk$json = {
  '1': 'ReviewFileRisk',
  '2': [
    {'1': 'file', '3': 1, '4': 1, '5': 9, '10': 'file'},
    {'1': 'score', '3': 2, '4': 1, '5': 1, '10': 'score'},
    {'1': 'level', '3': 3, '4': 1, '5': 9, '10': 'level'},
    {
      '1': 'reasons',
      '3': 4,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewRiskReason',
      '10': 'reasons'
    },
  ],
};

/// Descriptor for `ReviewFileRisk`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewFileRiskDescriptor = $convert.base64Decode(
    'Cg5SZXZpZXdGaWxlUmlzaxISCgRmaWxlGAEgASgJUgRmaWxlEhQKBXNjb3JlGAIgASgBUgVzY2'
    '9yZRIUCgVsZXZlbBgDIAEoCVIFbGV2ZWwSNAoHcmVhc29ucxgEIAMoCzIaLmFnZW50LnYxLlJl'
    'dmlld1Jpc2tSZWFzb25SB3JlYXNvbnM=');

@$core.Deprecated('Use reviewRiskReportDescriptor instead')
const ReviewRiskReport$json = {
  '1': 'ReviewRiskReport',
  '2': [
    {
      '1': 'files',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ReviewFileRisk',
      '10': 'files'
    },
    {'1': 'max_score', '3': 2, '4': 1, '5': 1, '10': 'maxScore'},
    {'1': 'gate_threshold', '3': 3, '4': 1, '5': 1, '10': 'gateThreshold'},
    {'1': 'gate_failed', '3': 4, '4': 1, '5': 8, '10': 'gateFailed'},
  ],
};

/// Descriptor for `ReviewRiskReport`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewRiskReportDescriptor = $convert.base64Decode(
    'ChBSZXZpZXdSaXNrUmVwb3J0Ei4KBWZpbGVzGAEgAygLMhguYWdlbnQudjEuUmV2aWV3RmlsZV'
    'Jpc2tSBWZpbGVzEhsKCW1heF9zY29yZRgCIAEoAVIIbWF4U2NvcmUSJQoOZ2F0ZV90aHJlc2hv'
    'bGQYAyABKAFSDWdhdGVUaHJlc2hvbGQSHwoLZ2F0ZV9mYWlsZWQYBCABKAhSCmdhdGVGYWlsZW'
    'Q=');

@$core.Deprecated('Use reviewFactsDescriptor instead')
const ReviewFacts$json = {
  '1': 'ReviewFacts',
  '2': [
    {
      '1': 'meta',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ReviewMeta',
      '10': 'meta'
    },
    {
      '1': 'change',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ReviewChangeSet',
      '10': 'change'
    },
    {
      '1': 'git_state',
      '3': 3,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ReviewGitState',
      '10': 'gitState'
    },
    {
      '1': 'analysis',
      '3': 4,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ReviewAnalysisReport',
      '10': 'analysis'
    },
    {
      '1': 'signatures',
      '3': 5,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ReviewSignatureReport',
      '10': 'signatures'
    },
    {
      '1': 'callgraph',
      '3': 6,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ReviewCallGraph',
      '10': 'callgraph'
    },
    {
      '1': 'style',
      '3': 7,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ReviewStyleFacts',
      '10': 'style'
    },
    {
      '1': 'summaries',
      '3': 8,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ReviewSummaryReport',
      '10': 'summaries'
    },
    {
      '1': 'cochange',
      '3': 9,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ReviewCoChangeReport',
      '10': 'cochange'
    },
    {
      '1': 'churn',
      '3': 10,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ReviewChurnReport',
      '10': 'churn'
    },
    {
      '1': 'salience',
      '3': 11,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ReviewSalienceReport',
      '10': 'salience'
    },
    {
      '1': 'risk',
      '3': 12,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.ReviewRiskReport',
      '10': 'risk'
    },
  ],
};

/// Descriptor for `ReviewFacts`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reviewFactsDescriptor = $convert.base64Decode(
    'CgtSZXZpZXdGYWN0cxIoCgRtZXRhGAEgASgLMhQuYWdlbnQudjEuUmV2aWV3TWV0YVIEbWV0YR'
    'IxCgZjaGFuZ2UYAiABKAsyGS5hZ2VudC52MS5SZXZpZXdDaGFuZ2VTZXRSBmNoYW5nZRI1Cgln'
    'aXRfc3RhdGUYAyABKAsyGC5hZ2VudC52MS5SZXZpZXdHaXRTdGF0ZVIIZ2l0U3RhdGUSOgoIYW'
    '5hbHlzaXMYBCABKAsyHi5hZ2VudC52MS5SZXZpZXdBbmFseXNpc1JlcG9ydFIIYW5hbHlzaXMS'
    'PwoKc2lnbmF0dXJlcxgFIAEoCzIfLmFnZW50LnYxLlJldmlld1NpZ25hdHVyZVJlcG9ydFIKc2'
    'lnbmF0dXJlcxI3CgljYWxsZ3JhcGgYBiABKAsyGS5hZ2VudC52MS5SZXZpZXdDYWxsR3JhcGhS'
    'CWNhbGxncmFwaBIwCgVzdHlsZRgHIAEoCzIaLmFnZW50LnYxLlJldmlld1N0eWxlRmFjdHNSBX'
    'N0eWxlEjsKCXN1bW1hcmllcxgIIAEoCzIdLmFnZW50LnYxLlJldmlld1N1bW1hcnlSZXBvcnRS'
    'CXN1bW1hcmllcxI6Cghjb2NoYW5nZRgJIAEoCzIeLmFnZW50LnYxLlJldmlld0NvQ2hhbmdlUm'
    'Vwb3J0Ughjb2NoYW5nZRIxCgVjaHVybhgKIAEoCzIbLmFnZW50LnYxLlJldmlld0NodXJuUmVw'
    'b3J0UgVjaHVybhI6CghzYWxpZW5jZRgLIAEoCzIeLmFnZW50LnYxLlJldmlld1NhbGllbmNlUm'
    'Vwb3J0UghzYWxpZW5jZRIuCgRyaXNrGAwgASgLMhouYWdlbnQudjEuUmV2aWV3Umlza1JlcG9y'
    'dFIEcmlzaw==');
