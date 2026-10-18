// This is a generated file - do not edit.
//
// Generated from agent/v1/github_install_registry.proto.

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

/// One GitHub App installation a tenant owns. Mirrors `agent_core::GithubInstall`.
class GithubInstall extends $pb.GeneratedMessage {
  factory GithubInstall({
    $core.String? installationId,
    $core.Iterable<$core.String>? selectedRepos,
    $fixnum.Int64? installedAt,
  }) {
    final result = create();
    if (installationId != null) result.installationId = installationId;
    if (selectedRepos != null) result.selectedRepos.addAll(selectedRepos);
    if (installedAt != null) result.installedAt = installedAt;
    return result;
  }

  GithubInstall._();

  factory GithubInstall.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GithubInstall.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GithubInstall',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'installationId')
    ..pPS(2, _omitFieldNames ? '' : 'selectedRepos')
    ..a<$fixnum.Int64>(
        3, _omitFieldNames ? '' : 'installedAt', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GithubInstall clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GithubInstall copyWith(void Function(GithubInstall) updates) =>
      super.copyWith((message) => updates(message as GithubInstall))
          as GithubInstall;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GithubInstall create() => GithubInstall._();
  @$core.override
  GithubInstall createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GithubInstall getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GithubInstall>(create);
  static GithubInstall? _defaultInstance;

  /// Decimal GitHub installation id (server-validated; the card id).
  @$pb.TagNumber(1)
  $core.String get installationId => $_getSZ(0);
  @$pb.TagNumber(1)
  set installationId($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasInstallationId() => $_has(0);
  @$pb.TagNumber(1)
  void clearInstallationId() => $_clearField(1);

  /// The repositories this install selected, each a flat `owner__name` slug
  /// (a single `safe_segment`). Capped on ingest.
  @$pb.TagNumber(2)
  $pb.PbList<$core.String> get selectedRepos => $_getList(1);

  /// When the install was recorded, Unix seconds.
  @$pb.TagNumber(3)
  $fixnum.Int64 get installedAt => $_getI64(2);
  @$pb.TagNumber(3)
  set installedAt($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasInstalledAt() => $_has(2);
  @$pb.TagNumber(3)
  void clearInstalledAt() => $_clearField(3);
}

class GithubInstallListRequest extends $pb.GeneratedMessage {
  factory GithubInstallListRequest() => create();

  GithubInstallListRequest._();

  factory GithubInstallListRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GithubInstallListRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GithubInstallListRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GithubInstallListRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GithubInstallListRequest copyWith(
          void Function(GithubInstallListRequest) updates) =>
      super.copyWith((message) => updates(message as GithubInstallListRequest))
          as GithubInstallListRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GithubInstallListRequest create() => GithubInstallListRequest._();
  @$core.override
  GithubInstallListRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GithubInstallListRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GithubInstallListRequest>(create);
  static GithubInstallListRequest? _defaultInstance;
}

class GithubInstallList extends $pb.GeneratedMessage {
  factory GithubInstallList({
    $core.Iterable<GithubInstall>? installs,
  }) {
    final result = create();
    if (installs != null) result.installs.addAll(installs);
    return result;
  }

  GithubInstallList._();

  factory GithubInstallList.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GithubInstallList.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GithubInstallList',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<GithubInstall>(1, _omitFieldNames ? '' : 'installs',
        subBuilder: GithubInstall.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GithubInstallList clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GithubInstallList copyWith(void Function(GithubInstallList) updates) =>
      super.copyWith((message) => updates(message as GithubInstallList))
          as GithubInstallList;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GithubInstallList create() => GithubInstallList._();
  @$core.override
  GithubInstallList createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GithubInstallList getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GithubInstallList>(create);
  static GithubInstallList? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<GithubInstall> get installs => $_getList(0);
}

class GithubInstallRef extends $pb.GeneratedMessage {
  factory GithubInstallRef({
    $core.String? installationId,
  }) {
    final result = create();
    if (installationId != null) result.installationId = installationId;
    return result;
  }

  GithubInstallRef._();

  factory GithubInstallRef.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GithubInstallRef.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GithubInstallRef',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'installationId')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GithubInstallRef clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GithubInstallRef copyWith(void Function(GithubInstallRef) updates) =>
      super.copyWith((message) => updates(message as GithubInstallRef))
          as GithubInstallRef;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GithubInstallRef create() => GithubInstallRef._();
  @$core.override
  GithubInstallRef createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GithubInstallRef getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GithubInstallRef>(create);
  static GithubInstallRef? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get installationId => $_getSZ(0);
  @$pb.TagNumber(1)
  set installationId($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasInstallationId() => $_has(0);
  @$pb.TagNumber(1)
  void clearInstallationId() => $_clearField(1);
}

class GithubInstallDeleteReply extends $pb.GeneratedMessage {
  factory GithubInstallDeleteReply({
    $core.bool? deleted,
  }) {
    final result = create();
    if (deleted != null) result.deleted = deleted;
    return result;
  }

  GithubInstallDeleteReply._();

  factory GithubInstallDeleteReply.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GithubInstallDeleteReply.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GithubInstallDeleteReply',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'deleted')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GithubInstallDeleteReply clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GithubInstallDeleteReply copyWith(
          void Function(GithubInstallDeleteReply) updates) =>
      super.copyWith((message) => updates(message as GithubInstallDeleteReply))
          as GithubInstallDeleteReply;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GithubInstallDeleteReply create() => GithubInstallDeleteReply._();
  @$core.override
  GithubInstallDeleteReply createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GithubInstallDeleteReply getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GithubInstallDeleteReply>(create);
  static GithubInstallDeleteReply? _defaultInstance;

  @$pb.TagNumber(1)
  $core.bool get deleted => $_getBF(0);
  @$pb.TagNumber(1)
  set deleted($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasDeleted() => $_has(0);
  @$pb.TagNumber(1)
  void clearDeleted() => $_clearField(1);
}

/// Start onboarding an installation (forge-auth F5c2). Carries nothing: the tenant and
/// subject come from the caller's VERIFIED principal, never the wire.
class BeginInstallRequest extends $pb.GeneratedMessage {
  factory BeginInstallRequest() => create();

  BeginInstallRequest._();

  factory BeginInstallRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory BeginInstallRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'BeginInstallRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BeginInstallRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BeginInstallRequest copyWith(void Function(BeginInstallRequest) updates) =>
      super.copyWith((message) => updates(message as BeginInstallRequest))
          as BeginInstallRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static BeginInstallRequest create() => BeginInstallRequest._();
  @$core.override
  BeginInstallRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static BeginInstallRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<BeginInstallRequest>(create);
  static BeginInstallRequest? _defaultInstance;
}

class BeginInstallReply extends $pb.GeneratedMessage {
  factory BeginInstallReply({
    $core.String? installUrl,
    $core.String? state,
    $fixnum.Int64? expiresAt,
  }) {
    final result = create();
    if (installUrl != null) result.installUrl = installUrl;
    if (state != null) result.state = state;
    if (expiresAt != null) result.expiresAt = expiresAt;
    return result;
  }

  BeginInstallReply._();

  factory BeginInstallReply.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory BeginInstallReply.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'BeginInstallReply',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'installUrl')
    ..aOS(2, _omitFieldNames ? '' : 'state')
    ..a<$fixnum.Int64>(
        3, _omitFieldNames ? '' : 'expiresAt', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BeginInstallReply clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BeginInstallReply copyWith(void Function(BeginInstallReply) updates) =>
      super.copyWith((message) => updates(message as BeginInstallReply))
          as BeginInstallReply;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static BeginInstallReply create() => BeginInstallReply._();
  @$core.override
  BeginInstallReply createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static BeginInstallReply getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<BeginInstallReply>(create);
  static BeginInstallReply? _defaultInstance;

  /// `https://github.com/apps/{slug}/installations/new?state=…` — send the browser here.
  @$pb.TagNumber(1)
  $core.String get installUrl => $_getSZ(0);
  @$pb.TagNumber(1)
  set installUrl($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasInstallUrl() => $_has(0);
  @$pb.TagNumber(1)
  void clearInstallUrl() => $_clearField(1);

  /// Single-use, bound to the caller that began it; GitHub hands it back to the App's
  /// Setup URL.
  @$pb.TagNumber(2)
  $core.String get state => $_getSZ(1);
  @$pb.TagNumber(2)
  set state($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasState() => $_has(1);
  @$pb.TagNumber(2)
  void clearState() => $_clearField(2);

  /// When `state` lapses, Unix seconds.
  @$pb.TagNumber(3)
  $fixnum.Int64 get expiresAt => $_getI64(2);
  @$pb.TagNumber(3)
  set expiresAt($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasExpiresAt() => $_has(2);
  @$pb.TagNumber(3)
  void clearExpiresAt() => $_clearField(3);
}

/// The query parameters GitHub appended to the App's Setup URL, relayed by the portal.
/// All three are browser-supplied and untrusted: `state` must be one this caller began,
/// and `installation_id` is never taken as proof — the server re-reads the installation's
/// account with the App JWT and requires the `code`'s user to administer it.
class CompleteInstallRequest extends $pb.GeneratedMessage {
  factory CompleteInstallRequest({
    $core.String? state,
    $core.String? code,
    $core.String? installationId,
  }) {
    final result = create();
    if (state != null) result.state = state;
    if (code != null) result.code = code;
    if (installationId != null) result.installationId = installationId;
    return result;
  }

  CompleteInstallRequest._();

  factory CompleteInstallRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CompleteInstallRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CompleteInstallRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'state')
    ..aOS(2, _omitFieldNames ? '' : 'code')
    ..aOS(3, _omitFieldNames ? '' : 'installationId')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CompleteInstallRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CompleteInstallRequest copyWith(
          void Function(CompleteInstallRequest) updates) =>
      super.copyWith((message) => updates(message as CompleteInstallRequest))
          as CompleteInstallRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CompleteInstallRequest create() => CompleteInstallRequest._();
  @$core.override
  CompleteInstallRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CompleteInstallRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<CompleteInstallRequest>(create);
  static CompleteInstallRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get state => $_getSZ(0);
  @$pb.TagNumber(1)
  set state($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasState() => $_has(0);
  @$pb.TagNumber(1)
  void clearState() => $_clearField(1);

  /// The install-time user-authorization code ("Request user authorization during
  /// installation" must be on).
  @$pb.TagNumber(2)
  $core.String get code => $_getSZ(1);
  @$pb.TagNumber(2)
  set code($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCode() => $_has(1);
  @$pb.TagNumber(2)
  void clearCode() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get installationId => $_getSZ(2);
  @$pb.TagNumber(3)
  set installationId($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasInstallationId() => $_has(2);
  @$pb.TagNumber(3)
  void clearInstallationId() => $_clearField(3);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
