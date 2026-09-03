// This is a generated file - do not edit.
//
// Generated from agent/v1/common.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

import 'common.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'common.pbenum.dart';

enum JsonValue_Kind {
  nullValue,
  boolValue,
  intValue,
  uintValue,
  doubleValue,
  stringValue,
  arrayValue,
  objectValue,
  bigNumber,
  notSet
}

/// A lossless, fully-binary encoding of an arbitrary JSON value — the wire twin of
/// agent-core's `serde_json::Value`. Numbers keep their exact type: integers use
/// the 64-bit `int_value`/`uint_value` arms, non-integers use `double_value`, and
/// anything beyond native range rides in `big_number` as a decimal string. An unset
/// `kind` decodes to JSON `null`.
class JsonValue extends $pb.GeneratedMessage {
  factory JsonValue({
    NullValue? nullValue,
    $core.bool? boolValue,
    $fixnum.Int64? intValue,
    $fixnum.Int64? uintValue,
    $core.double? doubleValue,
    $core.String? stringValue,
    JsonArray? arrayValue,
    JsonObject? objectValue,
    $core.String? bigNumber,
  }) {
    final result = create();
    if (nullValue != null) result.nullValue = nullValue;
    if (boolValue != null) result.boolValue = boolValue;
    if (intValue != null) result.intValue = intValue;
    if (uintValue != null) result.uintValue = uintValue;
    if (doubleValue != null) result.doubleValue = doubleValue;
    if (stringValue != null) result.stringValue = stringValue;
    if (arrayValue != null) result.arrayValue = arrayValue;
    if (objectValue != null) result.objectValue = objectValue;
    if (bigNumber != null) result.bigNumber = bigNumber;
    return result;
  }

  JsonValue._();

  factory JsonValue.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory JsonValue.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static const $core.Map<$core.int, JsonValue_Kind> _JsonValue_KindByTag = {
    1: JsonValue_Kind.nullValue,
    2: JsonValue_Kind.boolValue,
    3: JsonValue_Kind.intValue,
    4: JsonValue_Kind.uintValue,
    5: JsonValue_Kind.doubleValue,
    6: JsonValue_Kind.stringValue,
    7: JsonValue_Kind.arrayValue,
    8: JsonValue_Kind.objectValue,
    9: JsonValue_Kind.bigNumber,
    0: JsonValue_Kind.notSet
  };
  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'JsonValue',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..oo(0, [1, 2, 3, 4, 5, 6, 7, 8, 9])
    ..aE<NullValue>(1, _omitFieldNames ? '' : 'nullValue',
        enumValues: NullValue.values)
    ..aOB(2, _omitFieldNames ? '' : 'boolValue')
    ..a<$fixnum.Int64>(
        3, _omitFieldNames ? '' : 'intValue', $pb.PbFieldType.OS6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..a<$fixnum.Int64>(
        4, _omitFieldNames ? '' : 'uintValue', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aD(5, _omitFieldNames ? '' : 'doubleValue')
    ..aOS(6, _omitFieldNames ? '' : 'stringValue')
    ..aOM<JsonArray>(7, _omitFieldNames ? '' : 'arrayValue',
        subBuilder: JsonArray.create)
    ..aOM<JsonObject>(8, _omitFieldNames ? '' : 'objectValue',
        subBuilder: JsonObject.create)
    ..aOS(9, _omitFieldNames ? '' : 'bigNumber')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  JsonValue clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  JsonValue copyWith(void Function(JsonValue) updates) =>
      super.copyWith((message) => updates(message as JsonValue)) as JsonValue;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static JsonValue create() => JsonValue._();
  @$core.override
  JsonValue createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static JsonValue getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<JsonValue>(create);
  static JsonValue? _defaultInstance;

  @$pb.TagNumber(1)
  @$pb.TagNumber(2)
  @$pb.TagNumber(3)
  @$pb.TagNumber(4)
  @$pb.TagNumber(5)
  @$pb.TagNumber(6)
  @$pb.TagNumber(7)
  @$pb.TagNumber(8)
  @$pb.TagNumber(9)
  JsonValue_Kind whichKind() => _JsonValue_KindByTag[$_whichOneof(0)]!;
  @$pb.TagNumber(1)
  @$pb.TagNumber(2)
  @$pb.TagNumber(3)
  @$pb.TagNumber(4)
  @$pb.TagNumber(5)
  @$pb.TagNumber(6)
  @$pb.TagNumber(7)
  @$pb.TagNumber(8)
  @$pb.TagNumber(9)
  void clearKind() => $_clearField($_whichOneof(0));

  @$pb.TagNumber(1)
  NullValue get nullValue => $_getN(0);
  @$pb.TagNumber(1)
  set nullValue(NullValue value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasNullValue() => $_has(0);
  @$pb.TagNumber(1)
  void clearNullValue() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.bool get boolValue => $_getBF(1);
  @$pb.TagNumber(2)
  set boolValue($core.bool value) => $_setBool(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBoolValue() => $_has(1);
  @$pb.TagNumber(2)
  void clearBoolValue() => $_clearField(2);

  @$pb.TagNumber(3)
  $fixnum.Int64 get intValue => $_getI64(2);
  @$pb.TagNumber(3)
  set intValue($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasIntValue() => $_has(2);
  @$pb.TagNumber(3)
  void clearIntValue() => $_clearField(3);

  @$pb.TagNumber(4)
  $fixnum.Int64 get uintValue => $_getI64(3);
  @$pb.TagNumber(4)
  set uintValue($fixnum.Int64 value) => $_setInt64(3, value);
  @$pb.TagNumber(4)
  $core.bool hasUintValue() => $_has(3);
  @$pb.TagNumber(4)
  void clearUintValue() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.double get doubleValue => $_getN(4);
  @$pb.TagNumber(5)
  set doubleValue($core.double value) => $_setDouble(4, value);
  @$pb.TagNumber(5)
  $core.bool hasDoubleValue() => $_has(4);
  @$pb.TagNumber(5)
  void clearDoubleValue() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.String get stringValue => $_getSZ(5);
  @$pb.TagNumber(6)
  set stringValue($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasStringValue() => $_has(5);
  @$pb.TagNumber(6)
  void clearStringValue() => $_clearField(6);

  @$pb.TagNumber(7)
  JsonArray get arrayValue => $_getN(6);
  @$pb.TagNumber(7)
  set arrayValue(JsonArray value) => $_setField(7, value);
  @$pb.TagNumber(7)
  $core.bool hasArrayValue() => $_has(6);
  @$pb.TagNumber(7)
  void clearArrayValue() => $_clearField(7);
  @$pb.TagNumber(7)
  JsonArray ensureArrayValue() => $_ensure(6);

  @$pb.TagNumber(8)
  JsonObject get objectValue => $_getN(7);
  @$pb.TagNumber(8)
  set objectValue(JsonObject value) => $_setField(8, value);
  @$pb.TagNumber(8)
  $core.bool hasObjectValue() => $_has(7);
  @$pb.TagNumber(8)
  void clearObjectValue() => $_clearField(8);
  @$pb.TagNumber(8)
  JsonObject ensureObjectValue() => $_ensure(7);

  @$pb.TagNumber(9)
  $core.String get bigNumber => $_getSZ(8);
  @$pb.TagNumber(9)
  set bigNumber($core.String value) => $_setString(8, value);
  @$pb.TagNumber(9)
  $core.bool hasBigNumber() => $_has(8);
  @$pb.TagNumber(9)
  void clearBigNumber() => $_clearField(9);
}

class JsonArray extends $pb.GeneratedMessage {
  factory JsonArray({
    $core.Iterable<JsonValue>? values,
  }) {
    final result = create();
    if (values != null) result.values.addAll(values);
    return result;
  }

  JsonArray._();

  factory JsonArray.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory JsonArray.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'JsonArray',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<JsonValue>(1, _omitFieldNames ? '' : 'values',
        subBuilder: JsonValue.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  JsonArray clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  JsonArray copyWith(void Function(JsonArray) updates) =>
      super.copyWith((message) => updates(message as JsonArray)) as JsonArray;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static JsonArray create() => JsonArray._();
  @$core.override
  JsonArray createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static JsonArray getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<JsonArray>(create);
  static JsonArray? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<JsonValue> get values => $_getList(0);
}

class JsonObject extends $pb.GeneratedMessage {
  factory JsonObject({
    $core.Iterable<$core.MapEntry<$core.String, JsonValue>>? fields,
  }) {
    final result = create();
    if (fields != null) result.fields.addEntries(fields);
    return result;
  }

  JsonObject._();

  factory JsonObject.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory JsonObject.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'JsonObject',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..m<$core.String, JsonValue>(1, _omitFieldNames ? '' : 'fields',
        entryClassName: 'JsonObject.FieldsEntry',
        keyFieldType: $pb.PbFieldType.OS,
        valueFieldType: $pb.PbFieldType.OM,
        valueCreator: JsonValue.create,
        valueDefaultOrMaker: JsonValue.getDefault,
        packageName: const $pb.PackageName('agent.v1'))
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  JsonObject clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  JsonObject copyWith(void Function(JsonObject) updates) =>
      super.copyWith((message) => updates(message as JsonObject)) as JsonObject;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static JsonObject create() => JsonObject._();
  @$core.override
  JsonObject createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static JsonObject getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<JsonObject>(create);
  static JsonObject? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbMap<$core.String, JsonValue> get fields => $_getMap(0);
}

/// A single tool invocation requested by the model.
class ToolCall extends $pb.GeneratedMessage {
  factory ToolCall({
    $core.String? id,
    $core.String? name,
    JsonValue? arguments,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (name != null) result.name = name;
    if (arguments != null) result.arguments = arguments;
    return result;
  }

  ToolCall._();

  factory ToolCall.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ToolCall.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ToolCall',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aOS(2, _omitFieldNames ? '' : 'name')
    ..aOM<JsonValue>(3, _omitFieldNames ? '' : 'arguments',
        subBuilder: JsonValue.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ToolCall clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ToolCall copyWith(void Function(ToolCall) updates) =>
      super.copyWith((message) => updates(message as ToolCall)) as ToolCall;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ToolCall create() => ToolCall._();
  @$core.override
  ToolCall createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ToolCall getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ToolCall>(create);
  static ToolCall? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get name => $_getSZ(1);
  @$pb.TagNumber(2)
  set name($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasName() => $_has(1);
  @$pb.TagNumber(2)
  void clearName() => $_clearField(2);

  /// The arguments object (agent-core `serde_json::Value`).
  @$pb.TagNumber(3)
  JsonValue get arguments => $_getN(2);
  @$pb.TagNumber(3)
  set arguments(JsonValue value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasArguments() => $_has(2);
  @$pb.TagNumber(3)
  void clearArguments() => $_clearField(3);
  @$pb.TagNumber(3)
  JsonValue ensureArguments() => $_ensure(2);
}

class ContentBlock_Text extends $pb.GeneratedMessage {
  factory ContentBlock_Text({
    $core.String? text,
  }) {
    final result = create();
    if (text != null) result.text = text;
    return result;
  }

  ContentBlock_Text._();

  factory ContentBlock_Text.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ContentBlock_Text.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ContentBlock.Text',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'text')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ContentBlock_Text clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ContentBlock_Text copyWith(void Function(ContentBlock_Text) updates) =>
      super.copyWith((message) => updates(message as ContentBlock_Text))
          as ContentBlock_Text;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ContentBlock_Text create() => ContentBlock_Text._();
  @$core.override
  ContentBlock_Text createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ContentBlock_Text getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ContentBlock_Text>(create);
  static ContentBlock_Text? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get text => $_getSZ(0);
  @$pb.TagNumber(1)
  set text($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasText() => $_has(0);
  @$pb.TagNumber(1)
  void clearText() => $_clearField(1);
}

class ContentBlock_Image extends $pb.GeneratedMessage {
  factory ContentBlock_Image({
    $core.String? mediaType,
    $core.List<$core.int>? data,
  }) {
    final result = create();
    if (mediaType != null) result.mediaType = mediaType;
    if (data != null) result.data = data;
    return result;
  }

  ContentBlock_Image._();

  factory ContentBlock_Image.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ContentBlock_Image.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ContentBlock.Image',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'mediaType')
    ..a<$core.List<$core.int>>(
        2, _omitFieldNames ? '' : 'data', $pb.PbFieldType.OY)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ContentBlock_Image clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ContentBlock_Image copyWith(void Function(ContentBlock_Image) updates) =>
      super.copyWith((message) => updates(message as ContentBlock_Image))
          as ContentBlock_Image;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ContentBlock_Image create() => ContentBlock_Image._();
  @$core.override
  ContentBlock_Image createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ContentBlock_Image getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ContentBlock_Image>(create);
  static ContentBlock_Image? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get mediaType => $_getSZ(0);
  @$pb.TagNumber(1)
  set mediaType($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasMediaType() => $_has(0);
  @$pb.TagNumber(1)
  void clearMediaType() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.List<$core.int> get data => $_getN(1);
  @$pb.TagNumber(2)
  set data($core.List<$core.int> value) => $_setBytes(1, value);
  @$pb.TagNumber(2)
  $core.bool hasData() => $_has(1);
  @$pb.TagNumber(2)
  void clearData() => $_clearField(2);
}

class ContentBlock_Document extends $pb.GeneratedMessage {
  factory ContentBlock_Document({
    $core.String? mediaType,
    $core.List<$core.int>? data,
    $core.String? name,
  }) {
    final result = create();
    if (mediaType != null) result.mediaType = mediaType;
    if (data != null) result.data = data;
    if (name != null) result.name = name;
    return result;
  }

  ContentBlock_Document._();

  factory ContentBlock_Document.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ContentBlock_Document.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ContentBlock.Document',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'mediaType')
    ..a<$core.List<$core.int>>(
        2, _omitFieldNames ? '' : 'data', $pb.PbFieldType.OY)
    ..aOS(3, _omitFieldNames ? '' : 'name')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ContentBlock_Document clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ContentBlock_Document copyWith(
          void Function(ContentBlock_Document) updates) =>
      super.copyWith((message) => updates(message as ContentBlock_Document))
          as ContentBlock_Document;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ContentBlock_Document create() => ContentBlock_Document._();
  @$core.override
  ContentBlock_Document createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ContentBlock_Document getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ContentBlock_Document>(create);
  static ContentBlock_Document? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get mediaType => $_getSZ(0);
  @$pb.TagNumber(1)
  set mediaType($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasMediaType() => $_has(0);
  @$pb.TagNumber(1)
  void clearMediaType() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.List<$core.int> get data => $_getN(1);
  @$pb.TagNumber(2)
  set data($core.List<$core.int> value) => $_setBytes(1, value);
  @$pb.TagNumber(2)
  $core.bool hasData() => $_has(1);
  @$pb.TagNumber(2)
  void clearData() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get name => $_getSZ(2);
  @$pb.TagNumber(3)
  set name($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasName() => $_has(2);
  @$pb.TagNumber(3)
  void clearName() => $_clearField(3);
}

enum ContentBlock_Kind { text, image, document, notSet }

/// One typed piece of message content (agent-core `ContentBlock`): prose, an
/// image, or a document. `data` is raw bytes — not base64 — since proto carries
/// binary natively.
class ContentBlock extends $pb.GeneratedMessage {
  factory ContentBlock({
    ContentBlock_Text? text,
    ContentBlock_Image? image,
    ContentBlock_Document? document,
  }) {
    final result = create();
    if (text != null) result.text = text;
    if (image != null) result.image = image;
    if (document != null) result.document = document;
    return result;
  }

  ContentBlock._();

  factory ContentBlock.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ContentBlock.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static const $core.Map<$core.int, ContentBlock_Kind> _ContentBlock_KindByTag =
      {
    1: ContentBlock_Kind.text,
    2: ContentBlock_Kind.image,
    3: ContentBlock_Kind.document,
    0: ContentBlock_Kind.notSet
  };
  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ContentBlock',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..oo(0, [1, 2, 3])
    ..aOM<ContentBlock_Text>(1, _omitFieldNames ? '' : 'text',
        subBuilder: ContentBlock_Text.create)
    ..aOM<ContentBlock_Image>(2, _omitFieldNames ? '' : 'image',
        subBuilder: ContentBlock_Image.create)
    ..aOM<ContentBlock_Document>(3, _omitFieldNames ? '' : 'document',
        subBuilder: ContentBlock_Document.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ContentBlock clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ContentBlock copyWith(void Function(ContentBlock) updates) =>
      super.copyWith((message) => updates(message as ContentBlock))
          as ContentBlock;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ContentBlock create() => ContentBlock._();
  @$core.override
  ContentBlock createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ContentBlock getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ContentBlock>(create);
  static ContentBlock? _defaultInstance;

  @$pb.TagNumber(1)
  @$pb.TagNumber(2)
  @$pb.TagNumber(3)
  ContentBlock_Kind whichKind() => _ContentBlock_KindByTag[$_whichOneof(0)]!;
  @$pb.TagNumber(1)
  @$pb.TagNumber(2)
  @$pb.TagNumber(3)
  void clearKind() => $_clearField($_whichOneof(0));

  @$pb.TagNumber(1)
  ContentBlock_Text get text => $_getN(0);
  @$pb.TagNumber(1)
  set text(ContentBlock_Text value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasText() => $_has(0);
  @$pb.TagNumber(1)
  void clearText() => $_clearField(1);
  @$pb.TagNumber(1)
  ContentBlock_Text ensureText() => $_ensure(0);

  @$pb.TagNumber(2)
  ContentBlock_Image get image => $_getN(1);
  @$pb.TagNumber(2)
  set image(ContentBlock_Image value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasImage() => $_has(1);
  @$pb.TagNumber(2)
  void clearImage() => $_clearField(2);
  @$pb.TagNumber(2)
  ContentBlock_Image ensureImage() => $_ensure(1);

  @$pb.TagNumber(3)
  ContentBlock_Document get document => $_getN(2);
  @$pb.TagNumber(3)
  set document(ContentBlock_Document value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasDocument() => $_has(2);
  @$pb.TagNumber(3)
  void clearDocument() => $_clearField(3);
  @$pb.TagNumber(3)
  ContentBlock_Document ensureDocument() => $_ensure(2);
}

class Message extends $pb.GeneratedMessage {
  factory Message({
    Role? role,
    $core.String? content,
    $core.Iterable<ToolCall>? toolCalls,
    $core.String? toolCallId,
    $core.Iterable<ContentBlock>? blocks,
  }) {
    final result = create();
    if (role != null) result.role = role;
    if (content != null) result.content = content;
    if (toolCalls != null) result.toolCalls.addAll(toolCalls);
    if (toolCallId != null) result.toolCallId = toolCallId;
    if (blocks != null) result.blocks.addAll(blocks);
    return result;
  }

  Message._();

  factory Message.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory Message.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Message',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aE<Role>(1, _omitFieldNames ? '' : 'role', enumValues: Role.values)
    ..aOS(2, _omitFieldNames ? '' : 'content')
    ..pPM<ToolCall>(3, _omitFieldNames ? '' : 'toolCalls',
        subBuilder: ToolCall.create)
    ..aOS(4, _omitFieldNames ? '' : 'toolCallId')
    ..pPM<ContentBlock>(5, _omitFieldNames ? '' : 'blocks',
        subBuilder: ContentBlock.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Message clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Message copyWith(void Function(Message) updates) =>
      super.copyWith((message) => updates(message as Message)) as Message;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static Message create() => Message._();
  @$core.override
  Message createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static Message getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<Message>(create);
  static Message? _defaultInstance;

  @$pb.TagNumber(1)
  Role get role => $_getN(0);
  @$pb.TagNumber(1)
  set role(Role value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasRole() => $_has(0);
  @$pb.TagNumber(1)
  void clearRole() => $_clearField(1);

  /// The message's TEXT, always populated. Kept as field 2 with its original type
  /// so this change stays wire-compatible: a peer built before multimodal landed
  /// still reads the prose of every message, just without the media.
  @$pb.TagNumber(2)
  $core.String get content => $_getSZ(1);
  @$pb.TagNumber(2)
  set content($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasContent() => $_has(1);
  @$pb.TagNumber(2)
  void clearContent() => $_clearField(2);

  @$pb.TagNumber(3)
  $pb.PbList<ToolCall> get toolCalls => $_getList(2);

  @$pb.TagNumber(4)
  $core.String get toolCallId => $_getSZ(3);
  @$pb.TagNumber(4)
  set toolCallId($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasToolCallId() => $_has(3);
  @$pb.TagNumber(4)
  void clearToolCallId() => $_clearField(4);

  /// The full ordered block list. Set only when the content is NOT simply
  /// zero-or-one text block (i.e. when there is media, or several text blocks) —
  /// so the common text-only message costs nothing extra on the wire. When empty,
  /// decoders fold `content` into a single text block.
  @$pb.TagNumber(5)
  $pb.PbList<ContentBlock> get blocks => $_getList(4);
}

/// A tool's advertised interface (what we hand the model).
class ToolSchema extends $pb.GeneratedMessage {
  factory ToolSchema({
    $core.String? name,
    $core.String? description,
    JsonValue? parameters,
    $core.bool? parallelSafe,
  }) {
    final result = create();
    if (name != null) result.name = name;
    if (description != null) result.description = description;
    if (parameters != null) result.parameters = parameters;
    if (parallelSafe != null) result.parallelSafe = parallelSafe;
    return result;
  }

  ToolSchema._();

  factory ToolSchema.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ToolSchema.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ToolSchema',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'name')
    ..aOS(2, _omitFieldNames ? '' : 'description')
    ..aOM<JsonValue>(3, _omitFieldNames ? '' : 'parameters',
        subBuilder: JsonValue.create)
    ..aOB(4, _omitFieldNames ? '' : 'parallelSafe')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ToolSchema clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ToolSchema copyWith(void Function(ToolSchema) updates) =>
      super.copyWith((message) => updates(message as ToolSchema)) as ToolSchema;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ToolSchema create() => ToolSchema._();
  @$core.override
  ToolSchema createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ToolSchema getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ToolSchema>(create);
  static ToolSchema? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get name => $_getSZ(0);
  @$pb.TagNumber(1)
  set name($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasName() => $_has(0);
  @$pb.TagNumber(1)
  void clearName() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get description => $_getSZ(1);
  @$pb.TagNumber(2)
  set description($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasDescription() => $_has(1);
  @$pb.TagNumber(2)
  void clearDescription() => $_clearField(2);

  /// JSON Schema for the arguments object.
  @$pb.TagNumber(3)
  JsonValue get parameters => $_getN(2);
  @$pb.TagNumber(3)
  set parameters(JsonValue value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasParameters() => $_has(2);
  @$pb.TagNumber(3)
  void clearParameters() => $_clearField(3);
  @$pb.TagNumber(3)
  JsonValue ensureParameters() => $_ensure(2);

  /// Whether the tool is safe to run concurrently with other tool calls (the
  /// `Tool::parallel_safe` contract). Carried so a *remote* tool's concurrency
  /// contract survives the seam — a non-parallel-safe tool (e.g. `bash`) must not
  /// be run concurrently just because it is behind gRPC. Defaults to true.
  @$pb.TagNumber(4)
  $core.bool get parallelSafe => $_getBF(3);
  @$pb.TagNumber(4)
  set parallelSafe($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasParallelSafe() => $_has(3);
  @$pb.TagNumber(4)
  void clearParallelSafe() => $_clearField(4);
}

class Observation extends $pb.GeneratedMessage {
  factory Observation({
    $core.String? content,
    $core.bool? isError,
    $core.Iterable<ContentBlock>? blocks,
  }) {
    final result = create();
    if (content != null) result.content = content;
    if (isError != null) result.isError = isError;
    if (blocks != null) result.blocks.addAll(blocks);
    return result;
  }

  Observation._();

  factory Observation.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory Observation.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Observation',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'content')
    ..aOB(2, _omitFieldNames ? '' : 'isError')
    ..pPM<ContentBlock>(3, _omitFieldNames ? '' : 'blocks',
        subBuilder: ContentBlock.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Observation clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Observation copyWith(void Function(Observation) updates) =>
      super.copyWith((message) => updates(message as Observation))
          as Observation;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static Observation create() => Observation._();
  @$core.override
  Observation createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static Observation getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<Observation>(create);
  static Observation? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get content => $_getSZ(0);
  @$pb.TagNumber(1)
  set content($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasContent() => $_has(0);
  @$pb.TagNumber(1)
  void clearContent() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.bool get isError => $_getBF(1);
  @$pb.TagNumber(2)
  set isError($core.bool value) => $_setBool(1, value);
  @$pb.TagNumber(2)
  $core.bool hasIsError() => $_has(1);
  @$pb.TagNumber(2)
  void clearIsError() => $_clearField(2);

  /// Typed media the tool produced (a screenshot, an image read off disk).
  /// Additive: `content` remains the text summary every tool populates.
  @$pb.TagNumber(3)
  $pb.PbList<ContentBlock> get blocks => $_getList(2);
}

/// Ambient context handed to every tool invocation.
class ToolContext extends $pb.GeneratedMessage {
  factory ToolContext({
    $core.String? cwd,
  }) {
    final result = create();
    if (cwd != null) result.cwd = cwd;
    return result;
  }

  ToolContext._();

  factory ToolContext.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ToolContext.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ToolContext',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'cwd')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ToolContext clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ToolContext copyWith(void Function(ToolContext) updates) =>
      super.copyWith((message) => updates(message as ToolContext))
          as ToolContext;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ToolContext create() => ToolContext._();
  @$core.override
  ToolContext createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ToolContext getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ToolContext>(create);
  static ToolContext? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get cwd => $_getSZ(0);
  @$pb.TagNumber(1)
  set cwd($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasCwd() => $_has(0);
  @$pb.TagNumber(1)
  void clearCwd() => $_clearField(1);
}

class ModelCapabilities extends $pb.GeneratedMessage {
  factory ModelCapabilities({
    $core.bool? supportsTools,
    $core.int? contextWindow,
    $core.bool? supportsVision,
  }) {
    final result = create();
    if (supportsTools != null) result.supportsTools = supportsTools;
    if (contextWindow != null) result.contextWindow = contextWindow;
    if (supportsVision != null) result.supportsVision = supportsVision;
    return result;
  }

  ModelCapabilities._();

  factory ModelCapabilities.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ModelCapabilities.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ModelCapabilities',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'supportsTools')
    ..aI(2, _omitFieldNames ? '' : 'contextWindow',
        fieldType: $pb.PbFieldType.OU3)
    ..aOB(3, _omitFieldNames ? '' : 'supportsVision')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ModelCapabilities clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ModelCapabilities copyWith(void Function(ModelCapabilities) updates) =>
      super.copyWith((message) => updates(message as ModelCapabilities))
          as ModelCapabilities;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ModelCapabilities create() => ModelCapabilities._();
  @$core.override
  ModelCapabilities createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ModelCapabilities getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ModelCapabilities>(create);
  static ModelCapabilities? _defaultInstance;

  @$pb.TagNumber(1)
  $core.bool get supportsTools => $_getBF(0);
  @$pb.TagNumber(1)
  set supportsTools($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSupportsTools() => $_has(0);
  @$pb.TagNumber(1)
  void clearSupportsTools() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get contextWindow => $_getIZ(1);
  @$pb.TagNumber(2)
  set contextWindow($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasContextWindow() => $_has(1);
  @$pb.TagNumber(2)
  void clearContextWindow() => $_clearField(2);

  /// Whether the model accepts image content blocks. Carried on the wire so a
  /// `= "grpc"` provider client can strip media BEFORE sending, instead of the
  /// remote model rejecting the whole request.
  @$pb.TagNumber(3)
  $core.bool get supportsVision => $_getBF(2);
  @$pb.TagNumber(3)
  set supportsVision($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasSupportsVision() => $_has(2);
  @$pb.TagNumber(3)
  void clearSupportsVision() => $_clearField(3);
}

class Usage extends $pb.GeneratedMessage {
  factory Usage({
    $core.int? promptTokens,
    $core.int? completionTokens,
    $core.int? totalTokens,
  }) {
    final result = create();
    if (promptTokens != null) result.promptTokens = promptTokens;
    if (completionTokens != null) result.completionTokens = completionTokens;
    if (totalTokens != null) result.totalTokens = totalTokens;
    return result;
  }

  Usage._();

  factory Usage.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory Usage.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Usage',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aI(1, _omitFieldNames ? '' : 'promptTokens',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(2, _omitFieldNames ? '' : 'completionTokens',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(3, _omitFieldNames ? '' : 'totalTokens',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Usage clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Usage copyWith(void Function(Usage) updates) =>
      super.copyWith((message) => updates(message as Usage)) as Usage;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static Usage create() => Usage._();
  @$core.override
  Usage createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static Usage getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<Usage>(create);
  static Usage? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get promptTokens => $_getIZ(0);
  @$pb.TagNumber(1)
  set promptTokens($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPromptTokens() => $_has(0);
  @$pb.TagNumber(1)
  void clearPromptTokens() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get completionTokens => $_getIZ(1);
  @$pb.TagNumber(2)
  set completionTokens($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCompletionTokens() => $_has(1);
  @$pb.TagNumber(2)
  void clearCompletionTokens() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get totalTokens => $_getIZ(2);
  @$pb.TagNumber(3)
  set totalTokens($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasTotalTokens() => $_has(2);
  @$pb.TagNumber(3)
  void clearTotalTokens() => $_clearField(3);
}

/// Per-request routing signals (model-router 02b). Mirrors
/// `agent_core::RouteHint`; advisory and fail-soft — the receiver sanitizes every
/// value (hostile numbers dropped/clamped, over-long override ids dropped) and a
/// hint can only narrow selection over the receiver's configured fleet.
class RouteHint extends $pb.GeneratedMessage {
  factory RouteHint({
    TaskMode? taskMode,
    RouteRole? role,
    $core.int? minContext,
    $core.double? maxCost,
    PoolTier? tier,
    $core.String? overrideUpstream,
  }) {
    final result = create();
    if (taskMode != null) result.taskMode = taskMode;
    if (role != null) result.role = role;
    if (minContext != null) result.minContext = minContext;
    if (maxCost != null) result.maxCost = maxCost;
    if (tier != null) result.tier = tier;
    if (overrideUpstream != null) result.overrideUpstream = overrideUpstream;
    return result;
  }

  RouteHint._();

  factory RouteHint.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RouteHint.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RouteHint',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aE<TaskMode>(1, _omitFieldNames ? '' : 'taskMode',
        enumValues: TaskMode.values)
    ..aE<RouteRole>(2, _omitFieldNames ? '' : 'role',
        enumValues: RouteRole.values)
    ..aI(3, _omitFieldNames ? '' : 'minContext', fieldType: $pb.PbFieldType.OU3)
    ..aD(4, _omitFieldNames ? '' : 'maxCost', fieldType: $pb.PbFieldType.OF)
    ..aE<PoolTier>(5, _omitFieldNames ? '' : 'tier',
        enumValues: PoolTier.values)
    ..aOS(6, _omitFieldNames ? '' : 'overrideUpstream')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RouteHint clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RouteHint copyWith(void Function(RouteHint) updates) =>
      super.copyWith((message) => updates(message as RouteHint)) as RouteHint;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RouteHint create() => RouteHint._();
  @$core.override
  RouteHint createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RouteHint getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<RouteHint>(create);
  static RouteHint? _defaultInstance;

  @$pb.TagNumber(1)
  TaskMode get taskMode => $_getN(0);
  @$pb.TagNumber(1)
  set taskMode(TaskMode value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasTaskMode() => $_has(0);
  @$pb.TagNumber(1)
  void clearTaskMode() => $_clearField(1);

  @$pb.TagNumber(2)
  RouteRole get role => $_getN(1);
  @$pb.TagNumber(2)
  set role(RouteRole value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasRole() => $_has(1);
  @$pb.TagNumber(2)
  void clearRole() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get minContext => $_getIZ(2);
  @$pb.TagNumber(3)
  set minContext($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasMinContext() => $_has(2);
  @$pb.TagNumber(3)
  void clearMinContext() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.double get maxCost => $_getN(3);
  @$pb.TagNumber(4)
  set maxCost($core.double value) => $_setFloat(3, value);
  @$pb.TagNumber(4)
  $core.bool hasMaxCost() => $_has(3);
  @$pb.TagNumber(4)
  void clearMaxCost() => $_clearField(4);

  @$pb.TagNumber(5)
  PoolTier get tier => $_getN(4);
  @$pb.TagNumber(5)
  set tier(PoolTier value) => $_setField(5, value);
  @$pb.TagNumber(5)
  $core.bool hasTier() => $_has(4);
  @$pb.TagNumber(5)
  void clearTier() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.String get overrideUpstream => $_getSZ(5);
  @$pb.TagNumber(6)
  set overrideUpstream($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasOverrideUpstream() => $_has(5);
  @$pb.TagNumber(6)
  void clearOverrideUpstream() => $_clearField(6);
}

class CompletionRequest extends $pb.GeneratedMessage {
  factory CompletionRequest({
    $core.Iterable<Message>? messages,
    $core.Iterable<ToolSchema>? tools,
    $core.int? maxTokens,
    $core.double? temperature,
    RouteHint? routeHint,
  }) {
    final result = create();
    if (messages != null) result.messages.addAll(messages);
    if (tools != null) result.tools.addAll(tools);
    if (maxTokens != null) result.maxTokens = maxTokens;
    if (temperature != null) result.temperature = temperature;
    if (routeHint != null) result.routeHint = routeHint;
    return result;
  }

  CompletionRequest._();

  factory CompletionRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CompletionRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CompletionRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<Message>(1, _omitFieldNames ? '' : 'messages',
        subBuilder: Message.create)
    ..pPM<ToolSchema>(2, _omitFieldNames ? '' : 'tools',
        subBuilder: ToolSchema.create)
    ..aI(3, _omitFieldNames ? '' : 'maxTokens', fieldType: $pb.PbFieldType.OU3)
    ..aD(4, _omitFieldNames ? '' : 'temperature', fieldType: $pb.PbFieldType.OF)
    ..aOM<RouteHint>(5, _omitFieldNames ? '' : 'routeHint',
        subBuilder: RouteHint.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CompletionRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CompletionRequest copyWith(void Function(CompletionRequest) updates) =>
      super.copyWith((message) => updates(message as CompletionRequest))
          as CompletionRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CompletionRequest create() => CompletionRequest._();
  @$core.override
  CompletionRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CompletionRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<CompletionRequest>(create);
  static CompletionRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<Message> get messages => $_getList(0);

  @$pb.TagNumber(2)
  $pb.PbList<ToolSchema> get tools => $_getList(1);

  @$pb.TagNumber(3)
  $core.int get maxTokens => $_getIZ(2);
  @$pb.TagNumber(3)
  set maxTokens($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasMaxTokens() => $_has(2);
  @$pb.TagNumber(3)
  void clearMaxTokens() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.double get temperature => $_getN(3);
  @$pb.TagNumber(4)
  set temperature($core.double value) => $_setFloat(3, value);
  @$pb.TagNumber(4)
  $core.bool hasTemperature() => $_has(3);
  @$pb.TagNumber(4)
  void clearTemperature() => $_clearField(4);

  /// Routing signals for a routing provider; absent = today's behaviour.
  @$pb.TagNumber(5)
  RouteHint get routeHint => $_getN(4);
  @$pb.TagNumber(5)
  set routeHint(RouteHint value) => $_setField(5, value);
  @$pb.TagNumber(5)
  $core.bool hasRouteHint() => $_has(4);
  @$pb.TagNumber(5)
  void clearRouteHint() => $_clearField(5);
  @$pb.TagNumber(5)
  RouteHint ensureRouteHint() => $_ensure(4);
}

class CompletionResponse extends $pb.GeneratedMessage {
  factory CompletionResponse({
    Message? message,
    $core.String? finishReason,
    Usage? usage,
  }) {
    final result = create();
    if (message != null) result.message = message;
    if (finishReason != null) result.finishReason = finishReason;
    if (usage != null) result.usage = usage;
    return result;
  }

  CompletionResponse._();

  factory CompletionResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CompletionResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CompletionResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<Message>(1, _omitFieldNames ? '' : 'message',
        subBuilder: Message.create)
    ..aOS(2, _omitFieldNames ? '' : 'finishReason')
    ..aOM<Usage>(3, _omitFieldNames ? '' : 'usage', subBuilder: Usage.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CompletionResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CompletionResponse copyWith(void Function(CompletionResponse) updates) =>
      super.copyWith((message) => updates(message as CompletionResponse))
          as CompletionResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CompletionResponse create() => CompletionResponse._();
  @$core.override
  CompletionResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CompletionResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<CompletionResponse>(create);
  static CompletionResponse? _defaultInstance;

  @$pb.TagNumber(1)
  Message get message => $_getN(0);
  @$pb.TagNumber(1)
  set message(Message value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasMessage() => $_has(0);
  @$pb.TagNumber(1)
  void clearMessage() => $_clearField(1);
  @$pb.TagNumber(1)
  Message ensureMessage() => $_ensure(0);

  @$pb.TagNumber(2)
  $core.String get finishReason => $_getSZ(1);
  @$pb.TagNumber(2)
  set finishReason($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasFinishReason() => $_has(1);
  @$pb.TagNumber(2)
  void clearFinishReason() => $_clearField(2);

  @$pb.TagNumber(3)
  Usage get usage => $_getN(2);
  @$pb.TagNumber(3)
  set usage(Usage value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasUsage() => $_has(2);
  @$pb.TagNumber(3)
  void clearUsage() => $_clearField(3);
  @$pb.TagNumber(3)
  Usage ensureUsage() => $_ensure(2);
}

/// One increment of a streamed completion (server-streamed by Provider.Stream).
class CompletionChunk extends $pb.GeneratedMessage {
  factory CompletionChunk({
    $core.String? deltaText,
    ToolCall? toolCall,
    $core.String? finishReason,
    Usage? usage,
  }) {
    final result = create();
    if (deltaText != null) result.deltaText = deltaText;
    if (toolCall != null) result.toolCall = toolCall;
    if (finishReason != null) result.finishReason = finishReason;
    if (usage != null) result.usage = usage;
    return result;
  }

  CompletionChunk._();

  factory CompletionChunk.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CompletionChunk.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CompletionChunk',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'deltaText')
    ..aOM<ToolCall>(2, _omitFieldNames ? '' : 'toolCall',
        subBuilder: ToolCall.create)
    ..aOS(3, _omitFieldNames ? '' : 'finishReason')
    ..aOM<Usage>(4, _omitFieldNames ? '' : 'usage', subBuilder: Usage.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CompletionChunk clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CompletionChunk copyWith(void Function(CompletionChunk) updates) =>
      super.copyWith((message) => updates(message as CompletionChunk))
          as CompletionChunk;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CompletionChunk create() => CompletionChunk._();
  @$core.override
  CompletionChunk createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CompletionChunk getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<CompletionChunk>(create);
  static CompletionChunk? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get deltaText => $_getSZ(0);
  @$pb.TagNumber(1)
  set deltaText($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasDeltaText() => $_has(0);
  @$pb.TagNumber(1)
  void clearDeltaText() => $_clearField(1);

  @$pb.TagNumber(2)
  ToolCall get toolCall => $_getN(1);
  @$pb.TagNumber(2)
  set toolCall(ToolCall value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasToolCall() => $_has(1);
  @$pb.TagNumber(2)
  void clearToolCall() => $_clearField(2);
  @$pb.TagNumber(2)
  ToolCall ensureToolCall() => $_ensure(1);

  @$pb.TagNumber(3)
  $core.String get finishReason => $_getSZ(2);
  @$pb.TagNumber(3)
  set finishReason($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasFinishReason() => $_has(2);
  @$pb.TagNumber(3)
  void clearFinishReason() => $_clearField(3);

  @$pb.TagNumber(4)
  Usage get usage => $_getN(3);
  @$pb.TagNumber(4)
  set usage(Usage value) => $_setField(4, value);
  @$pb.TagNumber(4)
  $core.bool hasUsage() => $_has(3);
  @$pb.TagNumber(4)
  void clearUsage() => $_clearField(4);
  @$pb.TagNumber(4)
  Usage ensureUsage() => $_ensure(3);
}

/// A recalled memory item, ready to inject into context.
class MemoryItem extends $pb.GeneratedMessage {
  factory MemoryItem({
    $core.String? source,
    $core.String? content,
  }) {
    final result = create();
    if (source != null) result.source = source;
    if (content != null) result.content = content;
    return result;
  }

  MemoryItem._();

  factory MemoryItem.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory MemoryItem.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'MemoryItem',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'source')
    ..aOS(2, _omitFieldNames ? '' : 'content')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  MemoryItem clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  MemoryItem copyWith(void Function(MemoryItem) updates) =>
      super.copyWith((message) => updates(message as MemoryItem)) as MemoryItem;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static MemoryItem create() => MemoryItem._();
  @$core.override
  MemoryItem createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static MemoryItem getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<MemoryItem>(create);
  static MemoryItem? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get source => $_getSZ(0);
  @$pb.TagNumber(1)
  set source($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSource() => $_has(0);
  @$pb.TagNumber(1)
  void clearSource() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get content => $_getSZ(1);
  @$pb.TagNumber(2)
  set content($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasContent() => $_has(1);
  @$pb.TagNumber(2)
  void clearContent() => $_clearField(2);
}

class RecallQuery extends $pb.GeneratedMessage {
  factory RecallQuery({
    $core.String? text,
    $fixnum.Int64? limit,
  }) {
    final result = create();
    if (text != null) result.text = text;
    if (limit != null) result.limit = limit;
    return result;
  }

  RecallQuery._();

  factory RecallQuery.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RecallQuery.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RecallQuery',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'text')
    ..a<$fixnum.Int64>(2, _omitFieldNames ? '' : 'limit', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RecallQuery clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RecallQuery copyWith(void Function(RecallQuery) updates) =>
      super.copyWith((message) => updates(message as RecallQuery))
          as RecallQuery;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RecallQuery create() => RecallQuery._();
  @$core.override
  RecallQuery createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RecallQuery getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RecallQuery>(create);
  static RecallQuery? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get text => $_getSZ(0);
  @$pb.TagNumber(1)
  set text($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasText() => $_has(0);
  @$pb.TagNumber(1)
  void clearText() => $_clearField(1);

  /// agent-core `usize`.
  @$pb.TagNumber(2)
  $fixnum.Int64 get limit => $_getI64(1);
  @$pb.TagNumber(2)
  set limit($fixnum.Int64 value) => $_setInt64(1, value);
  @$pb.TagNumber(2)
  $core.bool hasLimit() => $_has(1);
  @$pb.TagNumber(2)
  void clearLimit() => $_clearField(2);
}

/// An append-only episodic event.
class MemoryEvent extends $pb.GeneratedMessage {
  factory MemoryEvent({
    $core.String? kind,
    Message? message,
    $fixnum.Int64? tsMs,
    $core.String? sessionId,
    Usage? usage,
    $core.int? iter,
  }) {
    final result = create();
    if (kind != null) result.kind = kind;
    if (message != null) result.message = message;
    if (tsMs != null) result.tsMs = tsMs;
    if (sessionId != null) result.sessionId = sessionId;
    if (usage != null) result.usage = usage;
    if (iter != null) result.iter = iter;
    return result;
  }

  MemoryEvent._();

  factory MemoryEvent.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory MemoryEvent.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'MemoryEvent',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'kind')
    ..aOM<Message>(2, _omitFieldNames ? '' : 'message',
        subBuilder: Message.create)
    ..a<$fixnum.Int64>(3, _omitFieldNames ? '' : 'tsMs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOS(4, _omitFieldNames ? '' : 'sessionId')
    ..aOM<Usage>(5, _omitFieldNames ? '' : 'usage', subBuilder: Usage.create)
    ..aI(6, _omitFieldNames ? '' : 'iter', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  MemoryEvent clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  MemoryEvent copyWith(void Function(MemoryEvent) updates) =>
      super.copyWith((message) => updates(message as MemoryEvent))
          as MemoryEvent;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static MemoryEvent create() => MemoryEvent._();
  @$core.override
  MemoryEvent createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static MemoryEvent getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<MemoryEvent>(create);
  static MemoryEvent? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get kind => $_getSZ(0);
  @$pb.TagNumber(1)
  set kind($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasKind() => $_has(0);
  @$pb.TagNumber(1)
  void clearKind() => $_clearField(1);

  @$pb.TagNumber(2)
  Message get message => $_getN(1);
  @$pb.TagNumber(2)
  set message(Message value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasMessage() => $_has(1);
  @$pb.TagNumber(2)
  void clearMessage() => $_clearField(2);
  @$pb.TagNumber(2)
  Message ensureMessage() => $_ensure(1);

  @$pb.TagNumber(3)
  $fixnum.Int64 get tsMs => $_getI64(2);
  @$pb.TagNumber(3)
  set tsMs($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasTsMs() => $_has(2);
  @$pb.TagNumber(3)
  void clearTsMs() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get sessionId => $_getSZ(3);
  @$pb.TagNumber(4)
  set sessionId($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasSessionId() => $_has(3);
  @$pb.TagNumber(4)
  void clearSessionId() => $_clearField(4);

  @$pb.TagNumber(5)
  Usage get usage => $_getN(4);
  @$pb.TagNumber(5)
  set usage(Usage value) => $_setField(5, value);
  @$pb.TagNumber(5)
  $core.bool hasUsage() => $_has(4);
  @$pb.TagNumber(5)
  void clearUsage() => $_clearField(5);
  @$pb.TagNumber(5)
  Usage ensureUsage() => $_ensure(4);

  @$pb.TagNumber(6)
  $core.int get iter => $_getIZ(5);
  @$pb.TagNumber(6)
  set iter($core.int value) => $_setUnsignedInt32(5, value);
  @$pb.TagNumber(6)
  $core.bool hasIter() => $_has(5);
  @$pb.TagNumber(6)
  void clearIter() => $_clearField(6);
}

/// A fixed, user-provided block of context (from a `context.d/` file).
class ContextBlock extends $pb.GeneratedMessage {
  factory ContextBlock({
    $core.String? source,
    $core.String? content,
  }) {
    final result = create();
    if (source != null) result.source = source;
    if (content != null) result.content = content;
    return result;
  }

  ContextBlock._();

  factory ContextBlock.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ContextBlock.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ContextBlock',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'source')
    ..aOS(2, _omitFieldNames ? '' : 'content')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ContextBlock clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ContextBlock copyWith(void Function(ContextBlock) updates) =>
      super.copyWith((message) => updates(message as ContextBlock))
          as ContextBlock;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ContextBlock create() => ContextBlock._();
  @$core.override
  ContextBlock createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ContextBlock getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ContextBlock>(create);
  static ContextBlock? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get source => $_getSZ(0);
  @$pb.TagNumber(1)
  set source($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSource() => $_has(0);
  @$pb.TagNumber(1)
  void clearSource() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get content => $_getSZ(1);
  @$pb.TagNumber(2)
  set content($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasContent() => $_has(1);
  @$pb.TagNumber(2)
  void clearContent() => $_clearField(2);
}

class ContextInput extends $pb.GeneratedMessage {
  factory ContextInput({
    $core.String? systemPrompt,
    $core.Iterable<ContextBlock>? prepend,
    $core.Iterable<MemoryItem>? recalled,
    $core.String? goal,
    $core.Iterable<ContextBlock>? append,
  }) {
    final result = create();
    if (systemPrompt != null) result.systemPrompt = systemPrompt;
    if (prepend != null) result.prepend.addAll(prepend);
    if (recalled != null) result.recalled.addAll(recalled);
    if (goal != null) result.goal = goal;
    if (append != null) result.append.addAll(append);
    return result;
  }

  ContextInput._();

  factory ContextInput.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ContextInput.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ContextInput',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'systemPrompt')
    ..pPM<ContextBlock>(2, _omitFieldNames ? '' : 'prepend',
        subBuilder: ContextBlock.create)
    ..pPM<MemoryItem>(3, _omitFieldNames ? '' : 'recalled',
        subBuilder: MemoryItem.create)
    ..aOS(4, _omitFieldNames ? '' : 'goal')
    ..pPM<ContextBlock>(5, _omitFieldNames ? '' : 'append',
        subBuilder: ContextBlock.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ContextInput clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ContextInput copyWith(void Function(ContextInput) updates) =>
      super.copyWith((message) => updates(message as ContextInput))
          as ContextInput;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ContextInput create() => ContextInput._();
  @$core.override
  ContextInput createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ContextInput getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ContextInput>(create);
  static ContextInput? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get systemPrompt => $_getSZ(0);
  @$pb.TagNumber(1)
  set systemPrompt($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSystemPrompt() => $_has(0);
  @$pb.TagNumber(1)
  void clearSystemPrompt() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<ContextBlock> get prepend => $_getList(1);

  @$pb.TagNumber(3)
  $pb.PbList<MemoryItem> get recalled => $_getList(2);

  @$pb.TagNumber(4)
  $core.String get goal => $_getSZ(3);
  @$pb.TagNumber(4)
  set goal($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasGoal() => $_has(3);
  @$pb.TagNumber(4)
  void clearGoal() => $_clearField(4);

  @$pb.TagNumber(5)
  $pb.PbList<ContextBlock> get append => $_getList(4);
}

/// The live message window handed to the model each turn.
class WorkingSet extends $pb.GeneratedMessage {
  factory WorkingSet({
    $core.Iterable<Message>? messages,
  }) {
    final result = create();
    if (messages != null) result.messages.addAll(messages);
    return result;
  }

  WorkingSet._();

  factory WorkingSet.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WorkingSet.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WorkingSet',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<Message>(1, _omitFieldNames ? '' : 'messages',
        subBuilder: Message.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WorkingSet clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WorkingSet copyWith(void Function(WorkingSet) updates) =>
      super.copyWith((message) => updates(message as WorkingSet)) as WorkingSet;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WorkingSet create() => WorkingSet._();
  @$core.override
  WorkingSet createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WorkingSet getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WorkingSet>(create);
  static WorkingSet? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<Message> get messages => $_getList(0);
}

class TokenBudget extends $pb.GeneratedMessage {
  factory TokenBudget({
    $core.int? maxContextTokens,
    $core.int? reserveOutput,
  }) {
    final result = create();
    if (maxContextTokens != null) result.maxContextTokens = maxContextTokens;
    if (reserveOutput != null) result.reserveOutput = reserveOutput;
    return result;
  }

  TokenBudget._();

  factory TokenBudget.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory TokenBudget.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'TokenBudget',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aI(1, _omitFieldNames ? '' : 'maxContextTokens',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(2, _omitFieldNames ? '' : 'reserveOutput',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TokenBudget clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TokenBudget copyWith(void Function(TokenBudget) updates) =>
      super.copyWith((message) => updates(message as TokenBudget))
          as TokenBudget;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static TokenBudget create() => TokenBudget._();
  @$core.override
  TokenBudget createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static TokenBudget getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<TokenBudget>(create);
  static TokenBudget? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get maxContextTokens => $_getIZ(0);
  @$pb.TagNumber(1)
  set maxContextTokens($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasMaxContextTokens() => $_has(0);
  @$pb.TagNumber(1)
  void clearMaxContextTokens() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get reserveOutput => $_getIZ(1);
  @$pb.TagNumber(2)
  set reserveOutput($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasReserveOutput() => $_has(1);
  @$pb.TagNumber(2)
  void clearReserveOutput() => $_clearField(2);
}

/// The policy authorization result. `agent-core::Decision::Allow` maps to
/// `allowed = true`; `Deny(reason)` maps to `allowed = false` + `deny_reason`.
class Decision extends $pb.GeneratedMessage {
  factory Decision({
    $core.bool? allowed,
    $core.String? denyReason,
  }) {
    final result = create();
    if (allowed != null) result.allowed = allowed;
    if (denyReason != null) result.denyReason = denyReason;
    return result;
  }

  Decision._();

  factory Decision.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory Decision.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Decision',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'allowed')
    ..aOS(2, _omitFieldNames ? '' : 'denyReason')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Decision clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Decision copyWith(void Function(Decision) updates) =>
      super.copyWith((message) => updates(message as Decision)) as Decision;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static Decision create() => Decision._();
  @$core.override
  Decision createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static Decision getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<Decision>(create);
  static Decision? _defaultInstance;

  @$pb.TagNumber(1)
  $core.bool get allowed => $_getBF(0);
  @$pb.TagNumber(1)
  set allowed($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasAllowed() => $_has(0);
  @$pb.TagNumber(1)
  void clearAllowed() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get denyReason => $_getSZ(1);
  @$pb.TagNumber(2)
  set denyReason($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasDenyReason() => $_has(1);
  @$pb.TagNumber(2)
  void clearDenyReason() => $_clearField(2);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
