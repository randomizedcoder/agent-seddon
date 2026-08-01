// This is a generated file - do not edit.
//
// Generated from agent/v1/prompt.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

/// Where a prompt lives. UNSPECIFIED in a List request means "every kind".
class PromptKind extends $pb.ProtobufEnum {
  static const PromptKind PROMPT_KIND_UNSPECIFIED =
      PromptKind._(0, _omitEnumNames ? '' : 'PROMPT_KIND_UNSPECIFIED');
  static const PromptKind PROMPT_KIND_SYSTEM =
      PromptKind._(1, _omitEnumNames ? '' : 'PROMPT_KIND_SYSTEM');
  static const PromptKind PROMPT_KIND_PREPEND =
      PromptKind._(2, _omitEnumNames ? '' : 'PROMPT_KIND_PREPEND');
  static const PromptKind PROMPT_KIND_APPEND =
      PromptKind._(3, _omitEnumNames ? '' : 'PROMPT_KIND_APPEND');
  static const PromptKind PROMPT_KIND_MODE_LENS =
      PromptKind._(4, _omitEnumNames ? '' : 'PROMPT_KIND_MODE_LENS');
  static const PromptKind PROMPT_KIND_SYSTEM_FRAGMENT =
      PromptKind._(5, _omitEnumNames ? '' : 'PROMPT_KIND_SYSTEM_FRAGMENT');

  static const $core.List<PromptKind> values = <PromptKind>[
    PROMPT_KIND_UNSPECIFIED,
    PROMPT_KIND_SYSTEM,
    PROMPT_KIND_PREPEND,
    PROMPT_KIND_APPEND,
    PROMPT_KIND_MODE_LENS,
    PROMPT_KIND_SYSTEM_FRAGMENT,
  ];

  static final $core.List<PromptKind?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 5);
  static PromptKind? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const PromptKind._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
