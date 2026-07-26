// This is a generated file - do not edit.
//
// Generated from agent/v1/repo.proto.

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

@$core.Deprecated('Use entryKindDescriptor instead')
const EntryKind$json = {
  '1': 'EntryKind',
  '2': [
    {'1': 'ENTRY_KIND_BLOB', '2': 0},
    {'1': 'ENTRY_KIND_TREE', '2': 1},
    {'1': 'ENTRY_KIND_SYMLINK', '2': 2},
    {'1': 'ENTRY_KIND_SUBMODULE', '2': 3},
  ],
};

/// Descriptor for `EntryKind`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List entryKindDescriptor = $convert.base64Decode(
    'CglFbnRyeUtpbmQSEwoPRU5UUllfS0lORF9CTE9CEAASEwoPRU5UUllfS0lORF9UUkVFEAESFg'
    'oSRU5UUllfS0lORF9TWU1MSU5LEAISGAoURU5UUllfS0lORF9TVUJNT0RVTEUQAw==');

@$core.Deprecated('Use changeKindDescriptor instead')
const ChangeKind$json = {
  '1': 'ChangeKind',
  '2': [
    {'1': 'CHANGE_KIND_MODIFIED', '2': 0},
    {'1': 'CHANGE_KIND_ADDED', '2': 1},
    {'1': 'CHANGE_KIND_DELETED', '2': 2},
    {'1': 'CHANGE_KIND_RENAMED', '2': 3},
    {'1': 'CHANGE_KIND_COPIED', '2': 4},
    {'1': 'CHANGE_KIND_TYPE_CHANGE', '2': 5},
  ],
};

/// Descriptor for `ChangeKind`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List changeKindDescriptor = $convert.base64Decode(
    'CgpDaGFuZ2VLaW5kEhgKFENIQU5HRV9LSU5EX01PRElGSUVEEAASFQoRQ0hBTkdFX0tJTkRfQU'
    'RERUQQARIXChNDSEFOR0VfS0lORF9ERUxFVEVEEAISFwoTQ0hBTkdFX0tJTkRfUkVOQU1FRBAD'
    'EhYKEkNIQU5HRV9LSU5EX0NPUElFRBAEEhsKF0NIQU5HRV9LSU5EX1RZUEVfQ0hBTkdFEAU=');

@$core.Deprecated('Use treeEntryDescriptor instead')
const TreeEntry$json = {
  '1': 'TreeEntry',
  '2': [
    {'1': 'path', '3': 1, '4': 1, '5': 9, '10': 'path'},
    {'1': 'oid', '3': 2, '4': 1, '5': 9, '10': 'oid'},
    {
      '1': 'kind',
      '3': 3,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.EntryKind',
      '10': 'kind'
    },
    {'1': 'mode', '3': 4, '4': 1, '5': 13, '10': 'mode'},
    {'1': 'size', '3': 5, '4': 1, '5': 4, '9': 0, '10': 'size', '17': true},
  ],
  '8': [
    {'1': '_size'},
  ],
};

/// Descriptor for `TreeEntry`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List treeEntryDescriptor = $convert.base64Decode(
    'CglUcmVlRW50cnkSEgoEcGF0aBgBIAEoCVIEcGF0aBIQCgNvaWQYAiABKAlSA29pZBInCgRraW'
    '5kGAMgASgOMhMuYWdlbnQudjEuRW50cnlLaW5kUgRraW5kEhIKBG1vZGUYBCABKA1SBG1vZGUS'
    'FwoEc2l6ZRgFIAEoBEgAUgRzaXpliAEBQgcKBV9zaXpl');

@$core.Deprecated('Use blobContentDescriptor instead')
const BlobContent$json = {
  '1': 'BlobContent',
  '2': [
    {'1': 'oid', '3': 1, '4': 1, '5': 9, '10': 'oid'},
    {'1': 'path', '3': 2, '4': 1, '5': 9, '10': 'path'},
    {'1': 'bytes_len', '3': 3, '4': 1, '5': 4, '10': 'bytesLen'},
    {'1': 'is_binary', '3': 4, '4': 1, '5': 8, '10': 'isBinary'},
    {'1': 'text', '3': 5, '4': 1, '5': 9, '10': 'text'},
  ],
};

/// Descriptor for `BlobContent`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List blobContentDescriptor = $convert.base64Decode(
    'CgtCbG9iQ29udGVudBIQCgNvaWQYASABKAlSA29pZBISCgRwYXRoGAIgASgJUgRwYXRoEhsKCW'
    'J5dGVzX2xlbhgDIAEoBFIIYnl0ZXNMZW4SGwoJaXNfYmluYXJ5GAQgASgIUghpc0JpbmFyeRIS'
    'CgR0ZXh0GAUgASgJUgR0ZXh0');

@$core.Deprecated('Use fileDiffDescriptor instead')
const FileDiff$json = {
  '1': 'FileDiff',
  '2': [
    {
      '1': 'change',
      '3': 1,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ChangeKind',
      '10': 'change'
    },
    {
      '1': 'old_path',
      '3': 2,
      '4': 1,
      '5': 9,
      '9': 0,
      '10': 'oldPath',
      '17': true
    },
    {
      '1': 'new_path',
      '3': 3,
      '4': 1,
      '5': 9,
      '9': 1,
      '10': 'newPath',
      '17': true
    },
    {
      '1': 'old_oid',
      '3': 4,
      '4': 1,
      '5': 9,
      '9': 2,
      '10': 'oldOid',
      '17': true
    },
    {
      '1': 'new_oid',
      '3': 5,
      '4': 1,
      '5': 9,
      '9': 3,
      '10': 'newOid',
      '17': true
    },
    {'1': 'additions', '3': 6, '4': 1, '5': 13, '10': 'additions'},
    {'1': 'deletions', '3': 7, '4': 1, '5': 13, '10': 'deletions'},
    {'1': 'patch', '3': 8, '4': 1, '5': 9, '10': 'patch'},
  ],
  '8': [
    {'1': '_old_path'},
    {'1': '_new_path'},
    {'1': '_old_oid'},
    {'1': '_new_oid'},
  ],
};

/// Descriptor for `FileDiff`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List fileDiffDescriptor = $convert.base64Decode(
    'CghGaWxlRGlmZhIsCgZjaGFuZ2UYASABKA4yFC5hZ2VudC52MS5DaGFuZ2VLaW5kUgZjaGFuZ2'
    'USHgoIb2xkX3BhdGgYAiABKAlIAFIHb2xkUGF0aIgBARIeCghuZXdfcGF0aBgDIAEoCUgBUgdu'
    'ZXdQYXRoiAEBEhwKB29sZF9vaWQYBCABKAlIAlIGb2xkT2lkiAEBEhwKB25ld19vaWQYBSABKA'
    'lIA1IGbmV3T2lkiAEBEhwKCWFkZGl0aW9ucxgGIAEoDVIJYWRkaXRpb25zEhwKCWRlbGV0aW9u'
    'cxgHIAEoDVIJZGVsZXRpb25zEhQKBXBhdGNoGAggASgJUgVwYXRjaEILCglfb2xkX3BhdGhCCw'
    'oJX25ld19wYXRoQgoKCF9vbGRfb2lkQgoKCF9uZXdfb2lk');

@$core.Deprecated('Use diffResultDescriptor instead')
const DiffResult$json = {
  '1': 'DiffResult',
  '2': [
    {'1': 'base', '3': 1, '4': 1, '5': 9, '10': 'base'},
    {'1': 'target', '3': 2, '4': 1, '5': 9, '10': 'target'},
    {
      '1': 'files',
      '3': 3,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.FileDiff',
      '10': 'files'
    },
  ],
};

/// Descriptor for `DiffResult`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List diffResultDescriptor = $convert.base64Decode(
    'CgpEaWZmUmVzdWx0EhIKBGJhc2UYASABKAlSBGJhc2USFgoGdGFyZ2V0GAIgASgJUgZ0YXJnZX'
    'QSKAoFZmlsZXMYAyADKAsyEi5hZ2VudC52MS5GaWxlRGlmZlIFZmlsZXM=');

@$core.Deprecated('Use commitInfoDescriptor instead')
const CommitInfo$json = {
  '1': 'CommitInfo',
  '2': [
    {'1': 'oid', '3': 1, '4': 1, '5': 9, '10': 'oid'},
    {'1': 'parents', '3': 2, '4': 3, '5': 9, '10': 'parents'},
    {'1': 'author', '3': 3, '4': 1, '5': 9, '10': 'author'},
    {'1': 'author_email', '3': 4, '4': 1, '5': 9, '10': 'authorEmail'},
    {'1': 'committed_ms', '3': 5, '4': 1, '5': 4, '10': 'committedMs'},
    {'1': 'summary', '3': 6, '4': 1, '5': 9, '10': 'summary'},
    {'1': 'body', '3': 7, '4': 1, '5': 9, '10': 'body'},
  ],
};

/// Descriptor for `CommitInfo`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List commitInfoDescriptor = $convert.base64Decode(
    'CgpDb21taXRJbmZvEhAKA29pZBgBIAEoCVIDb2lkEhgKB3BhcmVudHMYAiADKAlSB3BhcmVudH'
    'MSFgoGYXV0aG9yGAMgASgJUgZhdXRob3ISIQoMYXV0aG9yX2VtYWlsGAQgASgJUgthdXRob3JF'
    'bWFpbBIhCgxjb21taXR0ZWRfbXMYBSABKARSC2NvbW1pdHRlZE1zEhgKB3N1bW1hcnkYBiABKA'
    'lSB3N1bW1hcnkSEgoEYm9keRgHIAEoCVIEYm9keQ==');

@$core.Deprecated('Use grepHitDescriptor instead')
const GrepHit$json = {
  '1': 'GrepHit',
  '2': [
    {'1': 'path', '3': 1, '4': 1, '5': 9, '10': 'path'},
    {'1': 'line', '3': 2, '4': 1, '5': 13, '10': 'line'},
    {'1': 'text', '3': 3, '4': 1, '5': 9, '10': 'text'},
  ],
};

/// Descriptor for `GrepHit`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List grepHitDescriptor = $convert.base64Decode(
    'CgdHcmVwSGl0EhIKBHBhdGgYASABKAlSBHBhdGgSEgoEbGluZRgCIAEoDVIEbGluZRISCgR0ZX'
    'h0GAMgASgJUgR0ZXh0');

@$core.Deprecated('Use worktreeHandleDescriptor instead')
const WorktreeHandle$json = {
  '1': 'WorktreeHandle',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'path', '3': 2, '4': 1, '5': 9, '10': 'path'},
    {'1': 'head', '3': 3, '4': 1, '5': 9, '10': 'head'},
    {'1': 'revision', '3': 4, '4': 1, '5': 9, '10': 'revision'},
    {'1': 'writable', '3': 5, '4': 1, '5': 8, '10': 'writable'},
  ],
};

/// Descriptor for `WorktreeHandle`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List worktreeHandleDescriptor = $convert.base64Decode(
    'Cg5Xb3JrdHJlZUhhbmRsZRIOCgJpZBgBIAEoCVICaWQSEgoEcGF0aBgCIAEoCVIEcGF0aBISCg'
    'RoZWFkGAMgASgJUgRoZWFkEhoKCHJldmlzaW9uGAQgASgJUghyZXZpc2lvbhIaCgh3cml0YWJs'
    'ZRgFIAEoCFIId3JpdGFibGU=');

@$core.Deprecated('Use worktreeSpecDescriptor instead')
const WorktreeSpec$json = {
  '1': 'WorktreeSpec',
  '2': [
    {'1': 'revision', '3': 1, '4': 1, '5': 9, '10': 'revision'},
    {'1': 'writable', '3': 2, '4': 1, '5': 8, '10': 'writable'},
    {'1': 'id', '3': 3, '4': 1, '5': 9, '9': 0, '10': 'id', '17': true},
  ],
  '8': [
    {'1': '_id'},
  ],
};

/// Descriptor for `WorktreeSpec`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List worktreeSpecDescriptor = $convert.base64Decode(
    'CgxXb3JrdHJlZVNwZWMSGgoIcmV2aXNpb24YASABKAlSCHJldmlzaW9uEhoKCHdyaXRhYmxlGA'
    'IgASgIUgh3cml0YWJsZRITCgJpZBgDIAEoCUgAUgJpZIgBAUIFCgNfaWQ=');

@$core.Deprecated('Use checkpointDescriptor instead')
const Checkpoint$json = {
  '1': 'Checkpoint',
  '2': [
    {'1': 'name', '3': 1, '4': 1, '5': 9, '10': 'name'},
    {'1': 'oid', '3': 2, '4': 1, '5': 9, '10': 'oid'},
    {'1': 'ref_name', '3': 3, '4': 1, '5': 9, '10': 'refName'},
  ],
};

/// Descriptor for `Checkpoint`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List checkpointDescriptor = $convert.base64Decode(
    'CgpDaGVja3BvaW50EhIKBG5hbWUYASABKAlSBG5hbWUSEAoDb2lkGAIgASgJUgNvaWQSGQoIcm'
    'VmX25hbWUYAyABKAlSB3JlZk5hbWU=');

@$core.Deprecated('Use branchDescriptor instead')
const Branch$json = {
  '1': 'Branch',
  '2': [
    {'1': 'name', '3': 1, '4': 1, '5': 9, '10': 'name'},
    {'1': 'oid', '3': 2, '4': 1, '5': 9, '10': 'oid'},
  ],
};

/// Descriptor for `Branch`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List branchDescriptor = $convert.base64Decode(
    'CgZCcmFuY2gSEgoEbmFtZRgBIAEoCVIEbmFtZRIQCgNvaWQYAiABKAlSA29pZA==');

@$core.Deprecated('Use repoStatusDescriptor instead')
const RepoStatus$json = {
  '1': 'RepoStatus',
  '2': [
    {'1': 'mirror_path', '3': 1, '4': 1, '5': 9, '10': 'mirrorPath'},
    {'1': 'last_fetch_ms', '3': 2, '4': 1, '5': 4, '10': 'lastFetchMs'},
    {'1': 'live_worktrees', '3': 3, '4': 1, '5': 13, '10': 'liveWorktrees'},
    {
      '1': 'heads',
      '3': 4,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Branch',
      '10': 'heads'
    },
  ],
};

/// Descriptor for `RepoStatus`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List repoStatusDescriptor = $convert.base64Decode(
    'CgpSZXBvU3RhdHVzEh8KC21pcnJvcl9wYXRoGAEgASgJUgptaXJyb3JQYXRoEiIKDWxhc3RfZm'
    'V0Y2hfbXMYAiABKARSC2xhc3RGZXRjaE1zEiUKDmxpdmVfd29ya3RyZWVzGAMgASgNUg1saXZl'
    'V29ya3RyZWVzEiYKBWhlYWRzGAQgAygLMhAuYWdlbnQudjEuQnJhbmNoUgVoZWFkcw==');

@$core.Deprecated('Use resolveRequestDescriptor instead')
const ResolveRequest$json = {
  '1': 'ResolveRequest',
  '2': [
    {'1': 'revision', '3': 1, '4': 1, '5': 9, '10': 'revision'},
  ],
};

/// Descriptor for `ResolveRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List resolveRequestDescriptor = $convert.base64Decode(
    'Cg5SZXNvbHZlUmVxdWVzdBIaCghyZXZpc2lvbhgBIAEoCVIIcmV2aXNpb24=');

@$core.Deprecated('Use resolveResponseDescriptor instead')
const ResolveResponse$json = {
  '1': 'ResolveResponse',
  '2': [
    {'1': 'oid', '3': 1, '4': 1, '5': 9, '10': 'oid'},
  ],
};

/// Descriptor for `ResolveResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List resolveResponseDescriptor =
    $convert.base64Decode('Cg9SZXNvbHZlUmVzcG9uc2USEAoDb2lkGAEgASgJUgNvaWQ=');

@$core.Deprecated('Use readFileRequestDescriptor instead')
const ReadFileRequest$json = {
  '1': 'ReadFileRequest',
  '2': [
    {'1': 'revision', '3': 1, '4': 1, '5': 9, '10': 'revision'},
    {'1': 'path', '3': 2, '4': 1, '5': 9, '10': 'path'},
  ],
};

/// Descriptor for `ReadFileRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List readFileRequestDescriptor = $convert.base64Decode(
    'Cg9SZWFkRmlsZVJlcXVlc3QSGgoIcmV2aXNpb24YASABKAlSCHJldmlzaW9uEhIKBHBhdGgYAi'
    'ABKAlSBHBhdGg=');

@$core.Deprecated('Use listTreeRequestDescriptor instead')
const ListTreeRequest$json = {
  '1': 'ListTreeRequest',
  '2': [
    {'1': 'revision', '3': 1, '4': 1, '5': 9, '10': 'revision'},
    {'1': 'path', '3': 2, '4': 1, '5': 9, '10': 'path'},
    {'1': 'recursive', '3': 3, '4': 1, '5': 8, '10': 'recursive'},
  ],
};

/// Descriptor for `ListTreeRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List listTreeRequestDescriptor = $convert.base64Decode(
    'Cg9MaXN0VHJlZVJlcXVlc3QSGgoIcmV2aXNpb24YASABKAlSCHJldmlzaW9uEhIKBHBhdGgYAi'
    'ABKAlSBHBhdGgSHAoJcmVjdXJzaXZlGAMgASgIUglyZWN1cnNpdmU=');

@$core.Deprecated('Use listTreeResponseDescriptor instead')
const ListTreeResponse$json = {
  '1': 'ListTreeResponse',
  '2': [
    {
      '1': 'entries',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.TreeEntry',
      '10': 'entries'
    },
  ],
};

/// Descriptor for `ListTreeResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List listTreeResponseDescriptor = $convert.base64Decode(
    'ChBMaXN0VHJlZVJlc3BvbnNlEi0KB2VudHJpZXMYASADKAsyEy5hZ2VudC52MS5UcmVlRW50cn'
    'lSB2VudHJpZXM=');

@$core.Deprecated('Use diffRequestDescriptor instead')
const DiffRequest$json = {
  '1': 'DiffRequest',
  '2': [
    {'1': 'base', '3': 1, '4': 1, '5': 9, '10': 'base'},
    {'1': 'target', '3': 2, '4': 1, '5': 9, '10': 'target'},
    {'1': 'path_globs', '3': 3, '4': 3, '5': 9, '10': 'pathGlobs'},
  ],
};

/// Descriptor for `DiffRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List diffRequestDescriptor = $convert.base64Decode(
    'CgtEaWZmUmVxdWVzdBISCgRiYXNlGAEgASgJUgRiYXNlEhYKBnRhcmdldBgCIAEoCVIGdGFyZ2'
    'V0Eh0KCnBhdGhfZ2xvYnMYAyADKAlSCXBhdGhHbG9icw==');

@$core.Deprecated('Use grepRequestDescriptor instead')
const GrepRequest$json = {
  '1': 'GrepRequest',
  '2': [
    {'1': 'revision', '3': 1, '4': 1, '5': 9, '10': 'revision'},
    {'1': 'pattern', '3': 2, '4': 1, '5': 9, '10': 'pattern'},
    {'1': 'path_globs', '3': 3, '4': 3, '5': 9, '10': 'pathGlobs'},
    {'1': 'limit', '3': 4, '4': 1, '5': 4, '10': 'limit'},
  ],
};

/// Descriptor for `GrepRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List grepRequestDescriptor = $convert.base64Decode(
    'CgtHcmVwUmVxdWVzdBIaCghyZXZpc2lvbhgBIAEoCVIIcmV2aXNpb24SGAoHcGF0dGVybhgCIA'
    'EoCVIHcGF0dGVybhIdCgpwYXRoX2dsb2JzGAMgAygJUglwYXRoR2xvYnMSFAoFbGltaXQYBCAB'
    'KARSBWxpbWl0');

@$core.Deprecated('Use grepResponseDescriptor instead')
const GrepResponse$json = {
  '1': 'GrepResponse',
  '2': [
    {
      '1': 'hits',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.GrepHit',
      '10': 'hits'
    },
  ],
};

/// Descriptor for `GrepResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List grepResponseDescriptor = $convert.base64Decode(
    'CgxHcmVwUmVzcG9uc2USJQoEaGl0cxgBIAMoCzIRLmFnZW50LnYxLkdyZXBIaXRSBGhpdHM=');

@$core.Deprecated('Use logRequestDescriptor instead')
const LogRequest$json = {
  '1': 'LogRequest',
  '2': [
    {'1': 'revision', '3': 1, '4': 1, '5': 9, '10': 'revision'},
    {'1': 'path', '3': 2, '4': 1, '5': 9, '9': 0, '10': 'path', '17': true},
    {'1': 'limit', '3': 3, '4': 1, '5': 4, '10': 'limit'},
  ],
  '8': [
    {'1': '_path'},
  ],
};

/// Descriptor for `LogRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List logRequestDescriptor = $convert.base64Decode(
    'CgpMb2dSZXF1ZXN0EhoKCHJldmlzaW9uGAEgASgJUghyZXZpc2lvbhIXCgRwYXRoGAIgASgJSA'
    'BSBHBhdGiIAQESFAoFbGltaXQYAyABKARSBWxpbWl0QgcKBV9wYXRo');

@$core.Deprecated('Use logResponseDescriptor instead')
const LogResponse$json = {
  '1': 'LogResponse',
  '2': [
    {
      '1': 'commits',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.CommitInfo',
      '10': 'commits'
    },
  ],
};

/// Descriptor for `LogResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List logResponseDescriptor = $convert.base64Decode(
    'CgtMb2dSZXNwb25zZRIuCgdjb21taXRzGAEgAygLMhQuYWdlbnQudjEuQ29tbWl0SW5mb1IHY2'
    '9tbWl0cw==');

@$core.Deprecated('Use branchesRequestDescriptor instead')
const BranchesRequest$json = {
  '1': 'BranchesRequest',
};

/// Descriptor for `BranchesRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List branchesRequestDescriptor =
    $convert.base64Decode('Cg9CcmFuY2hlc1JlcXVlc3Q=');

@$core.Deprecated('Use branchesResponseDescriptor instead')
const BranchesResponse$json = {
  '1': 'BranchesResponse',
  '2': [
    {
      '1': 'branches',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Branch',
      '10': 'branches'
    },
  ],
};

/// Descriptor for `BranchesResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List branchesResponseDescriptor = $convert.base64Decode(
    'ChBCcmFuY2hlc1Jlc3BvbnNlEiwKCGJyYW5jaGVzGAEgAygLMhAuYWdlbnQudjEuQnJhbmNoUg'
    'hicmFuY2hlcw==');

@$core.Deprecated('Use repoStatusRequestDescriptor instead')
const RepoStatusRequest$json = {
  '1': 'RepoStatusRequest',
};

/// Descriptor for `RepoStatusRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List repoStatusRequestDescriptor =
    $convert.base64Decode('ChFSZXBvU3RhdHVzUmVxdWVzdA==');

@$core.Deprecated('Use fetchRequestDescriptor instead')
const FetchRequest$json = {
  '1': 'FetchRequest',
};

/// Descriptor for `FetchRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List fetchRequestDescriptor =
    $convert.base64Decode('CgxGZXRjaFJlcXVlc3Q=');

@$core.Deprecated('Use worktreeListRequestDescriptor instead')
const WorktreeListRequest$json = {
  '1': 'WorktreeListRequest',
};

/// Descriptor for `WorktreeListRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List worktreeListRequestDescriptor =
    $convert.base64Decode('ChNXb3JrdHJlZUxpc3RSZXF1ZXN0');

@$core.Deprecated('Use worktreeListResponseDescriptor instead')
const WorktreeListResponse$json = {
  '1': 'WorktreeListResponse',
  '2': [
    {
      '1': 'worktrees',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.WorktreeHandle',
      '10': 'worktrees'
    },
  ],
};

/// Descriptor for `WorktreeListResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List worktreeListResponseDescriptor = $convert.base64Decode(
    'ChRXb3JrdHJlZUxpc3RSZXNwb25zZRI2Cgl3b3JrdHJlZXMYASADKAsyGC5hZ2VudC52MS5Xb3'
    'JrdHJlZUhhbmRsZVIJd29ya3RyZWVz');

@$core.Deprecated('Use worktreeRemoveRequestDescriptor instead')
const WorktreeRemoveRequest$json = {
  '1': 'WorktreeRemoveRequest',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
  ],
};

/// Descriptor for `WorktreeRemoveRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List worktreeRemoveRequestDescriptor = $convert
    .base64Decode('ChVXb3JrdHJlZVJlbW92ZVJlcXVlc3QSDgoCaWQYASABKAlSAmlk');

@$core.Deprecated('Use worktreeRemoveResponseDescriptor instead')
const WorktreeRemoveResponse$json = {
  '1': 'WorktreeRemoveResponse',
};

/// Descriptor for `WorktreeRemoveResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List worktreeRemoveResponseDescriptor =
    $convert.base64Decode('ChZXb3JrdHJlZVJlbW92ZVJlc3BvbnNl');

@$core.Deprecated('Use checkpointRequestDescriptor instead')
const CheckpointRequest$json = {
  '1': 'CheckpointRequest',
  '2': [
    {'1': 'worktree_id', '3': 1, '4': 1, '5': 9, '10': 'worktreeId'},
    {'1': 'name', '3': 2, '4': 1, '5': 9, '10': 'name'},
  ],
};

/// Descriptor for `CheckpointRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List checkpointRequestDescriptor = $convert.base64Decode(
    'ChFDaGVja3BvaW50UmVxdWVzdBIfCgt3b3JrdHJlZV9pZBgBIAEoCVIKd29ya3RyZWVJZBISCg'
    'RuYW1lGAIgASgJUgRuYW1l');

@$core.Deprecated('Use pushRequestDescriptor instead')
const PushRequest$json = {
  '1': 'PushRequest',
  '2': [
    {
      '1': 'checkpoint',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.Checkpoint',
      '10': 'checkpoint'
    },
    {'1': 'remote_ref', '3': 2, '4': 1, '5': 9, '10': 'remoteRef'},
  ],
};

/// Descriptor for `PushRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List pushRequestDescriptor = $convert.base64Decode(
    'CgtQdXNoUmVxdWVzdBI0CgpjaGVja3BvaW50GAEgASgLMhQuYWdlbnQudjEuQ2hlY2twb2ludF'
    'IKY2hlY2twb2ludBIdCgpyZW1vdGVfcmVmGAIgASgJUglyZW1vdGVSZWY=');

@$core.Deprecated('Use pushResponseDescriptor instead')
const PushResponse$json = {
  '1': 'PushResponse',
};

/// Descriptor for `PushResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List pushResponseDescriptor =
    $convert.base64Decode('CgxQdXNoUmVzcG9uc2U=');
