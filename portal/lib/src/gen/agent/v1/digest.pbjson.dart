// This is a generated file - do not edit.
//
// Generated from agent/v1/digest.proto.

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

@$core.Deprecated('Use digestDescriptor instead')
const Digest$json = {
  '1': 'Digest',
  '2': [
    {'1': 'session_id', '3': 1, '4': 1, '5': 9, '10': 'sessionId'},
    {'1': 'user_id', '3': 2, '4': 1, '5': 9, '10': 'userId'},
    {'1': 'seq', '3': 3, '4': 1, '5': 4, '10': 'seq'},
    {'1': 'kind', '3': 4, '4': 1, '5': 9, '10': 'kind'},
    {'1': 'text', '3': 5, '4': 1, '5': 9, '10': 'text'},
    {'1': 'keywords', '3': 6, '4': 3, '5': 9, '10': 'keywords'},
    {'1': 'mode', '3': 7, '4': 1, '5': 9, '10': 'mode'},
    {'1': 'model', '3': 8, '4': 1, '5': 9, '10': 'model'},
    {'1': 'ts_ms', '3': 9, '4': 1, '5': 4, '10': 'tsMs'},
    {'1': 'duration_ms', '3': 10, '4': 1, '5': 13, '10': 'durationMs'},
    {'1': 'tokens', '3': 11, '4': 1, '5': 13, '10': 'tokens'},
  ],
};

/// Descriptor for `Digest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List digestDescriptor = $convert.base64Decode(
    'CgZEaWdlc3QSHQoKc2Vzc2lvbl9pZBgBIAEoCVIJc2Vzc2lvbklkEhcKB3VzZXJfaWQYAiABKA'
    'lSBnVzZXJJZBIQCgNzZXEYAyABKARSA3NlcRISCgRraW5kGAQgASgJUgRraW5kEhIKBHRleHQY'
    'BSABKAlSBHRleHQSGgoIa2V5d29yZHMYBiADKAlSCGtleXdvcmRzEhIKBG1vZGUYByABKAlSBG'
    '1vZGUSFAoFbW9kZWwYCCABKAlSBW1vZGVsEhMKBXRzX21zGAkgASgEUgR0c01zEh8KC2R1cmF0'
    'aW9uX21zGAogASgNUgpkdXJhdGlvbk1zEhYKBnRva2VucxgLIAEoDVIGdG9rZW5z');

@$core.Deprecated('Use putDigestRequestDescriptor instead')
const PutDigestRequest$json = {
  '1': 'PutDigestRequest',
  '2': [
    {
      '1': 'digest',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.Digest',
      '10': 'digest'
    },
  ],
};

/// Descriptor for `PutDigestRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List putDigestRequestDescriptor = $convert.base64Decode(
    'ChBQdXREaWdlc3RSZXF1ZXN0EigKBmRpZ2VzdBgBIAEoCzIQLmFnZW50LnYxLkRpZ2VzdFIGZG'
    'lnZXN0');

@$core.Deprecated('Use putDigestResponseDescriptor instead')
const PutDigestResponse$json = {
  '1': 'PutDigestResponse',
};

/// Descriptor for `PutDigestResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List putDigestResponseDescriptor =
    $convert.base64Decode('ChFQdXREaWdlc3RSZXNwb25zZQ==');

@$core.Deprecated('Use queryDigestsRequestDescriptor instead')
const QueryDigestsRequest$json = {
  '1': 'QueryDigestsRequest',
  '2': [
    {'1': 'session_id', '3': 1, '4': 1, '5': 9, '10': 'sessionId'},
    {'1': 'kind', '3': 2, '4': 1, '5': 9, '10': 'kind'},
    {'1': 'since_seq', '3': 3, '4': 1, '5': 4, '10': 'sinceSeq'},
    {'1': 'keywords_any', '3': 4, '4': 3, '5': 9, '10': 'keywordsAny'},
    {'1': 'limit', '3': 5, '4': 1, '5': 13, '10': 'limit'},
  ],
};

/// Descriptor for `QueryDigestsRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List queryDigestsRequestDescriptor = $convert.base64Decode(
    'ChNRdWVyeURpZ2VzdHNSZXF1ZXN0Eh0KCnNlc3Npb25faWQYASABKAlSCXNlc3Npb25JZBISCg'
    'RraW5kGAIgASgJUgRraW5kEhsKCXNpbmNlX3NlcRgDIAEoBFIIc2luY2VTZXESIQoMa2V5d29y'
    'ZHNfYW55GAQgAygJUgtrZXl3b3Jkc0FueRIUCgVsaW1pdBgFIAEoDVIFbGltaXQ=');

@$core.Deprecated('Use queryDigestsResponseDescriptor instead')
const QueryDigestsResponse$json = {
  '1': 'QueryDigestsResponse',
  '2': [
    {
      '1': 'digests',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Digest',
      '10': 'digests'
    },
  ],
};

/// Descriptor for `QueryDigestsResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List queryDigestsResponseDescriptor = $convert.base64Decode(
    'ChRRdWVyeURpZ2VzdHNSZXNwb25zZRIqCgdkaWdlc3RzGAEgAygLMhAuYWdlbnQudjEuRGlnZX'
    'N0UgdkaWdlc3Rz');
