// This is a generated file - do not edit.
//
// Generated from agent/v1/lsp.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

import 'lsp.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'lsp.pbenum.dart';

class LspPosition extends $pb.GeneratedMessage {
  factory LspPosition({
    $core.int? line,
    $core.int? character,
  }) {
    final result = create();
    if (line != null) result.line = line;
    if (character != null) result.character = character;
    return result;
  }

  LspPosition._();

  factory LspPosition.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspPosition.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspPosition',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aI(1, _omitFieldNames ? '' : 'line', fieldType: $pb.PbFieldType.OU3)
    ..aI(2, _omitFieldNames ? '' : 'character', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspPosition clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspPosition copyWith(void Function(LspPosition) updates) =>
      super.copyWith((message) => updates(message as LspPosition))
          as LspPosition;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspPosition create() => LspPosition._();
  @$core.override
  LspPosition createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspPosition getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspPosition>(create);
  static LspPosition? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get line => $_getIZ(0);
  @$pb.TagNumber(1)
  set line($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasLine() => $_has(0);
  @$pb.TagNumber(1)
  void clearLine() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get character => $_getIZ(1);
  @$pb.TagNumber(2)
  set character($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCharacter() => $_has(1);
  @$pb.TagNumber(2)
  void clearCharacter() => $_clearField(2);
}

/// Half-open.
class LspRange extends $pb.GeneratedMessage {
  factory LspRange({
    LspPosition? start,
    LspPosition? end,
  }) {
    final result = create();
    if (start != null) result.start = start;
    if (end != null) result.end = end;
    return result;
  }

  LspRange._();

  factory LspRange.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspRange.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspRange',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<LspPosition>(1, _omitFieldNames ? '' : 'start',
        subBuilder: LspPosition.create)
    ..aOM<LspPosition>(2, _omitFieldNames ? '' : 'end',
        subBuilder: LspPosition.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspRange clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspRange copyWith(void Function(LspRange) updates) =>
      super.copyWith((message) => updates(message as LspRange)) as LspRange;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspRange create() => LspRange._();
  @$core.override
  LspRange createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspRange getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<LspRange>(create);
  static LspRange? _defaultInstance;

  @$pb.TagNumber(1)
  LspPosition get start => $_getN(0);
  @$pb.TagNumber(1)
  set start(LspPosition value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasStart() => $_has(0);
  @$pb.TagNumber(1)
  void clearStart() => $_clearField(1);
  @$pb.TagNumber(1)
  LspPosition ensureStart() => $_ensure(0);

  @$pb.TagNumber(2)
  LspPosition get end => $_getN(1);
  @$pb.TagNumber(2)
  set end(LspPosition value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasEnd() => $_has(1);
  @$pb.TagNumber(2)
  void clearEnd() => $_clearField(2);
  @$pb.TagNumber(2)
  LspPosition ensureEnd() => $_ensure(1);
}

class LspLocation extends $pb.GeneratedMessage {
  factory LspLocation({
    $core.String? uri,
    LspRange? range,
  }) {
    final result = create();
    if (uri != null) result.uri = uri;
    if (range != null) result.range = range;
    return result;
  }

  LspLocation._();

  factory LspLocation.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspLocation.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspLocation',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'uri')
    ..aOM<LspRange>(2, _omitFieldNames ? '' : 'range',
        subBuilder: LspRange.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspLocation clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspLocation copyWith(void Function(LspLocation) updates) =>
      super.copyWith((message) => updates(message as LspLocation))
          as LspLocation;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspLocation create() => LspLocation._();
  @$core.override
  LspLocation createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspLocation getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspLocation>(create);
  static LspLocation? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get uri => $_getSZ(0);
  @$pb.TagNumber(1)
  set uri($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasUri() => $_has(0);
  @$pb.TagNumber(1)
  void clearUri() => $_clearField(1);

  @$pb.TagNumber(2)
  LspRange get range => $_getN(1);
  @$pb.TagNumber(2)
  set range(LspRange value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasRange() => $_has(1);
  @$pb.TagNumber(2)
  void clearRange() => $_clearField(2);
  @$pb.TagNumber(2)
  LspRange ensureRange() => $_ensure(1);
}

class LspDiagnostic extends $pb.GeneratedMessage {
  factory LspDiagnostic({
    LspRange? range,
    LspDiagnosticSeverity? severity,
    $core.String? message,
    $core.String? code,
    $core.String? source,
  }) {
    final result = create();
    if (range != null) result.range = range;
    if (severity != null) result.severity = severity;
    if (message != null) result.message = message;
    if (code != null) result.code = code;
    if (source != null) result.source = source;
    return result;
  }

  LspDiagnostic._();

  factory LspDiagnostic.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspDiagnostic.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspDiagnostic',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<LspRange>(1, _omitFieldNames ? '' : 'range',
        subBuilder: LspRange.create)
    ..aE<LspDiagnosticSeverity>(2, _omitFieldNames ? '' : 'severity',
        enumValues: LspDiagnosticSeverity.values)
    ..aOS(3, _omitFieldNames ? '' : 'message')
    ..aOS(4, _omitFieldNames ? '' : 'code')
    ..aOS(5, _omitFieldNames ? '' : 'source')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspDiagnostic clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspDiagnostic copyWith(void Function(LspDiagnostic) updates) =>
      super.copyWith((message) => updates(message as LspDiagnostic))
          as LspDiagnostic;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspDiagnostic create() => LspDiagnostic._();
  @$core.override
  LspDiagnostic createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspDiagnostic getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspDiagnostic>(create);
  static LspDiagnostic? _defaultInstance;

  @$pb.TagNumber(1)
  LspRange get range => $_getN(0);
  @$pb.TagNumber(1)
  set range(LspRange value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasRange() => $_has(0);
  @$pb.TagNumber(1)
  void clearRange() => $_clearField(1);
  @$pb.TagNumber(1)
  LspRange ensureRange() => $_ensure(0);

  @$pb.TagNumber(2)
  LspDiagnosticSeverity get severity => $_getN(1);
  @$pb.TagNumber(2)
  set severity(LspDiagnosticSeverity value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasSeverity() => $_has(1);
  @$pb.TagNumber(2)
  void clearSeverity() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get message => $_getSZ(2);
  @$pb.TagNumber(3)
  set message($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasMessage() => $_has(2);
  @$pb.TagNumber(3)
  void clearMessage() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get code => $_getSZ(3);
  @$pb.TagNumber(4)
  set code($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasCode() => $_has(3);
  @$pb.TagNumber(4)
  void clearCode() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get source => $_getSZ(4);
  @$pb.TagNumber(5)
  set source($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasSource() => $_has(4);
  @$pb.TagNumber(5)
  void clearSource() => $_clearField(5);
}

class LspHover extends $pb.GeneratedMessage {
  factory LspHover({
    $core.String? contents,
  }) {
    final result = create();
    if (contents != null) result.contents = contents;
    return result;
  }

  LspHover._();

  factory LspHover.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspHover.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspHover',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'contents')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspHover clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspHover copyWith(void Function(LspHover) updates) =>
      super.copyWith((message) => updates(message as LspHover)) as LspHover;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspHover create() => LspHover._();
  @$core.override
  LspHover createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspHover getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<LspHover>(create);
  static LspHover? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get contents => $_getSZ(0);
  @$pb.TagNumber(1)
  set contents($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasContents() => $_has(0);
  @$pb.TagNumber(1)
  void clearContents() => $_clearField(1);
}

class LspTextEdit extends $pb.GeneratedMessage {
  factory LspTextEdit({
    LspRange? range,
    $core.String? newText,
  }) {
    final result = create();
    if (range != null) result.range = range;
    if (newText != null) result.newText = newText;
    return result;
  }

  LspTextEdit._();

  factory LspTextEdit.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspTextEdit.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspTextEdit',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<LspRange>(1, _omitFieldNames ? '' : 'range',
        subBuilder: LspRange.create)
    ..aOS(2, _omitFieldNames ? '' : 'newText')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspTextEdit clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspTextEdit copyWith(void Function(LspTextEdit) updates) =>
      super.copyWith((message) => updates(message as LspTextEdit))
          as LspTextEdit;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspTextEdit create() => LspTextEdit._();
  @$core.override
  LspTextEdit createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspTextEdit getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspTextEdit>(create);
  static LspTextEdit? _defaultInstance;

  @$pb.TagNumber(1)
  LspRange get range => $_getN(0);
  @$pb.TagNumber(1)
  set range(LspRange value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasRange() => $_has(0);
  @$pb.TagNumber(1)
  void clearRange() => $_clearField(1);
  @$pb.TagNumber(1)
  LspRange ensureRange() => $_ensure(0);

  @$pb.TagNumber(2)
  $core.String get newText => $_getSZ(1);
  @$pb.TagNumber(2)
  set newText($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasNewText() => $_has(1);
  @$pb.TagNumber(2)
  void clearNewText() => $_clearField(2);
}

class LspFileEdits extends $pb.GeneratedMessage {
  factory LspFileEdits({
    $core.String? uri,
    $core.Iterable<LspTextEdit>? edits,
  }) {
    final result = create();
    if (uri != null) result.uri = uri;
    if (edits != null) result.edits.addAll(edits);
    return result;
  }

  LspFileEdits._();

  factory LspFileEdits.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspFileEdits.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspFileEdits',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'uri')
    ..pPM<LspTextEdit>(2, _omitFieldNames ? '' : 'edits',
        subBuilder: LspTextEdit.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspFileEdits clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspFileEdits copyWith(void Function(LspFileEdits) updates) =>
      super.copyWith((message) => updates(message as LspFileEdits))
          as LspFileEdits;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspFileEdits create() => LspFileEdits._();
  @$core.override
  LspFileEdits createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspFileEdits getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspFileEdits>(create);
  static LspFileEdits? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get uri => $_getSZ(0);
  @$pb.TagNumber(1)
  set uri($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasUri() => $_has(0);
  @$pb.TagNumber(1)
  void clearUri() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<LspTextEdit> get edits => $_getList(1);
}

/// Ordered, and a repeated field rather than a map: a rename's edits are applied
/// per file, and proto3 maps have no defined ordering.
class LspWorkspaceEdit extends $pb.GeneratedMessage {
  factory LspWorkspaceEdit({
    $core.Iterable<LspFileEdits>? changes,
  }) {
    final result = create();
    if (changes != null) result.changes.addAll(changes);
    return result;
  }

  LspWorkspaceEdit._();

  factory LspWorkspaceEdit.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspWorkspaceEdit.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspWorkspaceEdit',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<LspFileEdits>(1, _omitFieldNames ? '' : 'changes',
        subBuilder: LspFileEdits.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspWorkspaceEdit clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspWorkspaceEdit copyWith(void Function(LspWorkspaceEdit) updates) =>
      super.copyWith((message) => updates(message as LspWorkspaceEdit))
          as LspWorkspaceEdit;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspWorkspaceEdit create() => LspWorkspaceEdit._();
  @$core.override
  LspWorkspaceEdit createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspWorkspaceEdit getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspWorkspaceEdit>(create);
  static LspWorkspaceEdit? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<LspFileEdits> get changes => $_getList(0);
}

class LspDocumentSymbol extends $pb.GeneratedMessage {
  factory LspDocumentSymbol({
    $core.String? name,
    $core.String? kind,
    LspRange? range,
  }) {
    final result = create();
    if (name != null) result.name = name;
    if (kind != null) result.kind = kind;
    if (range != null) result.range = range;
    return result;
  }

  LspDocumentSymbol._();

  factory LspDocumentSymbol.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspDocumentSymbol.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspDocumentSymbol',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'name')
    ..aOS(2, _omitFieldNames ? '' : 'kind')
    ..aOM<LspRange>(3, _omitFieldNames ? '' : 'range',
        subBuilder: LspRange.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspDocumentSymbol clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspDocumentSymbol copyWith(void Function(LspDocumentSymbol) updates) =>
      super.copyWith((message) => updates(message as LspDocumentSymbol))
          as LspDocumentSymbol;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspDocumentSymbol create() => LspDocumentSymbol._();
  @$core.override
  LspDocumentSymbol createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspDocumentSymbol getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspDocumentSymbol>(create);
  static LspDocumentSymbol? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get name => $_getSZ(0);
  @$pb.TagNumber(1)
  set name($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasName() => $_has(0);
  @$pb.TagNumber(1)
  void clearName() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get kind => $_getSZ(1);
  @$pb.TagNumber(2)
  set kind($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasKind() => $_has(1);
  @$pb.TagNumber(2)
  void clearKind() => $_clearField(2);

  @$pb.TagNumber(3)
  LspRange get range => $_getN(2);
  @$pb.TagNumber(3)
  set range(LspRange value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasRange() => $_has(2);
  @$pb.TagNumber(3)
  void clearRange() => $_clearField(3);
  @$pb.TagNumber(3)
  LspRange ensureRange() => $_ensure(2);
}

class LspOpenRequest extends $pb.GeneratedMessage {
  factory LspOpenRequest({
    $core.String? uri,
    $core.String? text,
  }) {
    final result = create();
    if (uri != null) result.uri = uri;
    if (text != null) result.text = text;
    return result;
  }

  LspOpenRequest._();

  factory LspOpenRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspOpenRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspOpenRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'uri')
    ..aOS(2, _omitFieldNames ? '' : 'text')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspOpenRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspOpenRequest copyWith(void Function(LspOpenRequest) updates) =>
      super.copyWith((message) => updates(message as LspOpenRequest))
          as LspOpenRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspOpenRequest create() => LspOpenRequest._();
  @$core.override
  LspOpenRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspOpenRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspOpenRequest>(create);
  static LspOpenRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get uri => $_getSZ(0);
  @$pb.TagNumber(1)
  set uri($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasUri() => $_has(0);
  @$pb.TagNumber(1)
  void clearUri() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get text => $_getSZ(1);
  @$pb.TagNumber(2)
  set text($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasText() => $_has(1);
  @$pb.TagNumber(2)
  void clearText() => $_clearField(2);
}

class LspOpenResponse extends $pb.GeneratedMessage {
  factory LspOpenResponse() => create();

  LspOpenResponse._();

  factory LspOpenResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspOpenResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspOpenResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspOpenResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspOpenResponse copyWith(void Function(LspOpenResponse) updates) =>
      super.copyWith((message) => updates(message as LspOpenResponse))
          as LspOpenResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspOpenResponse create() => LspOpenResponse._();
  @$core.override
  LspOpenResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspOpenResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspOpenResponse>(create);
  static LspOpenResponse? _defaultInstance;
}

class LspRequestMsg extends $pb.GeneratedMessage {
  factory LspRequestMsg({
    LspMethod? method,
    $core.String? uri,
    LspPosition? position,
    $core.String? newName,
  }) {
    final result = create();
    if (method != null) result.method = method;
    if (uri != null) result.uri = uri;
    if (position != null) result.position = position;
    if (newName != null) result.newName = newName;
    return result;
  }

  LspRequestMsg._();

  factory LspRequestMsg.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspRequestMsg.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspRequestMsg',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aE<LspMethod>(1, _omitFieldNames ? '' : 'method',
        enumValues: LspMethod.values)
    ..aOS(2, _omitFieldNames ? '' : 'uri')
    ..aOM<LspPosition>(3, _omitFieldNames ? '' : 'position',
        subBuilder: LspPosition.create)
    ..aOS(4, _omitFieldNames ? '' : 'newName')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspRequestMsg clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspRequestMsg copyWith(void Function(LspRequestMsg) updates) =>
      super.copyWith((message) => updates(message as LspRequestMsg))
          as LspRequestMsg;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspRequestMsg create() => LspRequestMsg._();
  @$core.override
  LspRequestMsg createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspRequestMsg getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspRequestMsg>(create);
  static LspRequestMsg? _defaultInstance;

  @$pb.TagNumber(1)
  LspMethod get method => $_getN(0);
  @$pb.TagNumber(1)
  set method(LspMethod value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasMethod() => $_has(0);
  @$pb.TagNumber(1)
  void clearMethod() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get uri => $_getSZ(1);
  @$pb.TagNumber(2)
  set uri($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasUri() => $_has(1);
  @$pb.TagNumber(2)
  void clearUri() => $_clearField(2);

  /// Required by hover/definition/references/rename; absent otherwise.
  @$pb.TagNumber(3)
  LspPosition get position => $_getN(2);
  @$pb.TagNumber(3)
  set position(LspPosition value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasPosition() => $_has(2);
  @$pb.TagNumber(3)
  void clearPosition() => $_clearField(3);
  @$pb.TagNumber(3)
  LspPosition ensurePosition() => $_ensure(2);

  /// Required by rename.
  @$pb.TagNumber(4)
  $core.String get newName => $_getSZ(3);
  @$pb.TagNumber(4)
  set newName($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasNewName() => $_has(3);
  @$pb.TagNumber(4)
  void clearNewName() => $_clearField(4);
}

enum LspResultMsg_Kind {
  diagnostics,
  hover,
  locations,
  symbols,
  rename,
  notSet
}

/// The result is keyed by the method, so the variant is explicit rather than
/// inferred: a caller that asked for `hover` and received locations should see a
/// type error, not silently reinterpret them.
class LspResultMsg extends $pb.GeneratedMessage {
  factory LspResultMsg({
    LspDiagnostics? diagnostics,
    LspHoverResult? hover,
    LspLocations? locations,
    LspSymbols? symbols,
    LspWorkspaceEdit? rename,
  }) {
    final result = create();
    if (diagnostics != null) result.diagnostics = diagnostics;
    if (hover != null) result.hover = hover;
    if (locations != null) result.locations = locations;
    if (symbols != null) result.symbols = symbols;
    if (rename != null) result.rename = rename;
    return result;
  }

  LspResultMsg._();

  factory LspResultMsg.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspResultMsg.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static const $core.Map<$core.int, LspResultMsg_Kind> _LspResultMsg_KindByTag =
      {
    1: LspResultMsg_Kind.diagnostics,
    2: LspResultMsg_Kind.hover,
    3: LspResultMsg_Kind.locations,
    4: LspResultMsg_Kind.symbols,
    5: LspResultMsg_Kind.rename,
    0: LspResultMsg_Kind.notSet
  };
  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspResultMsg',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..oo(0, [1, 2, 3, 4, 5])
    ..aOM<LspDiagnostics>(1, _omitFieldNames ? '' : 'diagnostics',
        subBuilder: LspDiagnostics.create)
    ..aOM<LspHoverResult>(2, _omitFieldNames ? '' : 'hover',
        subBuilder: LspHoverResult.create)
    ..aOM<LspLocations>(3, _omitFieldNames ? '' : 'locations',
        subBuilder: LspLocations.create)
    ..aOM<LspSymbols>(4, _omitFieldNames ? '' : 'symbols',
        subBuilder: LspSymbols.create)
    ..aOM<LspWorkspaceEdit>(5, _omitFieldNames ? '' : 'rename',
        subBuilder: LspWorkspaceEdit.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspResultMsg clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspResultMsg copyWith(void Function(LspResultMsg) updates) =>
      super.copyWith((message) => updates(message as LspResultMsg))
          as LspResultMsg;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspResultMsg create() => LspResultMsg._();
  @$core.override
  LspResultMsg createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspResultMsg getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspResultMsg>(create);
  static LspResultMsg? _defaultInstance;

  @$pb.TagNumber(1)
  @$pb.TagNumber(2)
  @$pb.TagNumber(3)
  @$pb.TagNumber(4)
  @$pb.TagNumber(5)
  LspResultMsg_Kind whichKind() => _LspResultMsg_KindByTag[$_whichOneof(0)]!;
  @$pb.TagNumber(1)
  @$pb.TagNumber(2)
  @$pb.TagNumber(3)
  @$pb.TagNumber(4)
  @$pb.TagNumber(5)
  void clearKind() => $_clearField($_whichOneof(0));

  @$pb.TagNumber(1)
  LspDiagnostics get diagnostics => $_getN(0);
  @$pb.TagNumber(1)
  set diagnostics(LspDiagnostics value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasDiagnostics() => $_has(0);
  @$pb.TagNumber(1)
  void clearDiagnostics() => $_clearField(1);
  @$pb.TagNumber(1)
  LspDiagnostics ensureDiagnostics() => $_ensure(0);

  @$pb.TagNumber(2)
  LspHoverResult get hover => $_getN(1);
  @$pb.TagNumber(2)
  set hover(LspHoverResult value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasHover() => $_has(1);
  @$pb.TagNumber(2)
  void clearHover() => $_clearField(2);
  @$pb.TagNumber(2)
  LspHoverResult ensureHover() => $_ensure(1);

  @$pb.TagNumber(3)
  LspLocations get locations => $_getN(2);
  @$pb.TagNumber(3)
  set locations(LspLocations value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasLocations() => $_has(2);
  @$pb.TagNumber(3)
  void clearLocations() => $_clearField(3);
  @$pb.TagNumber(3)
  LspLocations ensureLocations() => $_ensure(2);

  @$pb.TagNumber(4)
  LspSymbols get symbols => $_getN(3);
  @$pb.TagNumber(4)
  set symbols(LspSymbols value) => $_setField(4, value);
  @$pb.TagNumber(4)
  $core.bool hasSymbols() => $_has(3);
  @$pb.TagNumber(4)
  void clearSymbols() => $_clearField(4);
  @$pb.TagNumber(4)
  LspSymbols ensureSymbols() => $_ensure(3);

  @$pb.TagNumber(5)
  LspWorkspaceEdit get rename => $_getN(4);
  @$pb.TagNumber(5)
  set rename(LspWorkspaceEdit value) => $_setField(5, value);
  @$pb.TagNumber(5)
  $core.bool hasRename() => $_has(4);
  @$pb.TagNumber(5)
  void clearRename() => $_clearField(5);
  @$pb.TagNumber(5)
  LspWorkspaceEdit ensureRename() => $_ensure(4);
}

class LspDiagnostics extends $pb.GeneratedMessage {
  factory LspDiagnostics({
    $core.Iterable<LspDiagnostic>? items,
  }) {
    final result = create();
    if (items != null) result.items.addAll(items);
    return result;
  }

  LspDiagnostics._();

  factory LspDiagnostics.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspDiagnostics.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspDiagnostics',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<LspDiagnostic>(1, _omitFieldNames ? '' : 'items',
        subBuilder: LspDiagnostic.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspDiagnostics clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspDiagnostics copyWith(void Function(LspDiagnostics) updates) =>
      super.copyWith((message) => updates(message as LspDiagnostics))
          as LspDiagnostics;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspDiagnostics create() => LspDiagnostics._();
  @$core.override
  LspDiagnostics createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspDiagnostics getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspDiagnostics>(create);
  static LspDiagnostics? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<LspDiagnostic> get items => $_getList(0);
}

/// `hover` is absent when the server had nothing to say — distinct from a hover
/// whose contents are empty.
class LspHoverResult extends $pb.GeneratedMessage {
  factory LspHoverResult({
    LspHover? hover,
  }) {
    final result = create();
    if (hover != null) result.hover = hover;
    return result;
  }

  LspHoverResult._();

  factory LspHoverResult.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspHoverResult.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspHoverResult',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<LspHover>(1, _omitFieldNames ? '' : 'hover',
        subBuilder: LspHover.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspHoverResult clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspHoverResult copyWith(void Function(LspHoverResult) updates) =>
      super.copyWith((message) => updates(message as LspHoverResult))
          as LspHoverResult;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspHoverResult create() => LspHoverResult._();
  @$core.override
  LspHoverResult createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspHoverResult getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspHoverResult>(create);
  static LspHoverResult? _defaultInstance;

  @$pb.TagNumber(1)
  LspHover get hover => $_getN(0);
  @$pb.TagNumber(1)
  set hover(LspHover value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasHover() => $_has(0);
  @$pb.TagNumber(1)
  void clearHover() => $_clearField(1);
  @$pb.TagNumber(1)
  LspHover ensureHover() => $_ensure(0);
}

class LspLocations extends $pb.GeneratedMessage {
  factory LspLocations({
    $core.Iterable<LspLocation>? items,
  }) {
    final result = create();
    if (items != null) result.items.addAll(items);
    return result;
  }

  LspLocations._();

  factory LspLocations.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspLocations.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspLocations',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<LspLocation>(1, _omitFieldNames ? '' : 'items',
        subBuilder: LspLocation.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspLocations clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspLocations copyWith(void Function(LspLocations) updates) =>
      super.copyWith((message) => updates(message as LspLocations))
          as LspLocations;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspLocations create() => LspLocations._();
  @$core.override
  LspLocations createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspLocations getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspLocations>(create);
  static LspLocations? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<LspLocation> get items => $_getList(0);
}

class LspSymbols extends $pb.GeneratedMessage {
  factory LspSymbols({
    $core.Iterable<LspDocumentSymbol>? items,
  }) {
    final result = create();
    if (items != null) result.items.addAll(items);
    return result;
  }

  LspSymbols._();

  factory LspSymbols.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspSymbols.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspSymbols',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<LspDocumentSymbol>(1, _omitFieldNames ? '' : 'items',
        subBuilder: LspDocumentSymbol.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspSymbols clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspSymbols copyWith(void Function(LspSymbols) updates) =>
      super.copyWith((message) => updates(message as LspSymbols)) as LspSymbols;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspSymbols create() => LspSymbols._();
  @$core.override
  LspSymbols createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspSymbols getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspSymbols>(create);
  static LspSymbols? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<LspDocumentSymbol> get items => $_getList(0);
}

class LspCapabilitiesRequest extends $pb.GeneratedMessage {
  factory LspCapabilitiesRequest({
    $core.String? language,
  }) {
    final result = create();
    if (language != null) result.language = language;
    return result;
  }

  LspCapabilitiesRequest._();

  factory LspCapabilitiesRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspCapabilitiesRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspCapabilitiesRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'language')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspCapabilitiesRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspCapabilitiesRequest copyWith(
          void Function(LspCapabilitiesRequest) updates) =>
      super.copyWith((message) => updates(message as LspCapabilitiesRequest))
          as LspCapabilitiesRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspCapabilitiesRequest create() => LspCapabilitiesRequest._();
  @$core.override
  LspCapabilitiesRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspCapabilitiesRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspCapabilitiesRequest>(create);
  static LspCapabilitiesRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get language => $_getSZ(0);
  @$pb.TagNumber(1)
  set language($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasLanguage() => $_has(0);
  @$pb.TagNumber(1)
  void clearLanguage() => $_clearField(1);
}

class LspCapabilities extends $pb.GeneratedMessage {
  factory LspCapabilities({
    $core.String? server,
    $core.Iterable<LspMethod>? methods,
  }) {
    final result = create();
    if (server != null) result.server = server;
    if (methods != null) result.methods.addAll(methods);
    return result;
  }

  LspCapabilities._();

  factory LspCapabilities.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspCapabilities.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspCapabilities',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'server')
    ..pc<LspMethod>(2, _omitFieldNames ? '' : 'methods', $pb.PbFieldType.KE,
        valueOf: LspMethod.valueOf,
        enumValues: LspMethod.values,
        defaultEnumValue: LspMethod.LSP_METHOD_DIAGNOSTICS)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspCapabilities clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspCapabilities copyWith(void Function(LspCapabilities) updates) =>
      super.copyWith((message) => updates(message as LspCapabilities))
          as LspCapabilities;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspCapabilities create() => LspCapabilities._();
  @$core.override
  LspCapabilities createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspCapabilities getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspCapabilities>(create);
  static LspCapabilities? _defaultInstance;

  /// Empty ⇒ no server configured for that language.
  @$pb.TagNumber(1)
  $core.String get server => $_getSZ(0);
  @$pb.TagNumber(1)
  set server($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasServer() => $_has(0);
  @$pb.TagNumber(1)
  void clearServer() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<LspMethod> get methods => $_getList(1);
}

class LspShutdownRequest extends $pb.GeneratedMessage {
  factory LspShutdownRequest() => create();

  LspShutdownRequest._();

  factory LspShutdownRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspShutdownRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspShutdownRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspShutdownRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspShutdownRequest copyWith(void Function(LspShutdownRequest) updates) =>
      super.copyWith((message) => updates(message as LspShutdownRequest))
          as LspShutdownRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspShutdownRequest create() => LspShutdownRequest._();
  @$core.override
  LspShutdownRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspShutdownRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspShutdownRequest>(create);
  static LspShutdownRequest? _defaultInstance;
}

class LspShutdownResponse extends $pb.GeneratedMessage {
  factory LspShutdownResponse() => create();

  LspShutdownResponse._();

  factory LspShutdownResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LspShutdownResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LspShutdownResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspShutdownResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LspShutdownResponse copyWith(void Function(LspShutdownResponse) updates) =>
      super.copyWith((message) => updates(message as LspShutdownResponse))
          as LspShutdownResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LspShutdownResponse create() => LspShutdownResponse._();
  @$core.override
  LspShutdownResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LspShutdownResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LspShutdownResponse>(create);
  static LspShutdownResponse? _defaultInstance;
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
