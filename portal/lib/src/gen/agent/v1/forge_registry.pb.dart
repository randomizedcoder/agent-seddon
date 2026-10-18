// This is a generated file - do not edit.
//
// Generated from agent/v1/forge_registry.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

/// A forge (git-host) card. Mirrors `agent_core::ForgeCard`. `kind` selects the impl
/// factory; `base_url` empty ⇒ the kind's registered default; `repo_encoding` is a
/// validated scheme string (`owner_name` | `path`).
class ForgeCard extends $pb.GeneratedMessage {
  factory ForgeCard({
    $core.String? id,
    $core.String? kind,
    $core.bool? enabled,
    $core.String? baseUrl,
    $core.String? tokenRef,
    $core.String? repoEncoding,
    $core.int? timeoutSecs,
    $core.int? maxRetries,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (kind != null) result.kind = kind;
    if (enabled != null) result.enabled = enabled;
    if (baseUrl != null) result.baseUrl = baseUrl;
    if (tokenRef != null) result.tokenRef = tokenRef;
    if (repoEncoding != null) result.repoEncoding = repoEncoding;
    if (timeoutSecs != null) result.timeoutSecs = timeoutSecs;
    if (maxRetries != null) result.maxRetries = maxRetries;
    return result;
  }

  ForgeCard._();

  factory ForgeCard.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ForgeCard.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ForgeCard',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aOS(2, _omitFieldNames ? '' : 'kind')
    ..aOB(3, _omitFieldNames ? '' : 'enabled')
    ..aOS(4, _omitFieldNames ? '' : 'baseUrl')
    ..aOS(5, _omitFieldNames ? '' : 'tokenRef')
    ..aOS(6, _omitFieldNames ? '' : 'repoEncoding')
    ..aI(7, _omitFieldNames ? '' : 'timeoutSecs',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(8, _omitFieldNames ? '' : 'maxRetries', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeCard clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeCard copyWith(void Function(ForgeCard) updates) =>
      super.copyWith((message) => updates(message as ForgeCard)) as ForgeCard;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ForgeCard create() => ForgeCard._();
  @$core.override
  ForgeCard createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ForgeCard getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ForgeCard>(create);
  static ForgeCard? _defaultInstance;

  /// Path-safe forge id (server-validated).
  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  /// Selects the Forge impl factory: github | gitlab | (gitea | bitbucket | …).
  @$pb.TagNumber(2)
  $core.String get kind => $_getSZ(1);
  @$pb.TagNumber(2)
  set kind($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasKind() => $_has(1);
  @$pb.TagNumber(2)
  void clearKind() => $_clearField(2);

  /// Whether this forge is active (a disabled card is stored but not built).
  @$pb.TagNumber(3)
  $core.bool get enabled => $_getBF(2);
  @$pb.TagNumber(3)
  set enabled($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasEnabled() => $_has(2);
  @$pb.TagNumber(3)
  void clearEnabled() => $_clearField(3);

  /// Empty ⇒ the kind's registered default (e.g. https://api.github.com); a non-empty
  /// value overrides it (self-hosted GitLab / GitHub Enterprise) and is SSRF-screened.
  @$pb.TagNumber(4)
  $core.String get baseUrl => $_getSZ(3);
  @$pb.TagNumber(4)
  set baseUrl($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasBaseUrl() => $_has(3);
  @$pb.TagNumber(4)
  void clearBaseUrl() => $_clearField(4);

  /// `env:NAME` | `file:/path` — NEVER a raw token.
  @$pb.TagNumber(5)
  $core.String get tokenRef => $_getSZ(4);
  @$pb.TagNumber(5)
  set tokenRef($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasTokenRef() => $_has(4);
  @$pb.TagNumber(5)
  void clearTokenRef() => $_clearField(5);

  /// How a flat `owner__name` slug maps to the host API path: `owner_name` (GitHub:
  /// split `owner__name`) | `path` (GitLab: `__`→`/`, subgroups). Required.
  @$pb.TagNumber(6)
  $core.String get repoEncoding => $_getSZ(5);
  @$pb.TagNumber(6)
  set repoEncoding($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasRepoEncoding() => $_has(5);
  @$pb.TagNumber(6)
  void clearRepoEncoding() => $_clearField(6);

  /// HTTP timeout, clamped on ingest.
  @$pb.TagNumber(7)
  $core.int get timeoutSecs => $_getIZ(6);
  @$pb.TagNumber(7)
  set timeoutSecs($core.int value) => $_setUnsignedInt32(6, value);
  @$pb.TagNumber(7)
  $core.bool hasTimeoutSecs() => $_has(6);
  @$pb.TagNumber(7)
  void clearTimeoutSecs() => $_clearField(7);

  /// HTTP retry budget, clamped on ingest.
  @$pb.TagNumber(8)
  $core.int get maxRetries => $_getIZ(7);
  @$pb.TagNumber(8)
  set maxRetries($core.int value) => $_setUnsignedInt32(7, value);
  @$pb.TagNumber(8)
  $core.bool hasMaxRetries() => $_has(7);
  @$pb.TagNumber(8)
  void clearMaxRetries() => $_clearField(8);
}

class ForgeListRequest extends $pb.GeneratedMessage {
  factory ForgeListRequest() => create();

  ForgeListRequest._();

  factory ForgeListRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ForgeListRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ForgeListRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeListRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeListRequest copyWith(void Function(ForgeListRequest) updates) =>
      super.copyWith((message) => updates(message as ForgeListRequest))
          as ForgeListRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ForgeListRequest create() => ForgeListRequest._();
  @$core.override
  ForgeListRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ForgeListRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ForgeListRequest>(create);
  static ForgeListRequest? _defaultInstance;
}

class ForgeList extends $pb.GeneratedMessage {
  factory ForgeList({
    $core.Iterable<ForgeCard>? forges,
  }) {
    final result = create();
    if (forges != null) result.forges.addAll(forges);
    return result;
  }

  ForgeList._();

  factory ForgeList.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ForgeList.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ForgeList',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<ForgeCard>(1, _omitFieldNames ? '' : 'forges',
        subBuilder: ForgeCard.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeList clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeList copyWith(void Function(ForgeList) updates) =>
      super.copyWith((message) => updates(message as ForgeList)) as ForgeList;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ForgeList create() => ForgeList._();
  @$core.override
  ForgeList createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ForgeList getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ForgeList>(create);
  static ForgeList? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<ForgeCard> get forges => $_getList(0);
}

class ForgeRef extends $pb.GeneratedMessage {
  factory ForgeRef({
    $core.String? id,
  }) {
    final result = create();
    if (id != null) result.id = id;
    return result;
  }

  ForgeRef._();

  factory ForgeRef.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ForgeRef.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ForgeRef',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeRef clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeRef copyWith(void Function(ForgeRef) updates) =>
      super.copyWith((message) => updates(message as ForgeRef)) as ForgeRef;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ForgeRef create() => ForgeRef._();
  @$core.override
  ForgeRef createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ForgeRef getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<ForgeRef>(create);
  static ForgeRef? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);
}

class ForgeDeleteReply extends $pb.GeneratedMessage {
  factory ForgeDeleteReply({
    $core.bool? deleted,
  }) {
    final result = create();
    if (deleted != null) result.deleted = deleted;
    return result;
  }

  ForgeDeleteReply._();

  factory ForgeDeleteReply.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ForgeDeleteReply.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ForgeDeleteReply',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'deleted')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeDeleteReply clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ForgeDeleteReply copyWith(void Function(ForgeDeleteReply) updates) =>
      super.copyWith((message) => updates(message as ForgeDeleteReply))
          as ForgeDeleteReply;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ForgeDeleteReply create() => ForgeDeleteReply._();
  @$core.override
  ForgeDeleteReply createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ForgeDeleteReply getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ForgeDeleteReply>(create);
  static ForgeDeleteReply? _defaultInstance;

  @$pb.TagNumber(1)
  $core.bool get deleted => $_getBF(0);
  @$pb.TagNumber(1)
  set deleted($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasDeleted() => $_has(0);
  @$pb.TagNumber(1)
  void clearDeleted() => $_clearField(1);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
