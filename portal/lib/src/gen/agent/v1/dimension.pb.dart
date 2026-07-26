// This is a generated file - do not edit.
//
// Generated from agent/v1/dimension.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

import 'common.pb.dart' as $1;

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

/// One dimension's summary of a step. `dimension` is a `safe_segment`'d slug;
/// `summary` is bounded + injection-screened before it is persisted.
class DimensionSummary extends $pb.GeneratedMessage {
  factory DimensionSummary({
    $core.String? dimension,
    $core.String? summary,
    $core.bool? isNew,
  }) {
    final result = create();
    if (dimension != null) result.dimension = dimension;
    if (summary != null) result.summary = summary;
    if (isNew != null) result.isNew = isNew;
    return result;
  }

  DimensionSummary._();

  factory DimensionSummary.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DimensionSummary.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DimensionSummary',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'dimension')
    ..aOS(2, _omitFieldNames ? '' : 'summary')
    ..aOB(3, _omitFieldNames ? '' : 'isNew')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DimensionSummary clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DimensionSummary copyWith(void Function(DimensionSummary) updates) =>
      super.copyWith((message) => updates(message as DimensionSummary))
          as DimensionSummary;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DimensionSummary create() => DimensionSummary._();
  @$core.override
  DimensionSummary createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DimensionSummary getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DimensionSummary>(create);
  static DimensionSummary? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get dimension => $_getSZ(0);
  @$pb.TagNumber(1)
  set dimension($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasDimension() => $_has(0);
  @$pb.TagNumber(1)
  void clearDimension() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get summary => $_getSZ(1);
  @$pb.TagNumber(2)
  set summary($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasSummary() => $_has(1);
  @$pb.TagNumber(2)
  void clearSummary() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.bool get isNew => $_getBF(2);
  @$pb.TagNumber(3)
  set isNew($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasIsNew() => $_has(2);
  @$pb.TagNumber(3)
  void clearIsNew() => $_clearField(3);
}

/// The accepted per-dimension summaries of a step (capped server-side).
class DimensionStep extends $pb.GeneratedMessage {
  factory DimensionStep({
    $core.Iterable<DimensionSummary>? summaries,
  }) {
    final result = create();
    if (summaries != null) result.summaries.addAll(summaries);
    return result;
  }

  DimensionStep._();

  factory DimensionStep.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DimensionStep.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DimensionStep',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<DimensionSummary>(1, _omitFieldNames ? '' : 'summaries',
        subBuilder: DimensionSummary.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DimensionStep clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DimensionStep copyWith(void Function(DimensionStep) updates) =>
      super.copyWith((message) => updates(message as DimensionStep))
          as DimensionStep;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DimensionStep create() => DimensionStep._();
  @$core.override
  DimensionStep createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DimensionStep getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DimensionStep>(create);
  static DimensionStep? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<DimensionSummary> get summaries => $_getList(0);
}

/// A recent episodic window to summarize (bounded server-side).
class SummarizeRequest extends $pb.GeneratedMessage {
  factory SummarizeRequest({
    $core.Iterable<$1.MemoryEvent>? events,
  }) {
    final result = create();
    if (events != null) result.events.addAll(events);
    return result;
  }

  SummarizeRequest._();

  factory SummarizeRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SummarizeRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SummarizeRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<$1.MemoryEvent>(1, _omitFieldNames ? '' : 'events',
        subBuilder: $1.MemoryEvent.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SummarizeRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SummarizeRequest copyWith(void Function(SummarizeRequest) updates) =>
      super.copyWith((message) => updates(message as SummarizeRequest))
          as SummarizeRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SummarizeRequest create() => SummarizeRequest._();
  @$core.override
  SummarizeRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SummarizeRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SummarizeRequest>(create);
  static SummarizeRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<$1.MemoryEvent> get events => $_getList(0);
}

/// Recall one dimension's history, most-recent first, capped at `limit`.
class DimensionRecallRequest extends $pb.GeneratedMessage {
  factory DimensionRecallRequest({
    $core.String? dimension,
    $core.int? limit,
  }) {
    final result = create();
    if (dimension != null) result.dimension = dimension;
    if (limit != null) result.limit = limit;
    return result;
  }

  DimensionRecallRequest._();

  factory DimensionRecallRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DimensionRecallRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DimensionRecallRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'dimension')
    ..aI(2, _omitFieldNames ? '' : 'limit', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DimensionRecallRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DimensionRecallRequest copyWith(
          void Function(DimensionRecallRequest) updates) =>
      super.copyWith((message) => updates(message as DimensionRecallRequest))
          as DimensionRecallRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DimensionRecallRequest create() => DimensionRecallRequest._();
  @$core.override
  DimensionRecallRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DimensionRecallRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DimensionRecallRequest>(create);
  static DimensionRecallRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get dimension => $_getSZ(0);
  @$pb.TagNumber(1)
  set dimension($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasDimension() => $_has(0);
  @$pb.TagNumber(1)
  void clearDimension() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get limit => $_getIZ(1);
  @$pb.TagNumber(2)
  set limit($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasLimit() => $_has(1);
  @$pb.TagNumber(2)
  void clearLimit() => $_clearField(2);
}

class DimensionRecallResponse extends $pb.GeneratedMessage {
  factory DimensionRecallResponse({
    $core.Iterable<$1.MemoryItem>? items,
  }) {
    final result = create();
    if (items != null) result.items.addAll(items);
    return result;
  }

  DimensionRecallResponse._();

  factory DimensionRecallResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DimensionRecallResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DimensionRecallResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<$1.MemoryItem>(1, _omitFieldNames ? '' : 'items',
        subBuilder: $1.MemoryItem.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DimensionRecallResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DimensionRecallResponse copyWith(
          void Function(DimensionRecallResponse) updates) =>
      super.copyWith((message) => updates(message as DimensionRecallResponse))
          as DimensionRecallResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DimensionRecallResponse create() => DimensionRecallResponse._();
  @$core.override
  DimensionRecallResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DimensionRecallResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DimensionRecallResponse>(create);
  static DimensionRecallResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<$1.MemoryItem> get items => $_getList(0);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
