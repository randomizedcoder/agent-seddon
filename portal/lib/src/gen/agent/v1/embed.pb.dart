// This is a generated file - do not edit.
//
// Generated from agent/v1/embed.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

class EmbCapabilitiesRequest extends $pb.GeneratedMessage {
  factory EmbCapabilitiesRequest() => create();

  EmbCapabilitiesRequest._();

  factory EmbCapabilitiesRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory EmbCapabilitiesRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'EmbCapabilitiesRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EmbCapabilitiesRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EmbCapabilitiesRequest copyWith(
          void Function(EmbCapabilitiesRequest) updates) =>
      super.copyWith((message) => updates(message as EmbCapabilitiesRequest))
          as EmbCapabilitiesRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static EmbCapabilitiesRequest create() => EmbCapabilitiesRequest._();
  @$core.override
  EmbCapabilitiesRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static EmbCapabilitiesRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<EmbCapabilitiesRequest>(create);
  static EmbCapabilitiesRequest? _defaultInstance;
}

class EmbCapabilities extends $pb.GeneratedMessage {
  factory EmbCapabilities({
    $core.int? dimensions,
    $core.int? maxBatch,
  }) {
    final result = create();
    if (dimensions != null) result.dimensions = dimensions;
    if (maxBatch != null) result.maxBatch = maxBatch;
    return result;
  }

  EmbCapabilities._();

  factory EmbCapabilities.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory EmbCapabilities.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'EmbCapabilities',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aI(1, _omitFieldNames ? '' : 'dimensions', fieldType: $pb.PbFieldType.OU3)
    ..aI(2, _omitFieldNames ? '' : 'maxBatch', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EmbCapabilities clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EmbCapabilities copyWith(void Function(EmbCapabilities) updates) =>
      super.copyWith((message) => updates(message as EmbCapabilities))
          as EmbCapabilities;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static EmbCapabilities create() => EmbCapabilities._();
  @$core.override
  EmbCapabilities createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static EmbCapabilities getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<EmbCapabilities>(create);
  static EmbCapabilities? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get dimensions => $_getIZ(0);
  @$pb.TagNumber(1)
  set dimensions($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasDimensions() => $_has(0);
  @$pb.TagNumber(1)
  void clearDimensions() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get maxBatch => $_getIZ(1);
  @$pb.TagNumber(2)
  set maxBatch($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasMaxBatch() => $_has(1);
  @$pb.TagNumber(2)
  void clearMaxBatch() => $_clearField(2);
}

class EmbQueryRequest extends $pb.GeneratedMessage {
  factory EmbQueryRequest({
    $core.String? text,
  }) {
    final result = create();
    if (text != null) result.text = text;
    return result;
  }

  EmbQueryRequest._();

  factory EmbQueryRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory EmbQueryRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'EmbQueryRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'text')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EmbQueryRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EmbQueryRequest copyWith(void Function(EmbQueryRequest) updates) =>
      super.copyWith((message) => updates(message as EmbQueryRequest))
          as EmbQueryRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static EmbQueryRequest create() => EmbQueryRequest._();
  @$core.override
  EmbQueryRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static EmbQueryRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<EmbQueryRequest>(create);
  static EmbQueryRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get text => $_getSZ(0);
  @$pb.TagNumber(1)
  set text($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasText() => $_has(0);
  @$pb.TagNumber(1)
  void clearText() => $_clearField(1);
}

class EmbDocsRequest extends $pb.GeneratedMessage {
  factory EmbDocsRequest({
    $core.Iterable<$core.String>? texts,
  }) {
    final result = create();
    if (texts != null) result.texts.addAll(texts);
    return result;
  }

  EmbDocsRequest._();

  factory EmbDocsRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory EmbDocsRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'EmbDocsRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPS(1, _omitFieldNames ? '' : 'texts')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EmbDocsRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EmbDocsRequest copyWith(void Function(EmbDocsRequest) updates) =>
      super.copyWith((message) => updates(message as EmbDocsRequest))
          as EmbDocsRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static EmbDocsRequest create() => EmbDocsRequest._();
  @$core.override
  EmbDocsRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static EmbDocsRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<EmbDocsRequest>(create);
  static EmbDocsRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<$core.String> get texts => $_getList(0);
}

class EmbVector extends $pb.GeneratedMessage {
  factory EmbVector({
    $core.Iterable<$core.double>? values,
  }) {
    final result = create();
    if (values != null) result.values.addAll(values);
    return result;
  }

  EmbVector._();

  factory EmbVector.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory EmbVector.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'EmbVector',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..p<$core.double>(1, _omitFieldNames ? '' : 'values', $pb.PbFieldType.KF)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EmbVector clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EmbVector copyWith(void Function(EmbVector) updates) =>
      super.copyWith((message) => updates(message as EmbVector)) as EmbVector;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static EmbVector create() => EmbVector._();
  @$core.override
  EmbVector createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static EmbVector getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<EmbVector>(create);
  static EmbVector? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<$core.double> get values => $_getList(0);
}

class EmbVectors extends $pb.GeneratedMessage {
  factory EmbVectors({
    $core.Iterable<EmbVector>? vectors,
  }) {
    final result = create();
    if (vectors != null) result.vectors.addAll(vectors);
    return result;
  }

  EmbVectors._();

  factory EmbVectors.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory EmbVectors.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'EmbVectors',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<EmbVector>(1, _omitFieldNames ? '' : 'vectors',
        subBuilder: EmbVector.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EmbVectors clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EmbVectors copyWith(void Function(EmbVectors) updates) =>
      super.copyWith((message) => updates(message as EmbVectors)) as EmbVectors;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static EmbVectors create() => EmbVectors._();
  @$core.override
  EmbVectors createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static EmbVectors getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<EmbVectors>(create);
  static EmbVectors? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<EmbVector> get vectors => $_getList(0);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
