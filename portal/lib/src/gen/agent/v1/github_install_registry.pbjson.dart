// This is a generated file - do not edit.
//
// Generated from agent/v1/github_install_registry.proto.

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

@$core.Deprecated('Use githubInstallDescriptor instead')
const GithubInstall$json = {
  '1': 'GithubInstall',
  '2': [
    {'1': 'installation_id', '3': 1, '4': 1, '5': 9, '10': 'installationId'},
    {'1': 'selected_repos', '3': 2, '4': 3, '5': 9, '10': 'selectedRepos'},
    {'1': 'installed_at', '3': 3, '4': 1, '5': 4, '10': 'installedAt'},
  ],
};

/// Descriptor for `GithubInstall`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List githubInstallDescriptor = $convert.base64Decode(
    'Cg1HaXRodWJJbnN0YWxsEicKD2luc3RhbGxhdGlvbl9pZBgBIAEoCVIOaW5zdGFsbGF0aW9uSW'
    'QSJQoOc2VsZWN0ZWRfcmVwb3MYAiADKAlSDXNlbGVjdGVkUmVwb3MSIQoMaW5zdGFsbGVkX2F0'
    'GAMgASgEUgtpbnN0YWxsZWRBdA==');

@$core.Deprecated('Use githubInstallListRequestDescriptor instead')
const GithubInstallListRequest$json = {
  '1': 'GithubInstallListRequest',
};

/// Descriptor for `GithubInstallListRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List githubInstallListRequestDescriptor =
    $convert.base64Decode('ChhHaXRodWJJbnN0YWxsTGlzdFJlcXVlc3Q=');

@$core.Deprecated('Use githubInstallListDescriptor instead')
const GithubInstallList$json = {
  '1': 'GithubInstallList',
  '2': [
    {
      '1': 'installs',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.GithubInstall',
      '10': 'installs'
    },
  ],
};

/// Descriptor for `GithubInstallList`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List githubInstallListDescriptor = $convert.base64Decode(
    'ChFHaXRodWJJbnN0YWxsTGlzdBIzCghpbnN0YWxscxgBIAMoCzIXLmFnZW50LnYxLkdpdGh1Yk'
    'luc3RhbGxSCGluc3RhbGxz');

@$core.Deprecated('Use githubInstallRefDescriptor instead')
const GithubInstallRef$json = {
  '1': 'GithubInstallRef',
  '2': [
    {'1': 'installation_id', '3': 1, '4': 1, '5': 9, '10': 'installationId'},
  ],
};

/// Descriptor for `GithubInstallRef`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List githubInstallRefDescriptor = $convert.base64Decode(
    'ChBHaXRodWJJbnN0YWxsUmVmEicKD2luc3RhbGxhdGlvbl9pZBgBIAEoCVIOaW5zdGFsbGF0aW'
    '9uSWQ=');

@$core.Deprecated('Use githubInstallDeleteReplyDescriptor instead')
const GithubInstallDeleteReply$json = {
  '1': 'GithubInstallDeleteReply',
  '2': [
    {'1': 'deleted', '3': 1, '4': 1, '5': 8, '10': 'deleted'},
  ],
};

/// Descriptor for `GithubInstallDeleteReply`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List githubInstallDeleteReplyDescriptor =
    $convert.base64Decode(
        'ChhHaXRodWJJbnN0YWxsRGVsZXRlUmVwbHkSGAoHZGVsZXRlZBgBIAEoCFIHZGVsZXRlZA==');

@$core.Deprecated('Use beginInstallRequestDescriptor instead')
const BeginInstallRequest$json = {
  '1': 'BeginInstallRequest',
};

/// Descriptor for `BeginInstallRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List beginInstallRequestDescriptor =
    $convert.base64Decode('ChNCZWdpbkluc3RhbGxSZXF1ZXN0');

@$core.Deprecated('Use beginInstallReplyDescriptor instead')
const BeginInstallReply$json = {
  '1': 'BeginInstallReply',
  '2': [
    {'1': 'install_url', '3': 1, '4': 1, '5': 9, '10': 'installUrl'},
    {'1': 'state', '3': 2, '4': 1, '5': 9, '10': 'state'},
    {'1': 'expires_at', '3': 3, '4': 1, '5': 4, '10': 'expiresAt'},
  ],
};

/// Descriptor for `BeginInstallReply`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List beginInstallReplyDescriptor = $convert.base64Decode(
    'ChFCZWdpbkluc3RhbGxSZXBseRIfCgtpbnN0YWxsX3VybBgBIAEoCVIKaW5zdGFsbFVybBIUCg'
    'VzdGF0ZRgCIAEoCVIFc3RhdGUSHQoKZXhwaXJlc19hdBgDIAEoBFIJZXhwaXJlc0F0');

@$core.Deprecated('Use completeInstallRequestDescriptor instead')
const CompleteInstallRequest$json = {
  '1': 'CompleteInstallRequest',
  '2': [
    {'1': 'state', '3': 1, '4': 1, '5': 9, '10': 'state'},
    {'1': 'code', '3': 2, '4': 1, '5': 9, '10': 'code'},
    {'1': 'installation_id', '3': 3, '4': 1, '5': 9, '10': 'installationId'},
  ],
};

/// Descriptor for `CompleteInstallRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List completeInstallRequestDescriptor = $convert.base64Decode(
    'ChZDb21wbGV0ZUluc3RhbGxSZXF1ZXN0EhQKBXN0YXRlGAEgASgJUgVzdGF0ZRISCgRjb2RlGA'
    'IgASgJUgRjb2RlEicKD2luc3RhbGxhdGlvbl9pZBgDIAEoCVIOaW5zdGFsbGF0aW9uSWQ=');
