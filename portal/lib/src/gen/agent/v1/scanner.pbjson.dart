// This is a generated file - do not edit.
//
// Generated from agent/v1/scanner.proto.

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

@$core.Deprecated('Use scanKindDescriptor instead')
const ScanKind$json = {
  '1': 'ScanKind',
  '2': [
    {'1': 'SCAN_KIND_TOOL_INPUT', '2': 0},
    {'1': 'SCAN_KIND_FILE_BODY', '2': 1},
    {'1': 'SCAN_KIND_WEB_CONTENT', '2': 2},
    {'1': 'SCAN_KIND_LOCKFILE', '2': 3},
  ],
};

/// Descriptor for `ScanKind`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List scanKindDescriptor = $convert.base64Decode(
    'CghTY2FuS2luZBIYChRTQ0FOX0tJTkRfVE9PTF9JTlBVVBAAEhcKE1NDQU5fS0lORF9GSUxFX0'
    'JPRFkQARIZChVTQ0FOX0tJTkRfV0VCX0NPTlRFTlQQAhIWChJTQ0FOX0tJTkRfTE9DS0ZJTEUQ'
    'Aw==');

@$core.Deprecated('Use scanSeverityDescriptor instead')
const ScanSeverity$json = {
  '1': 'ScanSeverity',
  '2': [
    {'1': 'SCAN_SEVERITY_INFO', '2': 0},
    {'1': 'SCAN_SEVERITY_LOW', '2': 1},
    {'1': 'SCAN_SEVERITY_MEDIUM', '2': 2},
    {'1': 'SCAN_SEVERITY_HIGH', '2': 3},
    {'1': 'SCAN_SEVERITY_CRITICAL', '2': 4},
  ],
};

/// Descriptor for `ScanSeverity`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List scanSeverityDescriptor = $convert.base64Decode(
    'CgxTY2FuU2V2ZXJpdHkSFgoSU0NBTl9TRVZFUklUWV9JTkZPEAASFQoRU0NBTl9TRVZFUklUWV'
    '9MT1cQARIYChRTQ0FOX1NFVkVSSVRZX01FRElVTRACEhYKElNDQU5fU0VWRVJJVFlfSElHSBAD'
    'EhoKFlNDQU5fU0VWRVJJVFlfQ1JJVElDQUwQBA==');

@$core.Deprecated('Use scanRequestDescriptor instead')
const ScanRequest$json = {
  '1': 'ScanRequest',
  '2': [
    {
      '1': 'kind',
      '3': 1,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ScanKind',
      '10': 'kind'
    },
    {'1': 'content', '3': 2, '4': 1, '5': 9, '10': 'content'},
  ],
};

/// Descriptor for `ScanRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List scanRequestDescriptor = $convert.base64Decode(
    'CgtTY2FuUmVxdWVzdBImCgRraW5kGAEgASgOMhIuYWdlbnQudjEuU2NhbktpbmRSBGtpbmQSGA'
    'oHY29udGVudBgCIAEoCVIHY29udGVudA==');

@$core.Deprecated('Use scanFindingDescriptor instead')
const ScanFinding$json = {
  '1': 'ScanFinding',
  '2': [
    {'1': 'rule', '3': 1, '4': 1, '5': 9, '10': 'rule'},
    {
      '1': 'severity',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ScanSeverity',
      '10': 'severity'
    },
    {'1': 'category', '3': 3, '4': 1, '5': 9, '10': 'category'},
    {'1': 'span_start', '3': 4, '4': 1, '5': 4, '10': 'spanStart'},
    {'1': 'span_end', '3': 5, '4': 1, '5': 4, '10': 'spanEnd'},
  ],
};

/// Descriptor for `ScanFinding`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List scanFindingDescriptor = $convert.base64Decode(
    'CgtTY2FuRmluZGluZxISCgRydWxlGAEgASgJUgRydWxlEjIKCHNldmVyaXR5GAIgASgOMhYuYW'
    'dlbnQudjEuU2NhblNldmVyaXR5UghzZXZlcml0eRIaCghjYXRlZ29yeRgDIAEoCVIIY2F0ZWdv'
    'cnkSHQoKc3Bhbl9zdGFydBgEIAEoBFIJc3BhblN0YXJ0EhkKCHNwYW5fZW5kGAUgASgEUgdzcG'
    'FuRW5k');

@$core.Deprecated('Use scanResponseDescriptor instead')
const ScanResponse$json = {
  '1': 'ScanResponse',
  '2': [
    {
      '1': 'findings',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.ScanFinding',
      '10': 'findings'
    },
  ],
};

/// Descriptor for `ScanResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List scanResponseDescriptor = $convert.base64Decode(
    'CgxTY2FuUmVzcG9uc2USMQoIZmluZGluZ3MYASADKAsyFS5hZ2VudC52MS5TY2FuRmluZGluZ1'
    'IIZmluZGluZ3M=');
