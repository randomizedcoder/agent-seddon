// This is a generated file - do not edit.
//
// Generated from agent/v1/ast.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

/// The kind of a code symbol. `SYMBOL_KIND_UNKNOWN` is the proto3 zero value and the
/// additive fallback for a kind a peer doesn't recognise.
class SymbolKind extends $pb.ProtobufEnum {
  static const SymbolKind SYMBOL_KIND_UNKNOWN =
      SymbolKind._(0, _omitEnumNames ? '' : 'SYMBOL_KIND_UNKNOWN');
  static const SymbolKind SYMBOL_KIND_FUNC =
      SymbolKind._(1, _omitEnumNames ? '' : 'SYMBOL_KIND_FUNC');
  static const SymbolKind SYMBOL_KIND_METHOD =
      SymbolKind._(2, _omitEnumNames ? '' : 'SYMBOL_KIND_METHOD');
  static const SymbolKind SYMBOL_KIND_INTERFACE =
      SymbolKind._(3, _omitEnumNames ? '' : 'SYMBOL_KIND_INTERFACE');
  static const SymbolKind SYMBOL_KIND_STRUCT =
      SymbolKind._(4, _omitEnumNames ? '' : 'SYMBOL_KIND_STRUCT');
  static const SymbolKind SYMBOL_KIND_TYPE =
      SymbolKind._(5, _omitEnumNames ? '' : 'SYMBOL_KIND_TYPE');
  static const SymbolKind SYMBOL_KIND_FIELD =
      SymbolKind._(6, _omitEnumNames ? '' : 'SYMBOL_KIND_FIELD');

  static const $core.List<SymbolKind> values = <SymbolKind>[
    SYMBOL_KIND_UNKNOWN,
    SYMBOL_KIND_FUNC,
    SYMBOL_KIND_METHOD,
    SYMBOL_KIND_INTERFACE,
    SYMBOL_KIND_STRUCT,
    SYMBOL_KIND_TYPE,
    SYMBOL_KIND_FIELD,
  ];

  static final $core.List<SymbolKind?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 6);
  static SymbolKind? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const SymbolKind._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
