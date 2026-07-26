// This is a generated file - do not edit.
//
// Generated from agent/v1/lsp.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

class LspMethod extends $pb.ProtobufEnum {
  static const LspMethod LSP_METHOD_DIAGNOSTICS =
      LspMethod._(0, _omitEnumNames ? '' : 'LSP_METHOD_DIAGNOSTICS');
  static const LspMethod LSP_METHOD_HOVER =
      LspMethod._(1, _omitEnumNames ? '' : 'LSP_METHOD_HOVER');
  static const LspMethod LSP_METHOD_DEFINITION =
      LspMethod._(2, _omitEnumNames ? '' : 'LSP_METHOD_DEFINITION');
  static const LspMethod LSP_METHOD_REFERENCES =
      LspMethod._(3, _omitEnumNames ? '' : 'LSP_METHOD_REFERENCES');
  static const LspMethod LSP_METHOD_RENAME =
      LspMethod._(4, _omitEnumNames ? '' : 'LSP_METHOD_RENAME');
  static const LspMethod LSP_METHOD_DOCUMENT_SYMBOLS =
      LspMethod._(5, _omitEnumNames ? '' : 'LSP_METHOD_DOCUMENT_SYMBOLS');

  static const $core.List<LspMethod> values = <LspMethod>[
    LSP_METHOD_DIAGNOSTICS,
    LSP_METHOD_HOVER,
    LSP_METHOD_DEFINITION,
    LSP_METHOD_REFERENCES,
    LSP_METHOD_RENAME,
    LSP_METHOD_DOCUMENT_SYMBOLS,
  ];

  static final $core.List<LspMethod?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 5);
  static LspMethod? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const LspMethod._(super.value, super.name);
}

class LspDiagnosticSeverity extends $pb.ProtobufEnum {
  static const LspDiagnosticSeverity LSP_DIAGNOSTIC_SEVERITY_ERROR =
      LspDiagnosticSeverity._(
          0, _omitEnumNames ? '' : 'LSP_DIAGNOSTIC_SEVERITY_ERROR');
  static const LspDiagnosticSeverity LSP_DIAGNOSTIC_SEVERITY_WARNING =
      LspDiagnosticSeverity._(
          1, _omitEnumNames ? '' : 'LSP_DIAGNOSTIC_SEVERITY_WARNING');
  static const LspDiagnosticSeverity LSP_DIAGNOSTIC_SEVERITY_INFORMATION =
      LspDiagnosticSeverity._(
          2, _omitEnumNames ? '' : 'LSP_DIAGNOSTIC_SEVERITY_INFORMATION');
  static const LspDiagnosticSeverity LSP_DIAGNOSTIC_SEVERITY_HINT =
      LspDiagnosticSeverity._(
          3, _omitEnumNames ? '' : 'LSP_DIAGNOSTIC_SEVERITY_HINT');

  static const $core.List<LspDiagnosticSeverity> values =
      <LspDiagnosticSeverity>[
    LSP_DIAGNOSTIC_SEVERITY_ERROR,
    LSP_DIAGNOSTIC_SEVERITY_WARNING,
    LSP_DIAGNOSTIC_SEVERITY_INFORMATION,
    LSP_DIAGNOSTIC_SEVERITY_HINT,
  ];

  static final $core.List<LspDiagnosticSeverity?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 3);
  static LspDiagnosticSeverity? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const LspDiagnosticSeverity._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
