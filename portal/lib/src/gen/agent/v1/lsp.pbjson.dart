// This is a generated file - do not edit.
//
// Generated from agent/v1/lsp.proto.

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

@$core.Deprecated('Use lspMethodDescriptor instead')
const LspMethod$json = {
  '1': 'LspMethod',
  '2': [
    {'1': 'LSP_METHOD_DIAGNOSTICS', '2': 0},
    {'1': 'LSP_METHOD_HOVER', '2': 1},
    {'1': 'LSP_METHOD_DEFINITION', '2': 2},
    {'1': 'LSP_METHOD_REFERENCES', '2': 3},
    {'1': 'LSP_METHOD_RENAME', '2': 4},
    {'1': 'LSP_METHOD_DOCUMENT_SYMBOLS', '2': 5},
  ],
};

/// Descriptor for `LspMethod`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List lspMethodDescriptor = $convert.base64Decode(
    'CglMc3BNZXRob2QSGgoWTFNQX01FVEhPRF9ESUFHTk9TVElDUxAAEhQKEExTUF9NRVRIT0RfSE'
    '9WRVIQARIZChVMU1BfTUVUSE9EX0RFRklOSVRJT04QAhIZChVMU1BfTUVUSE9EX1JFRkVSRU5D'
    'RVMQAxIVChFMU1BfTUVUSE9EX1JFTkFNRRAEEh8KG0xTUF9NRVRIT0RfRE9DVU1FTlRfU1lNQk'
    '9MUxAF');

@$core.Deprecated('Use lspDiagnosticSeverityDescriptor instead')
const LspDiagnosticSeverity$json = {
  '1': 'LspDiagnosticSeverity',
  '2': [
    {'1': 'LSP_DIAGNOSTIC_SEVERITY_ERROR', '2': 0},
    {'1': 'LSP_DIAGNOSTIC_SEVERITY_WARNING', '2': 1},
    {'1': 'LSP_DIAGNOSTIC_SEVERITY_INFORMATION', '2': 2},
    {'1': 'LSP_DIAGNOSTIC_SEVERITY_HINT', '2': 3},
  ],
};

/// Descriptor for `LspDiagnosticSeverity`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List lspDiagnosticSeverityDescriptor = $convert.base64Decode(
    'ChVMc3BEaWFnbm9zdGljU2V2ZXJpdHkSIQodTFNQX0RJQUdOT1NUSUNfU0VWRVJJVFlfRVJST1'
    'IQABIjCh9MU1BfRElBR05PU1RJQ19TRVZFUklUWV9XQVJOSU5HEAESJwojTFNQX0RJQUdOT1NU'
    'SUNfU0VWRVJJVFlfSU5GT1JNQVRJT04QAhIgChxMU1BfRElBR05PU1RJQ19TRVZFUklUWV9ISU'
    '5UEAM=');

@$core.Deprecated('Use lspPositionDescriptor instead')
const LspPosition$json = {
  '1': 'LspPosition',
  '2': [
    {'1': 'line', '3': 1, '4': 1, '5': 13, '10': 'line'},
    {'1': 'character', '3': 2, '4': 1, '5': 13, '10': 'character'},
  ],
};

/// Descriptor for `LspPosition`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspPositionDescriptor = $convert.base64Decode(
    'CgtMc3BQb3NpdGlvbhISCgRsaW5lGAEgASgNUgRsaW5lEhwKCWNoYXJhY3RlchgCIAEoDVIJY2'
    'hhcmFjdGVy');

@$core.Deprecated('Use lspRangeDescriptor instead')
const LspRange$json = {
  '1': 'LspRange',
  '2': [
    {
      '1': 'start',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.LspPosition',
      '10': 'start'
    },
    {
      '1': 'end',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.LspPosition',
      '10': 'end'
    },
  ],
};

/// Descriptor for `LspRange`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspRangeDescriptor = $convert.base64Decode(
    'CghMc3BSYW5nZRIrCgVzdGFydBgBIAEoCzIVLmFnZW50LnYxLkxzcFBvc2l0aW9uUgVzdGFydB'
    'InCgNlbmQYAiABKAsyFS5hZ2VudC52MS5Mc3BQb3NpdGlvblIDZW5k');

@$core.Deprecated('Use lspLocationDescriptor instead')
const LspLocation$json = {
  '1': 'LspLocation',
  '2': [
    {'1': 'uri', '3': 1, '4': 1, '5': 9, '10': 'uri'},
    {
      '1': 'range',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.LspRange',
      '10': 'range'
    },
  ],
};

/// Descriptor for `LspLocation`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspLocationDescriptor = $convert.base64Decode(
    'CgtMc3BMb2NhdGlvbhIQCgN1cmkYASABKAlSA3VyaRIoCgVyYW5nZRgCIAEoCzISLmFnZW50Ln'
    'YxLkxzcFJhbmdlUgVyYW5nZQ==');

@$core.Deprecated('Use lspDiagnosticDescriptor instead')
const LspDiagnostic$json = {
  '1': 'LspDiagnostic',
  '2': [
    {
      '1': 'range',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.LspRange',
      '10': 'range'
    },
    {
      '1': 'severity',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.LspDiagnosticSeverity',
      '10': 'severity'
    },
    {'1': 'message', '3': 3, '4': 1, '5': 9, '10': 'message'},
    {'1': 'code', '3': 4, '4': 1, '5': 9, '9': 0, '10': 'code', '17': true},
    {'1': 'source', '3': 5, '4': 1, '5': 9, '9': 1, '10': 'source', '17': true},
  ],
  '8': [
    {'1': '_code'},
    {'1': '_source'},
  ],
};

/// Descriptor for `LspDiagnostic`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspDiagnosticDescriptor = $convert.base64Decode(
    'Cg1Mc3BEaWFnbm9zdGljEigKBXJhbmdlGAEgASgLMhIuYWdlbnQudjEuTHNwUmFuZ2VSBXJhbm'
    'dlEjsKCHNldmVyaXR5GAIgASgOMh8uYWdlbnQudjEuTHNwRGlhZ25vc3RpY1NldmVyaXR5Ughz'
    'ZXZlcml0eRIYCgdtZXNzYWdlGAMgASgJUgdtZXNzYWdlEhcKBGNvZGUYBCABKAlIAFIEY29kZY'
    'gBARIbCgZzb3VyY2UYBSABKAlIAVIGc291cmNliAEBQgcKBV9jb2RlQgkKB19zb3VyY2U=');

@$core.Deprecated('Use lspHoverDescriptor instead')
const LspHover$json = {
  '1': 'LspHover',
  '2': [
    {'1': 'contents', '3': 1, '4': 1, '5': 9, '10': 'contents'},
  ],
};

/// Descriptor for `LspHover`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspHoverDescriptor = $convert
    .base64Decode('CghMc3BIb3ZlchIaCghjb250ZW50cxgBIAEoCVIIY29udGVudHM=');

@$core.Deprecated('Use lspTextEditDescriptor instead')
const LspTextEdit$json = {
  '1': 'LspTextEdit',
  '2': [
    {
      '1': 'range',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.LspRange',
      '10': 'range'
    },
    {'1': 'new_text', '3': 2, '4': 1, '5': 9, '10': 'newText'},
  ],
};

/// Descriptor for `LspTextEdit`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspTextEditDescriptor = $convert.base64Decode(
    'CgtMc3BUZXh0RWRpdBIoCgVyYW5nZRgBIAEoCzISLmFnZW50LnYxLkxzcFJhbmdlUgVyYW5nZR'
    'IZCghuZXdfdGV4dBgCIAEoCVIHbmV3VGV4dA==');

@$core.Deprecated('Use lspFileEditsDescriptor instead')
const LspFileEdits$json = {
  '1': 'LspFileEdits',
  '2': [
    {'1': 'uri', '3': 1, '4': 1, '5': 9, '10': 'uri'},
    {
      '1': 'edits',
      '3': 2,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.LspTextEdit',
      '10': 'edits'
    },
  ],
};

/// Descriptor for `LspFileEdits`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspFileEditsDescriptor = $convert.base64Decode(
    'CgxMc3BGaWxlRWRpdHMSEAoDdXJpGAEgASgJUgN1cmkSKwoFZWRpdHMYAiADKAsyFS5hZ2VudC'
    '52MS5Mc3BUZXh0RWRpdFIFZWRpdHM=');

@$core.Deprecated('Use lspWorkspaceEditDescriptor instead')
const LspWorkspaceEdit$json = {
  '1': 'LspWorkspaceEdit',
  '2': [
    {
      '1': 'changes',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.LspFileEdits',
      '10': 'changes'
    },
  ],
};

/// Descriptor for `LspWorkspaceEdit`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspWorkspaceEditDescriptor = $convert.base64Decode(
    'ChBMc3BXb3Jrc3BhY2VFZGl0EjAKB2NoYW5nZXMYASADKAsyFi5hZ2VudC52MS5Mc3BGaWxlRW'
    'RpdHNSB2NoYW5nZXM=');

@$core.Deprecated('Use lspDocumentSymbolDescriptor instead')
const LspDocumentSymbol$json = {
  '1': 'LspDocumentSymbol',
  '2': [
    {'1': 'name', '3': 1, '4': 1, '5': 9, '10': 'name'},
    {'1': 'kind', '3': 2, '4': 1, '5': 9, '10': 'kind'},
    {
      '1': 'range',
      '3': 3,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.LspRange',
      '10': 'range'
    },
  ],
};

/// Descriptor for `LspDocumentSymbol`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspDocumentSymbolDescriptor = $convert.base64Decode(
    'ChFMc3BEb2N1bWVudFN5bWJvbBISCgRuYW1lGAEgASgJUgRuYW1lEhIKBGtpbmQYAiABKAlSBG'
    'tpbmQSKAoFcmFuZ2UYAyABKAsyEi5hZ2VudC52MS5Mc3BSYW5nZVIFcmFuZ2U=');

@$core.Deprecated('Use lspOpenRequestDescriptor instead')
const LspOpenRequest$json = {
  '1': 'LspOpenRequest',
  '2': [
    {'1': 'uri', '3': 1, '4': 1, '5': 9, '10': 'uri'},
    {'1': 'text', '3': 2, '4': 1, '5': 9, '10': 'text'},
  ],
};

/// Descriptor for `LspOpenRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspOpenRequestDescriptor = $convert.base64Decode(
    'Cg5Mc3BPcGVuUmVxdWVzdBIQCgN1cmkYASABKAlSA3VyaRISCgR0ZXh0GAIgASgJUgR0ZXh0');

@$core.Deprecated('Use lspOpenResponseDescriptor instead')
const LspOpenResponse$json = {
  '1': 'LspOpenResponse',
};

/// Descriptor for `LspOpenResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspOpenResponseDescriptor =
    $convert.base64Decode('Cg9Mc3BPcGVuUmVzcG9uc2U=');

@$core.Deprecated('Use lspRequestMsgDescriptor instead')
const LspRequestMsg$json = {
  '1': 'LspRequestMsg',
  '2': [
    {
      '1': 'method',
      '3': 1,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.LspMethod',
      '10': 'method'
    },
    {'1': 'uri', '3': 2, '4': 1, '5': 9, '10': 'uri'},
    {
      '1': 'position',
      '3': 3,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.LspPosition',
      '9': 0,
      '10': 'position',
      '17': true
    },
    {
      '1': 'new_name',
      '3': 4,
      '4': 1,
      '5': 9,
      '9': 1,
      '10': 'newName',
      '17': true
    },
  ],
  '8': [
    {'1': '_position'},
    {'1': '_new_name'},
  ],
};

/// Descriptor for `LspRequestMsg`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspRequestMsgDescriptor = $convert.base64Decode(
    'Cg1Mc3BSZXF1ZXN0TXNnEisKBm1ldGhvZBgBIAEoDjITLmFnZW50LnYxLkxzcE1ldGhvZFIGbW'
    'V0aG9kEhAKA3VyaRgCIAEoCVIDdXJpEjYKCHBvc2l0aW9uGAMgASgLMhUuYWdlbnQudjEuTHNw'
    'UG9zaXRpb25IAFIIcG9zaXRpb26IAQESHgoIbmV3X25hbWUYBCABKAlIAVIHbmV3TmFtZYgBAU'
    'ILCglfcG9zaXRpb25CCwoJX25ld19uYW1l');

@$core.Deprecated('Use lspResultMsgDescriptor instead')
const LspResultMsg$json = {
  '1': 'LspResultMsg',
  '2': [
    {
      '1': 'diagnostics',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.LspDiagnostics',
      '9': 0,
      '10': 'diagnostics'
    },
    {
      '1': 'hover',
      '3': 2,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.LspHoverResult',
      '9': 0,
      '10': 'hover'
    },
    {
      '1': 'locations',
      '3': 3,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.LspLocations',
      '9': 0,
      '10': 'locations'
    },
    {
      '1': 'symbols',
      '3': 4,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.LspSymbols',
      '9': 0,
      '10': 'symbols'
    },
    {
      '1': 'rename',
      '3': 5,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.LspWorkspaceEdit',
      '9': 0,
      '10': 'rename'
    },
  ],
  '8': [
    {'1': 'kind'},
  ],
};

/// Descriptor for `LspResultMsg`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspResultMsgDescriptor = $convert.base64Decode(
    'CgxMc3BSZXN1bHRNc2cSPAoLZGlhZ25vc3RpY3MYASABKAsyGC5hZ2VudC52MS5Mc3BEaWFnbm'
    '9zdGljc0gAUgtkaWFnbm9zdGljcxIwCgVob3ZlchgCIAEoCzIYLmFnZW50LnYxLkxzcEhvdmVy'
    'UmVzdWx0SABSBWhvdmVyEjYKCWxvY2F0aW9ucxgDIAEoCzIWLmFnZW50LnYxLkxzcExvY2F0aW'
    '9uc0gAUglsb2NhdGlvbnMSMAoHc3ltYm9scxgEIAEoCzIULmFnZW50LnYxLkxzcFN5bWJvbHNI'
    'AFIHc3ltYm9scxI0CgZyZW5hbWUYBSABKAsyGi5hZ2VudC52MS5Mc3BXb3Jrc3BhY2VFZGl0SA'
    'BSBnJlbmFtZUIGCgRraW5k');

@$core.Deprecated('Use lspDiagnosticsDescriptor instead')
const LspDiagnostics$json = {
  '1': 'LspDiagnostics',
  '2': [
    {
      '1': 'items',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.LspDiagnostic',
      '10': 'items'
    },
  ],
};

/// Descriptor for `LspDiagnostics`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspDiagnosticsDescriptor = $convert.base64Decode(
    'Cg5Mc3BEaWFnbm9zdGljcxItCgVpdGVtcxgBIAMoCzIXLmFnZW50LnYxLkxzcERpYWdub3N0aW'
    'NSBWl0ZW1z');

@$core.Deprecated('Use lspHoverResultDescriptor instead')
const LspHoverResult$json = {
  '1': 'LspHoverResult',
  '2': [
    {
      '1': 'hover',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.LspHover',
      '9': 0,
      '10': 'hover',
      '17': true
    },
  ],
  '8': [
    {'1': '_hover'},
  ],
};

/// Descriptor for `LspHoverResult`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspHoverResultDescriptor = $convert.base64Decode(
    'Cg5Mc3BIb3ZlclJlc3VsdBItCgVob3ZlchgBIAEoCzISLmFnZW50LnYxLkxzcEhvdmVySABSBW'
    'hvdmVyiAEBQggKBl9ob3Zlcg==');

@$core.Deprecated('Use lspLocationsDescriptor instead')
const LspLocations$json = {
  '1': 'LspLocations',
  '2': [
    {
      '1': 'items',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.LspLocation',
      '10': 'items'
    },
  ],
};

/// Descriptor for `LspLocations`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspLocationsDescriptor = $convert.base64Decode(
    'CgxMc3BMb2NhdGlvbnMSKwoFaXRlbXMYASADKAsyFS5hZ2VudC52MS5Mc3BMb2NhdGlvblIFaX'
    'RlbXM=');

@$core.Deprecated('Use lspSymbolsDescriptor instead')
const LspSymbols$json = {
  '1': 'LspSymbols',
  '2': [
    {
      '1': 'items',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.LspDocumentSymbol',
      '10': 'items'
    },
  ],
};

/// Descriptor for `LspSymbols`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspSymbolsDescriptor = $convert.base64Decode(
    'CgpMc3BTeW1ib2xzEjEKBWl0ZW1zGAEgAygLMhsuYWdlbnQudjEuTHNwRG9jdW1lbnRTeW1ib2'
    'xSBWl0ZW1z');

@$core.Deprecated('Use lspCapabilitiesRequestDescriptor instead')
const LspCapabilitiesRequest$json = {
  '1': 'LspCapabilitiesRequest',
  '2': [
    {'1': 'language', '3': 1, '4': 1, '5': 9, '10': 'language'},
  ],
};

/// Descriptor for `LspCapabilitiesRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspCapabilitiesRequestDescriptor =
    $convert.base64Decode(
        'ChZMc3BDYXBhYmlsaXRpZXNSZXF1ZXN0EhoKCGxhbmd1YWdlGAEgASgJUghsYW5ndWFnZQ==');

@$core.Deprecated('Use lspCapabilitiesDescriptor instead')
const LspCapabilities$json = {
  '1': 'LspCapabilities',
  '2': [
    {'1': 'server', '3': 1, '4': 1, '5': 9, '10': 'server'},
    {
      '1': 'methods',
      '3': 2,
      '4': 3,
      '5': 14,
      '6': '.agent.v1.LspMethod',
      '10': 'methods'
    },
  ],
};

/// Descriptor for `LspCapabilities`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspCapabilitiesDescriptor = $convert.base64Decode(
    'Cg9Mc3BDYXBhYmlsaXRpZXMSFgoGc2VydmVyGAEgASgJUgZzZXJ2ZXISLQoHbWV0aG9kcxgCIA'
    'MoDjITLmFnZW50LnYxLkxzcE1ldGhvZFIHbWV0aG9kcw==');

@$core.Deprecated('Use lspShutdownRequestDescriptor instead')
const LspShutdownRequest$json = {
  '1': 'LspShutdownRequest',
};

/// Descriptor for `LspShutdownRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspShutdownRequestDescriptor =
    $convert.base64Decode('ChJMc3BTaHV0ZG93blJlcXVlc3Q=');

@$core.Deprecated('Use lspShutdownResponseDescriptor instead')
const LspShutdownResponse$json = {
  '1': 'LspShutdownResponse',
};

/// Descriptor for `LspShutdownResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List lspShutdownResponseDescriptor =
    $convert.base64Decode('ChNMc3BTaHV0ZG93blJlc3BvbnNl');
