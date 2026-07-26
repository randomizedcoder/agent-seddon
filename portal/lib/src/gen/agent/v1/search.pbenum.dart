// This is a generated file - do not edit.
//
// Generated from agent/v1/search.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

/// How the query text is interpreted. `SEARCH_MODE_LITERAL` is the proto3 zero
/// value and the common mode every backend supports.
class SearchMode extends $pb.ProtobufEnum {
  static const SearchMode SEARCH_MODE_LITERAL =
      SearchMode._(0, _omitEnumNames ? '' : 'SEARCH_MODE_LITERAL');
  static const SearchMode SEARCH_MODE_PHRASE =
      SearchMode._(1, _omitEnumNames ? '' : 'SEARCH_MODE_PHRASE');
  static const SearchMode SEARCH_MODE_FUZZY =
      SearchMode._(2, _omitEnumNames ? '' : 'SEARCH_MODE_FUZZY');
  static const SearchMode SEARCH_MODE_REGEX =
      SearchMode._(3, _omitEnumNames ? '' : 'SEARCH_MODE_REGEX');

  static const $core.List<SearchMode> values = <SearchMode>[
    SEARCH_MODE_LITERAL,
    SEARCH_MODE_PHRASE,
    SEARCH_MODE_FUZZY,
    SEARCH_MODE_REGEX,
  ];

  static final $core.List<SearchMode?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 3);
  static SearchMode? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const SearchMode._(super.value, super.name);
}

/// Freshness of the on-disk index relative to the working tree.
class IndexState extends $pb.ProtobufEnum {
  static const IndexState INDEX_STATE_FRESH =
      IndexState._(0, _omitEnumNames ? '' : 'INDEX_STATE_FRESH');
  static const IndexState INDEX_STATE_STALE =
      IndexState._(1, _omitEnumNames ? '' : 'INDEX_STATE_STALE');
  static const IndexState INDEX_STATE_MISSING =
      IndexState._(2, _omitEnumNames ? '' : 'INDEX_STATE_MISSING');
  static const IndexState INDEX_STATE_BUILDING =
      IndexState._(3, _omitEnumNames ? '' : 'INDEX_STATE_BUILDING');

  static const $core.List<IndexState> values = <IndexState>[
    INDEX_STATE_FRESH,
    INDEX_STATE_STALE,
    INDEX_STATE_MISSING,
    INDEX_STATE_BUILDING,
  ];

  static final $core.List<IndexState?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 3);
  static IndexState? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const IndexState._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
