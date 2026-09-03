// This is a generated file - do not edit.
//
// Generated from agent/v1/graph.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

class GraphEdge_Kind extends $pb.ProtobufEnum {
  static const GraphEdge_Kind KIND_UNSPECIFIED =
      GraphEdge_Kind._(0, _omitEnumNames ? '' : 'KIND_UNSPECIFIED');

  /// Data flow on the turn path (acyclic between nodes).
  static const GraphEdge_Kind KIND_MAIN =
      GraphEdge_Kind._(1, _omitEnumNames ? '' : 'KIND_MAIN');

  /// Fire-and-forget hand-off to the distiller; only from `anchor.delivery`.
  static const GraphEdge_Kind KIND_BACKGROUND =
      GraphEdge_Kind._(2, _omitEnumNames ? '' : 'KIND_BACKGROUND');

  /// Attachment: `from` names a configured resource, `to` the consuming node.
  static const GraphEdge_Kind KIND_CAPABILITY =
      GraphEdge_Kind._(3, _omitEnumNames ? '' : 'KIND_CAPABILITY');

  static const $core.List<GraphEdge_Kind> values = <GraphEdge_Kind>[
    KIND_UNSPECIFIED,
    KIND_MAIN,
    KIND_BACKGROUND,
    KIND_CAPABILITY,
  ];

  static final $core.List<GraphEdge_Kind?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 3);
  static GraphEdge_Kind? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const GraphEdge_Kind._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
