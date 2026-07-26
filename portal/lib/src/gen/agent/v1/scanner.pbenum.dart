// This is a generated file - do not edit.
//
// Generated from agent/v1/scanner.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

class ScanKind extends $pb.ProtobufEnum {
  static const ScanKind SCAN_KIND_TOOL_INPUT =
      ScanKind._(0, _omitEnumNames ? '' : 'SCAN_KIND_TOOL_INPUT');
  static const ScanKind SCAN_KIND_FILE_BODY =
      ScanKind._(1, _omitEnumNames ? '' : 'SCAN_KIND_FILE_BODY');
  static const ScanKind SCAN_KIND_WEB_CONTENT =
      ScanKind._(2, _omitEnumNames ? '' : 'SCAN_KIND_WEB_CONTENT');
  static const ScanKind SCAN_KIND_LOCKFILE =
      ScanKind._(3, _omitEnumNames ? '' : 'SCAN_KIND_LOCKFILE');

  static const $core.List<ScanKind> values = <ScanKind>[
    SCAN_KIND_TOOL_INPUT,
    SCAN_KIND_FILE_BODY,
    SCAN_KIND_WEB_CONTENT,
    SCAN_KIND_LOCKFILE,
  ];

  static final $core.List<ScanKind?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 3);
  static ScanKind? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const ScanKind._(super.value, super.name);
}

class ScanSeverity extends $pb.ProtobufEnum {
  static const ScanSeverity SCAN_SEVERITY_INFO =
      ScanSeverity._(0, _omitEnumNames ? '' : 'SCAN_SEVERITY_INFO');
  static const ScanSeverity SCAN_SEVERITY_LOW =
      ScanSeverity._(1, _omitEnumNames ? '' : 'SCAN_SEVERITY_LOW');
  static const ScanSeverity SCAN_SEVERITY_MEDIUM =
      ScanSeverity._(2, _omitEnumNames ? '' : 'SCAN_SEVERITY_MEDIUM');
  static const ScanSeverity SCAN_SEVERITY_HIGH =
      ScanSeverity._(3, _omitEnumNames ? '' : 'SCAN_SEVERITY_HIGH');
  static const ScanSeverity SCAN_SEVERITY_CRITICAL =
      ScanSeverity._(4, _omitEnumNames ? '' : 'SCAN_SEVERITY_CRITICAL');

  static const $core.List<ScanSeverity> values = <ScanSeverity>[
    SCAN_SEVERITY_INFO,
    SCAN_SEVERITY_LOW,
    SCAN_SEVERITY_MEDIUM,
    SCAN_SEVERITY_HIGH,
    SCAN_SEVERITY_CRITICAL,
  ];

  static final $core.List<ScanSeverity?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 4);
  static ScanSeverity? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const ScanSeverity._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
