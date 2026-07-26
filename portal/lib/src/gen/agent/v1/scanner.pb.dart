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

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

import 'scanner.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'scanner.pbenum.dart';

class ScanRequest extends $pb.GeneratedMessage {
  factory ScanRequest({
    ScanKind? kind,
    $core.String? content,
  }) {
    final result = create();
    if (kind != null) result.kind = kind;
    if (content != null) result.content = content;
    return result;
  }

  ScanRequest._();

  factory ScanRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ScanRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ScanRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aE<ScanKind>(1, _omitFieldNames ? '' : 'kind',
        enumValues: ScanKind.values)
    ..aOS(2, _omitFieldNames ? '' : 'content')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ScanRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ScanRequest copyWith(void Function(ScanRequest) updates) =>
      super.copyWith((message) => updates(message as ScanRequest))
          as ScanRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ScanRequest create() => ScanRequest._();
  @$core.override
  ScanRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ScanRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ScanRequest>(create);
  static ScanRequest? _defaultInstance;

  @$pb.TagNumber(1)
  ScanKind get kind => $_getN(0);
  @$pb.TagNumber(1)
  set kind(ScanKind value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasKind() => $_has(0);
  @$pb.TagNumber(1)
  void clearKind() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get content => $_getSZ(1);
  @$pb.TagNumber(2)
  set content($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasContent() => $_has(1);
  @$pb.TagNumber(2)
  void clearContent() => $_clearField(2);
}

/// One finding: which rule fired, how bad, and where.
///
/// The matched bytes are deliberately NOT carried — only the span. A denial
/// reason that echoed them would hand an attacker an oracle for probing what is
/// gated (parity spec 08). The span is a byte range into the scanned content.
class ScanFinding extends $pb.GeneratedMessage {
  factory ScanFinding({
    $core.String? rule,
    ScanSeverity? severity,
    $core.String? category,
    $fixnum.Int64? spanStart,
    $fixnum.Int64? spanEnd,
  }) {
    final result = create();
    if (rule != null) result.rule = rule;
    if (severity != null) result.severity = severity;
    if (category != null) result.category = category;
    if (spanStart != null) result.spanStart = spanStart;
    if (spanEnd != null) result.spanEnd = spanEnd;
    return result;
  }

  ScanFinding._();

  factory ScanFinding.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ScanFinding.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ScanFinding',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'rule')
    ..aE<ScanSeverity>(2, _omitFieldNames ? '' : 'severity',
        enumValues: ScanSeverity.values)
    ..aOS(3, _omitFieldNames ? '' : 'category')
    ..a<$fixnum.Int64>(
        4, _omitFieldNames ? '' : 'spanStart', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..a<$fixnum.Int64>(5, _omitFieldNames ? '' : 'spanEnd', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ScanFinding clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ScanFinding copyWith(void Function(ScanFinding) updates) =>
      super.copyWith((message) => updates(message as ScanFinding))
          as ScanFinding;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ScanFinding create() => ScanFinding._();
  @$core.override
  ScanFinding createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ScanFinding getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ScanFinding>(create);
  static ScanFinding? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get rule => $_getSZ(0);
  @$pb.TagNumber(1)
  set rule($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRule() => $_has(0);
  @$pb.TagNumber(1)
  void clearRule() => $_clearField(1);

  @$pb.TagNumber(2)
  ScanSeverity get severity => $_getN(1);
  @$pb.TagNumber(2)
  set severity(ScanSeverity value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasSeverity() => $_has(1);
  @$pb.TagNumber(2)
  void clearSeverity() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get category => $_getSZ(2);
  @$pb.TagNumber(3)
  set category($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasCategory() => $_has(2);
  @$pb.TagNumber(3)
  void clearCategory() => $_clearField(3);

  @$pb.TagNumber(4)
  $fixnum.Int64 get spanStart => $_getI64(3);
  @$pb.TagNumber(4)
  set spanStart($fixnum.Int64 value) => $_setInt64(3, value);
  @$pb.TagNumber(4)
  $core.bool hasSpanStart() => $_has(3);
  @$pb.TagNumber(4)
  void clearSpanStart() => $_clearField(4);

  @$pb.TagNumber(5)
  $fixnum.Int64 get spanEnd => $_getI64(4);
  @$pb.TagNumber(5)
  set spanEnd($fixnum.Int64 value) => $_setInt64(4, value);
  @$pb.TagNumber(5)
  $core.bool hasSpanEnd() => $_has(4);
  @$pb.TagNumber(5)
  void clearSpanEnd() => $_clearField(5);
}

class ScanResponse extends $pb.GeneratedMessage {
  factory ScanResponse({
    $core.Iterable<ScanFinding>? findings,
  }) {
    final result = create();
    if (findings != null) result.findings.addAll(findings);
    return result;
  }

  ScanResponse._();

  factory ScanResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ScanResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ScanResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<ScanFinding>(1, _omitFieldNames ? '' : 'findings',
        subBuilder: ScanFinding.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ScanResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ScanResponse copyWith(void Function(ScanResponse) updates) =>
      super.copyWith((message) => updates(message as ScanResponse))
          as ScanResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ScanResponse create() => ScanResponse._();
  @$core.override
  ScanResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ScanResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ScanResponse>(create);
  static ScanResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<ScanFinding> get findings => $_getList(0);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
