// This is a generated file - do not edit.
//
// Generated from agent/v1/ast.proto.

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

@$core.Deprecated('Use symbolKindDescriptor instead')
const SymbolKind$json = {
  '1': 'SymbolKind',
  '2': [
    {'1': 'SYMBOL_KIND_UNKNOWN', '2': 0},
    {'1': 'SYMBOL_KIND_FUNC', '2': 1},
    {'1': 'SYMBOL_KIND_METHOD', '2': 2},
    {'1': 'SYMBOL_KIND_INTERFACE', '2': 3},
    {'1': 'SYMBOL_KIND_STRUCT', '2': 4},
    {'1': 'SYMBOL_KIND_TYPE', '2': 5},
    {'1': 'SYMBOL_KIND_FIELD', '2': 6},
  ],
};

/// Descriptor for `SymbolKind`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List symbolKindDescriptor = $convert.base64Decode(
    'CgpTeW1ib2xLaW5kEhcKE1NZTUJPTF9LSU5EX1VOS05PV04QABIUChBTWU1CT0xfS0lORF9GVU'
    '5DEAESFgoSU1lNQk9MX0tJTkRfTUVUSE9EEAISGQoVU1lNQk9MX0tJTkRfSU5URVJGQUNFEAMS'
    'FgoSU1lNQk9MX0tJTkRfU1RSVUNUEAQSFAoQU1lNQk9MX0tJTkRfVFlQRRAFEhUKEVNZTUJPTF'
    '9LSU5EX0ZJRUxEEAY=');

@$core.Deprecated('Use symbolDescriptor instead')
const Symbol$json = {
  '1': 'Symbol',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 13, '10': 'id'},
    {
      '1': 'kind',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.SymbolKind',
      '10': 'kind'
    },
    {'1': 'name', '3': 3, '4': 1, '5': 9, '10': 'name'},
    {'1': 'recv', '3': 4, '4': 1, '5': 9, '10': 'recv'},
    {'1': 'package', '3': 5, '4': 1, '5': 9, '10': 'package'},
    {'1': 'file', '3': 6, '4': 1, '5': 9, '10': 'file'},
    {'1': 'line', '3': 7, '4': 1, '5': 13, '10': 'line'},
    {'1': 'exported', '3': 8, '4': 1, '5': 8, '10': 'exported'},
  ],
};

/// Descriptor for `Symbol`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List symbolDescriptor = $convert.base64Decode(
    'CgZTeW1ib2wSDgoCaWQYASABKA1SAmlkEigKBGtpbmQYAiABKA4yFC5hZ2VudC52MS5TeW1ib2'
    'xLaW5kUgRraW5kEhIKBG5hbWUYAyABKAlSBG5hbWUSEgoEcmVjdhgEIAEoCVIEcmVjdhIYCgdw'
    'YWNrYWdlGAUgASgJUgdwYWNrYWdlEhIKBGZpbGUYBiABKAlSBGZpbGUSEgoEbGluZRgHIAEoDV'
    'IEbGluZRIaCghleHBvcnRlZBgIIAEoCFIIZXhwb3J0ZWQ=');

@$core.Deprecated('Use symbolRefDescriptor instead')
const SymbolRef$json = {
  '1': 'SymbolRef',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 13, '9': 0, '10': 'id', '17': true},
    {'1': 'name', '3': 2, '4': 1, '5': 9, '10': 'name'},
    {
      '1': 'package',
      '3': 3,
      '4': 1,
      '5': 9,
      '9': 1,
      '10': 'package',
      '17': true
    },
    {'1': 'recv', '3': 4, '4': 1, '5': 9, '9': 2, '10': 'recv', '17': true},
  ],
  '8': [
    {'1': '_id'},
    {'1': '_package'},
    {'1': '_recv'},
  ],
};

/// Descriptor for `SymbolRef`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List symbolRefDescriptor = $convert.base64Decode(
    'CglTeW1ib2xSZWYSEwoCaWQYASABKA1IAFICaWSIAQESEgoEbmFtZRgCIAEoCVIEbmFtZRIdCg'
    'dwYWNrYWdlGAMgASgJSAFSB3BhY2thZ2WIAQESFwoEcmVjdhgEIAEoCUgCUgRyZWN2iAEBQgUK'
    'A19pZEIKCghfcGFja2FnZUIHCgVfcmVjdg==');

@$core.Deprecated('Use callEdgeDescriptor instead')
const CallEdge$json = {
  '1': 'CallEdge',
  '2': [
    {'1': 'caller_id', '3': 1, '4': 1, '5': 13, '10': 'callerId'},
    {'1': 'callee_id', '3': 2, '4': 1, '5': 13, '10': 'calleeId'},
  ],
};

/// Descriptor for `CallEdge`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List callEdgeDescriptor = $convert.base64Decode(
    'CghDYWxsRWRnZRIbCgljYWxsZXJfaWQYASABKA1SCGNhbGxlcklkEhsKCWNhbGxlZV9pZBgCIA'
    'EoDVIIY2FsbGVlSWQ=');

@$core.Deprecated('Use astCallGraphDescriptor instead')
const AstCallGraph$json = {
  '1': 'AstCallGraph',
  '2': [
    {
      '1': 'nodes',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Symbol',
      '10': 'nodes'
    },
    {
      '1': 'edges',
      '3': 2,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.CallEdge',
      '10': 'edges'
    },
    {'1': 'roots', '3': 3, '4': 3, '5': 13, '10': 'roots'},
    {'1': 'truncated', '3': 4, '4': 1, '5': 8, '10': 'truncated'},
    {'1': 'backend', '3': 5, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `AstCallGraph`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List astCallGraphDescriptor = $convert.base64Decode(
    'CgxBc3RDYWxsR3JhcGgSJgoFbm9kZXMYASADKAsyEC5hZ2VudC52MS5TeW1ib2xSBW5vZGVzEi'
    'gKBWVkZ2VzGAIgAygLMhIuYWdlbnQudjEuQ2FsbEVkZ2VSBWVkZ2VzEhQKBXJvb3RzGAMgAygN'
    'UgVyb290cxIcCgl0cnVuY2F0ZWQYBCABKAhSCXRydW5jYXRlZBIYCgdiYWNrZW5kGAUgASgJUg'
    'diYWNrZW5k');

@$core.Deprecated('Use callPathDescriptor instead')
const CallPath$json = {
  '1': 'CallPath',
  '2': [
    {
      '1': 'nodes',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Symbol',
      '10': 'nodes'
    },
  ],
};

/// Descriptor for `CallPath`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List callPathDescriptor = $convert.base64Decode(
    'CghDYWxsUGF0aBImCgVub2RlcxgBIAMoCzIQLmFnZW50LnYxLlN5bWJvbFIFbm9kZXM=');

@$core.Deprecated('Use astCapabilitiesDescriptor instead')
const AstCapabilities$json = {
  '1': 'AstCapabilities',
  '2': [
    {'1': 'backend', '3': 1, '4': 1, '5': 9, '10': 'backend'},
    {'1': 'languages', '3': 2, '4': 3, '5': 9, '10': 'languages'},
    {'1': 'verbs', '3': 3, '4': 3, '5': 9, '10': 'verbs'},
    {'1': 'incremental', '3': 4, '4': 1, '5': 8, '10': 'incremental'},
  ],
};

/// Descriptor for `AstCapabilities`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List astCapabilitiesDescriptor = $convert.base64Decode(
    'Cg9Bc3RDYXBhYmlsaXRpZXMSGAoHYmFja2VuZBgBIAEoCVIHYmFja2VuZBIcCglsYW5ndWFnZX'
    'MYAiADKAlSCWxhbmd1YWdlcxIUCgV2ZXJicxgDIAMoCVIFdmVyYnMSIAoLaW5jcmVtZW50YWwY'
    'BCABKAhSC2luY3JlbWVudGFs');

@$core.Deprecated('Use astStatusRequestDescriptor instead')
const AstStatusRequest$json = {
  '1': 'AstStatusRequest',
  '2': [
    {'1': 'backend', '3': 1, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `AstStatusRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List astStatusRequestDescriptor = $convert.base64Decode(
    'ChBBc3RTdGF0dXNSZXF1ZXN0EhgKB2JhY2tlbmQYASABKAlSB2JhY2tlbmQ=');

@$core.Deprecated('Use astStatusResponseDescriptor instead')
const AstStatusResponse$json = {
  '1': 'AstStatusResponse',
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

/// Descriptor for `AstStatusResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List astStatusResponseDescriptor = $convert.base64Decode(
    'ChFBc3RTdGF0dXNSZXNwb25zZRIxCghiYWNrZW5kcxgBIAMoCzIVLmFnZW50LnYxLkluZGV4U3'
    'RhdHVzUghiYWNrZW5kcw==');

@$core.Deprecated('Use astCapabilitiesRequestDescriptor instead')
const AstCapabilitiesRequest$json = {
  '1': 'AstCapabilitiesRequest',
  '2': [
    {'1': 'backend', '3': 1, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `AstCapabilitiesRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List astCapabilitiesRequestDescriptor =
    $convert.base64Decode(
        'ChZBc3RDYXBhYmlsaXRpZXNSZXF1ZXN0EhgKB2JhY2tlbmQYASABKAlSB2JhY2tlbmQ=');

@$core.Deprecated('Use astCapabilitiesResponseDescriptor instead')
const AstCapabilitiesResponse$json = {
  '1': 'AstCapabilitiesResponse',
  '2': [
    {
      '1': 'backends',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.AstCapabilities',
      '10': 'backends'
    },
  ],
};

/// Descriptor for `AstCapabilitiesResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List astCapabilitiesResponseDescriptor =
    $convert.base64Decode(
        'ChdBc3RDYXBhYmlsaXRpZXNSZXNwb25zZRI1CghiYWNrZW5kcxgBIAMoCzIZLmFnZW50LnYxLk'
        'FzdENhcGFiaWxpdGllc1IIYmFja2VuZHM=');

@$core.Deprecated('Use astReindexRequestDescriptor instead')
const AstReindexRequest$json = {
  '1': 'AstReindexRequest',
  '2': [
    {'1': 'backend', '3': 1, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `AstReindexRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List astReindexRequestDescriptor = $convert.base64Decode(
    'ChFBc3RSZWluZGV4UmVxdWVzdBIYCgdiYWNrZW5kGAEgASgJUgdiYWNrZW5k');

@$core.Deprecated('Use findSymbolRequestDescriptor instead')
const FindSymbolRequest$json = {
  '1': 'FindSymbolRequest',
  '2': [
    {'1': 'name', '3': 1, '4': 1, '5': 9, '10': 'name'},
    {
      '1': 'kind',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.SymbolKind',
      '9': 0,
      '10': 'kind',
      '17': true
    },
    {
      '1': 'package',
      '3': 3,
      '4': 1,
      '5': 9,
      '9': 1,
      '10': 'package',
      '17': true
    },
    {'1': 'exact', '3': 4, '4': 1, '5': 8, '10': 'exact'},
    {'1': 'limit', '3': 5, '4': 1, '5': 4, '10': 'limit'},
    {'1': 'backend', '3': 6, '4': 1, '5': 9, '10': 'backend'},
  ],
  '8': [
    {'1': '_kind'},
    {'1': '_package'},
  ],
};

/// Descriptor for `FindSymbolRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List findSymbolRequestDescriptor = $convert.base64Decode(
    'ChFGaW5kU3ltYm9sUmVxdWVzdBISCgRuYW1lGAEgASgJUgRuYW1lEi0KBGtpbmQYAiABKA4yFC'
    '5hZ2VudC52MS5TeW1ib2xLaW5kSABSBGtpbmSIAQESHQoHcGFja2FnZRgDIAEoCUgBUgdwYWNr'
    'YWdliAEBEhQKBWV4YWN0GAQgASgIUgVleGFjdBIUCgVsaW1pdBgFIAEoBFIFbGltaXQSGAoHYm'
    'Fja2VuZBgGIAEoCVIHYmFja2VuZEIHCgVfa2luZEIKCghfcGFja2FnZQ==');

@$core.Deprecated('Use symbolListDescriptor instead')
const SymbolList$json = {
  '1': 'SymbolList',
  '2': [
    {
      '1': 'symbols',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Symbol',
      '10': 'symbols'
    },
    {'1': 'backend', '3': 2, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `SymbolList`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List symbolListDescriptor = $convert.base64Decode(
    'CgpTeW1ib2xMaXN0EioKB3N5bWJvbHMYASADKAsyEC5hZ2VudC52MS5TeW1ib2xSB3N5bWJvbH'
    'MSGAoHYmFja2VuZBgCIAEoCVIHYmFja2VuZA==');

@$core.Deprecated('Use implementationsRequestDescriptor instead')
const ImplementationsRequest$json = {
  '1': 'ImplementationsRequest',
  '2': [
    {
      '1': 'iface',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.SymbolRef',
      '10': 'iface'
    },
    {'1': 'backend', '3': 2, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `ImplementationsRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List implementationsRequestDescriptor =
    $convert.base64Decode(
        'ChZJbXBsZW1lbnRhdGlvbnNSZXF1ZXN0EikKBWlmYWNlGAEgASgLMhMuYWdlbnQudjEuU3ltYm'
        '9sUmVmUgVpZmFjZRIYCgdiYWNrZW5kGAIgASgJUgdiYWNrZW5k');

@$core.Deprecated('Use interfaceOfRequestDescriptor instead')
const InterfaceOfRequest$json = {
  '1': 'InterfaceOfRequest',
  '2': [
    {
      '1': 'concrete_type',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.SymbolRef',
      '10': 'concreteType'
    },
    {'1': 'backend', '3': 2, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `InterfaceOfRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List interfaceOfRequestDescriptor = $convert.base64Decode(
    'ChJJbnRlcmZhY2VPZlJlcXVlc3QSOAoNY29uY3JldGVfdHlwZRgBIAEoCzITLmFnZW50LnYxLl'
    'N5bWJvbFJlZlIMY29uY3JldGVUeXBlEhgKB2JhY2tlbmQYAiABKAlSB2JhY2tlbmQ=');

@$core.Deprecated('Use callersRequestDescriptor instead')
const CallersRequest$json = {
  '1': 'CallersRequest',
  '2': [
    {
      '1': 'target',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.SymbolRef',
      '10': 'target'
    },
    {'1': 'hops', '3': 2, '4': 1, '5': 13, '10': 'hops'},
    {'1': 'backend', '3': 3, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `CallersRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List callersRequestDescriptor = $convert.base64Decode(
    'Cg5DYWxsZXJzUmVxdWVzdBIrCgZ0YXJnZXQYASABKAsyEy5hZ2VudC52MS5TeW1ib2xSZWZSBn'
    'RhcmdldBISCgRob3BzGAIgASgNUgRob3BzEhgKB2JhY2tlbmQYAyABKAlSB2JhY2tlbmQ=');

@$core.Deprecated('Use calleesRequestDescriptor instead')
const CalleesRequest$json = {
  '1': 'CalleesRequest',
  '2': [
    {
      '1': 'target',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.SymbolRef',
      '10': 'target'
    },
    {'1': 'hops', '3': 2, '4': 1, '5': 13, '10': 'hops'},
    {'1': 'backend', '3': 3, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `CalleesRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List calleesRequestDescriptor = $convert.base64Decode(
    'Cg5DYWxsZWVzUmVxdWVzdBIrCgZ0YXJnZXQYASABKAsyEy5hZ2VudC52MS5TeW1ib2xSZWZSBn'
    'RhcmdldBISCgRob3BzGAIgASgNUgRob3BzEhgKB2JhY2tlbmQYAyABKAlSB2JhY2tlbmQ=');

@$core.Deprecated('Use callchainRequestDescriptor instead')
const CallchainRequest$json = {
  '1': 'CallchainRequest',
  '2': [
    {
      '1': 'from',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.SymbolRef',
      '10': 'from'
    },
    {
      '1': 'to',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.SymbolRef',
      '10': 'to'
    },
    {'1': 'max_paths', '3': 3, '4': 1, '5': 13, '10': 'maxPaths'},
    {'1': 'backend', '3': 4, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `CallchainRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List callchainRequestDescriptor = $convert.base64Decode(
    'ChBDYWxsY2hhaW5SZXF1ZXN0EicKBGZyb20YASABKAsyEy5hZ2VudC52MS5TeW1ib2xSZWZSBG'
    'Zyb20SIwoCdG8YAiABKAsyEy5hZ2VudC52MS5TeW1ib2xSZWZSAnRvEhsKCW1heF9wYXRocxgD'
    'IAEoDVIIbWF4UGF0aHMSGAoHYmFja2VuZBgEIAEoCVIHYmFja2VuZA==');

@$core.Deprecated('Use callchainResponseDescriptor instead')
const CallchainResponse$json = {
  '1': 'CallchainResponse',
  '2': [
    {
      '1': 'paths',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.CallPath',
      '10': 'paths'
    },
    {'1': 'backend', '3': 2, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `CallchainResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List callchainResponseDescriptor = $convert.base64Decode(
    'ChFDYWxsY2hhaW5SZXNwb25zZRIoCgVwYXRocxgBIAMoCzISLmFnZW50LnYxLkNhbGxQYXRoUg'
    'VwYXRocxIYCgdiYWNrZW5kGAIgASgJUgdiYWNrZW5k');

@$core.Deprecated('Use blastRadiusRequestDescriptor instead')
const BlastRadiusRequest$json = {
  '1': 'BlastRadiusRequest',
  '2': [
    {'1': 'changed', '3': 1, '4': 3, '5': 9, '10': 'changed'},
    {'1': 'hops', '3': 2, '4': 1, '5': 13, '10': 'hops'},
    {'1': 'backend', '3': 3, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `BlastRadiusRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List blastRadiusRequestDescriptor = $convert.base64Decode(
    'ChJCbGFzdFJhZGl1c1JlcXVlc3QSGAoHY2hhbmdlZBgBIAMoCVIHY2hhbmdlZBISCgRob3BzGA'
    'IgASgNUgRob3BzEhgKB2JhY2tlbmQYAyABKAlSB2JhY2tlbmQ=');

@$core.Deprecated('Use dependencyPathRequestDescriptor instead')
const DependencyPathRequest$json = {
  '1': 'DependencyPathRequest',
  '2': [
    {'1': 'from_package', '3': 1, '4': 1, '5': 9, '10': 'fromPackage'},
    {'1': 'to_package', '3': 2, '4': 1, '5': 9, '10': 'toPackage'},
    {'1': 'backend', '3': 3, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `DependencyPathRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List dependencyPathRequestDescriptor = $convert.base64Decode(
    'ChVEZXBlbmRlbmN5UGF0aFJlcXVlc3QSIQoMZnJvbV9wYWNrYWdlGAEgASgJUgtmcm9tUGFja2'
    'FnZRIdCgp0b19wYWNrYWdlGAIgASgJUgl0b1BhY2thZ2USGAoHYmFja2VuZBgDIAEoCVIHYmFj'
    'a2VuZA==');

@$core.Deprecated('Use dependencyPathResponseDescriptor instead')
const DependencyPathResponse$json = {
  '1': 'DependencyPathResponse',
  '2': [
    {'1': 'packages', '3': 1, '4': 3, '5': 9, '10': 'packages'},
    {'1': 'backend', '3': 2, '4': 1, '5': 9, '10': 'backend'},
  ],
};

/// Descriptor for `DependencyPathResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List dependencyPathResponseDescriptor =
    $convert.base64Decode(
        'ChZEZXBlbmRlbmN5UGF0aFJlc3BvbnNlEhoKCHBhY2thZ2VzGAEgAygJUghwYWNrYWdlcxIYCg'
        'diYWNrZW5kGAIgASgJUgdiYWNrZW5k');
