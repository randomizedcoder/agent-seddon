// This is a generated file - do not edit.
//
// Generated from agent/v1/search.proto.

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

@$core.Deprecated('Use searchModeDescriptor instead')
const SearchMode$json = {
  '1': 'SearchMode',
  '2': [
    {'1': 'SEARCH_MODE_LITERAL', '2': 0},
    {'1': 'SEARCH_MODE_PHRASE', '2': 1},
    {'1': 'SEARCH_MODE_FUZZY', '2': 2},
    {'1': 'SEARCH_MODE_REGEX', '2': 3},
  ],
};

/// Descriptor for `SearchMode`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List searchModeDescriptor = $convert.base64Decode(
    'CgpTZWFyY2hNb2RlEhcKE1NFQVJDSF9NT0RFX0xJVEVSQUwQABIWChJTRUFSQ0hfTU9ERV9QSF'
    'JBU0UQARIVChFTRUFSQ0hfTU9ERV9GVVpaWRACEhUKEVNFQVJDSF9NT0RFX1JFR0VYEAM=');

@$core.Deprecated('Use indexStateDescriptor instead')
const IndexState$json = {
  '1': 'IndexState',
  '2': [
    {'1': 'INDEX_STATE_FRESH', '2': 0},
    {'1': 'INDEX_STATE_STALE', '2': 1},
    {'1': 'INDEX_STATE_MISSING', '2': 2},
    {'1': 'INDEX_STATE_BUILDING', '2': 3},
  ],
};

/// Descriptor for `IndexState`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List indexStateDescriptor = $convert.base64Decode(
    'CgpJbmRleFN0YXRlEhUKEUlOREVYX1NUQVRFX0ZSRVNIEAASFQoRSU5ERVhfU1RBVEVfU1RBTE'
    'UQARIXChNJTkRFWF9TVEFURV9NSVNTSU5HEAISGAoUSU5ERVhfU1RBVEVfQlVJTERJTkcQAw==');

@$core.Deprecated('Use searchQueryDescriptor instead')
const SearchQuery$json = {
  '1': 'SearchQuery',
  '2': [
    {'1': 'text', '3': 1, '4': 1, '5': 9, '10': 'text'},
    {
      '1': 'mode',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.SearchMode',
      '10': 'mode'
    },
    {'1': 'path_globs', '3': 3, '4': 3, '5': 9, '10': 'pathGlobs'},
    {'1': 'lang', '3': 4, '4': 1, '5': 9, '9': 0, '10': 'lang', '17': true},
    {'1': 'limit', '3': 5, '4': 1, '5': 4, '10': 'limit'},
    {
      '1': 'fuzzy_distance',
      '3': 6,
      '4': 1,
      '5': 13,
      '9': 1,
      '10': 'fuzzyDistance',
      '17': true
    },
  ],
  '8': [
    {'1': '_lang'},
    {'1': '_fuzzy_distance'},
  ],
};

/// Descriptor for `SearchQuery`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List searchQueryDescriptor = $convert.base64Decode(
    'CgtTZWFyY2hRdWVyeRISCgR0ZXh0GAEgASgJUgR0ZXh0EigKBG1vZGUYAiABKA4yFC5hZ2VudC'
    '52MS5TZWFyY2hNb2RlUgRtb2RlEh0KCnBhdGhfZ2xvYnMYAyADKAlSCXBhdGhHbG9icxIXCgRs'
    'YW5nGAQgASgJSABSBGxhbmeIAQESFAoFbGltaXQYBSABKARSBWxpbWl0EioKDmZ1enp5X2Rpc3'
    'RhbmNlGAYgASgNSAFSDWZ1enp5RGlzdGFuY2WIAQFCBwoFX2xhbmdCEQoPX2Z1enp5X2Rpc3Rh'
    'bmNl');

@$core.Deprecated('Use searchHitDescriptor instead')
const SearchHit$json = {
  '1': 'SearchHit',
  '2': [
    {'1': 'path', '3': 1, '4': 1, '5': 9, '10': 'path'},
    {'1': 'line', '3': 2, '4': 1, '5': 13, '10': 'line'},
    {'1': 'col_start', '3': 3, '4': 1, '5': 13, '10': 'colStart'},
    {'1': 'col_end', '3': 4, '4': 1, '5': 13, '10': 'colEnd'},
    {'1': 'score', '3': 5, '4': 1, '5': 2, '10': 'score'},
    {'1': 'snippet', '3': 6, '4': 1, '5': 9, '10': 'snippet'},
  ],
};

/// Descriptor for `SearchHit`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List searchHitDescriptor = $convert.base64Decode(
    'CglTZWFyY2hIaXQSEgoEcGF0aBgBIAEoCVIEcGF0aBISCgRsaW5lGAIgASgNUgRsaW5lEhsKCW'
    'NvbF9zdGFydBgDIAEoDVIIY29sU3RhcnQSFwoHY29sX2VuZBgEIAEoDVIGY29sRW5kEhQKBXNj'
    'b3JlGAUgASgCUgVzY29yZRIYCgdzbmlwcGV0GAYgASgJUgdzbmlwcGV0');

@$core.Deprecated('Use searchRequestDescriptor instead')
const SearchRequest$json = {
  '1': 'SearchRequest',
  '2': [
    {
      '1': 'query',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.SearchQuery',
      '10': 'query'
    },
    {'1': 'backend', '3': 2, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `SearchRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List searchRequestDescriptor = $convert.base64Decode(
    'Cg1TZWFyY2hSZXF1ZXN0EisKBXF1ZXJ5GAEgASgLMhUuYWdlbnQudjEuU2VhcmNoUXVlcnlSBX'
    'F1ZXJ5EhgKB2JhY2tlbmQYAiABKAlSB2JhY2tlbmQ=');

@$core.Deprecated('Use searchResponseDescriptor instead')
const SearchResponse$json = {
  '1': 'SearchResponse',
  '2': [
    {
      '1': 'hits',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.SearchHit',
      '10': 'hits'
    },
    {'1': 'backend', '3': 2, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `SearchResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List searchResponseDescriptor = $convert.base64Decode(
    'Cg5TZWFyY2hSZXNwb25zZRInCgRoaXRzGAEgAygLMhMuYWdlbnQudjEuU2VhcmNoSGl0UgRoaX'
    'RzEhgKB2JhY2tlbmQYAiABKAlSB2JhY2tlbmQ=');

@$core.Deprecated('Use searchCapabilitiesDescriptor instead')
const SearchCapabilities$json = {
  '1': 'SearchCapabilities',
  '2': [
    {'1': 'backend', '3': 1, '4': 1, '5': 9, '10': 'backend'},
    {
      '1': 'modes',
      '3': 2,
      '4': 3,
      '5': 14,
      '6': '.agent.v1.SearchMode',
      '10': 'modes'
    },
    {'1': 'content_search', '3': 3, '4': 1, '5': 8, '10': 'contentSearch'},
    {'1': 'scored', '3': 4, '4': 1, '5': 8, '10': 'scored'},
    {'1': 'incremental', '3': 5, '4': 1, '5': 8, '10': 'incremental'},
    {
      '1': 'max_concurrent_queries',
      '3': 6,
      '4': 1,
      '5': 13,
      '10': 'maxConcurrentQueries'
    },
  ],
};

/// Descriptor for `SearchCapabilities`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List searchCapabilitiesDescriptor = $convert.base64Decode(
    'ChJTZWFyY2hDYXBhYmlsaXRpZXMSGAoHYmFja2VuZBgBIAEoCVIHYmFja2VuZBIqCgVtb2Rlcx'
    'gCIAMoDjIULmFnZW50LnYxLlNlYXJjaE1vZGVSBW1vZGVzEiUKDmNvbnRlbnRfc2VhcmNoGAMg'
    'ASgIUg1jb250ZW50U2VhcmNoEhYKBnNjb3JlZBgEIAEoCFIGc2NvcmVkEiAKC2luY3JlbWVudG'
    'FsGAUgASgIUgtpbmNyZW1lbnRhbBI0ChZtYXhfY29uY3VycmVudF9xdWVyaWVzGAYgASgNUhRt'
    'YXhDb25jdXJyZW50UXVlcmllcw==');

@$core.Deprecated('Use searchCapabilitiesRequestDescriptor instead')
const SearchCapabilitiesRequest$json = {
  '1': 'SearchCapabilitiesRequest',
  '2': [
    {'1': 'backend', '3': 1, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `SearchCapabilitiesRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List searchCapabilitiesRequestDescriptor =
    $convert.base64Decode(
        'ChlTZWFyY2hDYXBhYmlsaXRpZXNSZXF1ZXN0EhgKB2JhY2tlbmQYASABKAlSB2JhY2tlbmQ=');

@$core.Deprecated('Use searchCapabilitiesResponseDescriptor instead')
const SearchCapabilitiesResponse$json = {
  '1': 'SearchCapabilitiesResponse',
  '2': [
    {
      '1': 'backends',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.SearchCapabilities',
      '10': 'backends'
    },
  ],
};

/// Descriptor for `SearchCapabilitiesResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List searchCapabilitiesResponseDescriptor =
    $convert.base64Decode(
        'ChpTZWFyY2hDYXBhYmlsaXRpZXNSZXNwb25zZRI4CghiYWNrZW5kcxgBIAMoCzIcLmFnZW50Ln'
        'YxLlNlYXJjaENhcGFiaWxpdGllc1IIYmFja2VuZHM=');

@$core.Deprecated('Use indexStatusDescriptor instead')
const IndexStatus$json = {
  '1': 'IndexStatus',
  '2': [
    {
      '1': 'state',
      '3': 1,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.IndexState',
      '10': 'state'
    },
    {'1': 'indexed_files', '3': 2, '4': 1, '5': 4, '10': 'indexedFiles'},
    {'1': 'last_indexed_ms', '3': 3, '4': 1, '5': 4, '10': 'lastIndexedMs'},
    {'1': 'manifest_digest', '3': 4, '4': 1, '5': 9, '10': 'manifestDigest'},
    {'1': 'backend', '3': 5, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `IndexStatus`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List indexStatusDescriptor = $convert.base64Decode(
    'CgtJbmRleFN0YXR1cxIqCgVzdGF0ZRgBIAEoDjIULmFnZW50LnYxLkluZGV4U3RhdGVSBXN0YX'
    'RlEiMKDWluZGV4ZWRfZmlsZXMYAiABKARSDGluZGV4ZWRGaWxlcxImCg9sYXN0X2luZGV4ZWRf'
    'bXMYAyABKARSDWxhc3RJbmRleGVkTXMSJwoPbWFuaWZlc3RfZGlnZXN0GAQgASgJUg5tYW5pZm'
    'VzdERpZ2VzdBIYCgdiYWNrZW5kGAUgASgJUgdiYWNrZW5k');

@$core.Deprecated('Use statusRequestDescriptor instead')
const StatusRequest$json = {
  '1': 'StatusRequest',
  '2': [
    {'1': 'backend', '3': 1, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `StatusRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List statusRequestDescriptor = $convert
    .base64Decode('Cg1TdGF0dXNSZXF1ZXN0EhgKB2JhY2tlbmQYASABKAlSB2JhY2tlbmQ=');

@$core.Deprecated('Use statusResponseDescriptor instead')
const StatusResponse$json = {
  '1': 'StatusResponse',
  '2': [
    {
      '1': 'backends',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.IndexStatus',
      '10': 'backends'
    },
  ],
};

/// Descriptor for `StatusResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List statusResponseDescriptor = $convert.base64Decode(
    'Cg5TdGF0dXNSZXNwb25zZRIxCghiYWNrZW5kcxgBIAMoCzIVLmFnZW50LnYxLkluZGV4U3RhdH'
    'VzUghiYWNrZW5kcw==');

@$core.Deprecated('Use reindexRequestDescriptor instead')
const ReindexRequest$json = {
  '1': 'ReindexRequest',
  '2': [
    {'1': 'backend', '3': 1, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `ReindexRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reindexRequestDescriptor = $convert
    .base64Decode('Cg5SZWluZGV4UmVxdWVzdBIYCgdiYWNrZW5kGAEgASgJUgdiYWNrZW5k');

@$core.Deprecated('Use listFilesRequestDescriptor instead')
const ListFilesRequest$json = {
  '1': 'ListFilesRequest',
  '2': [
    {'1': 'globs', '3': 1, '4': 3, '5': 9, '10': 'globs'},
    {'1': 'backend', '3': 2, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `ListFilesRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List listFilesRequestDescriptor = $convert.base64Decode(
    'ChBMaXN0RmlsZXNSZXF1ZXN0EhQKBWdsb2JzGAEgAygJUgVnbG9icxIYCgdiYWNrZW5kGAIgAS'
    'gJUgdiYWNrZW5k');

@$core.Deprecated('Use listFilesResponseDescriptor instead')
const ListFilesResponse$json = {
  '1': 'ListFilesResponse',
  '2': [
    {'1': 'paths', '3': 1, '4': 3, '5': 9, '10': 'paths'},
    {'1': 'backend', '3': 2, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `ListFilesResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List listFilesResponseDescriptor = $convert.base64Decode(
    'ChFMaXN0RmlsZXNSZXNwb25zZRIUCgVwYXRocxgBIAMoCVIFcGF0aHMSGAoHYmFja2VuZBgCIA'
    'EoCVIHYmFja2VuZA==');

@$core.Deprecated('Use reindexProgressDescriptor instead')
const ReindexProgress$json = {
  '1': 'ReindexProgress',
  '2': [
    {'1': 'files_done', '3': 1, '4': 1, '5': 4, '10': 'filesDone'},
    {'1': 'files_total', '3': 2, '4': 1, '5': 4, '10': 'filesTotal'},
    {'1': 'done', '3': 3, '4': 1, '5': 8, '10': 'done'},
    {'1': 'backend', '3': 4, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `ReindexProgress`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List reindexProgressDescriptor = $convert.base64Decode(
    'Cg9SZWluZGV4UHJvZ3Jlc3MSHQoKZmlsZXNfZG9uZRgBIAEoBFIJZmlsZXNEb25lEh8KC2ZpbG'
    'VzX3RvdGFsGAIgASgEUgpmaWxlc1RvdGFsEhIKBGRvbmUYAyABKAhSBGRvbmUSGAoHYmFja2Vu'
    'ZBgEIAEoCVIHYmFja2VuZA==');
