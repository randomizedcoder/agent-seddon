// This is a generated file - do not edit.
//
// Generated from agent/v1/review.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

import 'repo.pbenum.dart' as $1;
import 'review.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'review.pbenum.dart';

/// The target, encoded: `pr:<n>` | `branch:<name>` | `worktree`.
class ReviewCollectRequest extends $pb.GeneratedMessage {
  factory ReviewCollectRequest({
    $core.String? target,
  }) {
    final result = ReviewCollectRequest._();
    if (target != null) result.target = target;
    return result;
  }

  ReviewCollectRequest._();

  factory ReviewCollectRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCollectRequest()..mergeFromBuffer(data, registry);
  factory ReviewCollectRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCollectRequest()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewCollectRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewCollectRequest.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'target')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCollectRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCollectRequest copyWith(void Function(ReviewCollectRequest) updates) =>
      super.copyWith((message) => updates(message as ReviewCollectRequest))
          as ReviewCollectRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated(
      'Use ReviewCollectRequest() / ReviewCollectRequest.new instead')
  static ReviewCollectRequest create() => ReviewCollectRequest._();
  static $pb.GeneratedMessage $_createMessage() => ReviewCollectRequest._();
  @$core.override
  ReviewCollectRequest createEmptyInstance() => ReviewCollectRequest._();
  @$core.pragma('dart2js:noInline')
  static ReviewCollectRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewCollectRequest>(
          ReviewCollectRequest.$_createMessage);
  static ReviewCollectRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get target => $_getSZ(0);
  @$pb.TagNumber(1)
  set target($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasTarget() => $_has(0);
  @$pb.TagNumber(1)
  void clearTarget() => $_clearField(1);
}

class ReviewCollectorStatus extends $pb.GeneratedMessage {
  factory ReviewCollectorStatus({
    $core.String? collector,
    ReviewCollectStatus? status,
    $core.String? reason,
    $core.int? durationMs,
  }) {
    final result = ReviewCollectorStatus._();
    if (collector != null) result.collector = collector;
    if (status != null) result.status = status;
    if (reason != null) result.reason = reason;
    if (durationMs != null) result.durationMs = durationMs;
    return result;
  }

  ReviewCollectorStatus._();

  factory ReviewCollectorStatus.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCollectorStatus()..mergeFromBuffer(data, registry);
  factory ReviewCollectorStatus.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCollectorStatus()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewCollectorStatus',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewCollectorStatus.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'collector')
    ..aE<ReviewCollectStatus>(2, _omitFieldNames ? '' : 'status',
        enumValues: ReviewCollectStatus.values)
    ..aOS(3, _omitFieldNames ? '' : 'reason')
    ..aI(4, _omitFieldNames ? '' : 'durationMs', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCollectorStatus clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCollectorStatus copyWith(
          void Function(ReviewCollectorStatus) updates) =>
      super.copyWith((message) => updates(message as ReviewCollectorStatus))
          as ReviewCollectorStatus;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated(
      'Use ReviewCollectorStatus() / ReviewCollectorStatus.new instead')
  static ReviewCollectorStatus create() => ReviewCollectorStatus._();
  static $pb.GeneratedMessage $_createMessage() => ReviewCollectorStatus._();
  @$core.override
  ReviewCollectorStatus createEmptyInstance() => ReviewCollectorStatus._();
  @$core.pragma('dart2js:noInline')
  static ReviewCollectorStatus getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewCollectorStatus>(
          ReviewCollectorStatus.$_createMessage);
  static ReviewCollectorStatus? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get collector => $_getSZ(0);
  @$pb.TagNumber(1)
  set collector($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasCollector() => $_has(0);
  @$pb.TagNumber(1)
  void clearCollector() => $_clearField(1);

  @$pb.TagNumber(2)
  ReviewCollectStatus get status => $_getN(1);
  @$pb.TagNumber(2)
  set status(ReviewCollectStatus value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasStatus() => $_has(1);
  @$pb.TagNumber(2)
  void clearStatus() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get reason => $_getSZ(2);
  @$pb.TagNumber(3)
  set reason($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasReason() => $_has(2);
  @$pb.TagNumber(3)
  void clearReason() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get durationMs => $_getIZ(3);
  @$pb.TagNumber(4)
  set durationMs($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasDurationMs() => $_has(3);
  @$pb.TagNumber(4)
  void clearDurationMs() => $_clearField(4);
}

class ReviewMeta extends $pb.GeneratedMessage {
  factory ReviewMeta({
    $core.String? repoHash,
    $core.String? baseRev,
    $core.String? headRev,
    $core.int? totalMs,
    $core.Iterable<ReviewCollectorStatus>? collectors,
  }) {
    final result = ReviewMeta._();
    if (repoHash != null) result.repoHash = repoHash;
    if (baseRev != null) result.baseRev = baseRev;
    if (headRev != null) result.headRev = headRev;
    if (totalMs != null) result.totalMs = totalMs;
    if (collectors != null) result.collectors.addAll(collectors);
    return result;
  }

  ReviewMeta._();

  factory ReviewMeta.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewMeta()..mergeFromBuffer(data, registry);
  factory ReviewMeta.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewMeta()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewMeta',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewMeta.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'repoHash')
    ..aOS(2, _omitFieldNames ? '' : 'baseRev')
    ..aOS(3, _omitFieldNames ? '' : 'headRev')
    ..aI(4, _omitFieldNames ? '' : 'totalMs', fieldType: $pb.PbFieldType.OU3)
    ..pPM<ReviewCollectorStatus>(5, _omitFieldNames ? '' : 'collectors',
        subBuilder: ReviewCollectorStatus.$_createMessage)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewMeta clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewMeta copyWith(void Function(ReviewMeta) updates) =>
      super.copyWith((message) => updates(message as ReviewMeta)) as ReviewMeta;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewMeta() / ReviewMeta.new instead')
  static ReviewMeta create() => ReviewMeta._();
  static $pb.GeneratedMessage $_createMessage() => ReviewMeta._();
  @$core.override
  ReviewMeta createEmptyInstance() => ReviewMeta._();
  @$core.pragma('dart2js:noInline')
  static ReviewMeta getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewMeta>(ReviewMeta.$_createMessage);
  static ReviewMeta? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get repoHash => $_getSZ(0);
  @$pb.TagNumber(1)
  set repoHash($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRepoHash() => $_has(0);
  @$pb.TagNumber(1)
  void clearRepoHash() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get baseRev => $_getSZ(1);
  @$pb.TagNumber(2)
  set baseRev($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBaseRev() => $_has(1);
  @$pb.TagNumber(2)
  void clearBaseRev() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get headRev => $_getSZ(2);
  @$pb.TagNumber(3)
  set headRev($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasHeadRev() => $_has(2);
  @$pb.TagNumber(3)
  void clearHeadRev() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get totalMs => $_getIZ(3);
  @$pb.TagNumber(4)
  set totalMs($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasTotalMs() => $_has(3);
  @$pb.TagNumber(4)
  void clearTotalMs() => $_clearField(4);

  @$pb.TagNumber(5)
  $pb.PbList<ReviewCollectorStatus> get collectors => $_getList(4);
}

class ReviewChangedFile extends $pb.GeneratedMessage {
  factory ReviewChangedFile({
    $core.String? path,
    $1.ChangeKind? change,
    $core.int? additions,
    $core.int? deletions,
    $core.bool? isBinary,
    $core.String? lang,
    $core.String? patch,
    $core.String? oldPath,
  }) {
    final result = ReviewChangedFile._();
    if (path != null) result.path = path;
    if (change != null) result.change = change;
    if (additions != null) result.additions = additions;
    if (deletions != null) result.deletions = deletions;
    if (isBinary != null) result.isBinary = isBinary;
    if (lang != null) result.lang = lang;
    if (patch != null) result.patch = patch;
    if (oldPath != null) result.oldPath = oldPath;
    return result;
  }

  ReviewChangedFile._();

  factory ReviewChangedFile.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewChangedFile()..mergeFromBuffer(data, registry);
  factory ReviewChangedFile.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewChangedFile()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewChangedFile',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewChangedFile.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'path')
    ..aE<$1.ChangeKind>(2, _omitFieldNames ? '' : 'change',
        enumValues: $1.ChangeKind.values)
    ..aI(3, _omitFieldNames ? '' : 'additions', fieldType: $pb.PbFieldType.OU3)
    ..aI(4, _omitFieldNames ? '' : 'deletions', fieldType: $pb.PbFieldType.OU3)
    ..aOB(5, _omitFieldNames ? '' : 'isBinary')
    ..aOS(6, _omitFieldNames ? '' : 'lang')
    ..aOS(7, _omitFieldNames ? '' : 'patch')
    ..aOS(8, _omitFieldNames ? '' : 'oldPath')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewChangedFile clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewChangedFile copyWith(void Function(ReviewChangedFile) updates) =>
      super.copyWith((message) => updates(message as ReviewChangedFile))
          as ReviewChangedFile;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewChangedFile() / ReviewChangedFile.new instead')
  static ReviewChangedFile create() => ReviewChangedFile._();
  static $pb.GeneratedMessage $_createMessage() => ReviewChangedFile._();
  @$core.override
  ReviewChangedFile createEmptyInstance() => ReviewChangedFile._();
  @$core.pragma('dart2js:noInline')
  static ReviewChangedFile getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewChangedFile>(
          ReviewChangedFile.$_createMessage);
  static ReviewChangedFile? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get path => $_getSZ(0);
  @$pb.TagNumber(1)
  set path($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPath() => $_has(0);
  @$pb.TagNumber(1)
  void clearPath() => $_clearField(1);

  @$pb.TagNumber(2)
  $1.ChangeKind get change => $_getN(1);
  @$pb.TagNumber(2)
  set change($1.ChangeKind value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasChange() => $_has(1);
  @$pb.TagNumber(2)
  void clearChange() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get additions => $_getIZ(2);
  @$pb.TagNumber(3)
  set additions($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasAdditions() => $_has(2);
  @$pb.TagNumber(3)
  void clearAdditions() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get deletions => $_getIZ(3);
  @$pb.TagNumber(4)
  set deletions($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasDeletions() => $_has(3);
  @$pb.TagNumber(4)
  void clearDeletions() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.bool get isBinary => $_getBF(4);
  @$pb.TagNumber(5)
  set isBinary($core.bool value) => $_setBool(4, value);
  @$pb.TagNumber(5)
  $core.bool hasIsBinary() => $_has(4);
  @$pb.TagNumber(5)
  void clearIsBinary() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.String get lang => $_getSZ(5);
  @$pb.TagNumber(6)
  set lang($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasLang() => $_has(5);
  @$pb.TagNumber(6)
  void clearLang() => $_clearField(6);

  /// Unified-diff hunks (bounded; empty for binary/omitted files). Untrusted.
  @$pb.TagNumber(7)
  $core.String get patch => $_getSZ(6);
  @$pb.TagNumber(7)
  set patch($core.String value) => $_setString(6, value);
  @$pb.TagNumber(7)
  $core.bool hasPatch() => $_has(6);
  @$pb.TagNumber(7)
  void clearPatch() => $_clearField(7);

  /// For a rename (or a copy), the base-side path ("" otherwise): a search over the base branch
  /// finds the file there, so its hits are pre-PR content too.
  @$pb.TagNumber(8)
  $core.String get oldPath => $_getSZ(7);
  @$pb.TagNumber(8)
  set oldPath($core.String value) => $_setString(7, value);
  @$pb.TagNumber(8)
  $core.bool hasOldPath() => $_has(7);
  @$pb.TagNumber(8)
  void clearOldPath() => $_clearField(8);
}

class ReviewCommit extends $pb.GeneratedMessage {
  factory ReviewCommit({
    $core.String? short,
    $core.String? summary,
    $core.String? body,
    $core.String? author,
    $core.int? ageDays,
  }) {
    final result = ReviewCommit._();
    if (short != null) result.short = short;
    if (summary != null) result.summary = summary;
    if (body != null) result.body = body;
    if (author != null) result.author = author;
    if (ageDays != null) result.ageDays = ageDays;
    return result;
  }

  ReviewCommit._();

  factory ReviewCommit.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCommit()..mergeFromBuffer(data, registry);
  factory ReviewCommit.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCommit()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewCommit',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewCommit.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'short')
    ..aOS(2, _omitFieldNames ? '' : 'summary')
    ..aOS(3, _omitFieldNames ? '' : 'body')
    ..aOS(4, _omitFieldNames ? '' : 'author')
    ..aI(5, _omitFieldNames ? '' : 'ageDays', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCommit clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCommit copyWith(void Function(ReviewCommit) updates) =>
      super.copyWith((message) => updates(message as ReviewCommit))
          as ReviewCommit;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewCommit() / ReviewCommit.new instead')
  static ReviewCommit create() => ReviewCommit._();
  static $pb.GeneratedMessage $_createMessage() => ReviewCommit._();
  @$core.override
  ReviewCommit createEmptyInstance() => ReviewCommit._();
  @$core.pragma('dart2js:noInline')
  static ReviewCommit getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewCommit>(
          ReviewCommit.$_createMessage);
  static ReviewCommit? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get short => $_getSZ(0);
  @$pb.TagNumber(1)
  set short($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasShort() => $_has(0);
  @$pb.TagNumber(1)
  void clearShort() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get summary => $_getSZ(1);
  @$pb.TagNumber(2)
  set summary($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasSummary() => $_has(1);
  @$pb.TagNumber(2)
  void clearSummary() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get body => $_getSZ(2);
  @$pb.TagNumber(3)
  set body($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasBody() => $_has(2);
  @$pb.TagNumber(3)
  void clearBody() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get author => $_getSZ(3);
  @$pb.TagNumber(4)
  set author($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasAuthor() => $_has(3);
  @$pb.TagNumber(4)
  void clearAuthor() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.int get ageDays => $_getIZ(4);
  @$pb.TagNumber(5)
  set ageDays($core.int value) => $_setUnsignedInt32(4, value);
  @$pb.TagNumber(5)
  $core.bool hasAgeDays() => $_has(4);
  @$pb.TagNumber(5)
  void clearAgeDays() => $_clearField(5);
}

class ReviewChangeSet extends $pb.GeneratedMessage {
  factory ReviewChangeSet({
    $core.String? baseRev,
    $core.String? headRev,
    $core.Iterable<ReviewChangedFile>? files,
    $core.int? repoFileCount,
    $core.Iterable<ReviewCommit>? commits,
  }) {
    final result = ReviewChangeSet._();
    if (baseRev != null) result.baseRev = baseRev;
    if (headRev != null) result.headRev = headRev;
    if (files != null) result.files.addAll(files);
    if (repoFileCount != null) result.repoFileCount = repoFileCount;
    if (commits != null) result.commits.addAll(commits);
    return result;
  }

  ReviewChangeSet._();

  factory ReviewChangeSet.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewChangeSet()..mergeFromBuffer(data, registry);
  factory ReviewChangeSet.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewChangeSet()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewChangeSet',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewChangeSet.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'baseRev')
    ..aOS(2, _omitFieldNames ? '' : 'headRev')
    ..pPM<ReviewChangedFile>(3, _omitFieldNames ? '' : 'files',
        subBuilder: ReviewChangedFile.$_createMessage)
    ..aI(4, _omitFieldNames ? '' : 'repoFileCount',
        fieldType: $pb.PbFieldType.OU3)
    ..pPM<ReviewCommit>(5, _omitFieldNames ? '' : 'commits',
        subBuilder: ReviewCommit.$_createMessage)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewChangeSet clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewChangeSet copyWith(void Function(ReviewChangeSet) updates) =>
      super.copyWith((message) => updates(message as ReviewChangeSet))
          as ReviewChangeSet;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewChangeSet() / ReviewChangeSet.new instead')
  static ReviewChangeSet create() => ReviewChangeSet._();
  static $pb.GeneratedMessage $_createMessage() => ReviewChangeSet._();
  @$core.override
  ReviewChangeSet createEmptyInstance() => ReviewChangeSet._();
  @$core.pragma('dart2js:noInline')
  static ReviewChangeSet getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewChangeSet>(
          ReviewChangeSet.$_createMessage);
  static ReviewChangeSet? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get baseRev => $_getSZ(0);
  @$pb.TagNumber(1)
  set baseRev($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasBaseRev() => $_has(0);
  @$pb.TagNumber(1)
  void clearBaseRev() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get headRev => $_getSZ(1);
  @$pb.TagNumber(2)
  set headRev($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasHeadRev() => $_has(1);
  @$pb.TagNumber(2)
  void clearHeadRev() => $_clearField(2);

  @$pb.TagNumber(3)
  $pb.PbList<ReviewChangedFile> get files => $_getList(2);

  @$pb.TagNumber(4)
  $core.int get repoFileCount => $_getIZ(3);
  @$pb.TagNumber(4)
  set repoFileCount($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasRepoFileCount() => $_has(3);
  @$pb.TagNumber(4)
  void clearRepoFileCount() => $_clearField(4);

  @$pb.TagNumber(5)
  $pb.PbList<ReviewCommit> get commits => $_getList(4);
}

class ReviewGitState extends $pb.GeneratedMessage {
  factory ReviewGitState({
    $core.String? remoteUrlHash,
    ReviewForgeHost? host,
    ReviewRepoRelation? relationship,
    $core.String? defaultBranch,
    ReviewRepoLanguage? project,
  }) {
    final result = ReviewGitState._();
    if (remoteUrlHash != null) result.remoteUrlHash = remoteUrlHash;
    if (host != null) result.host = host;
    if (relationship != null) result.relationship = relationship;
    if (defaultBranch != null) result.defaultBranch = defaultBranch;
    if (project != null) result.project = project;
    return result;
  }

  ReviewGitState._();

  factory ReviewGitState.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewGitState()..mergeFromBuffer(data, registry);
  factory ReviewGitState.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewGitState()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewGitState',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewGitState.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'remoteUrlHash')
    ..aE<ReviewForgeHost>(2, _omitFieldNames ? '' : 'host',
        enumValues: ReviewForgeHost.values)
    ..aE<ReviewRepoRelation>(3, _omitFieldNames ? '' : 'relationship',
        enumValues: ReviewRepoRelation.values)
    ..aOS(4, _omitFieldNames ? '' : 'defaultBranch')
    ..aE<ReviewRepoLanguage>(5, _omitFieldNames ? '' : 'project',
        enumValues: ReviewRepoLanguage.values)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewGitState clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewGitState copyWith(void Function(ReviewGitState) updates) =>
      super.copyWith((message) => updates(message as ReviewGitState))
          as ReviewGitState;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewGitState() / ReviewGitState.new instead')
  static ReviewGitState create() => ReviewGitState._();
  static $pb.GeneratedMessage $_createMessage() => ReviewGitState._();
  @$core.override
  ReviewGitState createEmptyInstance() => ReviewGitState._();
  @$core.pragma('dart2js:noInline')
  static ReviewGitState getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewGitState>(
          ReviewGitState.$_createMessage);
  static ReviewGitState? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get remoteUrlHash => $_getSZ(0);
  @$pb.TagNumber(1)
  set remoteUrlHash($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRemoteUrlHash() => $_has(0);
  @$pb.TagNumber(1)
  void clearRemoteUrlHash() => $_clearField(1);

  @$pb.TagNumber(2)
  ReviewForgeHost get host => $_getN(1);
  @$pb.TagNumber(2)
  set host(ReviewForgeHost value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasHost() => $_has(1);
  @$pb.TagNumber(2)
  void clearHost() => $_clearField(2);

  @$pb.TagNumber(3)
  ReviewRepoRelation get relationship => $_getN(2);
  @$pb.TagNumber(3)
  set relationship(ReviewRepoRelation value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasRelationship() => $_has(2);
  @$pb.TagNumber(3)
  void clearRelationship() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get defaultBranch => $_getSZ(3);
  @$pb.TagNumber(4)
  set defaultBranch($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasDefaultBranch() => $_has(3);
  @$pb.TagNumber(4)
  void clearDefaultBranch() => $_clearField(4);

  @$pb.TagNumber(5)
  ReviewRepoLanguage get project => $_getN(4);
  @$pb.TagNumber(5)
  set project(ReviewRepoLanguage value) => $_setField(5, value);
  @$pb.TagNumber(5)
  $core.bool hasProject() => $_has(4);
  @$pb.TagNumber(5)
  void clearProject() => $_clearField(5);
}

/// One static-analysis finding (a linter diagnostic). All fields are
/// tool-derived and untrusted: `file` is repo-relative + confined, `message`
/// bounded. `in_change` marks a finding on a file the change touched.
class ReviewAnalysisFinding extends $pb.GeneratedMessage {
  factory ReviewAnalysisFinding({
    $core.String? tool,
    $core.String? rule,
    $core.String? severity,
    $core.String? file,
    $core.int? line,
    $core.String? message,
    $core.bool? inChange,
    $core.bool? generated,
  }) {
    final result = ReviewAnalysisFinding._();
    if (tool != null) result.tool = tool;
    if (rule != null) result.rule = rule;
    if (severity != null) result.severity = severity;
    if (file != null) result.file = file;
    if (line != null) result.line = line;
    if (message != null) result.message = message;
    if (inChange != null) result.inChange = inChange;
    if (generated != null) result.generated = generated;
    return result;
  }

  ReviewAnalysisFinding._();

  factory ReviewAnalysisFinding.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewAnalysisFinding()..mergeFromBuffer(data, registry);
  factory ReviewAnalysisFinding.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewAnalysisFinding()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewAnalysisFinding',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewAnalysisFinding.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'tool')
    ..aOS(2, _omitFieldNames ? '' : 'rule')
    ..aOS(3, _omitFieldNames ? '' : 'severity')
    ..aOS(4, _omitFieldNames ? '' : 'file')
    ..aI(5, _omitFieldNames ? '' : 'line', fieldType: $pb.PbFieldType.OU3)
    ..aOS(6, _omitFieldNames ? '' : 'message')
    ..aOB(7, _omitFieldNames ? '' : 'inChange')
    ..aOB(8, _omitFieldNames ? '' : 'generated')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewAnalysisFinding clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewAnalysisFinding copyWith(
          void Function(ReviewAnalysisFinding) updates) =>
      super.copyWith((message) => updates(message as ReviewAnalysisFinding))
          as ReviewAnalysisFinding;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated(
      'Use ReviewAnalysisFinding() / ReviewAnalysisFinding.new instead')
  static ReviewAnalysisFinding create() => ReviewAnalysisFinding._();
  static $pb.GeneratedMessage $_createMessage() => ReviewAnalysisFinding._();
  @$core.override
  ReviewAnalysisFinding createEmptyInstance() => ReviewAnalysisFinding._();
  @$core.pragma('dart2js:noInline')
  static ReviewAnalysisFinding getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewAnalysisFinding>(
          ReviewAnalysisFinding.$_createMessage);
  static ReviewAnalysisFinding? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get tool => $_getSZ(0);
  @$pb.TagNumber(1)
  set tool($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasTool() => $_has(0);
  @$pb.TagNumber(1)
  void clearTool() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get rule => $_getSZ(1);
  @$pb.TagNumber(2)
  set rule($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasRule() => $_has(1);
  @$pb.TagNumber(2)
  void clearRule() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get severity => $_getSZ(2);
  @$pb.TagNumber(3)
  set severity($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasSeverity() => $_has(2);
  @$pb.TagNumber(3)
  void clearSeverity() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get file => $_getSZ(3);
  @$pb.TagNumber(4)
  set file($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasFile() => $_has(3);
  @$pb.TagNumber(4)
  void clearFile() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.int get line => $_getIZ(4);
  @$pb.TagNumber(5)
  set line($core.int value) => $_setUnsignedInt32(4, value);
  @$pb.TagNumber(5)
  $core.bool hasLine() => $_has(4);
  @$pb.TagNumber(5)
  void clearLine() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.String get message => $_getSZ(5);
  @$pb.TagNumber(6)
  set message($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasMessage() => $_has(5);
  @$pb.TagNumber(6)
  void clearMessage() => $_clearField(6);

  @$pb.TagNumber(7)
  $core.bool get inChange => $_getBF(6);
  @$pb.TagNumber(7)
  set inChange($core.bool value) => $_setBool(6, value);
  @$pb.TagNumber(7)
  $core.bool hasInChange() => $_has(6);
  @$pb.TagNumber(7)
  void clearInChange() => $_clearField(7);

  /// The file is generated code (ranked last, tallied apart in the digest).
  @$pb.TagNumber(8)
  $core.bool get generated => $_getBF(7);
  @$pb.TagNumber(8)
  set generated($core.bool value) => $_setBool(7, value);
  @$pb.TagNumber(8)
  $core.bool hasGenerated() => $_has(7);
  @$pb.TagNumber(8)
  void clearGenerated() => $_clearField(8);
}

/// The outcome of one linter invocation (`status`: ok | skipped | timeout |
/// failed). Distinct from a finding — a clean or unrun tool still gets a run.
class ReviewAnalyzerRun extends $pb.GeneratedMessage {
  factory ReviewAnalyzerRun({
    $core.String? tool,
    $core.String? status,
    $core.String? reason,
    $core.int? durationMs,
    $core.int? findingCount,
  }) {
    final result = ReviewAnalyzerRun._();
    if (tool != null) result.tool = tool;
    if (status != null) result.status = status;
    if (reason != null) result.reason = reason;
    if (durationMs != null) result.durationMs = durationMs;
    if (findingCount != null) result.findingCount = findingCount;
    return result;
  }

  ReviewAnalyzerRun._();

  factory ReviewAnalyzerRun.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewAnalyzerRun()..mergeFromBuffer(data, registry);
  factory ReviewAnalyzerRun.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewAnalyzerRun()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewAnalyzerRun',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewAnalyzerRun.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'tool')
    ..aOS(2, _omitFieldNames ? '' : 'status')
    ..aOS(3, _omitFieldNames ? '' : 'reason')
    ..aI(4, _omitFieldNames ? '' : 'durationMs', fieldType: $pb.PbFieldType.OU3)
    ..aI(5, _omitFieldNames ? '' : 'findingCount',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewAnalyzerRun clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewAnalyzerRun copyWith(void Function(ReviewAnalyzerRun) updates) =>
      super.copyWith((message) => updates(message as ReviewAnalyzerRun))
          as ReviewAnalyzerRun;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewAnalyzerRun() / ReviewAnalyzerRun.new instead')
  static ReviewAnalyzerRun create() => ReviewAnalyzerRun._();
  static $pb.GeneratedMessage $_createMessage() => ReviewAnalyzerRun._();
  @$core.override
  ReviewAnalyzerRun createEmptyInstance() => ReviewAnalyzerRun._();
  @$core.pragma('dart2js:noInline')
  static ReviewAnalyzerRun getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewAnalyzerRun>(
          ReviewAnalyzerRun.$_createMessage);
  static ReviewAnalyzerRun? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get tool => $_getSZ(0);
  @$pb.TagNumber(1)
  set tool($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasTool() => $_has(0);
  @$pb.TagNumber(1)
  void clearTool() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get status => $_getSZ(1);
  @$pb.TagNumber(2)
  set status($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasStatus() => $_has(1);
  @$pb.TagNumber(2)
  void clearStatus() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get reason => $_getSZ(2);
  @$pb.TagNumber(3)
  set reason($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasReason() => $_has(2);
  @$pb.TagNumber(3)
  void clearReason() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get durationMs => $_getIZ(3);
  @$pb.TagNumber(4)
  set durationMs($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasDurationMs() => $_has(3);
  @$pb.TagNumber(4)
  void clearDurationMs() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.int get findingCount => $_getIZ(4);
  @$pb.TagNumber(5)
  set findingCount($core.int value) => $_setUnsignedInt32(4, value);
  @$pb.TagNumber(5)
  $core.bool hasFindingCount() => $_has(4);
  @$pb.TagNumber(5)
  void clearFindingCount() => $_clearField(5);
}

class ReviewAnalysisReport extends $pb.GeneratedMessage {
  factory ReviewAnalysisReport({
    $core.String? language,
    $core.Iterable<ReviewAnalyzerRun>? runs,
    $core.Iterable<ReviewAnalysisFinding>? findings,
  }) {
    final result = ReviewAnalysisReport._();
    if (language != null) result.language = language;
    if (runs != null) result.runs.addAll(runs);
    if (findings != null) result.findings.addAll(findings);
    return result;
  }

  ReviewAnalysisReport._();

  factory ReviewAnalysisReport.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewAnalysisReport()..mergeFromBuffer(data, registry);
  factory ReviewAnalysisReport.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewAnalysisReport()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewAnalysisReport',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewAnalysisReport.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'language')
    ..pPM<ReviewAnalyzerRun>(2, _omitFieldNames ? '' : 'runs',
        subBuilder: ReviewAnalyzerRun.$_createMessage)
    ..pPM<ReviewAnalysisFinding>(3, _omitFieldNames ? '' : 'findings',
        subBuilder: ReviewAnalysisFinding.$_createMessage)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewAnalysisReport clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewAnalysisReport copyWith(void Function(ReviewAnalysisReport) updates) =>
      super.copyWith((message) => updates(message as ReviewAnalysisReport))
          as ReviewAnalysisReport;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated(
      'Use ReviewAnalysisReport() / ReviewAnalysisReport.new instead')
  static ReviewAnalysisReport create() => ReviewAnalysisReport._();
  static $pb.GeneratedMessage $_createMessage() => ReviewAnalysisReport._();
  @$core.override
  ReviewAnalysisReport createEmptyInstance() => ReviewAnalysisReport._();
  @$core.pragma('dart2js:noInline')
  static ReviewAnalysisReport getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewAnalysisReport>(
          ReviewAnalysisReport.$_createMessage);
  static ReviewAnalysisReport? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get language => $_getSZ(0);
  @$pb.TagNumber(1)
  set language($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasLanguage() => $_has(0);
  @$pb.TagNumber(1)
  void clearLanguage() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<ReviewAnalyzerRun> get runs => $_getList(1);

  @$pb.TagNumber(3)
  $pb.PbList<ReviewAnalysisFinding> get findings => $_getList(2);
}

/// One changed top-level function signature (the cheap AST subset). Tool-derived
/// and untrusted: `file` is repo-relative + confined, `before`/`after` bounded.
class ReviewSignatureChange extends $pb.GeneratedMessage {
  factory ReviewSignatureChange({
    $core.String? file,
    $core.String? lang,
    $core.String? kind,
    $core.String? name,
    $core.String? before,
    $core.String? after,
  }) {
    final result = ReviewSignatureChange._();
    if (file != null) result.file = file;
    if (lang != null) result.lang = lang;
    if (kind != null) result.kind = kind;
    if (name != null) result.name = name;
    if (before != null) result.before = before;
    if (after != null) result.after = after;
    return result;
  }

  ReviewSignatureChange._();

  factory ReviewSignatureChange.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewSignatureChange()..mergeFromBuffer(data, registry);
  factory ReviewSignatureChange.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewSignatureChange()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewSignatureChange',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewSignatureChange.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'file')
    ..aOS(2, _omitFieldNames ? '' : 'lang')
    ..aOS(3, _omitFieldNames ? '' : 'kind')
    ..aOS(4, _omitFieldNames ? '' : 'name')
    ..aOS(5, _omitFieldNames ? '' : 'before')
    ..aOS(6, _omitFieldNames ? '' : 'after')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewSignatureChange clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewSignatureChange copyWith(
          void Function(ReviewSignatureChange) updates) =>
      super.copyWith((message) => updates(message as ReviewSignatureChange))
          as ReviewSignatureChange;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated(
      'Use ReviewSignatureChange() / ReviewSignatureChange.new instead')
  static ReviewSignatureChange create() => ReviewSignatureChange._();
  static $pb.GeneratedMessage $_createMessage() => ReviewSignatureChange._();
  @$core.override
  ReviewSignatureChange createEmptyInstance() => ReviewSignatureChange._();
  @$core.pragma('dart2js:noInline')
  static ReviewSignatureChange getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewSignatureChange>(
          ReviewSignatureChange.$_createMessage);
  static ReviewSignatureChange? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get file => $_getSZ(0);
  @$pb.TagNumber(1)
  set file($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasFile() => $_has(0);
  @$pb.TagNumber(1)
  void clearFile() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get lang => $_getSZ(1);
  @$pb.TagNumber(2)
  set lang($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasLang() => $_has(1);
  @$pb.TagNumber(2)
  void clearLang() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get kind => $_getSZ(2);
  @$pb.TagNumber(3)
  set kind($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasKind() => $_has(2);
  @$pb.TagNumber(3)
  void clearKind() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get name => $_getSZ(3);
  @$pb.TagNumber(4)
  set name($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasName() => $_has(3);
  @$pb.TagNumber(4)
  void clearName() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get before => $_getSZ(4);
  @$pb.TagNumber(5)
  set before($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasBefore() => $_has(4);
  @$pb.TagNumber(5)
  void clearBefore() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.String get after => $_getSZ(5);
  @$pb.TagNumber(6)
  set after($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasAfter() => $_has(5);
  @$pb.TagNumber(6)
  void clearAfter() => $_clearField(6);
}

class ReviewSignatureReport extends $pb.GeneratedMessage {
  factory ReviewSignatureReport({
    $core.Iterable<ReviewSignatureChange>? changes,
    $core.int? filesScanned,
    $core.bool? truncated,
  }) {
    final result = ReviewSignatureReport._();
    if (changes != null) result.changes.addAll(changes);
    if (filesScanned != null) result.filesScanned = filesScanned;
    if (truncated != null) result.truncated = truncated;
    return result;
  }

  ReviewSignatureReport._();

  factory ReviewSignatureReport.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewSignatureReport()..mergeFromBuffer(data, registry);
  factory ReviewSignatureReport.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewSignatureReport()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewSignatureReport',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewSignatureReport.$_createMessage)
    ..pPM<ReviewSignatureChange>(1, _omitFieldNames ? '' : 'changes',
        subBuilder: ReviewSignatureChange.$_createMessage)
    ..aI(2, _omitFieldNames ? '' : 'filesScanned',
        fieldType: $pb.PbFieldType.OU3)
    ..aOB(3, _omitFieldNames ? '' : 'truncated')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewSignatureReport clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewSignatureReport copyWith(
          void Function(ReviewSignatureReport) updates) =>
      super.copyWith((message) => updates(message as ReviewSignatureReport))
          as ReviewSignatureReport;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated(
      'Use ReviewSignatureReport() / ReviewSignatureReport.new instead')
  static ReviewSignatureReport create() => ReviewSignatureReport._();
  static $pb.GeneratedMessage $_createMessage() => ReviewSignatureReport._();
  @$core.override
  ReviewSignatureReport createEmptyInstance() => ReviewSignatureReport._();
  @$core.pragma('dart2js:noInline')
  static ReviewSignatureReport getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewSignatureReport>(
          ReviewSignatureReport.$_createMessage);
  static ReviewSignatureReport? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<ReviewSignatureChange> get changes => $_getList(0);

  @$pb.TagNumber(2)
  $core.int get filesScanned => $_getIZ(1);
  @$pb.TagNumber(2)
  set filesScanned($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasFilesScanned() => $_has(1);
  @$pb.TagNumber(2)
  void clearFilesScanned() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.bool get truncated => $_getBF(2);
  @$pb.TagNumber(3)
  set truncated($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasTruncated() => $_has(2);
  @$pb.TagNumber(3)
  void clearTruncated() => $_clearField(3);
}

/// One function/method node in the call graph. Tool-derived + untrusted: `file`
/// is repo-relative + confined, strings bounded.
class ReviewCallGraphNode extends $pb.GeneratedMessage {
  factory ReviewCallGraphNode({
    $core.int? id,
    $core.String? package,
    $core.String? name,
    $core.bool? exported,
    $core.String? file,
    $core.int? line,
    $core.double? centrality,
    $core.int? endLine,
    $core.bool? test,
    $core.bool? generated,
  }) {
    final result = ReviewCallGraphNode._();
    if (id != null) result.id = id;
    if (package != null) result.package = package;
    if (name != null) result.name = name;
    if (exported != null) result.exported = exported;
    if (file != null) result.file = file;
    if (line != null) result.line = line;
    if (centrality != null) result.centrality = centrality;
    if (endLine != null) result.endLine = endLine;
    if (test != null) result.test = test;
    if (generated != null) result.generated = generated;
    return result;
  }

  ReviewCallGraphNode._();

  factory ReviewCallGraphNode.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCallGraphNode()..mergeFromBuffer(data, registry);
  factory ReviewCallGraphNode.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCallGraphNode()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewCallGraphNode',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewCallGraphNode.$_createMessage)
    ..aI(1, _omitFieldNames ? '' : 'id', fieldType: $pb.PbFieldType.OU3)
    ..aOS(2, _omitFieldNames ? '' : 'package')
    ..aOS(3, _omitFieldNames ? '' : 'name')
    ..aOB(4, _omitFieldNames ? '' : 'exported')
    ..aOS(5, _omitFieldNames ? '' : 'file')
    ..aI(6, _omitFieldNames ? '' : 'line', fieldType: $pb.PbFieldType.OU3)
    ..aD(7, _omitFieldNames ? '' : 'centrality')
    ..aI(8, _omitFieldNames ? '' : 'endLine', fieldType: $pb.PbFieldType.OU3)
    ..aOB(9, _omitFieldNames ? '' : 'test')
    ..aOB(10, _omitFieldNames ? '' : 'generated')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCallGraphNode clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCallGraphNode copyWith(void Function(ReviewCallGraphNode) updates) =>
      super.copyWith((message) => updates(message as ReviewCallGraphNode))
          as ReviewCallGraphNode;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core
      .Deprecated('Use ReviewCallGraphNode() / ReviewCallGraphNode.new instead')
  static ReviewCallGraphNode create() => ReviewCallGraphNode._();
  static $pb.GeneratedMessage $_createMessage() => ReviewCallGraphNode._();
  @$core.override
  ReviewCallGraphNode createEmptyInstance() => ReviewCallGraphNode._();
  @$core.pragma('dart2js:noInline')
  static ReviewCallGraphNode getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewCallGraphNode>(
          ReviewCallGraphNode.$_createMessage);
  static ReviewCallGraphNode? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get id => $_getIZ(0);
  @$pb.TagNumber(1)
  set id($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get package => $_getSZ(1);
  @$pb.TagNumber(2)
  set package($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasPackage() => $_has(1);
  @$pb.TagNumber(2)
  void clearPackage() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get name => $_getSZ(2);
  @$pb.TagNumber(3)
  set name($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasName() => $_has(2);
  @$pb.TagNumber(3)
  void clearName() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.bool get exported => $_getBF(3);
  @$pb.TagNumber(4)
  set exported($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasExported() => $_has(3);
  @$pb.TagNumber(4)
  void clearExported() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get file => $_getSZ(4);
  @$pb.TagNumber(5)
  set file($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasFile() => $_has(4);
  @$pb.TagNumber(5)
  void clearFile() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.int get line => $_getIZ(5);
  @$pb.TagNumber(6)
  set line($core.int value) => $_setUnsignedInt32(5, value);
  @$pb.TagNumber(6)
  $core.bool hasLine() => $_has(5);
  @$pb.TagNumber(6)
  void clearLine() => $_clearField(6);

  @$pb.TagNumber(7)
  $core.double get centrality => $_getN(6);
  @$pb.TagNumber(7)
  set centrality($core.double value) => $_setDouble(6, value);
  @$pb.TagNumber(7)
  $core.bool hasCentrality() => $_has(6);
  @$pb.TagNumber(7)
  void clearCentrality() => $_clearField(7);

  /// Last line (0 = unknown): with `line`, the span a diff hunk must overlap.
  @$pb.TagNumber(8)
  $core.int get endLine => $_getIZ(7);
  @$pb.TagNumber(8)
  set endLine($core.int value) => $_setUnsignedInt32(7, value);
  @$pb.TagNumber(8)
  $core.bool hasEndLine() => $_has(7);
  @$pb.TagNumber(8)
  void clearEndLine() => $_clearField(8);

  /// In a `_test.go` file.
  @$pb.TagNumber(9)
  $core.bool get test => $_getBF(8);
  @$pb.TagNumber(9)
  set test($core.bool value) => $_setBool(8, value);
  @$pb.TagNumber(9)
  $core.bool hasTest() => $_has(8);
  @$pb.TagNumber(9)
  void clearTest() => $_clearField(9);

  /// In a generated file (`// Code generated … DO NOT EDIT.` before `package`).
  @$pb.TagNumber(10)
  $core.bool get generated => $_getBF(9);
  @$pb.TagNumber(10)
  set generated($core.bool value) => $_setBool(9, value);
  @$pb.TagNumber(10)
  $core.bool hasGenerated() => $_has(9);
  @$pb.TagNumber(10)
  void clearGenerated() => $_clearField(10);
}

class ReviewCallEdge extends $pb.GeneratedMessage {
  factory ReviewCallEdge({
    $core.int? callerId,
    $core.int? calleeId,
  }) {
    final result = ReviewCallEdge._();
    if (callerId != null) result.callerId = callerId;
    if (calleeId != null) result.calleeId = calleeId;
    return result;
  }

  ReviewCallEdge._();

  factory ReviewCallEdge.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCallEdge()..mergeFromBuffer(data, registry);
  factory ReviewCallEdge.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCallEdge()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewCallEdge',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewCallEdge.$_createMessage)
    ..aI(1, _omitFieldNames ? '' : 'callerId', fieldType: $pb.PbFieldType.OU3)
    ..aI(2, _omitFieldNames ? '' : 'calleeId', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCallEdge clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCallEdge copyWith(void Function(ReviewCallEdge) updates) =>
      super.copyWith((message) => updates(message as ReviewCallEdge))
          as ReviewCallEdge;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewCallEdge() / ReviewCallEdge.new instead')
  static ReviewCallEdge create() => ReviewCallEdge._();
  static $pb.GeneratedMessage $_createMessage() => ReviewCallEdge._();
  @$core.override
  ReviewCallEdge createEmptyInstance() => ReviewCallEdge._();
  @$core.pragma('dart2js:noInline')
  static ReviewCallEdge getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewCallEdge>(
          ReviewCallEdge.$_createMessage);
  static ReviewCallEdge? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get callerId => $_getIZ(0);
  @$pb.TagNumber(1)
  set callerId($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasCallerId() => $_has(0);
  @$pb.TagNumber(1)
  void clearCallerId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get calleeId => $_getIZ(1);
  @$pb.TagNumber(2)
  set calleeId($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCalleeId() => $_has(1);
  @$pb.TagNumber(2)
  void clearCalleeId() => $_clearField(2);
}

class ReviewPackageShape extends $pb.GeneratedMessage {
  factory ReviewPackageShape({
    $core.String? package,
    $core.int? files,
    $core.int? exportedFns,
    $core.int? types,
  }) {
    final result = ReviewPackageShape._();
    if (package != null) result.package = package;
    if (files != null) result.files = files;
    if (exportedFns != null) result.exportedFns = exportedFns;
    if (types != null) result.types = types;
    return result;
  }

  ReviewPackageShape._();

  factory ReviewPackageShape.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewPackageShape()..mergeFromBuffer(data, registry);
  factory ReviewPackageShape.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewPackageShape()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewPackageShape',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewPackageShape.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'package')
    ..aI(2, _omitFieldNames ? '' : 'files', fieldType: $pb.PbFieldType.OU3)
    ..aI(3, _omitFieldNames ? '' : 'exportedFns',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(4, _omitFieldNames ? '' : 'types', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewPackageShape clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewPackageShape copyWith(void Function(ReviewPackageShape) updates) =>
      super.copyWith((message) => updates(message as ReviewPackageShape))
          as ReviewPackageShape;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewPackageShape() / ReviewPackageShape.new instead')
  static ReviewPackageShape create() => ReviewPackageShape._();
  static $pb.GeneratedMessage $_createMessage() => ReviewPackageShape._();
  @$core.override
  ReviewPackageShape createEmptyInstance() => ReviewPackageShape._();
  @$core.pragma('dart2js:noInline')
  static ReviewPackageShape getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewPackageShape>(
          ReviewPackageShape.$_createMessage);
  static ReviewPackageShape? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get package => $_getSZ(0);
  @$pb.TagNumber(1)
  set package($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPackage() => $_has(0);
  @$pb.TagNumber(1)
  void clearPackage() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get files => $_getIZ(1);
  @$pb.TagNumber(2)
  set files($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasFiles() => $_has(1);
  @$pb.TagNumber(2)
  void clearFiles() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get exportedFns => $_getIZ(2);
  @$pb.TagNumber(3)
  set exportedFns($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasExportedFns() => $_has(2);
  @$pb.TagNumber(3)
  void clearExportedFns() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get types => $_getIZ(3);
  @$pb.TagNumber(4)
  set types($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasTypes() => $_has(3);
  @$pb.TagNumber(4)
  void clearTypes() => $_clearField(4);
}

class ReviewCallGraph extends $pb.GeneratedMessage {
  factory ReviewCallGraph({
    $core.Iterable<ReviewCallGraphNode>? nodes,
    $core.Iterable<ReviewCallEdge>? edges,
    $core.Iterable<$core.int>? changedFns,
    $core.Iterable<ReviewPackageShape>? packages,
    $core.bool? truncated,
    $core.int? changedGenerated,
  }) {
    final result = ReviewCallGraph._();
    if (nodes != null) result.nodes.addAll(nodes);
    if (edges != null) result.edges.addAll(edges);
    if (changedFns != null) result.changedFns.addAll(changedFns);
    if (packages != null) result.packages.addAll(packages);
    if (truncated != null) result.truncated = truncated;
    if (changedGenerated != null) result.changedGenerated = changedGenerated;
    return result;
  }

  ReviewCallGraph._();

  factory ReviewCallGraph.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCallGraph()..mergeFromBuffer(data, registry);
  factory ReviewCallGraph.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCallGraph()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewCallGraph',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewCallGraph.$_createMessage)
    ..pPM<ReviewCallGraphNode>(1, _omitFieldNames ? '' : 'nodes',
        subBuilder: ReviewCallGraphNode.$_createMessage)
    ..pPM<ReviewCallEdge>(2, _omitFieldNames ? '' : 'edges',
        subBuilder: ReviewCallEdge.$_createMessage)
    ..p<$core.int>(3, _omitFieldNames ? '' : 'changedFns', $pb.PbFieldType.KU3)
    ..pPM<ReviewPackageShape>(4, _omitFieldNames ? '' : 'packages',
        subBuilder: ReviewPackageShape.$_createMessage)
    ..aOB(5, _omitFieldNames ? '' : 'truncated')
    ..aI(6, _omitFieldNames ? '' : 'changedGenerated',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCallGraph clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCallGraph copyWith(void Function(ReviewCallGraph) updates) =>
      super.copyWith((message) => updates(message as ReviewCallGraph))
          as ReviewCallGraph;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewCallGraph() / ReviewCallGraph.new instead')
  static ReviewCallGraph create() => ReviewCallGraph._();
  static $pb.GeneratedMessage $_createMessage() => ReviewCallGraph._();
  @$core.override
  ReviewCallGraph createEmptyInstance() => ReviewCallGraph._();
  @$core.pragma('dart2js:noInline')
  static ReviewCallGraph getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewCallGraph>(
          ReviewCallGraph.$_createMessage);
  static ReviewCallGraph? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<ReviewCallGraphNode> get nodes => $_getList(0);

  @$pb.TagNumber(2)
  $pb.PbList<ReviewCallEdge> get edges => $_getList(1);

  @$pb.TagNumber(3)
  $pb.PbList<$core.int> get changedFns => $_getList(2);

  @$pb.TagNumber(4)
  $pb.PbList<ReviewPackageShape> get packages => $_getList(3);

  @$pb.TagNumber(5)
  $core.bool get truncated => $_getBF(4);
  @$pb.TagNumber(5)
  set truncated($core.bool value) => $_setBool(4, value);
  @$pb.TagNumber(5)
  $core.bool hasTruncated() => $_has(4);
  @$pb.TagNumber(5)
  void clearTruncated() => $_clearField(5);

  /// Generated functions a diff hunk touched, left out of `changed_fns`.
  @$pb.TagNumber(6)
  $core.int get changedGenerated => $_getIZ(5);
  @$pb.TagNumber(6)
  set changedGenerated($core.int value) => $_setUnsignedInt32(5, value);
  @$pb.TagNumber(6)
  $core.bool hasChangedGenerated() => $_has(5);
  @$pb.TagNumber(6)
  void clearChangedGenerated() => $_clearField(6);
}

/// Naming-convention verdicts (case-style strings, not identifier lists).
class ReviewNamingFacts extends $pb.GeneratedMessage {
  factory ReviewNamingFacts({
    $core.String? functions,
    $core.String? variables,
    $core.String? constants,
    $core.double? exportedRatio,
  }) {
    final result = ReviewNamingFacts._();
    if (functions != null) result.functions = functions;
    if (variables != null) result.variables = variables;
    if (constants != null) result.constants = constants;
    if (exportedRatio != null) result.exportedRatio = exportedRatio;
    return result;
  }

  ReviewNamingFacts._();

  factory ReviewNamingFacts.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewNamingFacts()..mergeFromBuffer(data, registry);
  factory ReviewNamingFacts.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewNamingFacts()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewNamingFacts',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewNamingFacts.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'functions')
    ..aOS(2, _omitFieldNames ? '' : 'variables')
    ..aOS(3, _omitFieldNames ? '' : 'constants')
    ..aD(4, _omitFieldNames ? '' : 'exportedRatio',
        fieldType: $pb.PbFieldType.OF)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewNamingFacts clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewNamingFacts copyWith(void Function(ReviewNamingFacts) updates) =>
      super.copyWith((message) => updates(message as ReviewNamingFacts))
          as ReviewNamingFacts;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewNamingFacts() / ReviewNamingFacts.new instead')
  static ReviewNamingFacts create() => ReviewNamingFacts._();
  static $pb.GeneratedMessage $_createMessage() => ReviewNamingFacts._();
  @$core.override
  ReviewNamingFacts createEmptyInstance() => ReviewNamingFacts._();
  @$core.pragma('dart2js:noInline')
  static ReviewNamingFacts getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewNamingFacts>(
          ReviewNamingFacts.$_createMessage);
  static ReviewNamingFacts? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get functions => $_getSZ(0);
  @$pb.TagNumber(1)
  set functions($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasFunctions() => $_has(0);
  @$pb.TagNumber(1)
  void clearFunctions() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get variables => $_getSZ(1);
  @$pb.TagNumber(2)
  set variables($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasVariables() => $_has(1);
  @$pb.TagNumber(2)
  void clearVariables() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get constants => $_getSZ(2);
  @$pb.TagNumber(3)
  set constants($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasConstants() => $_has(2);
  @$pb.TagNumber(3)
  void clearConstants() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.double get exportedRatio => $_getN(3);
  @$pb.TagNumber(4)
  set exportedRatio($core.double value) => $_setFloat(3, value);
  @$pb.TagNumber(4)
  $core.bool hasExportedRatio() => $_has(3);
  @$pb.TagNumber(4)
  void clearExportedRatio() => $_clearField(4);
}

class ReviewCommitStyleFacts extends $pb.GeneratedMessage {
  factory ReviewCommitStyleFacts({
    $core.double? conventionalRatio,
    $core.int? subjectLenP50,
    $core.int? subjectLenP95,
    $core.double? bodyPresentRatio,
    $core.int? sampledCommits,
  }) {
    final result = ReviewCommitStyleFacts._();
    if (conventionalRatio != null) result.conventionalRatio = conventionalRatio;
    if (subjectLenP50 != null) result.subjectLenP50 = subjectLenP50;
    if (subjectLenP95 != null) result.subjectLenP95 = subjectLenP95;
    if (bodyPresentRatio != null) result.bodyPresentRatio = bodyPresentRatio;
    if (sampledCommits != null) result.sampledCommits = sampledCommits;
    return result;
  }

  ReviewCommitStyleFacts._();

  factory ReviewCommitStyleFacts.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCommitStyleFacts()..mergeFromBuffer(data, registry);
  factory ReviewCommitStyleFacts.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCommitStyleFacts()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewCommitStyleFacts',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewCommitStyleFacts.$_createMessage)
    ..aD(1, _omitFieldNames ? '' : 'conventionalRatio',
        fieldType: $pb.PbFieldType.OF)
    ..aI(2, _omitFieldNames ? '' : 'subjectLenP50',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(3, _omitFieldNames ? '' : 'subjectLenP95',
        fieldType: $pb.PbFieldType.OU3)
    ..aD(4, _omitFieldNames ? '' : 'bodyPresentRatio',
        fieldType: $pb.PbFieldType.OF)
    ..aI(5, _omitFieldNames ? '' : 'sampledCommits',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCommitStyleFacts clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCommitStyleFacts copyWith(
          void Function(ReviewCommitStyleFacts) updates) =>
      super.copyWith((message) => updates(message as ReviewCommitStyleFacts))
          as ReviewCommitStyleFacts;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated(
      'Use ReviewCommitStyleFacts() / ReviewCommitStyleFacts.new instead')
  static ReviewCommitStyleFacts create() => ReviewCommitStyleFacts._();
  static $pb.GeneratedMessage $_createMessage() => ReviewCommitStyleFacts._();
  @$core.override
  ReviewCommitStyleFacts createEmptyInstance() => ReviewCommitStyleFacts._();
  @$core.pragma('dart2js:noInline')
  static ReviewCommitStyleFacts getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewCommitStyleFacts>(
          ReviewCommitStyleFacts.$_createMessage);
  static ReviewCommitStyleFacts? _defaultInstance;

  @$pb.TagNumber(1)
  $core.double get conventionalRatio => $_getN(0);
  @$pb.TagNumber(1)
  set conventionalRatio($core.double value) => $_setFloat(0, value);
  @$pb.TagNumber(1)
  $core.bool hasConventionalRatio() => $_has(0);
  @$pb.TagNumber(1)
  void clearConventionalRatio() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get subjectLenP50 => $_getIZ(1);
  @$pb.TagNumber(2)
  set subjectLenP50($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasSubjectLenP50() => $_has(1);
  @$pb.TagNumber(2)
  void clearSubjectLenP50() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get subjectLenP95 => $_getIZ(2);
  @$pb.TagNumber(3)
  set subjectLenP95($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasSubjectLenP95() => $_has(2);
  @$pb.TagNumber(3)
  void clearSubjectLenP95() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.double get bodyPresentRatio => $_getN(3);
  @$pb.TagNumber(4)
  set bodyPresentRatio($core.double value) => $_setFloat(3, value);
  @$pb.TagNumber(4)
  $core.bool hasBodyPresentRatio() => $_has(3);
  @$pb.TagNumber(4)
  void clearBodyPresentRatio() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.int get sampledCommits => $_getIZ(4);
  @$pb.TagNumber(5)
  set sampledCommits($core.int value) => $_setUnsignedInt32(4, value);
  @$pb.TagNumber(5)
  $core.bool hasSampledCommits() => $_has(4);
  @$pb.TagNumber(5)
  void clearSampledCommits() => $_clearField(5);
}

/// A deterministic house-style fingerprint — distributions/ratios only, no source
/// or identifiers. `files_scanned == 0` ⇒ the collector did not run.
class ReviewStyleFacts extends $pb.GeneratedMessage {
  factory ReviewStyleFacts({
    $core.double? commentDensity,
    $core.double? doccommentRatio,
    $core.bool? indentTabs,
    $core.int? lineLenP95,
    $core.int? fnLenMedian,
    ReviewNamingFacts? naming,
    ReviewCommitStyleFacts? commits,
    $core.bool? diffMatchesStyle,
    $core.int? filesScanned,
  }) {
    final result = ReviewStyleFacts._();
    if (commentDensity != null) result.commentDensity = commentDensity;
    if (doccommentRatio != null) result.doccommentRatio = doccommentRatio;
    if (indentTabs != null) result.indentTabs = indentTabs;
    if (lineLenP95 != null) result.lineLenP95 = lineLenP95;
    if (fnLenMedian != null) result.fnLenMedian = fnLenMedian;
    if (naming != null) result.naming = naming;
    if (commits != null) result.commits = commits;
    if (diffMatchesStyle != null) result.diffMatchesStyle = diffMatchesStyle;
    if (filesScanned != null) result.filesScanned = filesScanned;
    return result;
  }

  ReviewStyleFacts._();

  factory ReviewStyleFacts.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewStyleFacts()..mergeFromBuffer(data, registry);
  factory ReviewStyleFacts.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewStyleFacts()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewStyleFacts',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewStyleFacts.$_createMessage)
    ..aD(1, _omitFieldNames ? '' : 'commentDensity',
        fieldType: $pb.PbFieldType.OF)
    ..aD(2, _omitFieldNames ? '' : 'doccommentRatio',
        fieldType: $pb.PbFieldType.OF)
    ..aOB(3, _omitFieldNames ? '' : 'indentTabs')
    ..aI(4, _omitFieldNames ? '' : 'lineLenP95', fieldType: $pb.PbFieldType.OU3)
    ..aI(5, _omitFieldNames ? '' : 'fnLenMedian',
        fieldType: $pb.PbFieldType.OU3)
    ..aOM<ReviewNamingFacts>(6, _omitFieldNames ? '' : 'naming',
        subBuilder: ReviewNamingFacts.$_createMessage)
    ..aOM<ReviewCommitStyleFacts>(7, _omitFieldNames ? '' : 'commits',
        subBuilder: ReviewCommitStyleFacts.$_createMessage)
    ..aOB(8, _omitFieldNames ? '' : 'diffMatchesStyle')
    ..aI(9, _omitFieldNames ? '' : 'filesScanned',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewStyleFacts clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewStyleFacts copyWith(void Function(ReviewStyleFacts) updates) =>
      super.copyWith((message) => updates(message as ReviewStyleFacts))
          as ReviewStyleFacts;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewStyleFacts() / ReviewStyleFacts.new instead')
  static ReviewStyleFacts create() => ReviewStyleFacts._();
  static $pb.GeneratedMessage $_createMessage() => ReviewStyleFacts._();
  @$core.override
  ReviewStyleFacts createEmptyInstance() => ReviewStyleFacts._();
  @$core.pragma('dart2js:noInline')
  static ReviewStyleFacts getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewStyleFacts>(
          ReviewStyleFacts.$_createMessage);
  static ReviewStyleFacts? _defaultInstance;

  @$pb.TagNumber(1)
  $core.double get commentDensity => $_getN(0);
  @$pb.TagNumber(1)
  set commentDensity($core.double value) => $_setFloat(0, value);
  @$pb.TagNumber(1)
  $core.bool hasCommentDensity() => $_has(0);
  @$pb.TagNumber(1)
  void clearCommentDensity() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.double get doccommentRatio => $_getN(1);
  @$pb.TagNumber(2)
  set doccommentRatio($core.double value) => $_setFloat(1, value);
  @$pb.TagNumber(2)
  $core.bool hasDoccommentRatio() => $_has(1);
  @$pb.TagNumber(2)
  void clearDoccommentRatio() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.bool get indentTabs => $_getBF(2);
  @$pb.TagNumber(3)
  set indentTabs($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasIndentTabs() => $_has(2);
  @$pb.TagNumber(3)
  void clearIndentTabs() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get lineLenP95 => $_getIZ(3);
  @$pb.TagNumber(4)
  set lineLenP95($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasLineLenP95() => $_has(3);
  @$pb.TagNumber(4)
  void clearLineLenP95() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.int get fnLenMedian => $_getIZ(4);
  @$pb.TagNumber(5)
  set fnLenMedian($core.int value) => $_setUnsignedInt32(4, value);
  @$pb.TagNumber(5)
  $core.bool hasFnLenMedian() => $_has(4);
  @$pb.TagNumber(5)
  void clearFnLenMedian() => $_clearField(5);

  @$pb.TagNumber(6)
  ReviewNamingFacts get naming => $_getN(5);
  @$pb.TagNumber(6)
  set naming(ReviewNamingFacts value) => $_setField(6, value);
  @$pb.TagNumber(6)
  $core.bool hasNaming() => $_has(5);
  @$pb.TagNumber(6)
  void clearNaming() => $_clearField(6);
  @$pb.TagNumber(6)
  ReviewNamingFacts ensureNaming() => $_ensure(5);

  @$pb.TagNumber(7)
  ReviewCommitStyleFacts get commits => $_getN(6);
  @$pb.TagNumber(7)
  set commits(ReviewCommitStyleFacts value) => $_setField(7, value);
  @$pb.TagNumber(7)
  $core.bool hasCommits() => $_has(6);
  @$pb.TagNumber(7)
  void clearCommits() => $_clearField(7);
  @$pb.TagNumber(7)
  ReviewCommitStyleFacts ensureCommits() => $_ensure(6);

  @$pb.TagNumber(8)
  $core.bool get diffMatchesStyle => $_getBF(7);
  @$pb.TagNumber(8)
  set diffMatchesStyle($core.bool value) => $_setBool(7, value);
  @$pb.TagNumber(8)
  $core.bool hasDiffMatchesStyle() => $_has(7);
  @$pb.TagNumber(8)
  void clearDiffMatchesStyle() => $_clearField(8);

  @$pb.TagNumber(9)
  $core.int get filesScanned => $_getIZ(8);
  @$pb.TagNumber(9)
  set filesScanned($core.int value) => $_setUnsignedInt32(8, value);
  @$pb.TagNumber(9)
  $core.bool hasFilesScanned() => $_has(8);
  @$pb.TagNumber(9)
  void clearFilesScanned() => $_clearField(9);
}

/// One cheap-LLM summary of a changed function — SOFT (model-generated), bounded.
class ReviewFunctionSummary extends $pb.GeneratedMessage {
  factory ReviewFunctionSummary({
    $core.String? name,
    $core.String? file,
    $core.String? kind,
    $core.String? summary,
    $core.String? model,
    $core.int? durationMs,
  }) {
    final result = ReviewFunctionSummary._();
    if (name != null) result.name = name;
    if (file != null) result.file = file;
    if (kind != null) result.kind = kind;
    if (summary != null) result.summary = summary;
    if (model != null) result.model = model;
    if (durationMs != null) result.durationMs = durationMs;
    return result;
  }

  ReviewFunctionSummary._();

  factory ReviewFunctionSummary.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewFunctionSummary()..mergeFromBuffer(data, registry);
  factory ReviewFunctionSummary.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewFunctionSummary()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewFunctionSummary',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewFunctionSummary.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'name')
    ..aOS(2, _omitFieldNames ? '' : 'file')
    ..aOS(3, _omitFieldNames ? '' : 'kind')
    ..aOS(4, _omitFieldNames ? '' : 'summary')
    ..aOS(5, _omitFieldNames ? '' : 'model')
    ..aI(6, _omitFieldNames ? '' : 'durationMs', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewFunctionSummary clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewFunctionSummary copyWith(
          void Function(ReviewFunctionSummary) updates) =>
      super.copyWith((message) => updates(message as ReviewFunctionSummary))
          as ReviewFunctionSummary;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated(
      'Use ReviewFunctionSummary() / ReviewFunctionSummary.new instead')
  static ReviewFunctionSummary create() => ReviewFunctionSummary._();
  static $pb.GeneratedMessage $_createMessage() => ReviewFunctionSummary._();
  @$core.override
  ReviewFunctionSummary createEmptyInstance() => ReviewFunctionSummary._();
  @$core.pragma('dart2js:noInline')
  static ReviewFunctionSummary getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewFunctionSummary>(
          ReviewFunctionSummary.$_createMessage);
  static ReviewFunctionSummary? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get name => $_getSZ(0);
  @$pb.TagNumber(1)
  set name($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasName() => $_has(0);
  @$pb.TagNumber(1)
  void clearName() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get file => $_getSZ(1);
  @$pb.TagNumber(2)
  set file($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasFile() => $_has(1);
  @$pb.TagNumber(2)
  void clearFile() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get kind => $_getSZ(2);
  @$pb.TagNumber(3)
  set kind($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasKind() => $_has(2);
  @$pb.TagNumber(3)
  void clearKind() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get summary => $_getSZ(3);
  @$pb.TagNumber(4)
  set summary($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasSummary() => $_has(3);
  @$pb.TagNumber(4)
  void clearSummary() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get model => $_getSZ(4);
  @$pb.TagNumber(5)
  set model($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasModel() => $_has(4);
  @$pb.TagNumber(5)
  void clearModel() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.int get durationMs => $_getIZ(5);
  @$pb.TagNumber(6)
  set durationMs($core.int value) => $_setUnsignedInt32(5, value);
  @$pb.TagNumber(6)
  $core.bool hasDurationMs() => $_has(5);
  @$pb.TagNumber(6)
  void clearDurationMs() => $_clearField(6);
}

class ReviewSummaryReport extends $pb.GeneratedMessage {
  factory ReviewSummaryReport({
    $core.Iterable<ReviewFunctionSummary>? summaries,
    $core.int? requested,
    $core.int? produced,
    $core.int? omitted,
  }) {
    final result = ReviewSummaryReport._();
    if (summaries != null) result.summaries.addAll(summaries);
    if (requested != null) result.requested = requested;
    if (produced != null) result.produced = produced;
    if (omitted != null) result.omitted = omitted;
    return result;
  }

  ReviewSummaryReport._();

  factory ReviewSummaryReport.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewSummaryReport()..mergeFromBuffer(data, registry);
  factory ReviewSummaryReport.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewSummaryReport()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewSummaryReport',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewSummaryReport.$_createMessage)
    ..pPM<ReviewFunctionSummary>(1, _omitFieldNames ? '' : 'summaries',
        subBuilder: ReviewFunctionSummary.$_createMessage)
    ..aI(2, _omitFieldNames ? '' : 'requested', fieldType: $pb.PbFieldType.OU3)
    ..aI(3, _omitFieldNames ? '' : 'produced', fieldType: $pb.PbFieldType.OU3)
    ..aI(4, _omitFieldNames ? '' : 'omitted', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewSummaryReport clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewSummaryReport copyWith(void Function(ReviewSummaryReport) updates) =>
      super.copyWith((message) => updates(message as ReviewSummaryReport))
          as ReviewSummaryReport;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core
      .Deprecated('Use ReviewSummaryReport() / ReviewSummaryReport.new instead')
  static ReviewSummaryReport create() => ReviewSummaryReport._();
  static $pb.GeneratedMessage $_createMessage() => ReviewSummaryReport._();
  @$core.override
  ReviewSummaryReport createEmptyInstance() => ReviewSummaryReport._();
  @$core.pragma('dart2js:noInline')
  static ReviewSummaryReport getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewSummaryReport>(
          ReviewSummaryReport.$_createMessage);
  static ReviewSummaryReport? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<ReviewFunctionSummary> get summaries => $_getList(0);

  @$pb.TagNumber(2)
  $core.int get requested => $_getIZ(1);
  @$pb.TagNumber(2)
  set requested($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasRequested() => $_has(1);
  @$pb.TagNumber(2)
  void clearRequested() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get produced => $_getIZ(2);
  @$pb.TagNumber(3)
  set produced($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasProduced() => $_has(2);
  @$pb.TagNumber(3)
  void clearProduced() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get omitted => $_getIZ(3);
  @$pb.TagNumber(4)
  set omitted($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasOmitted() => $_has(3);
  @$pb.TagNumber(4)
  void clearOmitted() => $_clearField(4);
}

class ReviewCoChangePartner extends $pb.GeneratedMessage {
  factory ReviewCoChangePartner({
    $core.String? path,
    $core.double? confidence,
    $core.int? coOccurrences,
    $core.bool? inDiff,
  }) {
    final result = ReviewCoChangePartner._();
    if (path != null) result.path = path;
    if (confidence != null) result.confidence = confidence;
    if (coOccurrences != null) result.coOccurrences = coOccurrences;
    if (inDiff != null) result.inDiff = inDiff;
    return result;
  }

  ReviewCoChangePartner._();

  factory ReviewCoChangePartner.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCoChangePartner()..mergeFromBuffer(data, registry);
  factory ReviewCoChangePartner.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCoChangePartner()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewCoChangePartner',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewCoChangePartner.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'path')
    ..aD(2, _omitFieldNames ? '' : 'confidence')
    ..aI(3, _omitFieldNames ? '' : 'coOccurrences',
        fieldType: $pb.PbFieldType.OU3)
    ..aOB(4, _omitFieldNames ? '' : 'inDiff')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCoChangePartner clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCoChangePartner copyWith(
          void Function(ReviewCoChangePartner) updates) =>
      super.copyWith((message) => updates(message as ReviewCoChangePartner))
          as ReviewCoChangePartner;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated(
      'Use ReviewCoChangePartner() / ReviewCoChangePartner.new instead')
  static ReviewCoChangePartner create() => ReviewCoChangePartner._();
  static $pb.GeneratedMessage $_createMessage() => ReviewCoChangePartner._();
  @$core.override
  ReviewCoChangePartner createEmptyInstance() => ReviewCoChangePartner._();
  @$core.pragma('dart2js:noInline')
  static ReviewCoChangePartner getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewCoChangePartner>(
          ReviewCoChangePartner.$_createMessage);
  static ReviewCoChangePartner? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get path => $_getSZ(0);
  @$pb.TagNumber(1)
  set path($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPath() => $_has(0);
  @$pb.TagNumber(1)
  void clearPath() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.double get confidence => $_getN(1);
  @$pb.TagNumber(2)
  set confidence($core.double value) => $_setDouble(1, value);
  @$pb.TagNumber(2)
  $core.bool hasConfidence() => $_has(1);
  @$pb.TagNumber(2)
  void clearConfidence() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get coOccurrences => $_getIZ(2);
  @$pb.TagNumber(3)
  set coOccurrences($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasCoOccurrences() => $_has(2);
  @$pb.TagNumber(3)
  void clearCoOccurrences() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.bool get inDiff => $_getBF(3);
  @$pb.TagNumber(4)
  set inDiff($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasInDiff() => $_has(3);
  @$pb.TagNumber(4)
  void clearInDiff() => $_clearField(4);
}

class ReviewCoChangeEntry extends $pb.GeneratedMessage {
  factory ReviewCoChangeEntry({
    $core.String? path,
    $core.Iterable<ReviewCoChangePartner>? partners,
  }) {
    final result = ReviewCoChangeEntry._();
    if (path != null) result.path = path;
    if (partners != null) result.partners.addAll(partners);
    return result;
  }

  ReviewCoChangeEntry._();

  factory ReviewCoChangeEntry.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCoChangeEntry()..mergeFromBuffer(data, registry);
  factory ReviewCoChangeEntry.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCoChangeEntry()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewCoChangeEntry',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewCoChangeEntry.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'path')
    ..pPM<ReviewCoChangePartner>(2, _omitFieldNames ? '' : 'partners',
        subBuilder: ReviewCoChangePartner.$_createMessage)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCoChangeEntry clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCoChangeEntry copyWith(void Function(ReviewCoChangeEntry) updates) =>
      super.copyWith((message) => updates(message as ReviewCoChangeEntry))
          as ReviewCoChangeEntry;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core
      .Deprecated('Use ReviewCoChangeEntry() / ReviewCoChangeEntry.new instead')
  static ReviewCoChangeEntry create() => ReviewCoChangeEntry._();
  static $pb.GeneratedMessage $_createMessage() => ReviewCoChangeEntry._();
  @$core.override
  ReviewCoChangeEntry createEmptyInstance() => ReviewCoChangeEntry._();
  @$core.pragma('dart2js:noInline')
  static ReviewCoChangeEntry getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewCoChangeEntry>(
          ReviewCoChangeEntry.$_createMessage);
  static ReviewCoChangeEntry? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get path => $_getSZ(0);
  @$pb.TagNumber(1)
  set path($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPath() => $_has(0);
  @$pb.TagNumber(1)
  void clearPath() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<ReviewCoChangePartner> get partners => $_getList(1);
}

class ReviewCoChangeReport extends $pb.GeneratedMessage {
  factory ReviewCoChangeReport({
    $core.int? commitsScanned,
    $core.bool? truncated,
    $core.Iterable<ReviewCoChangeEntry>? entries,
    $core.int? missingPartners,
  }) {
    final result = ReviewCoChangeReport._();
    if (commitsScanned != null) result.commitsScanned = commitsScanned;
    if (truncated != null) result.truncated = truncated;
    if (entries != null) result.entries.addAll(entries);
    if (missingPartners != null) result.missingPartners = missingPartners;
    return result;
  }

  ReviewCoChangeReport._();

  factory ReviewCoChangeReport.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCoChangeReport()..mergeFromBuffer(data, registry);
  factory ReviewCoChangeReport.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewCoChangeReport()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewCoChangeReport',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewCoChangeReport.$_createMessage)
    ..aI(1, _omitFieldNames ? '' : 'commitsScanned',
        fieldType: $pb.PbFieldType.OU3)
    ..aOB(2, _omitFieldNames ? '' : 'truncated')
    ..pPM<ReviewCoChangeEntry>(3, _omitFieldNames ? '' : 'entries',
        subBuilder: ReviewCoChangeEntry.$_createMessage)
    ..aI(4, _omitFieldNames ? '' : 'missingPartners',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCoChangeReport clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewCoChangeReport copyWith(void Function(ReviewCoChangeReport) updates) =>
      super.copyWith((message) => updates(message as ReviewCoChangeReport))
          as ReviewCoChangeReport;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated(
      'Use ReviewCoChangeReport() / ReviewCoChangeReport.new instead')
  static ReviewCoChangeReport create() => ReviewCoChangeReport._();
  static $pb.GeneratedMessage $_createMessage() => ReviewCoChangeReport._();
  @$core.override
  ReviewCoChangeReport createEmptyInstance() => ReviewCoChangeReport._();
  @$core.pragma('dart2js:noInline')
  static ReviewCoChangeReport getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewCoChangeReport>(
          ReviewCoChangeReport.$_createMessage);
  static ReviewCoChangeReport? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get commitsScanned => $_getIZ(0);
  @$pb.TagNumber(1)
  set commitsScanned($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasCommitsScanned() => $_has(0);
  @$pb.TagNumber(1)
  void clearCommitsScanned() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.bool get truncated => $_getBF(1);
  @$pb.TagNumber(2)
  set truncated($core.bool value) => $_setBool(1, value);
  @$pb.TagNumber(2)
  $core.bool hasTruncated() => $_has(1);
  @$pb.TagNumber(2)
  void clearTruncated() => $_clearField(2);

  @$pb.TagNumber(3)
  $pb.PbList<ReviewCoChangeEntry> get entries => $_getList(2);

  @$pb.TagNumber(4)
  $core.int get missingPartners => $_getIZ(3);
  @$pb.TagNumber(4)
  set missingPartners($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasMissingPartners() => $_has(3);
  @$pb.TagNumber(4)
  void clearMissingPartners() => $_clearField(4);
}

class ReviewFileChurn extends $pb.GeneratedMessage {
  factory ReviewFileChurn({
    $core.String? path,
    $core.int? commits,
    $core.int? uniqueAuthors,
    $core.int? busFactor,
    $core.double? topAuthorShare,
    $core.String? churnTrend,
    $core.double? churnSlope,
    $fixnum.Int64? totalChurn,
  }) {
    final result = ReviewFileChurn._();
    if (path != null) result.path = path;
    if (commits != null) result.commits = commits;
    if (uniqueAuthors != null) result.uniqueAuthors = uniqueAuthors;
    if (busFactor != null) result.busFactor = busFactor;
    if (topAuthorShare != null) result.topAuthorShare = topAuthorShare;
    if (churnTrend != null) result.churnTrend = churnTrend;
    if (churnSlope != null) result.churnSlope = churnSlope;
    if (totalChurn != null) result.totalChurn = totalChurn;
    return result;
  }

  ReviewFileChurn._();

  factory ReviewFileChurn.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewFileChurn()..mergeFromBuffer(data, registry);
  factory ReviewFileChurn.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewFileChurn()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewFileChurn',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewFileChurn.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'path')
    ..aI(2, _omitFieldNames ? '' : 'commits', fieldType: $pb.PbFieldType.OU3)
    ..aI(3, _omitFieldNames ? '' : 'uniqueAuthors',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(4, _omitFieldNames ? '' : 'busFactor', fieldType: $pb.PbFieldType.OU3)
    ..aD(5, _omitFieldNames ? '' : 'topAuthorShare')
    ..aOS(6, _omitFieldNames ? '' : 'churnTrend')
    ..aD(7, _omitFieldNames ? '' : 'churnSlope')
    ..a<$fixnum.Int64>(
        8, _omitFieldNames ? '' : 'totalChurn', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewFileChurn clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewFileChurn copyWith(void Function(ReviewFileChurn) updates) =>
      super.copyWith((message) => updates(message as ReviewFileChurn))
          as ReviewFileChurn;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewFileChurn() / ReviewFileChurn.new instead')
  static ReviewFileChurn create() => ReviewFileChurn._();
  static $pb.GeneratedMessage $_createMessage() => ReviewFileChurn._();
  @$core.override
  ReviewFileChurn createEmptyInstance() => ReviewFileChurn._();
  @$core.pragma('dart2js:noInline')
  static ReviewFileChurn getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewFileChurn>(
          ReviewFileChurn.$_createMessage);
  static ReviewFileChurn? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get path => $_getSZ(0);
  @$pb.TagNumber(1)
  set path($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPath() => $_has(0);
  @$pb.TagNumber(1)
  void clearPath() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get commits => $_getIZ(1);
  @$pb.TagNumber(2)
  set commits($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCommits() => $_has(1);
  @$pb.TagNumber(2)
  void clearCommits() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get uniqueAuthors => $_getIZ(2);
  @$pb.TagNumber(3)
  set uniqueAuthors($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasUniqueAuthors() => $_has(2);
  @$pb.TagNumber(3)
  void clearUniqueAuthors() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get busFactor => $_getIZ(3);
  @$pb.TagNumber(4)
  set busFactor($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasBusFactor() => $_has(3);
  @$pb.TagNumber(4)
  void clearBusFactor() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.double get topAuthorShare => $_getN(4);
  @$pb.TagNumber(5)
  set topAuthorShare($core.double value) => $_setDouble(4, value);
  @$pb.TagNumber(5)
  $core.bool hasTopAuthorShare() => $_has(4);
  @$pb.TagNumber(5)
  void clearTopAuthorShare() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.String get churnTrend => $_getSZ(5);
  @$pb.TagNumber(6)
  set churnTrend($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasChurnTrend() => $_has(5);
  @$pb.TagNumber(6)
  void clearChurnTrend() => $_clearField(6);

  @$pb.TagNumber(7)
  $core.double get churnSlope => $_getN(6);
  @$pb.TagNumber(7)
  set churnSlope($core.double value) => $_setDouble(6, value);
  @$pb.TagNumber(7)
  $core.bool hasChurnSlope() => $_has(6);
  @$pb.TagNumber(7)
  void clearChurnSlope() => $_clearField(7);

  @$pb.TagNumber(8)
  $fixnum.Int64 get totalChurn => $_getI64(7);
  @$pb.TagNumber(8)
  set totalChurn($fixnum.Int64 value) => $_setInt64(7, value);
  @$pb.TagNumber(8)
  $core.bool hasTotalChurn() => $_has(7);
  @$pb.TagNumber(8)
  void clearTotalChurn() => $_clearField(8);
}

class ReviewChurnReport extends $pb.GeneratedMessage {
  factory ReviewChurnReport({
    $core.int? commitsScanned,
    $core.Iterable<ReviewFileChurn>? files,
  }) {
    final result = ReviewChurnReport._();
    if (commitsScanned != null) result.commitsScanned = commitsScanned;
    if (files != null) result.files.addAll(files);
    return result;
  }

  ReviewChurnReport._();

  factory ReviewChurnReport.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewChurnReport()..mergeFromBuffer(data, registry);
  factory ReviewChurnReport.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewChurnReport()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewChurnReport',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewChurnReport.$_createMessage)
    ..aI(1, _omitFieldNames ? '' : 'commitsScanned',
        fieldType: $pb.PbFieldType.OU3)
    ..pPM<ReviewFileChurn>(2, _omitFieldNames ? '' : 'files',
        subBuilder: ReviewFileChurn.$_createMessage)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewChurnReport clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewChurnReport copyWith(void Function(ReviewChurnReport) updates) =>
      super.copyWith((message) => updates(message as ReviewChurnReport))
          as ReviewChurnReport;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewChurnReport() / ReviewChurnReport.new instead')
  static ReviewChurnReport create() => ReviewChurnReport._();
  static $pb.GeneratedMessage $_createMessage() => ReviewChurnReport._();
  @$core.override
  ReviewChurnReport createEmptyInstance() => ReviewChurnReport._();
  @$core.pragma('dart2js:noInline')
  static ReviewChurnReport getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewChurnReport>(
          ReviewChurnReport.$_createMessage);
  static ReviewChurnReport? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get commitsScanned => $_getIZ(0);
  @$pb.TagNumber(1)
  set commitsScanned($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasCommitsScanned() => $_has(0);
  @$pb.TagNumber(1)
  void clearCommitsScanned() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<ReviewFileChurn> get files => $_getList(1);
}

class ReviewFileSalience extends $pb.GeneratedMessage {
  factory ReviewFileSalience({
    $core.String? file,
    $core.double? centrality,
    $core.int? busFactor,
    $core.bool? churnIncreasing,
    $core.String? class_5,
  }) {
    final result = ReviewFileSalience._();
    if (file != null) result.file = file;
    if (centrality != null) result.centrality = centrality;
    if (busFactor != null) result.busFactor = busFactor;
    if (churnIncreasing != null) result.churnIncreasing = churnIncreasing;
    if (class_5 != null) result.class_5 = class_5;
    return result;
  }

  ReviewFileSalience._();

  factory ReviewFileSalience.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewFileSalience()..mergeFromBuffer(data, registry);
  factory ReviewFileSalience.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewFileSalience()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewFileSalience',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewFileSalience.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'file')
    ..aD(2, _omitFieldNames ? '' : 'centrality')
    ..aI(3, _omitFieldNames ? '' : 'busFactor', fieldType: $pb.PbFieldType.OU3)
    ..aOB(4, _omitFieldNames ? '' : 'churnIncreasing')
    ..aOS(5, _omitFieldNames ? '' : 'class')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewFileSalience clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewFileSalience copyWith(void Function(ReviewFileSalience) updates) =>
      super.copyWith((message) => updates(message as ReviewFileSalience))
          as ReviewFileSalience;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewFileSalience() / ReviewFileSalience.new instead')
  static ReviewFileSalience create() => ReviewFileSalience._();
  static $pb.GeneratedMessage $_createMessage() => ReviewFileSalience._();
  @$core.override
  ReviewFileSalience createEmptyInstance() => ReviewFileSalience._();
  @$core.pragma('dart2js:noInline')
  static ReviewFileSalience getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewFileSalience>(
          ReviewFileSalience.$_createMessage);
  static ReviewFileSalience? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get file => $_getSZ(0);
  @$pb.TagNumber(1)
  set file($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasFile() => $_has(0);
  @$pb.TagNumber(1)
  void clearFile() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.double get centrality => $_getN(1);
  @$pb.TagNumber(2)
  set centrality($core.double value) => $_setDouble(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCentrality() => $_has(1);
  @$pb.TagNumber(2)
  void clearCentrality() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get busFactor => $_getIZ(2);
  @$pb.TagNumber(3)
  set busFactor($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasBusFactor() => $_has(2);
  @$pb.TagNumber(3)
  void clearBusFactor() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.bool get churnIncreasing => $_getBF(3);
  @$pb.TagNumber(4)
  set churnIncreasing($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasChurnIncreasing() => $_has(3);
  @$pb.TagNumber(4)
  void clearChurnIncreasing() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get class_5 => $_getSZ(4);
  @$pb.TagNumber(5)
  set class_5($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasClass_5() => $_has(4);
  @$pb.TagNumber(5)
  void clearClass_5() => $_clearField(5);
}

class ReviewSalienceReport extends $pb.GeneratedMessage {
  factory ReviewSalienceReport({
    $core.Iterable<ReviewFileSalience>? files,
  }) {
    final result = ReviewSalienceReport._();
    if (files != null) result.files.addAll(files);
    return result;
  }

  ReviewSalienceReport._();

  factory ReviewSalienceReport.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewSalienceReport()..mergeFromBuffer(data, registry);
  factory ReviewSalienceReport.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewSalienceReport()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewSalienceReport',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewSalienceReport.$_createMessage)
    ..pPM<ReviewFileSalience>(1, _omitFieldNames ? '' : 'files',
        subBuilder: ReviewFileSalience.$_createMessage)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewSalienceReport clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewSalienceReport copyWith(void Function(ReviewSalienceReport) updates) =>
      super.copyWith((message) => updates(message as ReviewSalienceReport))
          as ReviewSalienceReport;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated(
      'Use ReviewSalienceReport() / ReviewSalienceReport.new instead')
  static ReviewSalienceReport create() => ReviewSalienceReport._();
  static $pb.GeneratedMessage $_createMessage() => ReviewSalienceReport._();
  @$core.override
  ReviewSalienceReport createEmptyInstance() => ReviewSalienceReport._();
  @$core.pragma('dart2js:noInline')
  static ReviewSalienceReport getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewSalienceReport>(
          ReviewSalienceReport.$_createMessage);
  static ReviewSalienceReport? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<ReviewFileSalience> get files => $_getList(0);
}

class ReviewRiskReason extends $pb.GeneratedMessage {
  factory ReviewRiskReason({
    $core.String? kind,
    $core.double? weight,
    $core.String? detail,
  }) {
    final result = ReviewRiskReason._();
    if (kind != null) result.kind = kind;
    if (weight != null) result.weight = weight;
    if (detail != null) result.detail = detail;
    return result;
  }

  ReviewRiskReason._();

  factory ReviewRiskReason.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewRiskReason()..mergeFromBuffer(data, registry);
  factory ReviewRiskReason.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewRiskReason()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewRiskReason',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewRiskReason.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'kind')
    ..aD(2, _omitFieldNames ? '' : 'weight')
    ..aOS(3, _omitFieldNames ? '' : 'detail')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewRiskReason clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewRiskReason copyWith(void Function(ReviewRiskReason) updates) =>
      super.copyWith((message) => updates(message as ReviewRiskReason))
          as ReviewRiskReason;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewRiskReason() / ReviewRiskReason.new instead')
  static ReviewRiskReason create() => ReviewRiskReason._();
  static $pb.GeneratedMessage $_createMessage() => ReviewRiskReason._();
  @$core.override
  ReviewRiskReason createEmptyInstance() => ReviewRiskReason._();
  @$core.pragma('dart2js:noInline')
  static ReviewRiskReason getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewRiskReason>(
          ReviewRiskReason.$_createMessage);
  static ReviewRiskReason? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get kind => $_getSZ(0);
  @$pb.TagNumber(1)
  set kind($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasKind() => $_has(0);
  @$pb.TagNumber(1)
  void clearKind() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.double get weight => $_getN(1);
  @$pb.TagNumber(2)
  set weight($core.double value) => $_setDouble(1, value);
  @$pb.TagNumber(2)
  $core.bool hasWeight() => $_has(1);
  @$pb.TagNumber(2)
  void clearWeight() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get detail => $_getSZ(2);
  @$pb.TagNumber(3)
  set detail($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasDetail() => $_has(2);
  @$pb.TagNumber(3)
  void clearDetail() => $_clearField(3);
}

class ReviewFileRisk extends $pb.GeneratedMessage {
  factory ReviewFileRisk({
    $core.String? file,
    $core.double? score,
    $core.String? level,
    $core.Iterable<ReviewRiskReason>? reasons,
  }) {
    final result = ReviewFileRisk._();
    if (file != null) result.file = file;
    if (score != null) result.score = score;
    if (level != null) result.level = level;
    if (reasons != null) result.reasons.addAll(reasons);
    return result;
  }

  ReviewFileRisk._();

  factory ReviewFileRisk.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewFileRisk()..mergeFromBuffer(data, registry);
  factory ReviewFileRisk.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewFileRisk()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewFileRisk',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewFileRisk.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'file')
    ..aD(2, _omitFieldNames ? '' : 'score')
    ..aOS(3, _omitFieldNames ? '' : 'level')
    ..pPM<ReviewRiskReason>(4, _omitFieldNames ? '' : 'reasons',
        subBuilder: ReviewRiskReason.$_createMessage)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewFileRisk clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewFileRisk copyWith(void Function(ReviewFileRisk) updates) =>
      super.copyWith((message) => updates(message as ReviewFileRisk))
          as ReviewFileRisk;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewFileRisk() / ReviewFileRisk.new instead')
  static ReviewFileRisk create() => ReviewFileRisk._();
  static $pb.GeneratedMessage $_createMessage() => ReviewFileRisk._();
  @$core.override
  ReviewFileRisk createEmptyInstance() => ReviewFileRisk._();
  @$core.pragma('dart2js:noInline')
  static ReviewFileRisk getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewFileRisk>(
          ReviewFileRisk.$_createMessage);
  static ReviewFileRisk? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get file => $_getSZ(0);
  @$pb.TagNumber(1)
  set file($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasFile() => $_has(0);
  @$pb.TagNumber(1)
  void clearFile() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.double get score => $_getN(1);
  @$pb.TagNumber(2)
  set score($core.double value) => $_setDouble(1, value);
  @$pb.TagNumber(2)
  $core.bool hasScore() => $_has(1);
  @$pb.TagNumber(2)
  void clearScore() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get level => $_getSZ(2);
  @$pb.TagNumber(3)
  set level($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasLevel() => $_has(2);
  @$pb.TagNumber(3)
  void clearLevel() => $_clearField(3);

  @$pb.TagNumber(4)
  $pb.PbList<ReviewRiskReason> get reasons => $_getList(3);
}

class ReviewRiskReport extends $pb.GeneratedMessage {
  factory ReviewRiskReport({
    $core.Iterable<ReviewFileRisk>? files,
    $core.double? maxScore,
    $core.double? gateThreshold,
    $core.bool? gateFailed,
  }) {
    final result = ReviewRiskReport._();
    if (files != null) result.files.addAll(files);
    if (maxScore != null) result.maxScore = maxScore;
    if (gateThreshold != null) result.gateThreshold = gateThreshold;
    if (gateFailed != null) result.gateFailed = gateFailed;
    return result;
  }

  ReviewRiskReport._();

  factory ReviewRiskReport.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewRiskReport()..mergeFromBuffer(data, registry);
  factory ReviewRiskReport.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewRiskReport()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewRiskReport',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewRiskReport.$_createMessage)
    ..pPM<ReviewFileRisk>(1, _omitFieldNames ? '' : 'files',
        subBuilder: ReviewFileRisk.$_createMessage)
    ..aD(2, _omitFieldNames ? '' : 'maxScore')
    ..aD(3, _omitFieldNames ? '' : 'gateThreshold')
    ..aOB(4, _omitFieldNames ? '' : 'gateFailed')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewRiskReport clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewRiskReport copyWith(void Function(ReviewRiskReport) updates) =>
      super.copyWith((message) => updates(message as ReviewRiskReport))
          as ReviewRiskReport;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewRiskReport() / ReviewRiskReport.new instead')
  static ReviewRiskReport create() => ReviewRiskReport._();
  static $pb.GeneratedMessage $_createMessage() => ReviewRiskReport._();
  @$core.override
  ReviewRiskReport createEmptyInstance() => ReviewRiskReport._();
  @$core.pragma('dart2js:noInline')
  static ReviewRiskReport getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewRiskReport>(
          ReviewRiskReport.$_createMessage);
  static ReviewRiskReport? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<ReviewFileRisk> get files => $_getList(0);

  @$pb.TagNumber(2)
  $core.double get maxScore => $_getN(1);
  @$pb.TagNumber(2)
  set maxScore($core.double value) => $_setDouble(1, value);
  @$pb.TagNumber(2)
  $core.bool hasMaxScore() => $_has(1);
  @$pb.TagNumber(2)
  void clearMaxScore() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.double get gateThreshold => $_getN(2);
  @$pb.TagNumber(3)
  set gateThreshold($core.double value) => $_setDouble(2, value);
  @$pb.TagNumber(3)
  $core.bool hasGateThreshold() => $_has(2);
  @$pb.TagNumber(3)
  void clearGateThreshold() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.bool get gateFailed => $_getBF(3);
  @$pb.TagNumber(4)
  set gateFailed($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasGateFailed() => $_has(3);
  @$pb.TagNumber(4)
  void clearGateFailed() => $_clearField(4);
}

/// One (tool, rule) tally in the analysis digest — the bucketed summary so a
/// high-volume rule stays visible past the verbatim finding cap.
class ReviewRuleCount extends $pb.GeneratedMessage {
  factory ReviewRuleCount({
    $core.String? tool,
    $core.String? rule,
    $core.int? count,
    $core.int? inChange,
    $core.int? generated,
  }) {
    final result = ReviewRuleCount._();
    if (tool != null) result.tool = tool;
    if (rule != null) result.rule = rule;
    if (count != null) result.count = count;
    if (inChange != null) result.inChange = inChange;
    if (generated != null) result.generated = generated;
    return result;
  }

  ReviewRuleCount._();

  factory ReviewRuleCount.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewRuleCount()..mergeFromBuffer(data, registry);
  factory ReviewRuleCount.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewRuleCount()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewRuleCount',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewRuleCount.$_createMessage)
    ..aOS(1, _omitFieldNames ? '' : 'tool')
    ..aOS(2, _omitFieldNames ? '' : 'rule')
    ..aI(3, _omitFieldNames ? '' : 'count', fieldType: $pb.PbFieldType.OU3)
    ..aI(4, _omitFieldNames ? '' : 'inChange', fieldType: $pb.PbFieldType.OU3)
    ..aI(5, _omitFieldNames ? '' : 'generated', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewRuleCount clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewRuleCount copyWith(void Function(ReviewRuleCount) updates) =>
      super.copyWith((message) => updates(message as ReviewRuleCount))
          as ReviewRuleCount;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewRuleCount() / ReviewRuleCount.new instead')
  static ReviewRuleCount create() => ReviewRuleCount._();
  static $pb.GeneratedMessage $_createMessage() => ReviewRuleCount._();
  @$core.override
  ReviewRuleCount createEmptyInstance() => ReviewRuleCount._();
  @$core.pragma('dart2js:noInline')
  static ReviewRuleCount getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewRuleCount>(
          ReviewRuleCount.$_createMessage);
  static ReviewRuleCount? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get tool => $_getSZ(0);
  @$pb.TagNumber(1)
  set tool($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasTool() => $_has(0);
  @$pb.TagNumber(1)
  void clearTool() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get rule => $_getSZ(1);
  @$pb.TagNumber(2)
  set rule($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasRule() => $_has(1);
  @$pb.TagNumber(2)
  void clearRule() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get count => $_getIZ(2);
  @$pb.TagNumber(3)
  set count($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasCount() => $_has(2);
  @$pb.TagNumber(3)
  void clearCount() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get inChange => $_getIZ(3);
  @$pb.TagNumber(4)
  set inChange($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasInChange() => $_has(3);
  @$pb.TagNumber(4)
  void clearInChange() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.int get generated => $_getIZ(4);
  @$pb.TagNumber(5)
  set generated($core.int value) => $_setUnsignedInt32(4, value);
  @$pb.TagNumber(5)
  $core.bool hasGenerated() => $_has(4);
  @$pb.TagNumber(5)
  void clearGenerated() => $_clearField(5);
}

/// The unified analysis digest (review-analysis-depth Stage 2): every
/// ReviewAnalysisReport's findings deduped, risk-ranked, and rule-bucketed into
/// one section. Tool-derived — no model.
class ReviewAnalysisDigest extends $pb.GeneratedMessage {
  factory ReviewAnalysisDigest({
    $core.Iterable<ReviewAnalysisFinding>? findings,
    $core.int? total,
    $core.int? inChange,
    $core.Iterable<ReviewRuleCount>? ruleCounts,
    $core.int? generated,
  }) {
    final result = ReviewAnalysisDigest._();
    if (findings != null) result.findings.addAll(findings);
    if (total != null) result.total = total;
    if (inChange != null) result.inChange = inChange;
    if (ruleCounts != null) result.ruleCounts.addAll(ruleCounts);
    if (generated != null) result.generated = generated;
    return result;
  }

  ReviewAnalysisDigest._();

  factory ReviewAnalysisDigest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewAnalysisDigest()..mergeFromBuffer(data, registry);
  factory ReviewAnalysisDigest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewAnalysisDigest()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewAnalysisDigest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewAnalysisDigest.$_createMessage)
    ..pPM<ReviewAnalysisFinding>(1, _omitFieldNames ? '' : 'findings',
        subBuilder: ReviewAnalysisFinding.$_createMessage)
    ..aI(2, _omitFieldNames ? '' : 'total', fieldType: $pb.PbFieldType.OU3)
    ..aI(3, _omitFieldNames ? '' : 'inChange', fieldType: $pb.PbFieldType.OU3)
    ..pPM<ReviewRuleCount>(4, _omitFieldNames ? '' : 'ruleCounts',
        subBuilder: ReviewRuleCount.$_createMessage)
    ..aI(5, _omitFieldNames ? '' : 'generated', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewAnalysisDigest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewAnalysisDigest copyWith(void Function(ReviewAnalysisDigest) updates) =>
      super.copyWith((message) => updates(message as ReviewAnalysisDigest))
          as ReviewAnalysisDigest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated(
      'Use ReviewAnalysisDigest() / ReviewAnalysisDigest.new instead')
  static ReviewAnalysisDigest create() => ReviewAnalysisDigest._();
  static $pb.GeneratedMessage $_createMessage() => ReviewAnalysisDigest._();
  @$core.override
  ReviewAnalysisDigest createEmptyInstance() => ReviewAnalysisDigest._();
  @$core.pragma('dart2js:noInline')
  static ReviewAnalysisDigest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewAnalysisDigest>(
          ReviewAnalysisDigest.$_createMessage);
  static ReviewAnalysisDigest? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<ReviewAnalysisFinding> get findings => $_getList(0);

  @$pb.TagNumber(2)
  $core.int get total => $_getIZ(1);
  @$pb.TagNumber(2)
  set total($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasTotal() => $_has(1);
  @$pb.TagNumber(2)
  void clearTotal() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get inChange => $_getIZ(2);
  @$pb.TagNumber(3)
  set inChange($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasInChange() => $_has(2);
  @$pb.TagNumber(3)
  void clearInChange() => $_clearField(3);

  @$pb.TagNumber(4)
  $pb.PbList<ReviewRuleCount> get ruleCounts => $_getList(3);

  /// How many of `total` are on generated code.
  @$pb.TagNumber(5)
  $core.int get generated => $_getIZ(4);
  @$pb.TagNumber(5)
  set generated($core.int value) => $_setUnsignedInt32(4, value);
  @$pb.TagNumber(5)
  $core.bool hasGenerated() => $_has(4);
  @$pb.TagNumber(5)
  void clearGenerated() => $_clearField(5);
}

class ReviewFacts extends $pb.GeneratedMessage {
  factory ReviewFacts({
    ReviewMeta? meta,
    ReviewChangeSet? change,
    ReviewGitState? gitState,
    ReviewAnalysisReport? analysis,
    ReviewSignatureReport? signatures,
    ReviewCallGraph? callgraph,
    ReviewStyleFacts? style,
    ReviewSummaryReport? summaries,
    ReviewCoChangeReport? cochange,
    ReviewChurnReport? churn,
    ReviewSalienceReport? salience,
    ReviewRiskReport? risk,
    ReviewAnalysisReport? shellcheck,
    ReviewAnalysisReport? goChecks,
    ReviewAnalysisReport? nearby,
    ReviewAnalysisDigest? digest,
    $core.String? digestSummary,
  }) {
    final result = ReviewFacts._();
    if (meta != null) result.meta = meta;
    if (change != null) result.change = change;
    if (gitState != null) result.gitState = gitState;
    if (analysis != null) result.analysis = analysis;
    if (signatures != null) result.signatures = signatures;
    if (callgraph != null) result.callgraph = callgraph;
    if (style != null) result.style = style;
    if (summaries != null) result.summaries = summaries;
    if (cochange != null) result.cochange = cochange;
    if (churn != null) result.churn = churn;
    if (salience != null) result.salience = salience;
    if (risk != null) result.risk = risk;
    if (shellcheck != null) result.shellcheck = shellcheck;
    if (goChecks != null) result.goChecks = goChecks;
    if (nearby != null) result.nearby = nearby;
    if (digest != null) result.digest = digest;
    if (digestSummary != null) result.digestSummary = digestSummary;
    return result;
  }

  ReviewFacts._();

  factory ReviewFacts.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewFacts()..mergeFromBuffer(data, registry);
  factory ReviewFacts.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      ReviewFacts()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewFacts',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: ReviewFacts.$_createMessage)
    ..aOM<ReviewMeta>(1, _omitFieldNames ? '' : 'meta',
        subBuilder: ReviewMeta.$_createMessage)
    ..aOM<ReviewChangeSet>(2, _omitFieldNames ? '' : 'change',
        subBuilder: ReviewChangeSet.$_createMessage)
    ..aOM<ReviewGitState>(3, _omitFieldNames ? '' : 'gitState',
        subBuilder: ReviewGitState.$_createMessage)
    ..aOM<ReviewAnalysisReport>(4, _omitFieldNames ? '' : 'analysis',
        subBuilder: ReviewAnalysisReport.$_createMessage)
    ..aOM<ReviewSignatureReport>(5, _omitFieldNames ? '' : 'signatures',
        subBuilder: ReviewSignatureReport.$_createMessage)
    ..aOM<ReviewCallGraph>(6, _omitFieldNames ? '' : 'callgraph',
        subBuilder: ReviewCallGraph.$_createMessage)
    ..aOM<ReviewStyleFacts>(7, _omitFieldNames ? '' : 'style',
        subBuilder: ReviewStyleFacts.$_createMessage)
    ..aOM<ReviewSummaryReport>(8, _omitFieldNames ? '' : 'summaries',
        subBuilder: ReviewSummaryReport.$_createMessage)
    ..aOM<ReviewCoChangeReport>(9, _omitFieldNames ? '' : 'cochange',
        subBuilder: ReviewCoChangeReport.$_createMessage)
    ..aOM<ReviewChurnReport>(10, _omitFieldNames ? '' : 'churn',
        subBuilder: ReviewChurnReport.$_createMessage)
    ..aOM<ReviewSalienceReport>(11, _omitFieldNames ? '' : 'salience',
        subBuilder: ReviewSalienceReport.$_createMessage)
    ..aOM<ReviewRiskReport>(12, _omitFieldNames ? '' : 'risk',
        subBuilder: ReviewRiskReport.$_createMessage)
    ..aOM<ReviewAnalysisReport>(13, _omitFieldNames ? '' : 'shellcheck',
        subBuilder: ReviewAnalysisReport.$_createMessage)
    ..aOM<ReviewAnalysisReport>(14, _omitFieldNames ? '' : 'goChecks',
        subBuilder: ReviewAnalysisReport.$_createMessage)
    ..aOM<ReviewAnalysisReport>(15, _omitFieldNames ? '' : 'nearby',
        subBuilder: ReviewAnalysisReport.$_createMessage)
    ..aOM<ReviewAnalysisDigest>(16, _omitFieldNames ? '' : 'digest',
        subBuilder: ReviewAnalysisDigest.$_createMessage)
    ..aOS(17, _omitFieldNames ? '' : 'digestSummary')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewFacts clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewFacts copyWith(void Function(ReviewFacts) updates) =>
      super.copyWith((message) => updates(message as ReviewFacts))
          as ReviewFacts;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  @$core.Deprecated('Use ReviewFacts() / ReviewFacts.new instead')
  static ReviewFacts create() => ReviewFacts._();
  static $pb.GeneratedMessage $_createMessage() => ReviewFacts._();
  @$core.override
  ReviewFacts createEmptyInstance() => ReviewFacts._();
  @$core.pragma('dart2js:noInline')
  static ReviewFacts getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ReviewFacts>(
          ReviewFacts.$_createMessage);
  static ReviewFacts? _defaultInstance;

  @$pb.TagNumber(1)
  ReviewMeta get meta => $_getN(0);
  @$pb.TagNumber(1)
  set meta(ReviewMeta value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasMeta() => $_has(0);
  @$pb.TagNumber(1)
  void clearMeta() => $_clearField(1);
  @$pb.TagNumber(1)
  ReviewMeta ensureMeta() => $_ensure(0);

  @$pb.TagNumber(2)
  ReviewChangeSet get change => $_getN(1);
  @$pb.TagNumber(2)
  set change(ReviewChangeSet value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasChange() => $_has(1);
  @$pb.TagNumber(2)
  void clearChange() => $_clearField(2);
  @$pb.TagNumber(2)
  ReviewChangeSet ensureChange() => $_ensure(1);

  @$pb.TagNumber(3)
  ReviewGitState get gitState => $_getN(2);
  @$pb.TagNumber(3)
  set gitState(ReviewGitState value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasGitState() => $_has(2);
  @$pb.TagNumber(3)
  void clearGitState() => $_clearField(3);
  @$pb.TagNumber(3)
  ReviewGitState ensureGitState() => $_ensure(2);

  @$pb.TagNumber(4)
  ReviewAnalysisReport get analysis => $_getN(3);
  @$pb.TagNumber(4)
  set analysis(ReviewAnalysisReport value) => $_setField(4, value);
  @$pb.TagNumber(4)
  $core.bool hasAnalysis() => $_has(3);
  @$pb.TagNumber(4)
  void clearAnalysis() => $_clearField(4);
  @$pb.TagNumber(4)
  ReviewAnalysisReport ensureAnalysis() => $_ensure(3);

  @$pb.TagNumber(5)
  ReviewSignatureReport get signatures => $_getN(4);
  @$pb.TagNumber(5)
  set signatures(ReviewSignatureReport value) => $_setField(5, value);
  @$pb.TagNumber(5)
  $core.bool hasSignatures() => $_has(4);
  @$pb.TagNumber(5)
  void clearSignatures() => $_clearField(5);
  @$pb.TagNumber(5)
  ReviewSignatureReport ensureSignatures() => $_ensure(4);

  @$pb.TagNumber(6)
  ReviewCallGraph get callgraph => $_getN(5);
  @$pb.TagNumber(6)
  set callgraph(ReviewCallGraph value) => $_setField(6, value);
  @$pb.TagNumber(6)
  $core.bool hasCallgraph() => $_has(5);
  @$pb.TagNumber(6)
  void clearCallgraph() => $_clearField(6);
  @$pb.TagNumber(6)
  ReviewCallGraph ensureCallgraph() => $_ensure(5);

  @$pb.TagNumber(7)
  ReviewStyleFacts get style => $_getN(6);
  @$pb.TagNumber(7)
  set style(ReviewStyleFacts value) => $_setField(7, value);
  @$pb.TagNumber(7)
  $core.bool hasStyle() => $_has(6);
  @$pb.TagNumber(7)
  void clearStyle() => $_clearField(7);
  @$pb.TagNumber(7)
  ReviewStyleFacts ensureStyle() => $_ensure(6);

  @$pb.TagNumber(8)
  ReviewSummaryReport get summaries => $_getN(7);
  @$pb.TagNumber(8)
  set summaries(ReviewSummaryReport value) => $_setField(8, value);
  @$pb.TagNumber(8)
  $core.bool hasSummaries() => $_has(7);
  @$pb.TagNumber(8)
  void clearSummaries() => $_clearField(8);
  @$pb.TagNumber(8)
  ReviewSummaryReport ensureSummaries() => $_ensure(7);

  @$pb.TagNumber(9)
  ReviewCoChangeReport get cochange => $_getN(8);
  @$pb.TagNumber(9)
  set cochange(ReviewCoChangeReport value) => $_setField(9, value);
  @$pb.TagNumber(9)
  $core.bool hasCochange() => $_has(8);
  @$pb.TagNumber(9)
  void clearCochange() => $_clearField(9);
  @$pb.TagNumber(9)
  ReviewCoChangeReport ensureCochange() => $_ensure(8);

  @$pb.TagNumber(10)
  ReviewChurnReport get churn => $_getN(9);
  @$pb.TagNumber(10)
  set churn(ReviewChurnReport value) => $_setField(10, value);
  @$pb.TagNumber(10)
  $core.bool hasChurn() => $_has(9);
  @$pb.TagNumber(10)
  void clearChurn() => $_clearField(10);
  @$pb.TagNumber(10)
  ReviewChurnReport ensureChurn() => $_ensure(9);

  @$pb.TagNumber(11)
  ReviewSalienceReport get salience => $_getN(10);
  @$pb.TagNumber(11)
  set salience(ReviewSalienceReport value) => $_setField(11, value);
  @$pb.TagNumber(11)
  $core.bool hasSalience() => $_has(10);
  @$pb.TagNumber(11)
  void clearSalience() => $_clearField(11);
  @$pb.TagNumber(11)
  ReviewSalienceReport ensureSalience() => $_ensure(10);

  @$pb.TagNumber(12)
  ReviewRiskReport get risk => $_getN(11);
  @$pb.TagNumber(12)
  set risk(ReviewRiskReport value) => $_setField(12, value);
  @$pb.TagNumber(12)
  $core.bool hasRisk() => $_has(11);
  @$pb.TagNumber(12)
  void clearRisk() => $_clearField(12);
  @$pb.TagNumber(12)
  ReviewRiskReport ensureRisk() => $_ensure(11);

  /// review-fleet C12 collectors — all shaped as ReviewAnalysisReport.
  @$pb.TagNumber(13)
  ReviewAnalysisReport get shellcheck => $_getN(12);
  @$pb.TagNumber(13)
  set shellcheck(ReviewAnalysisReport value) => $_setField(13, value);
  @$pb.TagNumber(13)
  $core.bool hasShellcheck() => $_has(12);
  @$pb.TagNumber(13)
  void clearShellcheck() => $_clearField(13);
  @$pb.TagNumber(13)
  ReviewAnalysisReport ensureShellcheck() => $_ensure(12);

  @$pb.TagNumber(14)
  ReviewAnalysisReport get goChecks => $_getN(13);
  @$pb.TagNumber(14)
  set goChecks(ReviewAnalysisReport value) => $_setField(14, value);
  @$pb.TagNumber(14)
  $core.bool hasGoChecks() => $_has(13);
  @$pb.TagNumber(14)
  void clearGoChecks() => $_clearField(14);
  @$pb.TagNumber(14)
  ReviewAnalysisReport ensureGoChecks() => $_ensure(13);

  @$pb.TagNumber(15)
  ReviewAnalysisReport get nearby => $_getN(14);
  @$pb.TagNumber(15)
  set nearby(ReviewAnalysisReport value) => $_setField(15, value);
  @$pb.TagNumber(15)
  $core.bool hasNearby() => $_has(14);
  @$pb.TagNumber(15)
  void clearNearby() => $_clearField(15);
  @$pb.TagNumber(15)
  ReviewAnalysisReport ensureNearby() => $_ensure(14);

  /// Unified analysis digest (Stage 2) — deduped/risk-ranked/bucketed findings.
  @$pb.TagNumber(16)
  ReviewAnalysisDigest get digest => $_getN(15);
  @$pb.TagNumber(16)
  set digest(ReviewAnalysisDigest value) => $_setField(16, value);
  @$pb.TagNumber(16)
  $core.bool hasDigest() => $_has(15);
  @$pb.TagNumber(16)
  void clearDigest() => $_clearField(16);
  @$pb.TagNumber(16)
  ReviewAnalysisDigest ensureDigest() => $_ensure(15);

  /// Cheap local-LLM prose summary of the digest (Stage 3) — SOFT, may be empty.
  @$pb.TagNumber(17)
  $core.String get digestSummary => $_getSZ(16);
  @$pb.TagNumber(17)
  set digestSummary($core.String value) => $_setString(16, value);
  @$pb.TagNumber(17)
  $core.bool hasDigestSummary() => $_has(16);
  @$pb.TagNumber(17)
  void clearDigestSummary() => $_clearField(17);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
