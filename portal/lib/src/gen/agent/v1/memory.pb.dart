// This is a generated file - do not edit.
//
// Generated from agent/v1/memory.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

import 'common.pb.dart' as $0;

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

class RecallResponse extends $pb.GeneratedMessage {
  factory RecallResponse({
    $core.Iterable<$0.MemoryItem>? items,
  }) {
    final result = create();
    if (items != null) result.items.addAll(items);
    return result;
  }

  RecallResponse._();

  factory RecallResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RecallResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RecallResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<$0.MemoryItem>(1, _omitFieldNames ? '' : 'items',
        subBuilder: $0.MemoryItem.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RecallResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RecallResponse copyWith(void Function(RecallResponse) updates) =>
      super.copyWith((message) => updates(message as RecallResponse))
          as RecallResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RecallResponse create() => RecallResponse._();
  @$core.override
  RecallResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RecallResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RecallResponse>(create);
  static RecallResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<$0.MemoryItem> get items => $_getList(0);
}

class AppendResponse extends $pb.GeneratedMessage {
  factory AppendResponse() => create();

  AppendResponse._();

  factory AppendResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory AppendResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'AppendResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AppendResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AppendResponse copyWith(void Function(AppendResponse) updates) =>
      super.copyWith((message) => updates(message as AppendResponse))
          as AppendResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static AppendResponse create() => AppendResponse._();
  @$core.override
  AppendResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static AppendResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<AppendResponse>(create);
  static AppendResponse? _defaultInstance;
}

class DistillRequest extends $pb.GeneratedMessage {
  factory DistillRequest() => create();

  DistillRequest._();

  factory DistillRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DistillRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DistillRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DistillRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DistillRequest copyWith(void Function(DistillRequest) updates) =>
      super.copyWith((message) => updates(message as DistillRequest))
          as DistillRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DistillRequest create() => DistillRequest._();
  @$core.override
  DistillRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DistillRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DistillRequest>(create);
  static DistillRequest? _defaultInstance;
}

class DistillResponse extends $pb.GeneratedMessage {
  factory DistillResponse({
    $fixnum.Int64? count,
  }) {
    final result = create();
    if (count != null) result.count = count;
    return result;
  }

  DistillResponse._();

  factory DistillResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DistillResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DistillResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..a<$fixnum.Int64>(1, _omitFieldNames ? '' : 'count', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DistillResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DistillResponse copyWith(void Function(DistillResponse) updates) =>
      super.copyWith((message) => updates(message as DistillResponse))
          as DistillResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DistillResponse create() => DistillResponse._();
  @$core.override
  DistillResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DistillResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DistillResponse>(create);
  static DistillResponse? _defaultInstance;

  /// Number of semantic facts written (agent-core `usize`).
  @$pb.TagNumber(1)
  $fixnum.Int64 get count => $_getI64(0);
  @$pb.TagNumber(1)
  set count($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasCount() => $_has(0);
  @$pb.TagNumber(1)
  void clearCount() => $_clearField(1);
}

class RecentRequest extends $pb.GeneratedMessage {
  factory RecentRequest({
    $fixnum.Int64? limit,
  }) {
    final result = create();
    if (limit != null) result.limit = limit;
    return result;
  }

  RecentRequest._();

  factory RecentRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RecentRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RecentRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..a<$fixnum.Int64>(1, _omitFieldNames ? '' : 'limit', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RecentRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RecentRequest copyWith(void Function(RecentRequest) updates) =>
      super.copyWith((message) => updates(message as RecentRequest))
          as RecentRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RecentRequest create() => RecentRequest._();
  @$core.override
  RecentRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RecentRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RecentRequest>(create);
  static RecentRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $fixnum.Int64 get limit => $_getI64(0);
  @$pb.TagNumber(1)
  set limit($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasLimit() => $_has(0);
  @$pb.TagNumber(1)
  void clearLimit() => $_clearField(1);
}

class RecentResponse extends $pb.GeneratedMessage {
  factory RecentResponse({
    $core.Iterable<$0.MemoryEvent>? events,
  }) {
    final result = create();
    if (events != null) result.events.addAll(events);
    return result;
  }

  RecentResponse._();

  factory RecentResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RecentResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RecentResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<$0.MemoryEvent>(1, _omitFieldNames ? '' : 'events',
        subBuilder: $0.MemoryEvent.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RecentResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RecentResponse copyWith(void Function(RecentResponse) updates) =>
      super.copyWith((message) => updates(message as RecentResponse))
          as RecentResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RecentResponse create() => RecentResponse._();
  @$core.override
  RecentResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RecentResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RecentResponse>(create);
  static RecentResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<$0.MemoryEvent> get events => $_getList(0);
}

class SemanticDistillRequest extends $pb.GeneratedMessage {
  factory SemanticDistillRequest({
    $core.Iterable<$0.MemoryEvent>? episodic,
  }) {
    final result = create();
    if (episodic != null) result.episodic.addAll(episodic);
    return result;
  }

  SemanticDistillRequest._();

  factory SemanticDistillRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SemanticDistillRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SemanticDistillRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<$0.MemoryEvent>(1, _omitFieldNames ? '' : 'episodic',
        subBuilder: $0.MemoryEvent.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SemanticDistillRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SemanticDistillRequest copyWith(
          void Function(SemanticDistillRequest) updates) =>
      super.copyWith((message) => updates(message as SemanticDistillRequest))
          as SemanticDistillRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SemanticDistillRequest create() => SemanticDistillRequest._();
  @$core.override
  SemanticDistillRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SemanticDistillRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SemanticDistillRequest>(create);
  static SemanticDistillRequest? _defaultInstance;

  /// The recent episodic window to promote from.
  @$pb.TagNumber(1)
  $pb.PbList<$0.MemoryEvent> get episodic => $_getList(0);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
