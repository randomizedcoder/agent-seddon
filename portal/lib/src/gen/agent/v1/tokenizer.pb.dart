// This is a generated file - do not edit.
//
// Generated from agent/v1/tokenizer.proto.

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

class TokCountRequest extends $pb.GeneratedMessage {
  factory TokCountRequest({
    $core.String? text,
    $core.String? model,
  }) {
    final result = create();
    if (text != null) result.text = text;
    if (model != null) result.model = model;
    return result;
  }

  TokCountRequest._();

  factory TokCountRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory TokCountRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'TokCountRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'text')
    ..aOS(2, _omitFieldNames ? '' : 'model')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TokCountRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TokCountRequest copyWith(void Function(TokCountRequest) updates) =>
      super.copyWith((message) => updates(message as TokCountRequest))
          as TokCountRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static TokCountRequest create() => TokCountRequest._();
  @$core.override
  TokCountRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static TokCountRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<TokCountRequest>(create);
  static TokCountRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get text => $_getSZ(0);
  @$pb.TagNumber(1)
  set text($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasText() => $_has(0);
  @$pb.TagNumber(1)
  void clearText() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get model => $_getSZ(1);
  @$pb.TagNumber(2)
  set model($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasModel() => $_has(1);
  @$pb.TagNumber(2)
  void clearModel() => $_clearField(2);
}

class TokCountMessagesRequest extends $pb.GeneratedMessage {
  factory TokCountMessagesRequest({
    $core.Iterable<$1.Message>? messages,
    $core.String? model,
  }) {
    final result = create();
    if (messages != null) result.messages.addAll(messages);
    if (model != null) result.model = model;
    return result;
  }

  TokCountMessagesRequest._();

  factory TokCountMessagesRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory TokCountMessagesRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'TokCountMessagesRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<$1.Message>(1, _omitFieldNames ? '' : 'messages',
        subBuilder: $1.Message.create)
    ..aOS(2, _omitFieldNames ? '' : 'model')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TokCountMessagesRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TokCountMessagesRequest copyWith(
          void Function(TokCountMessagesRequest) updates) =>
      super.copyWith((message) => updates(message as TokCountMessagesRequest))
          as TokCountMessagesRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static TokCountMessagesRequest create() => TokCountMessagesRequest._();
  @$core.override
  TokCountMessagesRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static TokCountMessagesRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<TokCountMessagesRequest>(create);
  static TokCountMessagesRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<$1.Message> get messages => $_getList(0);

  @$pb.TagNumber(2)
  $core.String get model => $_getSZ(1);
  @$pb.TagNumber(2)
  set model($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasModel() => $_has(1);
  @$pb.TagNumber(2)
  void clearModel() => $_clearField(2);
}

class TokCount extends $pb.GeneratedMessage {
  factory TokCount({
    $core.int? tokens,
  }) {
    final result = create();
    if (tokens != null) result.tokens = tokens;
    return result;
  }

  TokCount._();

  factory TokCount.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory TokCount.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'TokCount',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aI(1, _omitFieldNames ? '' : 'tokens', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TokCount clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TokCount copyWith(void Function(TokCount) updates) =>
      super.copyWith((message) => updates(message as TokCount)) as TokCount;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static TokCount create() => TokCount._();
  @$core.override
  TokCount createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static TokCount getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<TokCount>(create);
  static TokCount? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get tokens => $_getIZ(0);
  @$pb.TagNumber(1)
  set tokens($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasTokens() => $_has(0);
  @$pb.TagNumber(1)
  void clearTokens() => $_clearField(1);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
