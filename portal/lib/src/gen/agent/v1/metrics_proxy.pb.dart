// This is a generated file - do not edit.
//
// Generated from agent/v1/metrics_proxy.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

class PromQuery extends $pb.GeneratedMessage {
  factory PromQuery({
    $core.String? query,
    $fixnum.Int64? timeUnixMs,
  }) {
    final result = create();
    if (query != null) result.query = query;
    if (timeUnixMs != null) result.timeUnixMs = timeUnixMs;
    return result;
  }

  PromQuery._();

  factory PromQuery.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PromQuery.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PromQuery',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'query')
    ..aInt64(2, _omitFieldNames ? '' : 'timeUnixMs')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromQuery clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromQuery copyWith(void Function(PromQuery) updates) =>
      super.copyWith((message) => updates(message as PromQuery)) as PromQuery;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PromQuery create() => PromQuery._();
  @$core.override
  PromQuery createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PromQuery getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<PromQuery>(create);
  static PromQuery? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get query => $_getSZ(0);
  @$pb.TagNumber(1)
  set query($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasQuery() => $_has(0);
  @$pb.TagNumber(1)
  void clearQuery() => $_clearField(1);

  @$pb.TagNumber(2)
  $fixnum.Int64 get timeUnixMs => $_getI64(1);
  @$pb.TagNumber(2)
  set timeUnixMs($fixnum.Int64 value) => $_setInt64(1, value);
  @$pb.TagNumber(2)
  $core.bool hasTimeUnixMs() => $_has(1);
  @$pb.TagNumber(2)
  void clearTimeUnixMs() => $_clearField(2);
}

class PromRangeQuery extends $pb.GeneratedMessage {
  factory PromRangeQuery({
    $core.String? query,
    $fixnum.Int64? startUnixMs,
    $fixnum.Int64? endUnixMs,
    $core.int? stepSecs,
  }) {
    final result = create();
    if (query != null) result.query = query;
    if (startUnixMs != null) result.startUnixMs = startUnixMs;
    if (endUnixMs != null) result.endUnixMs = endUnixMs;
    if (stepSecs != null) result.stepSecs = stepSecs;
    return result;
  }

  PromRangeQuery._();

  factory PromRangeQuery.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PromRangeQuery.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PromRangeQuery',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'query')
    ..aInt64(2, _omitFieldNames ? '' : 'startUnixMs')
    ..aInt64(3, _omitFieldNames ? '' : 'endUnixMs')
    ..aI(4, _omitFieldNames ? '' : 'stepSecs', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromRangeQuery clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromRangeQuery copyWith(void Function(PromRangeQuery) updates) =>
      super.copyWith((message) => updates(message as PromRangeQuery))
          as PromRangeQuery;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PromRangeQuery create() => PromRangeQuery._();
  @$core.override
  PromRangeQuery createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PromRangeQuery getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PromRangeQuery>(create);
  static PromRangeQuery? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get query => $_getSZ(0);
  @$pb.TagNumber(1)
  set query($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasQuery() => $_has(0);
  @$pb.TagNumber(1)
  void clearQuery() => $_clearField(1);

  @$pb.TagNumber(2)
  $fixnum.Int64 get startUnixMs => $_getI64(1);
  @$pb.TagNumber(2)
  set startUnixMs($fixnum.Int64 value) => $_setInt64(1, value);
  @$pb.TagNumber(2)
  $core.bool hasStartUnixMs() => $_has(1);
  @$pb.TagNumber(2)
  void clearStartUnixMs() => $_clearField(2);

  @$pb.TagNumber(3)
  $fixnum.Int64 get endUnixMs => $_getI64(2);
  @$pb.TagNumber(3)
  set endUnixMs($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasEndUnixMs() => $_has(2);
  @$pb.TagNumber(3)
  void clearEndUnixMs() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get stepSecs => $_getIZ(3);
  @$pb.TagNumber(4)
  set stepSecs($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasStepSecs() => $_has(3);
  @$pb.TagNumber(4)
  void clearStepSecs() => $_clearField(4);
}

class PromSample extends $pb.GeneratedMessage {
  factory PromSample({
    $fixnum.Int64? tUnixMs,
    $core.double? value,
  }) {
    final result = create();
    if (tUnixMs != null) result.tUnixMs = tUnixMs;
    if (value != null) result.value = value;
    return result;
  }

  PromSample._();

  factory PromSample.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PromSample.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PromSample',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aInt64(1, _omitFieldNames ? '' : 'tUnixMs')
    ..aD(2, _omitFieldNames ? '' : 'value')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromSample clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromSample copyWith(void Function(PromSample) updates) =>
      super.copyWith((message) => updates(message as PromSample)) as PromSample;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PromSample create() => PromSample._();
  @$core.override
  PromSample createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PromSample getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PromSample>(create);
  static PromSample? _defaultInstance;

  @$pb.TagNumber(1)
  $fixnum.Int64 get tUnixMs => $_getI64(0);
  @$pb.TagNumber(1)
  set tUnixMs($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasTUnixMs() => $_has(0);
  @$pb.TagNumber(1)
  void clearTUnixMs() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.double get value => $_getN(1);
  @$pb.TagNumber(2)
  set value($core.double value) => $_setDouble(1, value);
  @$pb.TagNumber(2)
  $core.bool hasValue() => $_has(1);
  @$pb.TagNumber(2)
  void clearValue() => $_clearField(2);
}

class PromSeries extends $pb.GeneratedMessage {
  factory PromSeries({
    $core.Iterable<$core.MapEntry<$core.String, $core.String>>? labels,
    $core.Iterable<PromSample>? samples,
  }) {
    final result = create();
    if (labels != null) result.labels.addEntries(labels);
    if (samples != null) result.samples.addAll(samples);
    return result;
  }

  PromSeries._();

  factory PromSeries.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PromSeries.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PromSeries',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..m<$core.String, $core.String>(1, _omitFieldNames ? '' : 'labels',
        entryClassName: 'PromSeries.LabelsEntry',
        keyFieldType: $pb.PbFieldType.OS,
        valueFieldType: $pb.PbFieldType.OS,
        packageName: const $pb.PackageName('agent.v1'))
    ..pPM<PromSample>(2, _omitFieldNames ? '' : 'samples',
        subBuilder: PromSample.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromSeries clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromSeries copyWith(void Function(PromSeries) updates) =>
      super.copyWith((message) => updates(message as PromSeries)) as PromSeries;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PromSeries create() => PromSeries._();
  @$core.override
  PromSeries createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PromSeries getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PromSeries>(create);
  static PromSeries? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbMap<$core.String, $core.String> get labels => $_getMap(0);

  @$pb.TagNumber(2)
  $pb.PbList<PromSample> get samples => $_getList(1);
}

class PromResult extends $pb.GeneratedMessage {
  factory PromResult({
    $core.String? resultType,
    $core.Iterable<PromSeries>? series,
    $core.String? error,
  }) {
    final result = create();
    if (resultType != null) result.resultType = resultType;
    if (series != null) result.series.addAll(series);
    if (error != null) result.error = error;
    return result;
  }

  PromResult._();

  factory PromResult.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PromResult.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PromResult',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'resultType')
    ..pPM<PromSeries>(2, _omitFieldNames ? '' : 'series',
        subBuilder: PromSeries.create)
    ..aOS(3, _omitFieldNames ? '' : 'error')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromResult clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PromResult copyWith(void Function(PromResult) updates) =>
      super.copyWith((message) => updates(message as PromResult)) as PromResult;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PromResult create() => PromResult._();
  @$core.override
  PromResult createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PromResult getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PromResult>(create);
  static PromResult? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get resultType => $_getSZ(0);
  @$pb.TagNumber(1)
  set resultType($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasResultType() => $_has(0);
  @$pb.TagNumber(1)
  void clearResultType() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<PromSeries> get series => $_getList(1);

  @$pb.TagNumber(3)
  $core.String get error => $_getSZ(2);
  @$pb.TagNumber(3)
  set error($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasError() => $_has(2);
  @$pb.TagNumber(3)
  void clearError() => $_clearField(3);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
