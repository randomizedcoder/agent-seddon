// This is a generated file - do not edit.
//
// Generated from agent/v1/web.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

class WebFormat extends $pb.ProtobufEnum {
  static const WebFormat WEB_FORMAT_MARKDOWN =
      WebFormat._(0, _omitEnumNames ? '' : 'WEB_FORMAT_MARKDOWN');
  static const WebFormat WEB_FORMAT_TEXT =
      WebFormat._(1, _omitEnumNames ? '' : 'WEB_FORMAT_TEXT');
  static const WebFormat WEB_FORMAT_HTML =
      WebFormat._(2, _omitEnumNames ? '' : 'WEB_FORMAT_HTML');

  static const $core.List<WebFormat> values = <WebFormat>[
    WEB_FORMAT_MARKDOWN,
    WEB_FORMAT_TEXT,
    WEB_FORMAT_HTML,
  ];

  static final $core.List<WebFormat?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 2);
  static WebFormat? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const WebFormat._(super.value, super.name);
}

class WebCacheState extends $pb.ProtobufEnum {
  static const WebCacheState WEB_CACHE_STATE_MISSING =
      WebCacheState._(0, _omitEnumNames ? '' : 'WEB_CACHE_STATE_MISSING');
  static const WebCacheState WEB_CACHE_STATE_FRESH =
      WebCacheState._(1, _omitEnumNames ? '' : 'WEB_CACHE_STATE_FRESH');
  static const WebCacheState WEB_CACHE_STATE_STALE =
      WebCacheState._(2, _omitEnumNames ? '' : 'WEB_CACHE_STATE_STALE');

  static const $core.List<WebCacheState> values = <WebCacheState>[
    WEB_CACHE_STATE_MISSING,
    WEB_CACHE_STATE_FRESH,
    WEB_CACHE_STATE_STALE,
  ];

  static final $core.List<WebCacheState?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 2);
  static WebCacheState? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const WebCacheState._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
