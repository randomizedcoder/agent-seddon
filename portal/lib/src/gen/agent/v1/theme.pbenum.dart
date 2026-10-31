// This is a generated file - do not edit.
//
// Generated from agent/v1/theme.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

/// Spacing between and inside controls.
class Density extends $pb.ProtobufEnum {
  static const Density DENSITY_UNSPECIFIED =
      Density._(0, _omitEnumNames ? '' : 'DENSITY_UNSPECIFIED');
  static const Density DENSITY_COMPACT =
      Density._(1, _omitEnumNames ? '' : 'DENSITY_COMPACT');
  static const Density DENSITY_STANDARD =
      Density._(2, _omitEnumNames ? '' : 'DENSITY_STANDARD');
  static const Density DENSITY_COMFORTABLE =
      Density._(3, _omitEnumNames ? '' : 'DENSITY_COMFORTABLE');

  static const $core.List<Density> values = <Density>[
    DENSITY_UNSPECIFIED,
    DENSITY_COMPACT,
    DENSITY_STANDARD,
    DENSITY_COMFORTABLE,
  ];

  static final $core.List<Density?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 3);
  static Density? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const Density._(super.value, super.name);
}

/// The WCAG 2.2 contrast level text pairs must meet.
class ContrastLevel extends $pb.ProtobufEnum {
  static const ContrastLevel CONTRAST_LEVEL_UNSPECIFIED =
      ContrastLevel._(0, _omitEnumNames ? '' : 'CONTRAST_LEVEL_UNSPECIFIED');
  static const ContrastLevel CONTRAST_LEVEL_AA =
      ContrastLevel._(1, _omitEnumNames ? '' : 'CONTRAST_LEVEL_AA');
  static const ContrastLevel CONTRAST_LEVEL_AAA =
      ContrastLevel._(2, _omitEnumNames ? '' : 'CONTRAST_LEVEL_AAA');

  static const $core.List<ContrastLevel> values = <ContrastLevel>[
    CONTRAST_LEVEL_UNSPECIFIED,
    CONTRAST_LEVEL_AA,
    CONTRAST_LEVEL_AAA,
  ];

  static final $core.List<ContrastLevel?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 2);
  static ContrastLevel? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const ContrastLevel._(super.value, super.name);
}

/// Who sees a user theme.
class Visibility extends $pb.ProtobufEnum {
  static const Visibility VISIBILITY_UNSPECIFIED =
      Visibility._(0, _omitEnumNames ? '' : 'VISIBILITY_UNSPECIFIED');
  static const Visibility VISIBILITY_PRIVATE =
      Visibility._(1, _omitEnumNames ? '' : 'VISIBILITY_PRIVATE');
  static const Visibility VISIBILITY_TENANT =
      Visibility._(2, _omitEnumNames ? '' : 'VISIBILITY_TENANT');

  static const $core.List<Visibility> values = <Visibility>[
    VISIBILITY_UNSPECIFIED,
    VISIBILITY_PRIVATE,
    VISIBILITY_TENANT,
  ];

  static final $core.List<Visibility?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 2);
  static Visibility? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const Visibility._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
