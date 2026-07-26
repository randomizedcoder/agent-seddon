// This is a generated file - do not edit.
//
// Generated from agent/v1/repo.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

import 'repo.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'repo.pbenum.dart';

class TreeEntry extends $pb.GeneratedMessage {
  factory TreeEntry({
    $core.String? path,
    $core.String? oid,
    EntryKind? kind,
    $core.int? mode,
    $fixnum.Int64? size,
  }) {
    final result = create();
    if (path != null) result.path = path;
    if (oid != null) result.oid = oid;
    if (kind != null) result.kind = kind;
    if (mode != null) result.mode = mode;
    if (size != null) result.size = size;
    return result;
  }

  TreeEntry._();

  factory TreeEntry.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory TreeEntry.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'TreeEntry',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'path')
    ..aOS(2, _omitFieldNames ? '' : 'oid')
    ..aE<EntryKind>(3, _omitFieldNames ? '' : 'kind',
        enumValues: EntryKind.values)
    ..aI(4, _omitFieldNames ? '' : 'mode', fieldType: $pb.PbFieldType.OU3)
    ..a<$fixnum.Int64>(5, _omitFieldNames ? '' : 'size', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TreeEntry clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TreeEntry copyWith(void Function(TreeEntry) updates) =>
      super.copyWith((message) => updates(message as TreeEntry)) as TreeEntry;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static TreeEntry create() => TreeEntry._();
  @$core.override
  TreeEntry createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static TreeEntry getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<TreeEntry>(create);
  static TreeEntry? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get path => $_getSZ(0);
  @$pb.TagNumber(1)
  set path($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPath() => $_has(0);
  @$pb.TagNumber(1)
  void clearPath() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get oid => $_getSZ(1);
  @$pb.TagNumber(2)
  set oid($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasOid() => $_has(1);
  @$pb.TagNumber(2)
  void clearOid() => $_clearField(2);

  @$pb.TagNumber(3)
  EntryKind get kind => $_getN(2);
  @$pb.TagNumber(3)
  set kind(EntryKind value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasKind() => $_has(2);
  @$pb.TagNumber(3)
  void clearKind() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get mode => $_getIZ(3);
  @$pb.TagNumber(4)
  set mode($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasMode() => $_has(3);
  @$pb.TagNumber(4)
  void clearMode() => $_clearField(4);

  @$pb.TagNumber(5)
  $fixnum.Int64 get size => $_getI64(4);
  @$pb.TagNumber(5)
  set size($fixnum.Int64 value) => $_setInt64(4, value);
  @$pb.TagNumber(5)
  $core.bool hasSize() => $_has(4);
  @$pb.TagNumber(5)
  void clearSize() => $_clearField(5);
}

class BlobContent extends $pb.GeneratedMessage {
  factory BlobContent({
    $core.String? oid,
    $core.String? path,
    $fixnum.Int64? bytesLen,
    $core.bool? isBinary,
    $core.String? text,
  }) {
    final result = create();
    if (oid != null) result.oid = oid;
    if (path != null) result.path = path;
    if (bytesLen != null) result.bytesLen = bytesLen;
    if (isBinary != null) result.isBinary = isBinary;
    if (text != null) result.text = text;
    return result;
  }

  BlobContent._();

  factory BlobContent.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory BlobContent.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'BlobContent',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'oid')
    ..aOS(2, _omitFieldNames ? '' : 'path')
    ..a<$fixnum.Int64>(
        3, _omitFieldNames ? '' : 'bytesLen', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOB(4, _omitFieldNames ? '' : 'isBinary')
    ..aOS(5, _omitFieldNames ? '' : 'text')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BlobContent clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BlobContent copyWith(void Function(BlobContent) updates) =>
      super.copyWith((message) => updates(message as BlobContent))
          as BlobContent;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static BlobContent create() => BlobContent._();
  @$core.override
  BlobContent createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static BlobContent getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<BlobContent>(create);
  static BlobContent? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get oid => $_getSZ(0);
  @$pb.TagNumber(1)
  set oid($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasOid() => $_has(0);
  @$pb.TagNumber(1)
  void clearOid() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get path => $_getSZ(1);
  @$pb.TagNumber(2)
  set path($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasPath() => $_has(1);
  @$pb.TagNumber(2)
  void clearPath() => $_clearField(2);

  @$pb.TagNumber(3)
  $fixnum.Int64 get bytesLen => $_getI64(2);
  @$pb.TagNumber(3)
  set bytesLen($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasBytesLen() => $_has(2);
  @$pb.TagNumber(3)
  void clearBytesLen() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.bool get isBinary => $_getBF(3);
  @$pb.TagNumber(4)
  set isBinary($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasIsBinary() => $_has(3);
  @$pb.TagNumber(4)
  void clearIsBinary() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get text => $_getSZ(4);
  @$pb.TagNumber(5)
  set text($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasText() => $_has(4);
  @$pb.TagNumber(5)
  void clearText() => $_clearField(5);
}

class FileDiff extends $pb.GeneratedMessage {
  factory FileDiff({
    ChangeKind? change,
    $core.String? oldPath,
    $core.String? newPath,
    $core.String? oldOid,
    $core.String? newOid,
    $core.int? additions,
    $core.int? deletions,
    $core.String? patch,
  }) {
    final result = create();
    if (change != null) result.change = change;
    if (oldPath != null) result.oldPath = oldPath;
    if (newPath != null) result.newPath = newPath;
    if (oldOid != null) result.oldOid = oldOid;
    if (newOid != null) result.newOid = newOid;
    if (additions != null) result.additions = additions;
    if (deletions != null) result.deletions = deletions;
    if (patch != null) result.patch = patch;
    return result;
  }

  FileDiff._();

  factory FileDiff.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory FileDiff.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'FileDiff',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aE<ChangeKind>(1, _omitFieldNames ? '' : 'change',
        enumValues: ChangeKind.values)
    ..aOS(2, _omitFieldNames ? '' : 'oldPath')
    ..aOS(3, _omitFieldNames ? '' : 'newPath')
    ..aOS(4, _omitFieldNames ? '' : 'oldOid')
    ..aOS(5, _omitFieldNames ? '' : 'newOid')
    ..aI(6, _omitFieldNames ? '' : 'additions', fieldType: $pb.PbFieldType.OU3)
    ..aI(7, _omitFieldNames ? '' : 'deletions', fieldType: $pb.PbFieldType.OU3)
    ..aOS(8, _omitFieldNames ? '' : 'patch')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FileDiff clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FileDiff copyWith(void Function(FileDiff) updates) =>
      super.copyWith((message) => updates(message as FileDiff)) as FileDiff;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static FileDiff create() => FileDiff._();
  @$core.override
  FileDiff createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static FileDiff getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<FileDiff>(create);
  static FileDiff? _defaultInstance;

  @$pb.TagNumber(1)
  ChangeKind get change => $_getN(0);
  @$pb.TagNumber(1)
  set change(ChangeKind value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasChange() => $_has(0);
  @$pb.TagNumber(1)
  void clearChange() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get oldPath => $_getSZ(1);
  @$pb.TagNumber(2)
  set oldPath($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasOldPath() => $_has(1);
  @$pb.TagNumber(2)
  void clearOldPath() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get newPath => $_getSZ(2);
  @$pb.TagNumber(3)
  set newPath($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasNewPath() => $_has(2);
  @$pb.TagNumber(3)
  void clearNewPath() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get oldOid => $_getSZ(3);
  @$pb.TagNumber(4)
  set oldOid($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasOldOid() => $_has(3);
  @$pb.TagNumber(4)
  void clearOldOid() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get newOid => $_getSZ(4);
  @$pb.TagNumber(5)
  set newOid($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasNewOid() => $_has(4);
  @$pb.TagNumber(5)
  void clearNewOid() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.int get additions => $_getIZ(5);
  @$pb.TagNumber(6)
  set additions($core.int value) => $_setUnsignedInt32(5, value);
  @$pb.TagNumber(6)
  $core.bool hasAdditions() => $_has(5);
  @$pb.TagNumber(6)
  void clearAdditions() => $_clearField(6);

  @$pb.TagNumber(7)
  $core.int get deletions => $_getIZ(6);
  @$pb.TagNumber(7)
  set deletions($core.int value) => $_setUnsignedInt32(6, value);
  @$pb.TagNumber(7)
  $core.bool hasDeletions() => $_has(6);
  @$pb.TagNumber(7)
  void clearDeletions() => $_clearField(7);

  @$pb.TagNumber(8)
  $core.String get patch => $_getSZ(7);
  @$pb.TagNumber(8)
  set patch($core.String value) => $_setString(7, value);
  @$pb.TagNumber(8)
  $core.bool hasPatch() => $_has(7);
  @$pb.TagNumber(8)
  void clearPatch() => $_clearField(8);
}

class DiffResult extends $pb.GeneratedMessage {
  factory DiffResult({
    $core.String? base,
    $core.String? target,
    $core.Iterable<FileDiff>? files,
  }) {
    final result = create();
    if (base != null) result.base = base;
    if (target != null) result.target = target;
    if (files != null) result.files.addAll(files);
    return result;
  }

  DiffResult._();

  factory DiffResult.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DiffResult.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DiffResult',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'base')
    ..aOS(2, _omitFieldNames ? '' : 'target')
    ..pPM<FileDiff>(3, _omitFieldNames ? '' : 'files',
        subBuilder: FileDiff.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DiffResult clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DiffResult copyWith(void Function(DiffResult) updates) =>
      super.copyWith((message) => updates(message as DiffResult)) as DiffResult;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DiffResult create() => DiffResult._();
  @$core.override
  DiffResult createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DiffResult getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DiffResult>(create);
  static DiffResult? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get base => $_getSZ(0);
  @$pb.TagNumber(1)
  set base($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasBase() => $_has(0);
  @$pb.TagNumber(1)
  void clearBase() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get target => $_getSZ(1);
  @$pb.TagNumber(2)
  set target($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasTarget() => $_has(1);
  @$pb.TagNumber(2)
  void clearTarget() => $_clearField(2);

  @$pb.TagNumber(3)
  $pb.PbList<FileDiff> get files => $_getList(2);
}

class CommitInfo extends $pb.GeneratedMessage {
  factory CommitInfo({
    $core.String? oid,
    $core.Iterable<$core.String>? parents,
    $core.String? author,
    $core.String? authorEmail,
    $fixnum.Int64? committedMs,
    $core.String? summary,
    $core.String? body,
  }) {
    final result = create();
    if (oid != null) result.oid = oid;
    if (parents != null) result.parents.addAll(parents);
    if (author != null) result.author = author;
    if (authorEmail != null) result.authorEmail = authorEmail;
    if (committedMs != null) result.committedMs = committedMs;
    if (summary != null) result.summary = summary;
    if (body != null) result.body = body;
    return result;
  }

  CommitInfo._();

  factory CommitInfo.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CommitInfo.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CommitInfo',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'oid')
    ..pPS(2, _omitFieldNames ? '' : 'parents')
    ..aOS(3, _omitFieldNames ? '' : 'author')
    ..aOS(4, _omitFieldNames ? '' : 'authorEmail')
    ..a<$fixnum.Int64>(
        5, _omitFieldNames ? '' : 'committedMs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOS(6, _omitFieldNames ? '' : 'summary')
    ..aOS(7, _omitFieldNames ? '' : 'body')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CommitInfo clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CommitInfo copyWith(void Function(CommitInfo) updates) =>
      super.copyWith((message) => updates(message as CommitInfo)) as CommitInfo;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CommitInfo create() => CommitInfo._();
  @$core.override
  CommitInfo createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CommitInfo getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<CommitInfo>(create);
  static CommitInfo? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get oid => $_getSZ(0);
  @$pb.TagNumber(1)
  set oid($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasOid() => $_has(0);
  @$pb.TagNumber(1)
  void clearOid() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<$core.String> get parents => $_getList(1);

  @$pb.TagNumber(3)
  $core.String get author => $_getSZ(2);
  @$pb.TagNumber(3)
  set author($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasAuthor() => $_has(2);
  @$pb.TagNumber(3)
  void clearAuthor() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get authorEmail => $_getSZ(3);
  @$pb.TagNumber(4)
  set authorEmail($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasAuthorEmail() => $_has(3);
  @$pb.TagNumber(4)
  void clearAuthorEmail() => $_clearField(4);

  @$pb.TagNumber(5)
  $fixnum.Int64 get committedMs => $_getI64(4);
  @$pb.TagNumber(5)
  set committedMs($fixnum.Int64 value) => $_setInt64(4, value);
  @$pb.TagNumber(5)
  $core.bool hasCommittedMs() => $_has(4);
  @$pb.TagNumber(5)
  void clearCommittedMs() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.String get summary => $_getSZ(5);
  @$pb.TagNumber(6)
  set summary($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasSummary() => $_has(5);
  @$pb.TagNumber(6)
  void clearSummary() => $_clearField(6);

  @$pb.TagNumber(7)
  $core.String get body => $_getSZ(6);
  @$pb.TagNumber(7)
  set body($core.String value) => $_setString(6, value);
  @$pb.TagNumber(7)
  $core.bool hasBody() => $_has(6);
  @$pb.TagNumber(7)
  void clearBody() => $_clearField(7);
}

class GrepHit extends $pb.GeneratedMessage {
  factory GrepHit({
    $core.String? path,
    $core.int? line,
    $core.String? text,
  }) {
    final result = create();
    if (path != null) result.path = path;
    if (line != null) result.line = line;
    if (text != null) result.text = text;
    return result;
  }

  GrepHit._();

  factory GrepHit.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GrepHit.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GrepHit',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'path')
    ..aI(2, _omitFieldNames ? '' : 'line', fieldType: $pb.PbFieldType.OU3)
    ..aOS(3, _omitFieldNames ? '' : 'text')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GrepHit clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GrepHit copyWith(void Function(GrepHit) updates) =>
      super.copyWith((message) => updates(message as GrepHit)) as GrepHit;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GrepHit create() => GrepHit._();
  @$core.override
  GrepHit createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GrepHit getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<GrepHit>(create);
  static GrepHit? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get path => $_getSZ(0);
  @$pb.TagNumber(1)
  set path($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPath() => $_has(0);
  @$pb.TagNumber(1)
  void clearPath() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get line => $_getIZ(1);
  @$pb.TagNumber(2)
  set line($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasLine() => $_has(1);
  @$pb.TagNumber(2)
  void clearLine() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get text => $_getSZ(2);
  @$pb.TagNumber(3)
  set text($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasText() => $_has(2);
  @$pb.TagNumber(3)
  void clearText() => $_clearField(3);
}

class WorktreeHandle extends $pb.GeneratedMessage {
  factory WorktreeHandle({
    $core.String? id,
    $core.String? path,
    $core.String? head,
    $core.String? revision,
    $core.bool? writable,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (path != null) result.path = path;
    if (head != null) result.head = head;
    if (revision != null) result.revision = revision;
    if (writable != null) result.writable = writable;
    return result;
  }

  WorktreeHandle._();

  factory WorktreeHandle.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WorktreeHandle.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WorktreeHandle',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aOS(2, _omitFieldNames ? '' : 'path')
    ..aOS(3, _omitFieldNames ? '' : 'head')
    ..aOS(4, _omitFieldNames ? '' : 'revision')
    ..aOB(5, _omitFieldNames ? '' : 'writable')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WorktreeHandle clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WorktreeHandle copyWith(void Function(WorktreeHandle) updates) =>
      super.copyWith((message) => updates(message as WorktreeHandle))
          as WorktreeHandle;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WorktreeHandle create() => WorktreeHandle._();
  @$core.override
  WorktreeHandle createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WorktreeHandle getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WorktreeHandle>(create);
  static WorktreeHandle? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get path => $_getSZ(1);
  @$pb.TagNumber(2)
  set path($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasPath() => $_has(1);
  @$pb.TagNumber(2)
  void clearPath() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get head => $_getSZ(2);
  @$pb.TagNumber(3)
  set head($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasHead() => $_has(2);
  @$pb.TagNumber(3)
  void clearHead() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get revision => $_getSZ(3);
  @$pb.TagNumber(4)
  set revision($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasRevision() => $_has(3);
  @$pb.TagNumber(4)
  void clearRevision() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.bool get writable => $_getBF(4);
  @$pb.TagNumber(5)
  set writable($core.bool value) => $_setBool(4, value);
  @$pb.TagNumber(5)
  $core.bool hasWritable() => $_has(4);
  @$pb.TagNumber(5)
  void clearWritable() => $_clearField(5);
}

class WorktreeSpec extends $pb.GeneratedMessage {
  factory WorktreeSpec({
    $core.String? revision,
    $core.bool? writable,
    $core.String? id,
  }) {
    final result = create();
    if (revision != null) result.revision = revision;
    if (writable != null) result.writable = writable;
    if (id != null) result.id = id;
    return result;
  }

  WorktreeSpec._();

  factory WorktreeSpec.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WorktreeSpec.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WorktreeSpec',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'revision')
    ..aOB(2, _omitFieldNames ? '' : 'writable')
    ..aOS(3, _omitFieldNames ? '' : 'id')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WorktreeSpec clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WorktreeSpec copyWith(void Function(WorktreeSpec) updates) =>
      super.copyWith((message) => updates(message as WorktreeSpec))
          as WorktreeSpec;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WorktreeSpec create() => WorktreeSpec._();
  @$core.override
  WorktreeSpec createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WorktreeSpec getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WorktreeSpec>(create);
  static WorktreeSpec? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get revision => $_getSZ(0);
  @$pb.TagNumber(1)
  set revision($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRevision() => $_has(0);
  @$pb.TagNumber(1)
  void clearRevision() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.bool get writable => $_getBF(1);
  @$pb.TagNumber(2)
  set writable($core.bool value) => $_setBool(1, value);
  @$pb.TagNumber(2)
  $core.bool hasWritable() => $_has(1);
  @$pb.TagNumber(2)
  void clearWritable() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get id => $_getSZ(2);
  @$pb.TagNumber(3)
  set id($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasId() => $_has(2);
  @$pb.TagNumber(3)
  void clearId() => $_clearField(3);
}

class Checkpoint extends $pb.GeneratedMessage {
  factory Checkpoint({
    $core.String? name,
    $core.String? oid,
    $core.String? refName,
  }) {
    final result = create();
    if (name != null) result.name = name;
    if (oid != null) result.oid = oid;
    if (refName != null) result.refName = refName;
    return result;
  }

  Checkpoint._();

  factory Checkpoint.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory Checkpoint.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Checkpoint',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'name')
    ..aOS(2, _omitFieldNames ? '' : 'oid')
    ..aOS(3, _omitFieldNames ? '' : 'refName')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Checkpoint clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Checkpoint copyWith(void Function(Checkpoint) updates) =>
      super.copyWith((message) => updates(message as Checkpoint)) as Checkpoint;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static Checkpoint create() => Checkpoint._();
  @$core.override
  Checkpoint createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static Checkpoint getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<Checkpoint>(create);
  static Checkpoint? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get name => $_getSZ(0);
  @$pb.TagNumber(1)
  set name($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasName() => $_has(0);
  @$pb.TagNumber(1)
  void clearName() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get oid => $_getSZ(1);
  @$pb.TagNumber(2)
  set oid($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasOid() => $_has(1);
  @$pb.TagNumber(2)
  void clearOid() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get refName => $_getSZ(2);
  @$pb.TagNumber(3)
  set refName($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasRefName() => $_has(2);
  @$pb.TagNumber(3)
  void clearRefName() => $_clearField(3);
}

/// A branch (or remote-tracking branch) and its resolved head oid.
class Branch extends $pb.GeneratedMessage {
  factory Branch({
    $core.String? name,
    $core.String? oid,
  }) {
    final result = create();
    if (name != null) result.name = name;
    if (oid != null) result.oid = oid;
    return result;
  }

  Branch._();

  factory Branch.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory Branch.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Branch',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'name')
    ..aOS(2, _omitFieldNames ? '' : 'oid')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Branch clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Branch copyWith(void Function(Branch) updates) =>
      super.copyWith((message) => updates(message as Branch)) as Branch;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static Branch create() => Branch._();
  @$core.override
  Branch createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static Branch getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<Branch>(create);
  static Branch? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get name => $_getSZ(0);
  @$pb.TagNumber(1)
  set name($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasName() => $_has(0);
  @$pb.TagNumber(1)
  void clearName() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get oid => $_getSZ(1);
  @$pb.TagNumber(2)
  set oid($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasOid() => $_has(1);
  @$pb.TagNumber(2)
  void clearOid() => $_clearField(2);
}

class RepoStatus extends $pb.GeneratedMessage {
  factory RepoStatus({
    $core.String? mirrorPath,
    $fixnum.Int64? lastFetchMs,
    $core.int? liveWorktrees,
    $core.Iterable<Branch>? heads,
  }) {
    final result = create();
    if (mirrorPath != null) result.mirrorPath = mirrorPath;
    if (lastFetchMs != null) result.lastFetchMs = lastFetchMs;
    if (liveWorktrees != null) result.liveWorktrees = liveWorktrees;
    if (heads != null) result.heads.addAll(heads);
    return result;
  }

  RepoStatus._();

  factory RepoStatus.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RepoStatus.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RepoStatus',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'mirrorPath')
    ..a<$fixnum.Int64>(
        2, _omitFieldNames ? '' : 'lastFetchMs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aI(3, _omitFieldNames ? '' : 'liveWorktrees',
        fieldType: $pb.PbFieldType.OU3)
    ..pPM<Branch>(4, _omitFieldNames ? '' : 'heads', subBuilder: Branch.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RepoStatus clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RepoStatus copyWith(void Function(RepoStatus) updates) =>
      super.copyWith((message) => updates(message as RepoStatus)) as RepoStatus;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RepoStatus create() => RepoStatus._();
  @$core.override
  RepoStatus createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RepoStatus getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RepoStatus>(create);
  static RepoStatus? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get mirrorPath => $_getSZ(0);
  @$pb.TagNumber(1)
  set mirrorPath($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasMirrorPath() => $_has(0);
  @$pb.TagNumber(1)
  void clearMirrorPath() => $_clearField(1);

  @$pb.TagNumber(2)
  $fixnum.Int64 get lastFetchMs => $_getI64(1);
  @$pb.TagNumber(2)
  set lastFetchMs($fixnum.Int64 value) => $_setInt64(1, value);
  @$pb.TagNumber(2)
  $core.bool hasLastFetchMs() => $_has(1);
  @$pb.TagNumber(2)
  void clearLastFetchMs() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get liveWorktrees => $_getIZ(2);
  @$pb.TagNumber(3)
  set liveWorktrees($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasLiveWorktrees() => $_has(2);
  @$pb.TagNumber(3)
  void clearLiveWorktrees() => $_clearField(3);

  @$pb.TagNumber(4)
  $pb.PbList<Branch> get heads => $_getList(3);
}

class ResolveRequest extends $pb.GeneratedMessage {
  factory ResolveRequest({
    $core.String? revision,
  }) {
    final result = create();
    if (revision != null) result.revision = revision;
    return result;
  }

  ResolveRequest._();

  factory ResolveRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ResolveRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ResolveRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'revision')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ResolveRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ResolveRequest copyWith(void Function(ResolveRequest) updates) =>
      super.copyWith((message) => updates(message as ResolveRequest))
          as ResolveRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ResolveRequest create() => ResolveRequest._();
  @$core.override
  ResolveRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ResolveRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ResolveRequest>(create);
  static ResolveRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get revision => $_getSZ(0);
  @$pb.TagNumber(1)
  set revision($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRevision() => $_has(0);
  @$pb.TagNumber(1)
  void clearRevision() => $_clearField(1);
}

class ResolveResponse extends $pb.GeneratedMessage {
  factory ResolveResponse({
    $core.String? oid,
  }) {
    final result = create();
    if (oid != null) result.oid = oid;
    return result;
  }

  ResolveResponse._();

  factory ResolveResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ResolveResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ResolveResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'oid')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ResolveResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ResolveResponse copyWith(void Function(ResolveResponse) updates) =>
      super.copyWith((message) => updates(message as ResolveResponse))
          as ResolveResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ResolveResponse create() => ResolveResponse._();
  @$core.override
  ResolveResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ResolveResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ResolveResponse>(create);
  static ResolveResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get oid => $_getSZ(0);
  @$pb.TagNumber(1)
  set oid($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasOid() => $_has(0);
  @$pb.TagNumber(1)
  void clearOid() => $_clearField(1);
}

class ReadFileRequest extends $pb.GeneratedMessage {
  factory ReadFileRequest({
    $core.String? revision,
    $core.String? path,
  }) {
    final result = create();
    if (revision != null) result.revision = revision;
    if (path != null) result.path = path;
    return result;
  }

  ReadFileRequest._();

  factory ReadFileRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ReadFileRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReadFileRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'revision')
    ..aOS(2, _omitFieldNames ? '' : 'path')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReadFileRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReadFileRequest copyWith(void Function(ReadFileRequest) updates) =>
      super.copyWith((message) => updates(message as ReadFileRequest))
          as ReadFileRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ReadFileRequest create() => ReadFileRequest._();
  @$core.override
  ReadFileRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ReadFileRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReadFileRequest>(create);
  static ReadFileRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get revision => $_getSZ(0);
  @$pb.TagNumber(1)
  set revision($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRevision() => $_has(0);
  @$pb.TagNumber(1)
  void clearRevision() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get path => $_getSZ(1);
  @$pb.TagNumber(2)
  set path($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasPath() => $_has(1);
  @$pb.TagNumber(2)
  void clearPath() => $_clearField(2);
}

class ListTreeRequest extends $pb.GeneratedMessage {
  factory ListTreeRequest({
    $core.String? revision,
    $core.String? path,
    $core.bool? recursive,
  }) {
    final result = create();
    if (revision != null) result.revision = revision;
    if (path != null) result.path = path;
    if (recursive != null) result.recursive = recursive;
    return result;
  }

  ListTreeRequest._();

  factory ListTreeRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ListTreeRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ListTreeRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'revision')
    ..aOS(2, _omitFieldNames ? '' : 'path')
    ..aOB(3, _omitFieldNames ? '' : 'recursive')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListTreeRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListTreeRequest copyWith(void Function(ListTreeRequest) updates) =>
      super.copyWith((message) => updates(message as ListTreeRequest))
          as ListTreeRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ListTreeRequest create() => ListTreeRequest._();
  @$core.override
  ListTreeRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ListTreeRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ListTreeRequest>(create);
  static ListTreeRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get revision => $_getSZ(0);
  @$pb.TagNumber(1)
  set revision($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRevision() => $_has(0);
  @$pb.TagNumber(1)
  void clearRevision() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get path => $_getSZ(1);
  @$pb.TagNumber(2)
  set path($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasPath() => $_has(1);
  @$pb.TagNumber(2)
  void clearPath() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.bool get recursive => $_getBF(2);
  @$pb.TagNumber(3)
  set recursive($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasRecursive() => $_has(2);
  @$pb.TagNumber(3)
  void clearRecursive() => $_clearField(3);
}

class ListTreeResponse extends $pb.GeneratedMessage {
  factory ListTreeResponse({
    $core.Iterable<TreeEntry>? entries,
  }) {
    final result = create();
    if (entries != null) result.entries.addAll(entries);
    return result;
  }

  ListTreeResponse._();

  factory ListTreeResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ListTreeResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ListTreeResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<TreeEntry>(1, _omitFieldNames ? '' : 'entries',
        subBuilder: TreeEntry.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListTreeResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListTreeResponse copyWith(void Function(ListTreeResponse) updates) =>
      super.copyWith((message) => updates(message as ListTreeResponse))
          as ListTreeResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ListTreeResponse create() => ListTreeResponse._();
  @$core.override
  ListTreeResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ListTreeResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ListTreeResponse>(create);
  static ListTreeResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<TreeEntry> get entries => $_getList(0);
}

class DiffRequest extends $pb.GeneratedMessage {
  factory DiffRequest({
    $core.String? base,
    $core.String? target,
    $core.Iterable<$core.String>? pathGlobs,
  }) {
    final result = create();
    if (base != null) result.base = base;
    if (target != null) result.target = target;
    if (pathGlobs != null) result.pathGlobs.addAll(pathGlobs);
    return result;
  }

  DiffRequest._();

  factory DiffRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DiffRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DiffRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'base')
    ..aOS(2, _omitFieldNames ? '' : 'target')
    ..pPS(3, _omitFieldNames ? '' : 'pathGlobs')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DiffRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DiffRequest copyWith(void Function(DiffRequest) updates) =>
      super.copyWith((message) => updates(message as DiffRequest))
          as DiffRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DiffRequest create() => DiffRequest._();
  @$core.override
  DiffRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DiffRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DiffRequest>(create);
  static DiffRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get base => $_getSZ(0);
  @$pb.TagNumber(1)
  set base($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasBase() => $_has(0);
  @$pb.TagNumber(1)
  void clearBase() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get target => $_getSZ(1);
  @$pb.TagNumber(2)
  set target($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasTarget() => $_has(1);
  @$pb.TagNumber(2)
  void clearTarget() => $_clearField(2);

  @$pb.TagNumber(3)
  $pb.PbList<$core.String> get pathGlobs => $_getList(2);
}

class GrepRequest extends $pb.GeneratedMessage {
  factory GrepRequest({
    $core.String? revision,
    $core.String? pattern,
    $core.Iterable<$core.String>? pathGlobs,
    $fixnum.Int64? limit,
  }) {
    final result = create();
    if (revision != null) result.revision = revision;
    if (pattern != null) result.pattern = pattern;
    if (pathGlobs != null) result.pathGlobs.addAll(pathGlobs);
    if (limit != null) result.limit = limit;
    return result;
  }

  GrepRequest._();

  factory GrepRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GrepRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GrepRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'revision')
    ..aOS(2, _omitFieldNames ? '' : 'pattern')
    ..pPS(3, _omitFieldNames ? '' : 'pathGlobs')
    ..a<$fixnum.Int64>(4, _omitFieldNames ? '' : 'limit', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GrepRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GrepRequest copyWith(void Function(GrepRequest) updates) =>
      super.copyWith((message) => updates(message as GrepRequest))
          as GrepRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GrepRequest create() => GrepRequest._();
  @$core.override
  GrepRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GrepRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GrepRequest>(create);
  static GrepRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get revision => $_getSZ(0);
  @$pb.TagNumber(1)
  set revision($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRevision() => $_has(0);
  @$pb.TagNumber(1)
  void clearRevision() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get pattern => $_getSZ(1);
  @$pb.TagNumber(2)
  set pattern($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasPattern() => $_has(1);
  @$pb.TagNumber(2)
  void clearPattern() => $_clearField(2);

  @$pb.TagNumber(3)
  $pb.PbList<$core.String> get pathGlobs => $_getList(2);

  @$pb.TagNumber(4)
  $fixnum.Int64 get limit => $_getI64(3);
  @$pb.TagNumber(4)
  set limit($fixnum.Int64 value) => $_setInt64(3, value);
  @$pb.TagNumber(4)
  $core.bool hasLimit() => $_has(3);
  @$pb.TagNumber(4)
  void clearLimit() => $_clearField(4);
}

class GrepResponse extends $pb.GeneratedMessage {
  factory GrepResponse({
    $core.Iterable<GrepHit>? hits,
  }) {
    final result = create();
    if (hits != null) result.hits.addAll(hits);
    return result;
  }

  GrepResponse._();

  factory GrepResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GrepResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GrepResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<GrepHit>(1, _omitFieldNames ? '' : 'hits', subBuilder: GrepHit.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GrepResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GrepResponse copyWith(void Function(GrepResponse) updates) =>
      super.copyWith((message) => updates(message as GrepResponse))
          as GrepResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GrepResponse create() => GrepResponse._();
  @$core.override
  GrepResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GrepResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GrepResponse>(create);
  static GrepResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<GrepHit> get hits => $_getList(0);
}

class LogRequest extends $pb.GeneratedMessage {
  factory LogRequest({
    $core.String? revision,
    $core.String? path,
    $fixnum.Int64? limit,
  }) {
    final result = create();
    if (revision != null) result.revision = revision;
    if (path != null) result.path = path;
    if (limit != null) result.limit = limit;
    return result;
  }

  LogRequest._();

  factory LogRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LogRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LogRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'revision')
    ..aOS(2, _omitFieldNames ? '' : 'path')
    ..a<$fixnum.Int64>(3, _omitFieldNames ? '' : 'limit', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LogRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LogRequest copyWith(void Function(LogRequest) updates) =>
      super.copyWith((message) => updates(message as LogRequest)) as LogRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LogRequest create() => LogRequest._();
  @$core.override
  LogRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LogRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LogRequest>(create);
  static LogRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get revision => $_getSZ(0);
  @$pb.TagNumber(1)
  set revision($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRevision() => $_has(0);
  @$pb.TagNumber(1)
  void clearRevision() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get path => $_getSZ(1);
  @$pb.TagNumber(2)
  set path($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasPath() => $_has(1);
  @$pb.TagNumber(2)
  void clearPath() => $_clearField(2);

  @$pb.TagNumber(3)
  $fixnum.Int64 get limit => $_getI64(2);
  @$pb.TagNumber(3)
  set limit($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasLimit() => $_has(2);
  @$pb.TagNumber(3)
  void clearLimit() => $_clearField(3);
}

class LogResponse extends $pb.GeneratedMessage {
  factory LogResponse({
    $core.Iterable<CommitInfo>? commits,
  }) {
    final result = create();
    if (commits != null) result.commits.addAll(commits);
    return result;
  }

  LogResponse._();

  factory LogResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LogResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LogResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<CommitInfo>(1, _omitFieldNames ? '' : 'commits',
        subBuilder: CommitInfo.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LogResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LogResponse copyWith(void Function(LogResponse) updates) =>
      super.copyWith((message) => updates(message as LogResponse))
          as LogResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LogResponse create() => LogResponse._();
  @$core.override
  LogResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LogResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LogResponse>(create);
  static LogResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<CommitInfo> get commits => $_getList(0);
}

class BranchesRequest extends $pb.GeneratedMessage {
  factory BranchesRequest() => create();

  BranchesRequest._();

  factory BranchesRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory BranchesRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'BranchesRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BranchesRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BranchesRequest copyWith(void Function(BranchesRequest) updates) =>
      super.copyWith((message) => updates(message as BranchesRequest))
          as BranchesRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static BranchesRequest create() => BranchesRequest._();
  @$core.override
  BranchesRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static BranchesRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<BranchesRequest>(create);
  static BranchesRequest? _defaultInstance;
}

class BranchesResponse extends $pb.GeneratedMessage {
  factory BranchesResponse({
    $core.Iterable<Branch>? branches,
  }) {
    final result = create();
    if (branches != null) result.branches.addAll(branches);
    return result;
  }

  BranchesResponse._();

  factory BranchesResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory BranchesResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'BranchesResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<Branch>(1, _omitFieldNames ? '' : 'branches',
        subBuilder: Branch.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BranchesResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BranchesResponse copyWith(void Function(BranchesResponse) updates) =>
      super.copyWith((message) => updates(message as BranchesResponse))
          as BranchesResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static BranchesResponse create() => BranchesResponse._();
  @$core.override
  BranchesResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static BranchesResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<BranchesResponse>(create);
  static BranchesResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<Branch> get branches => $_getList(0);
}

class RepoStatusRequest extends $pb.GeneratedMessage {
  factory RepoStatusRequest() => create();

  RepoStatusRequest._();

  factory RepoStatusRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RepoStatusRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RepoStatusRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RepoStatusRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RepoStatusRequest copyWith(void Function(RepoStatusRequest) updates) =>
      super.copyWith((message) => updates(message as RepoStatusRequest))
          as RepoStatusRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RepoStatusRequest create() => RepoStatusRequest._();
  @$core.override
  RepoStatusRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RepoStatusRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RepoStatusRequest>(create);
  static RepoStatusRequest? _defaultInstance;
}

class FetchRequest extends $pb.GeneratedMessage {
  factory FetchRequest() => create();

  FetchRequest._();

  factory FetchRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory FetchRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'FetchRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FetchRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FetchRequest copyWith(void Function(FetchRequest) updates) =>
      super.copyWith((message) => updates(message as FetchRequest))
          as FetchRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static FetchRequest create() => FetchRequest._();
  @$core.override
  FetchRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static FetchRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<FetchRequest>(create);
  static FetchRequest? _defaultInstance;
}

class WorktreeListRequest extends $pb.GeneratedMessage {
  factory WorktreeListRequest() => create();

  WorktreeListRequest._();

  factory WorktreeListRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WorktreeListRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WorktreeListRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WorktreeListRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WorktreeListRequest copyWith(void Function(WorktreeListRequest) updates) =>
      super.copyWith((message) => updates(message as WorktreeListRequest))
          as WorktreeListRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WorktreeListRequest create() => WorktreeListRequest._();
  @$core.override
  WorktreeListRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WorktreeListRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WorktreeListRequest>(create);
  static WorktreeListRequest? _defaultInstance;
}

class WorktreeListResponse extends $pb.GeneratedMessage {
  factory WorktreeListResponse({
    $core.Iterable<WorktreeHandle>? worktrees,
  }) {
    final result = create();
    if (worktrees != null) result.worktrees.addAll(worktrees);
    return result;
  }

  WorktreeListResponse._();

  factory WorktreeListResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WorktreeListResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WorktreeListResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<WorktreeHandle>(1, _omitFieldNames ? '' : 'worktrees',
        subBuilder: WorktreeHandle.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WorktreeListResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WorktreeListResponse copyWith(void Function(WorktreeListResponse) updates) =>
      super.copyWith((message) => updates(message as WorktreeListResponse))
          as WorktreeListResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WorktreeListResponse create() => WorktreeListResponse._();
  @$core.override
  WorktreeListResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WorktreeListResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WorktreeListResponse>(create);
  static WorktreeListResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<WorktreeHandle> get worktrees => $_getList(0);
}

class WorktreeRemoveRequest extends $pb.GeneratedMessage {
  factory WorktreeRemoveRequest({
    $core.String? id,
  }) {
    final result = create();
    if (id != null) result.id = id;
    return result;
  }

  WorktreeRemoveRequest._();

  factory WorktreeRemoveRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WorktreeRemoveRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WorktreeRemoveRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WorktreeRemoveRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WorktreeRemoveRequest copyWith(
          void Function(WorktreeRemoveRequest) updates) =>
      super.copyWith((message) => updates(message as WorktreeRemoveRequest))
          as WorktreeRemoveRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WorktreeRemoveRequest create() => WorktreeRemoveRequest._();
  @$core.override
  WorktreeRemoveRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WorktreeRemoveRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WorktreeRemoveRequest>(create);
  static WorktreeRemoveRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);
}

class WorktreeRemoveResponse extends $pb.GeneratedMessage {
  factory WorktreeRemoveResponse() => create();

  WorktreeRemoveResponse._();

  factory WorktreeRemoveResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WorktreeRemoveResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WorktreeRemoveResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WorktreeRemoveResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WorktreeRemoveResponse copyWith(
          void Function(WorktreeRemoveResponse) updates) =>
      super.copyWith((message) => updates(message as WorktreeRemoveResponse))
          as WorktreeRemoveResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WorktreeRemoveResponse create() => WorktreeRemoveResponse._();
  @$core.override
  WorktreeRemoveResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WorktreeRemoveResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WorktreeRemoveResponse>(create);
  static WorktreeRemoveResponse? _defaultInstance;
}

class CheckpointRequest extends $pb.GeneratedMessage {
  factory CheckpointRequest({
    $core.String? worktreeId,
    $core.String? name,
  }) {
    final result = create();
    if (worktreeId != null) result.worktreeId = worktreeId;
    if (name != null) result.name = name;
    return result;
  }

  CheckpointRequest._();

  factory CheckpointRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CheckpointRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CheckpointRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'worktreeId')
    ..aOS(2, _omitFieldNames ? '' : 'name')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CheckpointRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CheckpointRequest copyWith(void Function(CheckpointRequest) updates) =>
      super.copyWith((message) => updates(message as CheckpointRequest))
          as CheckpointRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CheckpointRequest create() => CheckpointRequest._();
  @$core.override
  CheckpointRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CheckpointRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<CheckpointRequest>(create);
  static CheckpointRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get worktreeId => $_getSZ(0);
  @$pb.TagNumber(1)
  set worktreeId($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasWorktreeId() => $_has(0);
  @$pb.TagNumber(1)
  void clearWorktreeId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get name => $_getSZ(1);
  @$pb.TagNumber(2)
  set name($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasName() => $_has(1);
  @$pb.TagNumber(2)
  void clearName() => $_clearField(2);
}

class PushRequest extends $pb.GeneratedMessage {
  factory PushRequest({
    Checkpoint? checkpoint,
    $core.String? remoteRef,
  }) {
    final result = create();
    if (checkpoint != null) result.checkpoint = checkpoint;
    if (remoteRef != null) result.remoteRef = remoteRef;
    return result;
  }

  PushRequest._();

  factory PushRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PushRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PushRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<Checkpoint>(1, _omitFieldNames ? '' : 'checkpoint',
        subBuilder: Checkpoint.create)
    ..aOS(2, _omitFieldNames ? '' : 'remoteRef')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PushRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PushRequest copyWith(void Function(PushRequest) updates) =>
      super.copyWith((message) => updates(message as PushRequest))
          as PushRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PushRequest create() => PushRequest._();
  @$core.override
  PushRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PushRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PushRequest>(create);
  static PushRequest? _defaultInstance;

  @$pb.TagNumber(1)
  Checkpoint get checkpoint => $_getN(0);
  @$pb.TagNumber(1)
  set checkpoint(Checkpoint value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasCheckpoint() => $_has(0);
  @$pb.TagNumber(1)
  void clearCheckpoint() => $_clearField(1);
  @$pb.TagNumber(1)
  Checkpoint ensureCheckpoint() => $_ensure(0);

  @$pb.TagNumber(2)
  $core.String get remoteRef => $_getSZ(1);
  @$pb.TagNumber(2)
  set remoteRef($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasRemoteRef() => $_has(1);
  @$pb.TagNumber(2)
  void clearRemoteRef() => $_clearField(2);
}

class PushResponse extends $pb.GeneratedMessage {
  factory PushResponse() => create();

  PushResponse._();

  factory PushResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PushResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PushResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PushResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PushResponse copyWith(void Function(PushResponse) updates) =>
      super.copyWith((message) => updates(message as PushResponse))
          as PushResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PushResponse create() => PushResponse._();
  @$core.override
  PushResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PushResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PushResponse>(create);
  static PushResponse? _defaultInstance;
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
