// This is a generated file - do not edit.
//
// Generated from agent/v1/auth.proto.

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

@$core.Deprecated('Use exchangeRequestDescriptor instead')
const ExchangeRequest$json = {
  '1': 'ExchangeRequest',
  '2': [
    {'1': 'id_token', '3': 1, '4': 1, '5': 9, '10': 'idToken'},
    {'1': 'client_kind', '3': 2, '4': 1, '5': 9, '10': 'clientKind'},
    {'1': 'use_client_cert', '3': 3, '4': 1, '5': 8, '10': 'useClientCert'},
    {'1': 'code', '3': 4, '4': 1, '5': 9, '10': 'code'},
    {'1': 'state', '3': 5, '4': 1, '5': 9, '10': 'state'},
    {'1': 'code_verifier', '3': 6, '4': 1, '5': 9, '10': 'codeVerifier'},
  ],
};

/// Descriptor for `ExchangeRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List exchangeRequestDescriptor = $convert.base64Decode(
    'Cg9FeGNoYW5nZVJlcXVlc3QSGQoIaWRfdG9rZW4YASABKAlSB2lkVG9rZW4SHwoLY2xpZW50X2'
    'tpbmQYAiABKAlSCmNsaWVudEtpbmQSJgoPdXNlX2NsaWVudF9jZXJ0GAMgASgIUg11c2VDbGll'
    'bnRDZXJ0EhIKBGNvZGUYBCABKAlSBGNvZGUSFAoFc3RhdGUYBSABKAlSBXN0YXRlEiMKDWNvZG'
    'VfdmVyaWZpZXIYBiABKAlSDGNvZGVWZXJpZmllcg==');

@$core.Deprecated('Use exchangeResponseDescriptor instead')
const ExchangeResponse$json = {
  '1': 'ExchangeResponse',
  '2': [
    {'1': 'access_token', '3': 1, '4': 1, '5': 9, '10': 'accessToken'},
    {'1': 'token_type', '3': 2, '4': 1, '5': 9, '10': 'tokenType'},
    {'1': 'expires_at', '3': 3, '4': 1, '5': 4, '10': 'expiresAt'},
    {
      '1': 'principal',
      '3': 4,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.WhoAmIResponse',
      '10': 'principal'
    },
    {'1': 'refresh_handle', '3': 5, '4': 1, '5': 9, '10': 'refreshHandle'},
    {
      '1': 'session_expires_at',
      '3': 6,
      '4': 1,
      '5': 4,
      '10': 'sessionExpiresAt'
    },
  ],
};

/// Descriptor for `ExchangeResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List exchangeResponseDescriptor = $convert.base64Decode(
    'ChBFeGNoYW5nZVJlc3BvbnNlEiEKDGFjY2Vzc190b2tlbhgBIAEoCVILYWNjZXNzVG9rZW4SHQ'
    'oKdG9rZW5fdHlwZRgCIAEoCVIJdG9rZW5UeXBlEh0KCmV4cGlyZXNfYXQYAyABKARSCWV4cGly'
    'ZXNBdBI2CglwcmluY2lwYWwYBCABKAsyGC5hZ2VudC52MS5XaG9BbUlSZXNwb25zZVIJcHJpbm'
    'NpcGFsEiUKDnJlZnJlc2hfaGFuZGxlGAUgASgJUg1yZWZyZXNoSGFuZGxlEiwKEnNlc3Npb25f'
    'ZXhwaXJlc19hdBgGIAEoBFIQc2Vzc2lvbkV4cGlyZXNBdA==');

@$core.Deprecated('Use issuersRequestDescriptor instead')
const IssuersRequest$json = {
  '1': 'IssuersRequest',
};

/// Descriptor for `IssuersRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List issuersRequestDescriptor =
    $convert.base64Decode('Cg5Jc3N1ZXJzUmVxdWVzdA==');

@$core.Deprecated('Use issuersResponseDescriptor instead')
const IssuersResponse$json = {
  '1': 'IssuersResponse',
  '2': [
    {
      '1': 'issuers',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.LoginIssuer',
      '10': 'issuers'
    },
  ],
};

/// Descriptor for `IssuersResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List issuersResponseDescriptor = $convert.base64Decode(
    'Cg9Jc3N1ZXJzUmVzcG9uc2USLwoHaXNzdWVycxgBIAMoCzIVLmFnZW50LnYxLkxvZ2luSXNzdW'
    'VyUgdpc3N1ZXJz');

@$core.Deprecated('Use loginIssuerDescriptor instead')
const LoginIssuer$json = {
  '1': 'LoginIssuer',
  '2': [
    {'1': 'name', '3': 1, '4': 1, '5': 9, '10': 'name'},
    {'1': 'profile', '3': 2, '4': 1, '5': 9, '10': 'profile'},
  ],
};

/// Descriptor for `LoginIssuer`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List loginIssuerDescriptor = $convert.base64Decode(
    'CgtMb2dpbklzc3VlchISCgRuYW1lGAEgASgJUgRuYW1lEhgKB3Byb2ZpbGUYAiABKAlSB3Byb2'
    'ZpbGU=');

@$core.Deprecated('Use beginRequestDescriptor instead')
const BeginRequest$json = {
  '1': 'BeginRequest',
  '2': [
    {'1': 'issuer', '3': 1, '4': 1, '5': 9, '10': 'issuer'},
    {'1': 'redirect_uri', '3': 2, '4': 1, '5': 9, '10': 'redirectUri'},
    {'1': 'code_challenge', '3': 3, '4': 1, '5': 9, '10': 'codeChallenge'},
  ],
};

/// Descriptor for `BeginRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List beginRequestDescriptor = $convert.base64Decode(
    'CgxCZWdpblJlcXVlc3QSFgoGaXNzdWVyGAEgASgJUgZpc3N1ZXISIQoMcmVkaXJlY3RfdXJpGA'
    'IgASgJUgtyZWRpcmVjdFVyaRIlCg5jb2RlX2NoYWxsZW5nZRgDIAEoCVINY29kZUNoYWxsZW5n'
    'ZQ==');

@$core.Deprecated('Use beginResponseDescriptor instead')
const BeginResponse$json = {
  '1': 'BeginResponse',
  '2': [
    {'1': 'authorize_url', '3': 1, '4': 1, '5': 9, '10': 'authorizeUrl'},
    {'1': 'state', '3': 2, '4': 1, '5': 9, '10': 'state'},
    {'1': 'expires_at', '3': 3, '4': 1, '5': 4, '10': 'expiresAt'},
  ],
};

/// Descriptor for `BeginResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List beginResponseDescriptor = $convert.base64Decode(
    'Cg1CZWdpblJlc3BvbnNlEiMKDWF1dGhvcml6ZV91cmwYASABKAlSDGF1dGhvcml6ZVVybBIUCg'
    'VzdGF0ZRgCIAEoCVIFc3RhdGUSHQoKZXhwaXJlc19hdBgDIAEoBFIJZXhwaXJlc0F0');

@$core.Deprecated('Use jwksRequestDescriptor instead')
const JwksRequest$json = {
  '1': 'JwksRequest',
};

/// Descriptor for `JwksRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List jwksRequestDescriptor =
    $convert.base64Decode('CgtKd2tzUmVxdWVzdA==');

@$core.Deprecated('Use jwksResponseDescriptor instead')
const JwksResponse$json = {
  '1': 'JwksResponse',
  '2': [
    {'1': 'jwks_json', '3': 1, '4': 1, '5': 9, '10': 'jwksJson'},
  ],
};

/// Descriptor for `JwksResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List jwksResponseDescriptor = $convert.base64Decode(
    'CgxKd2tzUmVzcG9uc2USGwoJandrc19qc29uGAEgASgJUghqd2tzSnNvbg==');

@$core.Deprecated('Use whoAmIRequestDescriptor instead')
const WhoAmIRequest$json = {
  '1': 'WhoAmIRequest',
};

/// Descriptor for `WhoAmIRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List whoAmIRequestDescriptor =
    $convert.base64Decode('Cg1XaG9BbUlSZXF1ZXN0');

@$core.Deprecated('Use whoAmIResponseDescriptor instead')
const WhoAmIResponse$json = {
  '1': 'WhoAmIResponse',
  '2': [
    {'1': 'tenant', '3': 1, '4': 1, '5': 9, '10': 'tenant'},
    {'1': 'subject', '3': 2, '4': 1, '5': 9, '10': 'subject'},
    {'1': 'issuer', '3': 3, '4': 1, '5': 9, '10': 'issuer'},
    {'1': 'email', '3': 4, '4': 1, '5': 9, '10': 'email'},
    {'1': 'roles', '3': 5, '4': 3, '5': 9, '10': 'roles'},
    {'1': 'permissions', '3': 6, '4': 3, '5': 9, '10': 'permissions'},
    {'1': 'perms_ref', '3': 7, '4': 1, '5': 8, '10': 'permsRef'},
    {'1': 'amr', '3': 8, '4': 3, '5': 9, '10': 'amr'},
    {'1': 'expires_at', '3': 9, '4': 1, '5': 4, '10': 'expiresAt'},
    {'1': 'sid', '3': 10, '4': 1, '5': 9, '10': 'sid'},
  ],
};

/// Descriptor for `WhoAmIResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List whoAmIResponseDescriptor = $convert.base64Decode(
    'Cg5XaG9BbUlSZXNwb25zZRIWCgZ0ZW5hbnQYASABKAlSBnRlbmFudBIYCgdzdWJqZWN0GAIgAS'
    'gJUgdzdWJqZWN0EhYKBmlzc3VlchgDIAEoCVIGaXNzdWVyEhQKBWVtYWlsGAQgASgJUgVlbWFp'
    'bBIUCgVyb2xlcxgFIAMoCVIFcm9sZXMSIAoLcGVybWlzc2lvbnMYBiADKAlSC3Blcm1pc3Npb2'
    '5zEhsKCXBlcm1zX3JlZhgHIAEoCFIIcGVybXNSZWYSEAoDYW1yGAggAygJUgNhbXISHQoKZXhw'
    'aXJlc19hdBgJIAEoBFIJZXhwaXJlc0F0EhAKA3NpZBgKIAEoCVIDc2lk');

@$core.Deprecated('Use refreshRequestDescriptor instead')
const RefreshRequest$json = {
  '1': 'RefreshRequest',
  '2': [
    {'1': 'refresh_handle', '3': 1, '4': 1, '5': 9, '10': 'refreshHandle'},
  ],
};

/// Descriptor for `RefreshRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List refreshRequestDescriptor = $convert.base64Decode(
    'Cg5SZWZyZXNoUmVxdWVzdBIlCg5yZWZyZXNoX2hhbmRsZRgBIAEoCVINcmVmcmVzaEhhbmRsZQ'
    '==');

@$core.Deprecated('Use logoutRequestDescriptor instead')
const LogoutRequest$json = {
  '1': 'LogoutRequest',
};

/// Descriptor for `LogoutRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List logoutRequestDescriptor =
    $convert.base64Decode('Cg1Mb2dvdXRSZXF1ZXN0');

@$core.Deprecated('Use logoutResponseDescriptor instead')
const LogoutResponse$json = {
  '1': 'LogoutResponse',
  '2': [
    {'1': 'revoked', '3': 1, '4': 1, '5': 8, '10': 'revoked'},
  ],
};

/// Descriptor for `LogoutResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List logoutResponseDescriptor = $convert
    .base64Decode('Cg5Mb2dvdXRSZXNwb25zZRIYCgdyZXZva2VkGAEgASgIUgdyZXZva2Vk');

@$core.Deprecated('Use listMySessionsRequestDescriptor instead')
const ListMySessionsRequest$json = {
  '1': 'ListMySessionsRequest',
};

/// Descriptor for `ListMySessionsRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List listMySessionsRequestDescriptor =
    $convert.base64Decode('ChVMaXN0TXlTZXNzaW9uc1JlcXVlc3Q=');

@$core.Deprecated('Use listSessionsRequestDescriptor instead')
const ListSessionsRequest$json = {
  '1': 'ListSessionsRequest',
  '2': [
    {'1': 'tenant', '3': 1, '4': 1, '5': 9, '10': 'tenant'},
  ],
};

/// Descriptor for `ListSessionsRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List listSessionsRequestDescriptor =
    $convert.base64Decode(
        'ChNMaXN0U2Vzc2lvbnNSZXF1ZXN0EhYKBnRlbmFudBgBIAEoCVIGdGVuYW50');

@$core.Deprecated('Use listSessionsResponseDescriptor instead')
const ListSessionsResponse$json = {
  '1': 'ListSessionsResponse',
  '2': [
    {
      '1': 'sessions',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.AuthSessionInfo',
      '10': 'sessions'
    },
  ],
};

/// Descriptor for `ListSessionsResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List listSessionsResponseDescriptor = $convert.base64Decode(
    'ChRMaXN0U2Vzc2lvbnNSZXNwb25zZRI1CghzZXNzaW9ucxgBIAMoCzIZLmFnZW50LnYxLkF1dG'
    'hTZXNzaW9uSW5mb1IIc2Vzc2lvbnM=');

@$core.Deprecated('Use revokeMySessionRequestDescriptor instead')
const RevokeMySessionRequest$json = {
  '1': 'RevokeMySessionRequest',
  '2': [
    {'1': 'sid', '3': 1, '4': 1, '5': 9, '10': 'sid'},
  ],
};

/// Descriptor for `RevokeMySessionRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List revokeMySessionRequestDescriptor = $convert
    .base64Decode('ChZSZXZva2VNeVNlc3Npb25SZXF1ZXN0EhAKA3NpZBgBIAEoCVIDc2lk');

@$core.Deprecated('Use revokeSessionRequestDescriptor instead')
const RevokeSessionRequest$json = {
  '1': 'RevokeSessionRequest',
  '2': [
    {'1': 'tenant', '3': 1, '4': 1, '5': 9, '10': 'tenant'},
    {'1': 'sid', '3': 2, '4': 1, '5': 9, '10': 'sid'},
  ],
};

/// Descriptor for `RevokeSessionRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List revokeSessionRequestDescriptor = $convert.base64Decode(
    'ChRSZXZva2VTZXNzaW9uUmVxdWVzdBIWCgZ0ZW5hbnQYASABKAlSBnRlbmFudBIQCgNzaWQYAi'
    'ABKAlSA3NpZA==');

@$core.Deprecated('Use revokeSessionResponseDescriptor instead')
const RevokeSessionResponse$json = {
  '1': 'RevokeSessionResponse',
  '2': [
    {'1': 'revoked', '3': 1, '4': 1, '5': 8, '10': 'revoked'},
  ],
};

/// Descriptor for `RevokeSessionResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List revokeSessionResponseDescriptor =
    $convert.base64Decode(
        'ChVSZXZva2VTZXNzaW9uUmVzcG9uc2USGAoHcmV2b2tlZBgBIAEoCFIHcmV2b2tlZA==');

@$core.Deprecated('Use authSessionInfoDescriptor instead')
const AuthSessionInfo$json = {
  '1': 'AuthSessionInfo',
  '2': [
    {'1': 'sid', '3': 1, '4': 1, '5': 9, '10': 'sid'},
    {'1': 'tenant', '3': 2, '4': 1, '5': 9, '10': 'tenant'},
    {'1': 'subject', '3': 3, '4': 1, '5': 9, '10': 'subject'},
    {'1': 'issuer', '3': 4, '4': 1, '5': 9, '10': 'issuer'},
    {'1': 'email', '3': 5, '4': 1, '5': 9, '10': 'email'},
    {'1': 'client_kind', '3': 6, '4': 1, '5': 9, '10': 'clientKind'},
    {'1': 'created_at', '3': 7, '4': 1, '5': 4, '10': 'createdAt'},
    {'1': 'last_seen_at', '3': 8, '4': 1, '5': 4, '10': 'lastSeenAt'},
    {'1': 'expires_at', '3': 9, '4': 1, '5': 4, '10': 'expiresAt'},
    {'1': 'revoked_at', '3': 10, '4': 1, '5': 4, '10': 'revokedAt'},
    {'1': 'revoke_reason', '3': 11, '4': 1, '5': 9, '10': 'revokeReason'},
    {'1': 'current', '3': 12, '4': 1, '5': 8, '10': 'current'},
  ],
};

/// Descriptor for `AuthSessionInfo`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List authSessionInfoDescriptor = $convert.base64Decode(
    'Cg9BdXRoU2Vzc2lvbkluZm8SEAoDc2lkGAEgASgJUgNzaWQSFgoGdGVuYW50GAIgASgJUgZ0ZW'
    '5hbnQSGAoHc3ViamVjdBgDIAEoCVIHc3ViamVjdBIWCgZpc3N1ZXIYBCABKAlSBmlzc3VlchIU'
    'CgVlbWFpbBgFIAEoCVIFZW1haWwSHwoLY2xpZW50X2tpbmQYBiABKAlSCmNsaWVudEtpbmQSHQ'
    'oKY3JlYXRlZF9hdBgHIAEoBFIJY3JlYXRlZEF0EiAKDGxhc3Rfc2Vlbl9hdBgIIAEoBFIKbGFz'
    'dFNlZW5BdBIdCgpleHBpcmVzX2F0GAkgASgEUglleHBpcmVzQXQSHQoKcmV2b2tlZF9hdBgKIA'
    'EoBFIJcmV2b2tlZEF0EiMKDXJldm9rZV9yZWFzb24YCyABKAlSDHJldm9rZVJlYXNvbhIYCgdj'
    'dXJyZW50GAwgASgIUgdjdXJyZW50');

@$core.Deprecated('Use roleBindingDescriptor instead')
const RoleBinding$json = {
  '1': 'RoleBinding',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'tenant', '3': 2, '4': 1, '5': 9, '10': 'tenant'},
    {'1': 'subject_kind', '3': 3, '4': 1, '5': 9, '10': 'subjectKind'},
    {'1': 'subject', '3': 4, '4': 1, '5': 9, '10': 'subject'},
    {'1': 'roles', '3': 5, '4': 3, '5': 9, '10': 'roles'},
    {'1': 'granted_by', '3': 6, '4': 1, '5': 9, '10': 'grantedBy'},
    {'1': 'granted_at', '3': 7, '4': 1, '5': 4, '10': 'grantedAt'},
    {'1': 'expires_at', '3': 8, '4': 1, '5': 4, '10': 'expiresAt'},
  ],
};

/// Descriptor for `RoleBinding`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List roleBindingDescriptor = $convert.base64Decode(
    'CgtSb2xlQmluZGluZxIOCgJpZBgBIAEoCVICaWQSFgoGdGVuYW50GAIgASgJUgZ0ZW5hbnQSIQ'
    'oMc3ViamVjdF9raW5kGAMgASgJUgtzdWJqZWN0S2luZBIYCgdzdWJqZWN0GAQgASgJUgdzdWJq'
    'ZWN0EhQKBXJvbGVzGAUgAygJUgVyb2xlcxIdCgpncmFudGVkX2J5GAYgASgJUglncmFudGVkQn'
    'kSHQoKZ3JhbnRlZF9hdBgHIAEoBFIJZ3JhbnRlZEF0Eh0KCmV4cGlyZXNfYXQYCCABKARSCWV4'
    'cGlyZXNBdA==');

@$core.Deprecated('Use listBindingsRequestDescriptor instead')
const ListBindingsRequest$json = {
  '1': 'ListBindingsRequest',
  '2': [
    {'1': 'tenant', '3': 1, '4': 1, '5': 9, '10': 'tenant'},
  ],
};

/// Descriptor for `ListBindingsRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List listBindingsRequestDescriptor =
    $convert.base64Decode(
        'ChNMaXN0QmluZGluZ3NSZXF1ZXN0EhYKBnRlbmFudBgBIAEoCVIGdGVuYW50');

@$core.Deprecated('Use listBindingsResponseDescriptor instead')
const ListBindingsResponse$json = {
  '1': 'ListBindingsResponse',
  '2': [
    {
      '1': 'bindings',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.RoleBinding',
      '10': 'bindings'
    },
  ],
};

/// Descriptor for `ListBindingsResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List listBindingsResponseDescriptor = $convert.base64Decode(
    'ChRMaXN0QmluZGluZ3NSZXNwb25zZRIxCghiaW5kaW5ncxgBIAMoCzIVLmFnZW50LnYxLlJvbG'
    'VCaW5kaW5nUghiaW5kaW5ncw==');

@$core.Deprecated('Use getBindingRequestDescriptor instead')
const GetBindingRequest$json = {
  '1': 'GetBindingRequest',
  '2': [
    {'1': 'tenant', '3': 1, '4': 1, '5': 9, '10': 'tenant'},
    {'1': 'id', '3': 2, '4': 1, '5': 9, '10': 'id'},
  ],
};

/// Descriptor for `GetBindingRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List getBindingRequestDescriptor = $convert.base64Decode(
    'ChFHZXRCaW5kaW5nUmVxdWVzdBIWCgZ0ZW5hbnQYASABKAlSBnRlbmFudBIOCgJpZBgCIAEoCV'
    'ICaWQ=');

@$core.Deprecated('Use getBindingResponseDescriptor instead')
const GetBindingResponse$json = {
  '1': 'GetBindingResponse',
  '2': [
    {
      '1': 'binding',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.RoleBinding',
      '10': 'binding'
    },
  ],
};

/// Descriptor for `GetBindingResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List getBindingResponseDescriptor = $convert.base64Decode(
    'ChJHZXRCaW5kaW5nUmVzcG9uc2USLwoHYmluZGluZxgBIAEoCzIVLmFnZW50LnYxLlJvbGVCaW'
    '5kaW5nUgdiaW5kaW5n');

@$core.Deprecated('Use putBindingRequestDescriptor instead')
const PutBindingRequest$json = {
  '1': 'PutBindingRequest',
  '2': [
    {
      '1': 'binding',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.RoleBinding',
      '10': 'binding'
    },
    {'1': 'keep_sessions', '3': 2, '4': 1, '5': 8, '10': 'keepSessions'},
  ],
};

/// Descriptor for `PutBindingRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List putBindingRequestDescriptor = $convert.base64Decode(
    'ChFQdXRCaW5kaW5nUmVxdWVzdBIvCgdiaW5kaW5nGAEgASgLMhUuYWdlbnQudjEuUm9sZUJpbm'
    'RpbmdSB2JpbmRpbmcSIwoNa2VlcF9zZXNzaW9ucxgCIAEoCFIMa2VlcFNlc3Npb25z');

@$core.Deprecated('Use putBindingResponseDescriptor instead')
const PutBindingResponse$json = {
  '1': 'PutBindingResponse',
  '2': [
    {
      '1': 'binding',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.RoleBinding',
      '10': 'binding'
    },
    {'1': 'revoked_sessions', '3': 2, '4': 1, '5': 13, '10': 'revokedSessions'},
  ],
};

/// Descriptor for `PutBindingResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List putBindingResponseDescriptor = $convert.base64Decode(
    'ChJQdXRCaW5kaW5nUmVzcG9uc2USLwoHYmluZGluZxgBIAEoCzIVLmFnZW50LnYxLlJvbGVCaW'
    '5kaW5nUgdiaW5kaW5nEikKEHJldm9rZWRfc2Vzc2lvbnMYAiABKA1SD3Jldm9rZWRTZXNzaW9u'
    'cw==');

@$core.Deprecated('Use deleteBindingRequestDescriptor instead')
const DeleteBindingRequest$json = {
  '1': 'DeleteBindingRequest',
  '2': [
    {'1': 'tenant', '3': 1, '4': 1, '5': 9, '10': 'tenant'},
    {'1': 'id', '3': 2, '4': 1, '5': 9, '10': 'id'},
    {'1': 'keep_sessions', '3': 3, '4': 1, '5': 8, '10': 'keepSessions'},
  ],
};

/// Descriptor for `DeleteBindingRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List deleteBindingRequestDescriptor = $convert.base64Decode(
    'ChREZWxldGVCaW5kaW5nUmVxdWVzdBIWCgZ0ZW5hbnQYASABKAlSBnRlbmFudBIOCgJpZBgCIA'
    'EoCVICaWQSIwoNa2VlcF9zZXNzaW9ucxgDIAEoCFIMa2VlcFNlc3Npb25z');

@$core.Deprecated('Use deleteBindingResponseDescriptor instead')
const DeleteBindingResponse$json = {
  '1': 'DeleteBindingResponse',
  '2': [
    {'1': 'deleted', '3': 1, '4': 1, '5': 8, '10': 'deleted'},
    {'1': 'revoked_sessions', '3': 2, '4': 1, '5': 13, '10': 'revokedSessions'},
  ],
};

/// Descriptor for `DeleteBindingResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List deleteBindingResponseDescriptor = $convert.base64Decode(
    'ChVEZWxldGVCaW5kaW5nUmVzcG9uc2USGAoHZGVsZXRlZBgBIAEoCFIHZGVsZXRlZBIpChByZX'
    'Zva2VkX3Nlc3Npb25zGAIgASgNUg9yZXZva2VkU2Vzc2lvbnM=');
