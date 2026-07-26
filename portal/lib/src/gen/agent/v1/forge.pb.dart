// This is a generated file - do not edit.
//
// Generated from agent/v1/forge.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

import 'forge.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'forge.pbenum.dart';

class ForgeNumber extends $pb.GeneratedMessage {
  factory ForgeNumber({
    $fixnum.Int64? number,
  }) {
    final result = create();
    if (number != null) result.number = number;
    return result;
  }

  ForgeNumber._();

  factory ForgeNumber.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ForgeNumber.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ForgeNumber',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..a<$fixnum.Int64>(1, _omitFieldNames ? '' : 'number', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeNumber clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeNumber copyWith(void Function(ForgeNumber) updates) =>
      super.copyWith((message) => updates(message as ForgeNumber))
          as ForgeNumber;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ForgeNumber create() => ForgeNumber._();
  @$core.override
  ForgeNumber createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ForgeNumber getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ForgeNumber>(create);
  static ForgeNumber? _defaultInstance;

  @$pb.TagNumber(1)
  $fixnum.Int64 get number => $_getI64(0);
  @$pb.TagNumber(1)
  set number($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasNumber() => $_has(0);
  @$pb.TagNumber(1)
  void clearNumber() => $_clearField(1);
}

class ForgePage extends $pb.GeneratedMessage {
  factory ForgePage({
    $core.int? page,
  }) {
    final result = create();
    if (page != null) result.page = page;
    return result;
  }

  ForgePage._();

  factory ForgePage.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ForgePage.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ForgePage',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aI(1, _omitFieldNames ? '' : 'page', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgePage clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgePage copyWith(void Function(ForgePage) updates) =>
      super.copyWith((message) => updates(message as ForgePage)) as ForgePage;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ForgePage create() => ForgePage._();
  @$core.override
  ForgePage createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ForgePage getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ForgePage>(create);
  static ForgePage? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get page => $_getIZ(0);
  @$pb.TagNumber(1)
  set page($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPage() => $_has(0);
  @$pb.TagNumber(1)
  void clearPage() => $_clearField(1);
}

class ForgePullRequest extends $pb.GeneratedMessage {
  factory ForgePullRequest({
    $fixnum.Int64? number,
    $core.String? title,
    $core.String? body,
    $core.String? state,
    $core.String? author,
    $core.String? url,
    $core.String? sourceBranch,
    $core.String? targetBranch,
    $core.bool? draft,
  }) {
    final result = create();
    if (number != null) result.number = number;
    if (title != null) result.title = title;
    if (body != null) result.body = body;
    if (state != null) result.state = state;
    if (author != null) result.author = author;
    if (url != null) result.url = url;
    if (sourceBranch != null) result.sourceBranch = sourceBranch;
    if (targetBranch != null) result.targetBranch = targetBranch;
    if (draft != null) result.draft = draft;
    return result;
  }

  ForgePullRequest._();

  factory ForgePullRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ForgePullRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ForgePullRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..a<$fixnum.Int64>(1, _omitFieldNames ? '' : 'number', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOS(2, _omitFieldNames ? '' : 'title')
    ..aOS(3, _omitFieldNames ? '' : 'body')
    ..aOS(4, _omitFieldNames ? '' : 'state')
    ..aOS(5, _omitFieldNames ? '' : 'author')
    ..aOS(6, _omitFieldNames ? '' : 'url')
    ..aOS(7, _omitFieldNames ? '' : 'sourceBranch')
    ..aOS(8, _omitFieldNames ? '' : 'targetBranch')
    ..aOB(9, _omitFieldNames ? '' : 'draft')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgePullRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgePullRequest copyWith(void Function(ForgePullRequest) updates) =>
      super.copyWith((message) => updates(message as ForgePullRequest))
          as ForgePullRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ForgePullRequest create() => ForgePullRequest._();
  @$core.override
  ForgePullRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ForgePullRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ForgePullRequest>(create);
  static ForgePullRequest? _defaultInstance;

  /// The user-facing number (#42), not an internal id.
  @$pb.TagNumber(1)
  $fixnum.Int64 get number => $_getI64(0);
  @$pb.TagNumber(1)
  set number($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasNumber() => $_has(0);
  @$pb.TagNumber(1)
  void clearNumber() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get title => $_getSZ(1);
  @$pb.TagNumber(2)
  set title($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasTitle() => $_has(1);
  @$pb.TagNumber(2)
  void clearTitle() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get body => $_getSZ(2);
  @$pb.TagNumber(3)
  set body($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasBody() => $_has(2);
  @$pb.TagNumber(3)
  void clearBody() => $_clearField(3);

  /// open / closed / merged.
  @$pb.TagNumber(4)
  $core.String get state => $_getSZ(3);
  @$pb.TagNumber(4)
  set state($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasState() => $_has(3);
  @$pb.TagNumber(4)
  void clearState() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get author => $_getSZ(4);
  @$pb.TagNumber(5)
  set author($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasAuthor() => $_has(4);
  @$pb.TagNumber(5)
  void clearAuthor() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.String get url => $_getSZ(5);
  @$pb.TagNumber(6)
  set url($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasUrl() => $_has(5);
  @$pb.TagNumber(6)
  void clearUrl() => $_clearField(6);

  @$pb.TagNumber(7)
  $core.String get sourceBranch => $_getSZ(6);
  @$pb.TagNumber(7)
  set sourceBranch($core.String value) => $_setString(6, value);
  @$pb.TagNumber(7)
  $core.bool hasSourceBranch() => $_has(6);
  @$pb.TagNumber(7)
  void clearSourceBranch() => $_clearField(7);

  @$pb.TagNumber(8)
  $core.String get targetBranch => $_getSZ(7);
  @$pb.TagNumber(8)
  set targetBranch($core.String value) => $_setString(7, value);
  @$pb.TagNumber(8)
  $core.bool hasTargetBranch() => $_has(7);
  @$pb.TagNumber(8)
  void clearTargetBranch() => $_clearField(8);

  @$pb.TagNumber(9)
  $core.bool get draft => $_getBF(8);
  @$pb.TagNumber(9)
  set draft($core.bool value) => $_setBool(8, value);
  @$pb.TagNumber(9)
  $core.bool hasDraft() => $_has(8);
  @$pb.TagNumber(9)
  void clearDraft() => $_clearField(9);
}

class ForgeComment extends $pb.GeneratedMessage {
  factory ForgeComment({
    $core.String? author,
    $core.String? body,
    $core.String? url,
  }) {
    final result = create();
    if (author != null) result.author = author;
    if (body != null) result.body = body;
    if (url != null) result.url = url;
    return result;
  }

  ForgeComment._();

  factory ForgeComment.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ForgeComment.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ForgeComment',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'author')
    ..aOS(2, _omitFieldNames ? '' : 'body')
    ..aOS(3, _omitFieldNames ? '' : 'url')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeComment clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeComment copyWith(void Function(ForgeComment) updates) =>
      super.copyWith((message) => updates(message as ForgeComment))
          as ForgeComment;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ForgeComment create() => ForgeComment._();
  @$core.override
  ForgeComment createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ForgeComment getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ForgeComment>(create);
  static ForgeComment? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get author => $_getSZ(0);
  @$pb.TagNumber(1)
  set author($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasAuthor() => $_has(0);
  @$pb.TagNumber(1)
  void clearAuthor() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get body => $_getSZ(1);
  @$pb.TagNumber(2)
  set body($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBody() => $_has(1);
  @$pb.TagNumber(2)
  void clearBody() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get url => $_getSZ(2);
  @$pb.TagNumber(3)
  set url($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasUrl() => $_has(2);
  @$pb.TagNumber(3)
  void clearUrl() => $_clearField(3);
}

class ForgeIssue extends $pb.GeneratedMessage {
  factory ForgeIssue({
    $fixnum.Int64? number,
    $core.String? title,
    $core.String? body,
    $core.String? state,
    $core.String? author,
    $core.String? url,
    $core.Iterable<$core.String>? labels,
    $core.Iterable<ForgeComment>? comments,
  }) {
    final result = create();
    if (number != null) result.number = number;
    if (title != null) result.title = title;
    if (body != null) result.body = body;
    if (state != null) result.state = state;
    if (author != null) result.author = author;
    if (url != null) result.url = url;
    if (labels != null) result.labels.addAll(labels);
    if (comments != null) result.comments.addAll(comments);
    return result;
  }

  ForgeIssue._();

  factory ForgeIssue.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ForgeIssue.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ForgeIssue',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..a<$fixnum.Int64>(1, _omitFieldNames ? '' : 'number', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOS(2, _omitFieldNames ? '' : 'title')
    ..aOS(3, _omitFieldNames ? '' : 'body')
    ..aOS(4, _omitFieldNames ? '' : 'state')
    ..aOS(5, _omitFieldNames ? '' : 'author')
    ..aOS(6, _omitFieldNames ? '' : 'url')
    ..pPS(7, _omitFieldNames ? '' : 'labels')
    ..pPM<ForgeComment>(8, _omitFieldNames ? '' : 'comments',
        subBuilder: ForgeComment.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeIssue clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeIssue copyWith(void Function(ForgeIssue) updates) =>
      super.copyWith((message) => updates(message as ForgeIssue)) as ForgeIssue;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ForgeIssue create() => ForgeIssue._();
  @$core.override
  ForgeIssue createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ForgeIssue getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ForgeIssue>(create);
  static ForgeIssue? _defaultInstance;

  @$pb.TagNumber(1)
  $fixnum.Int64 get number => $_getI64(0);
  @$pb.TagNumber(1)
  set number($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasNumber() => $_has(0);
  @$pb.TagNumber(1)
  void clearNumber() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get title => $_getSZ(1);
  @$pb.TagNumber(2)
  set title($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasTitle() => $_has(1);
  @$pb.TagNumber(2)
  void clearTitle() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get body => $_getSZ(2);
  @$pb.TagNumber(3)
  set body($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasBody() => $_has(2);
  @$pb.TagNumber(3)
  void clearBody() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get state => $_getSZ(3);
  @$pb.TagNumber(4)
  set state($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasState() => $_has(3);
  @$pb.TagNumber(4)
  void clearState() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get author => $_getSZ(4);
  @$pb.TagNumber(5)
  set author($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasAuthor() => $_has(4);
  @$pb.TagNumber(5)
  void clearAuthor() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.String get url => $_getSZ(5);
  @$pb.TagNumber(6)
  set url($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasUrl() => $_has(5);
  @$pb.TagNumber(6)
  void clearUrl() => $_clearField(6);

  @$pb.TagNumber(7)
  $pb.PbList<$core.String> get labels => $_getList(6);

  /// Populated by ImportIssue; empty in list results.
  @$pb.TagNumber(8)
  $pb.PbList<ForgeComment> get comments => $_getList(7);
}

/// `next_page` absent ⇒ this is the last page, so a caller can paginate without
/// knowing the platform's pagination dialect.
class ForgePrPage extends $pb.GeneratedMessage {
  factory ForgePrPage({
    $core.Iterable<ForgePullRequest>? items,
    $core.int? nextPage,
  }) {
    final result = create();
    if (items != null) result.items.addAll(items);
    if (nextPage != null) result.nextPage = nextPage;
    return result;
  }

  ForgePrPage._();

  factory ForgePrPage.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ForgePrPage.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ForgePrPage',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<ForgePullRequest>(1, _omitFieldNames ? '' : 'items',
        subBuilder: ForgePullRequest.create)
    ..aI(2, _omitFieldNames ? '' : 'nextPage', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgePrPage clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgePrPage copyWith(void Function(ForgePrPage) updates) =>
      super.copyWith((message) => updates(message as ForgePrPage))
          as ForgePrPage;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ForgePrPage create() => ForgePrPage._();
  @$core.override
  ForgePrPage createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ForgePrPage getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ForgePrPage>(create);
  static ForgePrPage? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<ForgePullRequest> get items => $_getList(0);

  @$pb.TagNumber(2)
  $core.int get nextPage => $_getIZ(1);
  @$pb.TagNumber(2)
  set nextPage($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasNextPage() => $_has(1);
  @$pb.TagNumber(2)
  void clearNextPage() => $_clearField(2);
}

class ForgeIssuePage extends $pb.GeneratedMessage {
  factory ForgeIssuePage({
    $core.Iterable<ForgeIssue>? items,
    $core.int? nextPage,
  }) {
    final result = create();
    if (items != null) result.items.addAll(items);
    if (nextPage != null) result.nextPage = nextPage;
    return result;
  }

  ForgeIssuePage._();

  factory ForgeIssuePage.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ForgeIssuePage.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ForgeIssuePage',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<ForgeIssue>(1, _omitFieldNames ? '' : 'items',
        subBuilder: ForgeIssue.create)
    ..aI(2, _omitFieldNames ? '' : 'nextPage', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeIssuePage clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeIssuePage copyWith(void Function(ForgeIssuePage) updates) =>
      super.copyWith((message) => updates(message as ForgeIssuePage))
          as ForgeIssuePage;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ForgeIssuePage create() => ForgeIssuePage._();
  @$core.override
  ForgeIssuePage createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ForgeIssuePage getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ForgeIssuePage>(create);
  static ForgeIssuePage? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<ForgeIssue> get items => $_getList(0);

  @$pb.TagNumber(2)
  $core.int get nextPage => $_getIZ(1);
  @$pb.TagNumber(2)
  set nextPage($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasNextPage() => $_has(1);
  @$pb.TagNumber(2)
  void clearNextPage() => $_clearField(2);
}

class ForgeCreatePrRequest extends $pb.GeneratedMessage {
  factory ForgeCreatePrRequest({
    $core.String? title,
    $core.String? body,
    $core.String? sourceBranch,
    $core.String? targetBranch,
    $core.bool? draft,
  }) {
    final result = create();
    if (title != null) result.title = title;
    if (body != null) result.body = body;
    if (sourceBranch != null) result.sourceBranch = sourceBranch;
    if (targetBranch != null) result.targetBranch = targetBranch;
    if (draft != null) result.draft = draft;
    return result;
  }

  ForgeCreatePrRequest._();

  factory ForgeCreatePrRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ForgeCreatePrRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ForgeCreatePrRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'title')
    ..aOS(2, _omitFieldNames ? '' : 'body')
    ..aOS(3, _omitFieldNames ? '' : 'sourceBranch')
    ..aOS(4, _omitFieldNames ? '' : 'targetBranch')
    ..aOB(5, _omitFieldNames ? '' : 'draft')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeCreatePrRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeCreatePrRequest copyWith(void Function(ForgeCreatePrRequest) updates) =>
      super.copyWith((message) => updates(message as ForgeCreatePrRequest))
          as ForgeCreatePrRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ForgeCreatePrRequest create() => ForgeCreatePrRequest._();
  @$core.override
  ForgeCreatePrRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ForgeCreatePrRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ForgeCreatePrRequest>(create);
  static ForgeCreatePrRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get title => $_getSZ(0);
  @$pb.TagNumber(1)
  set title($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasTitle() => $_has(0);
  @$pb.TagNumber(1)
  void clearTitle() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get body => $_getSZ(1);
  @$pb.TagNumber(2)
  set body($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBody() => $_has(1);
  @$pb.TagNumber(2)
  void clearBody() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get sourceBranch => $_getSZ(2);
  @$pb.TagNumber(3)
  set sourceBranch($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasSourceBranch() => $_has(2);
  @$pb.TagNumber(3)
  void clearSourceBranch() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get targetBranch => $_getSZ(3);
  @$pb.TagNumber(4)
  set targetBranch($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasTargetBranch() => $_has(3);
  @$pb.TagNumber(4)
  void clearTargetBranch() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.bool get draft => $_getBF(4);
  @$pb.TagNumber(5)
  set draft($core.bool value) => $_setBool(4, value);
  @$pb.TagNumber(5)
  $core.bool hasDraft() => $_has(4);
  @$pb.TagNumber(5)
  void clearDraft() => $_clearField(5);
}

class ForgeCommentRequest extends $pb.GeneratedMessage {
  factory ForgeCommentRequest({
    $fixnum.Int64? number,
    $core.String? body,
  }) {
    final result = create();
    if (number != null) result.number = number;
    if (body != null) result.body = body;
    return result;
  }

  ForgeCommentRequest._();

  factory ForgeCommentRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ForgeCommentRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ForgeCommentRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..a<$fixnum.Int64>(1, _omitFieldNames ? '' : 'number', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOS(2, _omitFieldNames ? '' : 'body')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeCommentRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeCommentRequest copyWith(void Function(ForgeCommentRequest) updates) =>
      super.copyWith((message) => updates(message as ForgeCommentRequest))
          as ForgeCommentRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ForgeCommentRequest create() => ForgeCommentRequest._();
  @$core.override
  ForgeCommentRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ForgeCommentRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ForgeCommentRequest>(create);
  static ForgeCommentRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $fixnum.Int64 get number => $_getI64(0);
  @$pb.TagNumber(1)
  set number($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasNumber() => $_has(0);
  @$pb.TagNumber(1)
  void clearNumber() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get body => $_getSZ(1);
  @$pb.TagNumber(2)
  set body($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBody() => $_has(1);
  @$pb.TagNumber(2)
  void clearBody() => $_clearField(2);
}

class ForgeReviewRequest extends $pb.GeneratedMessage {
  factory ForgeReviewRequest({
    $fixnum.Int64? number,
    ForgeReviewVerdict? verdict,
    $core.String? body,
  }) {
    final result = create();
    if (number != null) result.number = number;
    if (verdict != null) result.verdict = verdict;
    if (body != null) result.body = body;
    return result;
  }

  ForgeReviewRequest._();

  factory ForgeReviewRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ForgeReviewRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ForgeReviewRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..a<$fixnum.Int64>(1, _omitFieldNames ? '' : 'number', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aE<ForgeReviewVerdict>(2, _omitFieldNames ? '' : 'verdict',
        enumValues: ForgeReviewVerdict.values)
    ..aOS(3, _omitFieldNames ? '' : 'body')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeReviewRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeReviewRequest copyWith(void Function(ForgeReviewRequest) updates) =>
      super.copyWith((message) => updates(message as ForgeReviewRequest))
          as ForgeReviewRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ForgeReviewRequest create() => ForgeReviewRequest._();
  @$core.override
  ForgeReviewRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ForgeReviewRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ForgeReviewRequest>(create);
  static ForgeReviewRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $fixnum.Int64 get number => $_getI64(0);
  @$pb.TagNumber(1)
  set number($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasNumber() => $_has(0);
  @$pb.TagNumber(1)
  void clearNumber() => $_clearField(1);

  @$pb.TagNumber(2)
  ForgeReviewVerdict get verdict => $_getN(1);
  @$pb.TagNumber(2)
  set verdict(ForgeReviewVerdict value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasVerdict() => $_has(1);
  @$pb.TagNumber(2)
  void clearVerdict() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get body => $_getSZ(2);
  @$pb.TagNumber(3)
  set body($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasBody() => $_has(2);
  @$pb.TagNumber(3)
  void clearBody() => $_clearField(3);
}

class Task extends $pb.GeneratedMessage {
  factory Task({
    $core.String? content,
    TaskStatus? status,
    TaskPriority? priority,
  }) {
    final result = create();
    if (content != null) result.content = content;
    if (status != null) result.status = status;
    if (priority != null) result.priority = priority;
    return result;
  }

  Task._();

  factory Task.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory Task.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Task',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'content')
    ..aE<TaskStatus>(2, _omitFieldNames ? '' : 'status',
        enumValues: TaskStatus.values)
    ..aE<TaskPriority>(3, _omitFieldNames ? '' : 'priority',
        enumValues: TaskPriority.values)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Task clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Task copyWith(void Function(Task) updates) =>
      super.copyWith((message) => updates(message as Task)) as Task;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static Task create() => Task._();
  @$core.override
  Task createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static Task getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<Task>(create);
  static Task? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get content => $_getSZ(0);
  @$pb.TagNumber(1)
  set content($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasContent() => $_has(0);
  @$pb.TagNumber(1)
  void clearContent() => $_clearField(1);

  @$pb.TagNumber(2)
  TaskStatus get status => $_getN(1);
  @$pb.TagNumber(2)
  set status(TaskStatus value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasStatus() => $_has(1);
  @$pb.TagNumber(2)
  void clearStatus() => $_clearField(2);

  @$pb.TagNumber(3)
  TaskPriority get priority => $_getN(2);
  @$pb.TagNumber(3)
  set priority(TaskPriority value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasPriority() => $_has(2);
  @$pb.TagNumber(3)
  void clearPriority() => $_clearField(3);
}

class TaskWriteRequest extends $pb.GeneratedMessage {
  factory TaskWriteRequest({
    $core.Iterable<Task>? todos,
  }) {
    final result = create();
    if (todos != null) result.todos.addAll(todos);
    return result;
  }

  TaskWriteRequest._();

  factory TaskWriteRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory TaskWriteRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'TaskWriteRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<Task>(1, _omitFieldNames ? '' : 'todos', subBuilder: Task.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TaskWriteRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TaskWriteRequest copyWith(void Function(TaskWriteRequest) updates) =>
      super.copyWith((message) => updates(message as TaskWriteRequest))
          as TaskWriteRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static TaskWriteRequest create() => TaskWriteRequest._();
  @$core.override
  TaskWriteRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static TaskWriteRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<TaskWriteRequest>(create);
  static TaskWriteRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<Task> get todos => $_getList(0);
}

/// Patches one existing todo, matched by `content`. Absent fields are unchanged.
class TaskUpdateRequest extends $pb.GeneratedMessage {
  factory TaskUpdateRequest({
    $core.String? content,
    TaskStatus? status,
    TaskPriority? priority,
  }) {
    final result = create();
    if (content != null) result.content = content;
    if (status != null) result.status = status;
    if (priority != null) result.priority = priority;
    return result;
  }

  TaskUpdateRequest._();

  factory TaskUpdateRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory TaskUpdateRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'TaskUpdateRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'content')
    ..aE<TaskStatus>(2, _omitFieldNames ? '' : 'status',
        enumValues: TaskStatus.values)
    ..aE<TaskPriority>(3, _omitFieldNames ? '' : 'priority',
        enumValues: TaskPriority.values)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TaskUpdateRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TaskUpdateRequest copyWith(void Function(TaskUpdateRequest) updates) =>
      super.copyWith((message) => updates(message as TaskUpdateRequest))
          as TaskUpdateRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static TaskUpdateRequest create() => TaskUpdateRequest._();
  @$core.override
  TaskUpdateRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static TaskUpdateRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<TaskUpdateRequest>(create);
  static TaskUpdateRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get content => $_getSZ(0);
  @$pb.TagNumber(1)
  set content($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasContent() => $_has(0);
  @$pb.TagNumber(1)
  void clearContent() => $_clearField(1);

  @$pb.TagNumber(2)
  TaskStatus get status => $_getN(1);
  @$pb.TagNumber(2)
  set status(TaskStatus value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasStatus() => $_has(1);
  @$pb.TagNumber(2)
  void clearStatus() => $_clearField(2);

  @$pb.TagNumber(3)
  TaskPriority get priority => $_getN(2);
  @$pb.TagNumber(3)
  set priority(TaskPriority value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasPriority() => $_has(2);
  @$pb.TagNumber(3)
  void clearPriority() => $_clearField(3);
}

class TaskListRequest extends $pb.GeneratedMessage {
  factory TaskListRequest() => create();

  TaskListRequest._();

  factory TaskListRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory TaskListRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'TaskListRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TaskListRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TaskListRequest copyWith(void Function(TaskListRequest) updates) =>
      super.copyWith((message) => updates(message as TaskListRequest))
          as TaskListRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static TaskListRequest create() => TaskListRequest._();
  @$core.override
  TaskListRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static TaskListRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<TaskListRequest>(create);
  static TaskListRequest? _defaultInstance;
}

class TaskClearRequest extends $pb.GeneratedMessage {
  factory TaskClearRequest() => create();

  TaskClearRequest._();

  factory TaskClearRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory TaskClearRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'TaskClearRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TaskClearRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TaskClearRequest copyWith(void Function(TaskClearRequest) updates) =>
      super.copyWith((message) => updates(message as TaskClearRequest))
          as TaskClearRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static TaskClearRequest create() => TaskClearRequest._();
  @$core.override
  TaskClearRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static TaskClearRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<TaskClearRequest>(create);
  static TaskClearRequest? _defaultInstance;
}

class TaskClearResponse extends $pb.GeneratedMessage {
  factory TaskClearResponse() => create();

  TaskClearResponse._();

  factory TaskClearResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory TaskClearResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'TaskClearResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TaskClearResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TaskClearResponse copyWith(void Function(TaskClearResponse) updates) =>
      super.copyWith((message) => updates(message as TaskClearResponse))
          as TaskClearResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static TaskClearResponse create() => TaskClearResponse._();
  @$core.override
  TaskClearResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static TaskClearResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<TaskClearResponse>(create);
  static TaskClearResponse? _defaultInstance;
}

class TaskList extends $pb.GeneratedMessage {
  factory TaskList({
    $core.Iterable<Task>? todos,
  }) {
    final result = create();
    if (todos != null) result.todos.addAll(todos);
    return result;
  }

  TaskList._();

  factory TaskList.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory TaskList.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'TaskList',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<Task>(1, _omitFieldNames ? '' : 'todos', subBuilder: Task.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TaskList clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  TaskList copyWith(void Function(TaskList) updates) =>
      super.copyWith((message) => updates(message as TaskList)) as TaskList;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static TaskList create() => TaskList._();
  @$core.override
  TaskList createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static TaskList getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<TaskList>(create);
  static TaskList? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<Task> get todos => $_getList(0);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
