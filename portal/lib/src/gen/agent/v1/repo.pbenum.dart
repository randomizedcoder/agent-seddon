// This is a generated file - do not edit.
//
// Generated from agent/v1/repo.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

/// The kind of object a TreeEntry points at.
class EntryKind extends $pb.ProtobufEnum {
  static const EntryKind ENTRY_KIND_BLOB =
      EntryKind._(0, _omitEnumNames ? '' : 'ENTRY_KIND_BLOB');
  static const EntryKind ENTRY_KIND_TREE =
      EntryKind._(1, _omitEnumNames ? '' : 'ENTRY_KIND_TREE');
  static const EntryKind ENTRY_KIND_SYMLINK =
      EntryKind._(2, _omitEnumNames ? '' : 'ENTRY_KIND_SYMLINK');
  static const EntryKind ENTRY_KIND_SUBMODULE =
      EntryKind._(3, _omitEnumNames ? '' : 'ENTRY_KIND_SUBMODULE');

  static const $core.List<EntryKind> values = <EntryKind>[
    ENTRY_KIND_BLOB,
    ENTRY_KIND_TREE,
    ENTRY_KIND_SYMLINK,
    ENTRY_KIND_SUBMODULE,
  ];

  static final $core.List<EntryKind?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 3);
  static EntryKind? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const EntryKind._(super.value, super.name);
}

/// Per-file change class in a base..target diff. MODIFIED is the proto3 zero value.
class ChangeKind extends $pb.ProtobufEnum {
  static const ChangeKind CHANGE_KIND_MODIFIED =
      ChangeKind._(0, _omitEnumNames ? '' : 'CHANGE_KIND_MODIFIED');
  static const ChangeKind CHANGE_KIND_ADDED =
      ChangeKind._(1, _omitEnumNames ? '' : 'CHANGE_KIND_ADDED');
  static const ChangeKind CHANGE_KIND_DELETED =
      ChangeKind._(2, _omitEnumNames ? '' : 'CHANGE_KIND_DELETED');
  static const ChangeKind CHANGE_KIND_RENAMED =
      ChangeKind._(3, _omitEnumNames ? '' : 'CHANGE_KIND_RENAMED');
  static const ChangeKind CHANGE_KIND_COPIED =
      ChangeKind._(4, _omitEnumNames ? '' : 'CHANGE_KIND_COPIED');
  static const ChangeKind CHANGE_KIND_TYPE_CHANGE =
      ChangeKind._(5, _omitEnumNames ? '' : 'CHANGE_KIND_TYPE_CHANGE');

  static const $core.List<ChangeKind> values = <ChangeKind>[
    CHANGE_KIND_MODIFIED,
    CHANGE_KIND_ADDED,
    CHANGE_KIND_DELETED,
    CHANGE_KIND_RENAMED,
    CHANGE_KIND_COPIED,
    CHANGE_KIND_TYPE_CHANGE,
  ];

  static final $core.List<ChangeKind?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 5);
  static ChangeKind? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const ChangeKind._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
