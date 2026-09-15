// This is a generated file - do not edit.
//
// Generated from agent/v1/review_fleet.proto.

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

/// One durable roster row — a review session's identity, the repo it watches, how
/// it reaches its forge, and its triggers. Mirrors `agent_core::FleetSession`.
class FleetSession extends $pb.GeneratedMessage {
  factory FleetSession({
    $core.String? id,
    $core.String? user,
    $core.String? repo,
    $core.String? backend,
    $core.String? baseUrl,
    $core.String? tokenRef,
    $core.String? skill,
    $core.String? slackTriggerChannel,
    $core.String? slackProgressChannel,
    $fixnum.Int64? pollSecs,
    $core.bool? enabled,
    $fixnum.Int64? createdAt,
    $fixnum.Int64? updatedAt,
    $core.String? forgeId,
    $core.String? transportId,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (user != null) result.user = user;
    if (repo != null) result.repo = repo;
    if (backend != null) result.backend = backend;
    if (baseUrl != null) result.baseUrl = baseUrl;
    if (tokenRef != null) result.tokenRef = tokenRef;
    if (skill != null) result.skill = skill;
    if (slackTriggerChannel != null)
      result.slackTriggerChannel = slackTriggerChannel;
    if (slackProgressChannel != null)
      result.slackProgressChannel = slackProgressChannel;
    if (pollSecs != null) result.pollSecs = pollSecs;
    if (enabled != null) result.enabled = enabled;
    if (createdAt != null) result.createdAt = createdAt;
    if (updatedAt != null) result.updatedAt = updatedAt;
    if (forgeId != null) result.forgeId = forgeId;
    if (transportId != null) result.transportId = transportId;
    return result;
  }

  FleetSession._();

  factory FleetSession.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory FleetSession.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'FleetSession',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aOS(2, _omitFieldNames ? '' : 'user')
    ..aOS(3, _omitFieldNames ? '' : 'repo')
    ..aOS(4, _omitFieldNames ? '' : 'backend')
    ..aOS(5, _omitFieldNames ? '' : 'baseUrl')
    ..aOS(6, _omitFieldNames ? '' : 'tokenRef')
    ..aOS(7, _omitFieldNames ? '' : 'skill')
    ..aOS(8, _omitFieldNames ? '' : 'slackTriggerChannel')
    ..aOS(9, _omitFieldNames ? '' : 'slackProgressChannel')
    ..a<$fixnum.Int64>(
        10, _omitFieldNames ? '' : 'pollSecs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOB(11, _omitFieldNames ? '' : 'enabled')
    ..aInt64(12, _omitFieldNames ? '' : 'createdAt')
    ..aInt64(13, _omitFieldNames ? '' : 'updatedAt')
    ..aOS(14, _omitFieldNames ? '' : 'forgeId')
    ..aOS(15, _omitFieldNames ? '' : 'transportId')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FleetSession clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FleetSession copyWith(void Function(FleetSession) updates) =>
      super.copyWith((message) => updates(message as FleetSession))
          as FleetSession;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static FleetSession create() => FleetSession._();
  @$core.override
  FleetSession createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static FleetSession getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<FleetSession>(create);
  static FleetSession? _defaultInstance;

  /// Identity — path-safe segments (server-validated). The workspace is
  /// `root/<user>/<id>`.
  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get user => $_getSZ(1);
  @$pb.TagNumber(2)
  set user($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasUser() => $_has(1);
  @$pb.TagNumber(2)
  void clearUser() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get repo => $_getSZ(2);
  @$pb.TagNumber(3)
  set repo($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasRepo() => $_has(2);
  @$pb.TagNumber(3)
  void clearRepo() => $_clearField(3);

  /// Forge: "github" | "gitlab" | "" (resolve as a registered forge).
  @$pb.TagNumber(4)
  $core.String get backend => $_getSZ(3);
  @$pb.TagNumber(4)
  set backend($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasBackend() => $_has(3);
  @$pb.TagNumber(4)
  void clearBackend() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get baseUrl => $_getSZ(4);
  @$pb.TagNumber(5)
  set baseUrl($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasBaseUrl() => $_has(4);
  @$pb.TagNumber(5)
  void clearBaseUrl() => $_clearField(5);

  /// Kind-prefixed token reference: `env:NAME` or `file:/path` — NEVER the secret.
  @$pb.TagNumber(6)
  $core.String get tokenRef => $_getSZ(5);
  @$pb.TagNumber(6)
  set tokenRef($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasTokenRef() => $_has(5);
  @$pb.TagNumber(6)
  void clearTokenRef() => $_clearField(6);

  /// Review skill/prompt selector ("" ⇒ the default review skill).
  @$pb.TagNumber(7)
  $core.String get skill => $_getSZ(6);
  @$pb.TagNumber(7)
  set skill($core.String value) => $_setString(6, value);
  @$pb.TagNumber(7)
  $core.bool hasSkill() => $_has(6);
  @$pb.TagNumber(7)
  void clearSkill() => $_clearField(7);

  /// Slack triggers ("" ⇒ no Slack trigger / no progress surfacing).
  @$pb.TagNumber(8)
  $core.String get slackTriggerChannel => $_getSZ(7);
  @$pb.TagNumber(8)
  set slackTriggerChannel($core.String value) => $_setString(7, value);
  @$pb.TagNumber(8)
  $core.bool hasSlackTriggerChannel() => $_has(7);
  @$pb.TagNumber(8)
  void clearSlackTriggerChannel() => $_clearField(8);

  @$pb.TagNumber(9)
  $core.String get slackProgressChannel => $_getSZ(8);
  @$pb.TagNumber(9)
  set slackProgressChannel($core.String value) => $_setString(8, value);
  @$pb.TagNumber(9)
  $core.bool hasSlackProgressChannel() => $_has(8);
  @$pb.TagNumber(9)
  void clearSlackProgressChannel() => $_clearField(9);

  /// Forge poll interval in seconds (clamped to [MIN, MAX] on ingest; 0 ⇒ default).
  @$pb.TagNumber(10)
  $fixnum.Int64 get pollSecs => $_getI64(9);
  @$pb.TagNumber(10)
  set pollSecs($fixnum.Int64 value) => $_setInt64(9, value);
  @$pb.TagNumber(10)
  $core.bool hasPollSecs() => $_has(9);
  @$pb.TagNumber(10)
  void clearPollSecs() => $_clearField(10);

  @$pb.TagNumber(11)
  $core.bool get enabled => $_getBF(10);
  @$pb.TagNumber(11)
  set enabled($core.bool value) => $_setBool(10, value);
  @$pb.TagNumber(11)
  $core.bool hasEnabled() => $_has(10);
  @$pb.TagNumber(11)
  void clearEnabled() => $_clearField(11);

  /// Unix-seconds metadata (clamped non-negative on ingest).
  @$pb.TagNumber(12)
  $fixnum.Int64 get createdAt => $_getI64(11);
  @$pb.TagNumber(12)
  set createdAt($fixnum.Int64 value) => $_setInt64(11, value);
  @$pb.TagNumber(12)
  $core.bool hasCreatedAt() => $_has(11);
  @$pb.TagNumber(12)
  void clearCreatedAt() => $_clearField(12);

  @$pb.TagNumber(13)
  $fixnum.Int64 get updatedAt => $_getI64(12);
  @$pb.TagNumber(13)
  set updatedAt($fixnum.Int64 value) => $_setInt64(12, value);
  @$pb.TagNumber(13)
  $core.bool hasUpdatedAt() => $_has(12);
  @$pb.TagNumber(13)
  void clearUpdatedAt() => $_clearField(13);

  /// Persisted forge card id (config C36 / D1b). When set, the row's forge is built
  /// from the `ForgeCard` of this id in the `ForgeRegistry` (its kind/base_url/
  /// token_ref/repo_encoding), and the inline `backend`/`base_url`/`token_ref` above
  /// are ignored. Empty ⇒ the inline fields are used (unchanged behavior). Path-safe.
  @$pb.TagNumber(14)
  $core.String get forgeId => $_getSZ(13);
  @$pb.TagNumber(14)
  set forgeId($core.String value) => $_setString(13, value);
  @$pb.TagNumber(14)
  $core.bool hasForgeId() => $_has(13);
  @$pb.TagNumber(14)
  void clearForgeId() => $_clearField(14);

  /// Persisted transport card id (config C37 / D2b). When set, the row's Slack watch
  /// (and, later, its progress feed) is driven by the `TransportCard` of this id in the
  /// `TransportRegistry`: its `app_token_ref` supplies the Socket-Mode token and its
  /// `trigger`-purpose channel bindings supersede `slack_trigger_channel` above. Empty ⇒
  /// the inline `slack_*` fields + the `[review_fleet.slack]` default token are used
  /// (unchanged behavior). Path-safe.
  @$pb.TagNumber(15)
  $core.String get transportId => $_getSZ(14);
  @$pb.TagNumber(15)
  set transportId($core.String value) => $_setString(14, value);
  @$pb.TagNumber(15)
  $core.bool hasTransportId() => $_has(14);
  @$pb.TagNumber(15)
  void clearTransportId() => $_clearField(15);
}

class FleetListRequest extends $pb.GeneratedMessage {
  factory FleetListRequest() => create();

  FleetListRequest._();

  factory FleetListRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory FleetListRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'FleetListRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FleetListRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FleetListRequest copyWith(void Function(FleetListRequest) updates) =>
      super.copyWith((message) => updates(message as FleetListRequest))
          as FleetListRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static FleetListRequest create() => FleetListRequest._();
  @$core.override
  FleetListRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static FleetListRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<FleetListRequest>(create);
  static FleetListRequest? _defaultInstance;
}

class FleetSessionList extends $pb.GeneratedMessage {
  factory FleetSessionList({
    $core.Iterable<FleetSession>? sessions,
  }) {
    final result = create();
    if (sessions != null) result.sessions.addAll(sessions);
    return result;
  }

  FleetSessionList._();

  factory FleetSessionList.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory FleetSessionList.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'FleetSessionList',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<FleetSession>(1, _omitFieldNames ? '' : 'sessions',
        subBuilder: FleetSession.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FleetSessionList clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FleetSessionList copyWith(void Function(FleetSessionList) updates) =>
      super.copyWith((message) => updates(message as FleetSessionList))
          as FleetSessionList;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static FleetSessionList create() => FleetSessionList._();
  @$core.override
  FleetSessionList createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static FleetSessionList getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<FleetSessionList>(create);
  static FleetSessionList? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<FleetSession> get sessions => $_getList(0);
}

class FleetSessionRef extends $pb.GeneratedMessage {
  factory FleetSessionRef({
    $core.String? id,
  }) {
    final result = create();
    if (id != null) result.id = id;
    return result;
  }

  FleetSessionRef._();

  factory FleetSessionRef.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory FleetSessionRef.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'FleetSessionRef',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FleetSessionRef clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FleetSessionRef copyWith(void Function(FleetSessionRef) updates) =>
      super.copyWith((message) => updates(message as FleetSessionRef))
          as FleetSessionRef;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static FleetSessionRef create() => FleetSessionRef._();
  @$core.override
  FleetSessionRef createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static FleetSessionRef getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<FleetSessionRef>(create);
  static FleetSessionRef? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);
}

class FleetDeleteReply extends $pb.GeneratedMessage {
  factory FleetDeleteReply({
    $core.bool? deleted,
  }) {
    final result = create();
    if (deleted != null) result.deleted = deleted;
    return result;
  }

  FleetDeleteReply._();

  factory FleetDeleteReply.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory FleetDeleteReply.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'FleetDeleteReply',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'deleted')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FleetDeleteReply clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FleetDeleteReply copyWith(void Function(FleetDeleteReply) updates) =>
      super.copyWith((message) => updates(message as FleetDeleteReply))
          as FleetDeleteReply;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static FleetDeleteReply create() => FleetDeleteReply._();
  @$core.override
  FleetDeleteReply createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static FleetDeleteReply getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<FleetDeleteReply>(create);
  static FleetDeleteReply? _defaultInstance;

  @$pb.TagNumber(1)
  $core.bool get deleted => $_getBF(0);
  @$pb.TagNumber(1)
  set deleted($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasDeleted() => $_has(0);
  @$pb.TagNumber(1)
  void clearDeleted() => $_clearField(1);
}

class FleetSetEnabledRequest extends $pb.GeneratedMessage {
  factory FleetSetEnabledRequest({
    $core.String? id,
    $core.bool? enabled,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (enabled != null) result.enabled = enabled;
    return result;
  }

  FleetSetEnabledRequest._();

  factory FleetSetEnabledRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory FleetSetEnabledRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'FleetSetEnabledRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aOB(2, _omitFieldNames ? '' : 'enabled')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FleetSetEnabledRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FleetSetEnabledRequest copyWith(
          void Function(FleetSetEnabledRequest) updates) =>
      super.copyWith((message) => updates(message as FleetSetEnabledRequest))
          as FleetSetEnabledRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static FleetSetEnabledRequest create() => FleetSetEnabledRequest._();
  @$core.override
  FleetSetEnabledRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static FleetSetEnabledRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<FleetSetEnabledRequest>(create);
  static FleetSetEnabledRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.bool get enabled => $_getBF(1);
  @$pb.TagNumber(2)
  set enabled($core.bool value) => $_setBool(1, value);
  @$pb.TagNumber(2)
  $core.bool hasEnabled() => $_has(1);
  @$pb.TagNumber(2)
  void clearEnabled() => $_clearField(2);
}

/// Manually queue a review for one roster session's PR (review-fleet C8). Until the
/// real triggers land (forge poll C6 / Slack watch C7, increment 4), this is how a
/// review is kicked off. `session_id` names the roster row; `pr_number` the PR to
/// review.
class ReviewNowRequest extends $pb.GeneratedMessage {
  factory ReviewNowRequest({
    $core.String? sessionId,
    $fixnum.Int64? prNumber,
  }) {
    final result = create();
    if (sessionId != null) result.sessionId = sessionId;
    if (prNumber != null) result.prNumber = prNumber;
    return result;
  }

  ReviewNowRequest._();

  factory ReviewNowRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ReviewNowRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewNowRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'sessionId')
    ..a<$fixnum.Int64>(
        2, _omitFieldNames ? '' : 'prNumber', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewNowRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewNowRequest copyWith(void Function(ReviewNowRequest) updates) =>
      super.copyWith((message) => updates(message as ReviewNowRequest))
          as ReviewNowRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ReviewNowRequest create() => ReviewNowRequest._();
  @$core.override
  ReviewNowRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ReviewNowRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewNowRequest>(create);
  static ReviewNowRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get sessionId => $_getSZ(0);
  @$pb.TagNumber(1)
  set sessionId($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSessionId() => $_has(0);
  @$pb.TagNumber(1)
  void clearSessionId() => $_clearField(1);

  @$pb.TagNumber(2)
  $fixnum.Int64 get prNumber => $_getI64(1);
  @$pb.TagNumber(2)
  set prNumber($fixnum.Int64 value) => $_setInt64(1, value);
  @$pb.TagNumber(2)
  $core.bool hasPrNumber() => $_has(1);
  @$pb.TagNumber(2)
  void clearPrNumber() => $_clearField(2);
}

/// Whether the trigger was queued or folded into one already pending/in-flight (the
/// bounded queue coalesces duplicates and on overflow — never a silent drop).
class ReviewNowReply extends $pb.GeneratedMessage {
  factory ReviewNowReply({
    $core.bool? accepted,
  }) {
    final result = create();
    if (accepted != null) result.accepted = accepted;
    return result;
  }

  ReviewNowReply._();

  factory ReviewNowReply.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ReviewNowReply.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewNowReply',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'accepted')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewNowReply clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewNowReply copyWith(void Function(ReviewNowReply) updates) =>
      super.copyWith((message) => updates(message as ReviewNowReply))
          as ReviewNowReply;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ReviewNowReply create() => ReviewNowReply._();
  @$core.override
  ReviewNowReply createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ReviewNowReply getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewNowReply>(create);
  static ReviewNowReply? _defaultInstance;

  @$pb.TagNumber(1)
  $core.bool get accepted => $_getBF(0);
  @$pb.TagNumber(1)
  set accepted($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasAccepted() => $_has(0);
  @$pb.TagNumber(1)
  void clearAccepted() => $_clearField(1);
}

/// Approve a persisted review draft and post it to its forge (review-fleet C17, the
/// approve→post tail). `review_id` names the draft row (server-minted Uuid, persisted at
/// `drafted`); the approval gesture is the ONLY thing that posts — nothing is posted
/// without an explicit Approve call. Idempotent: a draft already `posted` is a no-op.
class ApproveRequest extends $pb.GeneratedMessage {
  factory ApproveRequest({
    $core.String? reviewId,
  }) {
    final result = create();
    if (reviewId != null) result.reviewId = reviewId;
    return result;
  }

  ApproveRequest._();

  factory ApproveRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ApproveRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ApproveRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'reviewId')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ApproveRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ApproveRequest copyWith(void Function(ApproveRequest) updates) =>
      super.copyWith((message) => updates(message as ApproveRequest))
          as ApproveRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ApproveRequest create() => ApproveRequest._();
  @$core.override
  ApproveRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ApproveRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ApproveRequest>(create);
  static ApproveRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get reviewId => $_getSZ(0);
  @$pb.TagNumber(1)
  set reviewId($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasReviewId() => $_has(0);
  @$pb.TagNumber(1)
  void clearReviewId() => $_clearField(1);
}

/// The outcome of an Approve. `status` is one of:
///   "posted"         — the draft was posted this call (freshly).
///   "already_posted" — idempotent no-op: the draft was already posted.
///   "not_found"      — no persisted draft for this `review_id`.
/// `detail` carries the posted comment/review URL when `status = "posted"`, else "".
class ApproveReply extends $pb.GeneratedMessage {
  factory ApproveReply({
    $core.String? status,
    $core.String? detail,
  }) {
    final result = create();
    if (status != null) result.status = status;
    if (detail != null) result.detail = detail;
    return result;
  }

  ApproveReply._();

  factory ApproveReply.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ApproveReply.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ApproveReply',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'status')
    ..aOS(2, _omitFieldNames ? '' : 'detail')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ApproveReply clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ApproveReply copyWith(void Function(ApproveReply) updates) =>
      super.copyWith((message) => updates(message as ApproveReply))
          as ApproveReply;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ApproveReply create() => ApproveReply._();
  @$core.override
  ApproveReply createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ApproveReply getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ApproveReply>(create);
  static ApproveReply? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get status => $_getSZ(0);
  @$pb.TagNumber(1)
  set status($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasStatus() => $_has(0);
  @$pb.TagNumber(1)
  void clearStatus() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get detail => $_getSZ(1);
  @$pb.TagNumber(2)
  set detail($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasDetail() => $_has(1);
  @$pb.TagNumber(2)
  void clearDetail() => $_clearField(2);
}

/// One persisted review draft's METADATA (review-fleet C14). Mirrors the non-body
/// fields of `agent_core::ReviewDraftRecord` — the `.md` body is fetched separately
/// via `GetReview` (it is large + LLM-authored, so it is never bundled into a list).
/// `repo` is the roster repo key (`owner__name`); `status` is one of drafted |
/// approved | posted | superseded.
class ReviewSummary extends $pb.GeneratedMessage {
  factory ReviewSummary({
    $core.String? reviewId,
    $core.String? repo,
    $fixnum.Int64? prNumber,
    $core.String? headSha,
    $core.double? riskScore,
    $core.bool? gateFailed,
    $core.int? nFindings,
    $core.int? filesChanged,
    $core.int? additions,
    $core.int? deletions,
    $core.String? status,
  }) {
    final result = create();
    if (reviewId != null) result.reviewId = reviewId;
    if (repo != null) result.repo = repo;
    if (prNumber != null) result.prNumber = prNumber;
    if (headSha != null) result.headSha = headSha;
    if (riskScore != null) result.riskScore = riskScore;
    if (gateFailed != null) result.gateFailed = gateFailed;
    if (nFindings != null) result.nFindings = nFindings;
    if (filesChanged != null) result.filesChanged = filesChanged;
    if (additions != null) result.additions = additions;
    if (deletions != null) result.deletions = deletions;
    if (status != null) result.status = status;
    return result;
  }

  ReviewSummary._();

  factory ReviewSummary.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ReviewSummary.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReviewSummary',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'reviewId')
    ..aOS(2, _omitFieldNames ? '' : 'repo')
    ..a<$fixnum.Int64>(
        3, _omitFieldNames ? '' : 'prNumber', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOS(4, _omitFieldNames ? '' : 'headSha')
    ..aD(5, _omitFieldNames ? '' : 'riskScore')
    ..aOB(6, _omitFieldNames ? '' : 'gateFailed')
    ..aI(7, _omitFieldNames ? '' : 'nFindings', fieldType: $pb.PbFieldType.OU3)
    ..aI(8, _omitFieldNames ? '' : 'filesChanged',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(9, _omitFieldNames ? '' : 'additions', fieldType: $pb.PbFieldType.OU3)
    ..aI(10, _omitFieldNames ? '' : 'deletions', fieldType: $pb.PbFieldType.OU3)
    ..aOS(11, _omitFieldNames ? '' : 'status')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewSummary clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReviewSummary copyWith(void Function(ReviewSummary) updates) =>
      super.copyWith((message) => updates(message as ReviewSummary))
          as ReviewSummary;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ReviewSummary create() => ReviewSummary._();
  @$core.override
  ReviewSummary createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ReviewSummary getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReviewSummary>(create);
  static ReviewSummary? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get reviewId => $_getSZ(0);
  @$pb.TagNumber(1)
  set reviewId($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasReviewId() => $_has(0);
  @$pb.TagNumber(1)
  void clearReviewId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get repo => $_getSZ(1);
  @$pb.TagNumber(2)
  set repo($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasRepo() => $_has(1);
  @$pb.TagNumber(2)
  void clearRepo() => $_clearField(2);

  @$pb.TagNumber(3)
  $fixnum.Int64 get prNumber => $_getI64(2);
  @$pb.TagNumber(3)
  set prNumber($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasPrNumber() => $_has(2);
  @$pb.TagNumber(3)
  void clearPrNumber() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get headSha => $_getSZ(3);
  @$pb.TagNumber(4)
  set headSha($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasHeadSha() => $_has(3);
  @$pb.TagNumber(4)
  void clearHeadSha() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.double get riskScore => $_getN(4);
  @$pb.TagNumber(5)
  set riskScore($core.double value) => $_setDouble(4, value);
  @$pb.TagNumber(5)
  $core.bool hasRiskScore() => $_has(4);
  @$pb.TagNumber(5)
  void clearRiskScore() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.bool get gateFailed => $_getBF(5);
  @$pb.TagNumber(6)
  set gateFailed($core.bool value) => $_setBool(5, value);
  @$pb.TagNumber(6)
  $core.bool hasGateFailed() => $_has(5);
  @$pb.TagNumber(6)
  void clearGateFailed() => $_clearField(6);

  @$pb.TagNumber(7)
  $core.int get nFindings => $_getIZ(6);
  @$pb.TagNumber(7)
  set nFindings($core.int value) => $_setUnsignedInt32(6, value);
  @$pb.TagNumber(7)
  $core.bool hasNFindings() => $_has(6);
  @$pb.TagNumber(7)
  void clearNFindings() => $_clearField(7);

  @$pb.TagNumber(8)
  $core.int get filesChanged => $_getIZ(7);
  @$pb.TagNumber(8)
  set filesChanged($core.int value) => $_setUnsignedInt32(7, value);
  @$pb.TagNumber(8)
  $core.bool hasFilesChanged() => $_has(7);
  @$pb.TagNumber(8)
  void clearFilesChanged() => $_clearField(8);

  @$pb.TagNumber(9)
  $core.int get additions => $_getIZ(8);
  @$pb.TagNumber(9)
  set additions($core.int value) => $_setUnsignedInt32(8, value);
  @$pb.TagNumber(9)
  $core.bool hasAdditions() => $_has(8);
  @$pb.TagNumber(9)
  void clearAdditions() => $_clearField(9);

  @$pb.TagNumber(10)
  $core.int get deletions => $_getIZ(9);
  @$pb.TagNumber(10)
  set deletions($core.int value) => $_setUnsignedInt32(9, value);
  @$pb.TagNumber(10)
  $core.bool hasDeletions() => $_has(9);
  @$pb.TagNumber(10)
  void clearDeletions() => $_clearField(10);

  @$pb.TagNumber(11)
  $core.String get status => $_getSZ(10);
  @$pb.TagNumber(11)
  set status($core.String value) => $_setString(10, value);
  @$pb.TagNumber(11)
  $core.bool hasStatus() => $_has(10);
  @$pb.TagNumber(11)
  void clearStatus() => $_clearField(11);
}

/// List persisted review drafts (review-fleet C14). All filters are optional AND-ed
/// (empty ⇒ no constraint); every filter value is bound as a query argument, never
/// interpolated. `limit` is clamped to a server cap (0 ⇒ the cap). Read-only.
class ListReviewsRequest extends $pb.GeneratedMessage {
  factory ListReviewsRequest({
    $core.String? repo,
    $core.String? sessionId,
    $core.String? status,
    $core.int? limit,
  }) {
    final result = create();
    if (repo != null) result.repo = repo;
    if (sessionId != null) result.sessionId = sessionId;
    if (status != null) result.status = status;
    if (limit != null) result.limit = limit;
    return result;
  }

  ListReviewsRequest._();

  factory ListReviewsRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ListReviewsRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ListReviewsRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'repo')
    ..aOS(2, _omitFieldNames ? '' : 'sessionId')
    ..aOS(3, _omitFieldNames ? '' : 'status')
    ..aI(4, _omitFieldNames ? '' : 'limit', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListReviewsRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListReviewsRequest copyWith(void Function(ListReviewsRequest) updates) =>
      super.copyWith((message) => updates(message as ListReviewsRequest))
          as ListReviewsRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ListReviewsRequest create() => ListReviewsRequest._();
  @$core.override
  ListReviewsRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ListReviewsRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ListReviewsRequest>(create);
  static ListReviewsRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get repo => $_getSZ(0);
  @$pb.TagNumber(1)
  set repo($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRepo() => $_has(0);
  @$pb.TagNumber(1)
  void clearRepo() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get sessionId => $_getSZ(1);
  @$pb.TagNumber(2)
  set sessionId($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasSessionId() => $_has(1);
  @$pb.TagNumber(2)
  void clearSessionId() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get status => $_getSZ(2);
  @$pb.TagNumber(3)
  set status($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasStatus() => $_has(2);
  @$pb.TagNumber(3)
  void clearStatus() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get limit => $_getIZ(3);
  @$pb.TagNumber(4)
  set limit($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasLimit() => $_has(3);
  @$pb.TagNumber(4)
  void clearLimit() => $_clearField(4);
}

class ListReviewsReply extends $pb.GeneratedMessage {
  factory ListReviewsReply({
    $core.Iterable<ReviewSummary>? reviews,
  }) {
    final result = create();
    if (reviews != null) result.reviews.addAll(reviews);
    return result;
  }

  ListReviewsReply._();

  factory ListReviewsReply.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ListReviewsReply.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ListReviewsReply',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<ReviewSummary>(1, _omitFieldNames ? '' : 'reviews',
        subBuilder: ReviewSummary.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListReviewsReply clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListReviewsReply copyWith(void Function(ListReviewsReply) updates) =>
      super.copyWith((message) => updates(message as ListReviewsReply))
          as ListReviewsReply;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ListReviewsReply create() => ListReviewsReply._();
  @$core.override
  ListReviewsReply createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ListReviewsReply getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ListReviewsReply>(create);
  static ListReviewsReply? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<ReviewSummary> get reviews => $_getList(0);
}

/// Fetch one draft's metadata + rendered markdown BODY (review-fleet C14). `review_id`
/// is untrusted wire input, looked up as a bound query arg; the server reads the body
/// from the draft's own server-minted `draft_path` (never a wire-supplied path). The
/// body is size-capped in the reply (`truncated` set when the on-disk `.md` exceeded
/// the cap).
class GetReviewRequest extends $pb.GeneratedMessage {
  factory GetReviewRequest({
    $core.String? reviewId,
  }) {
    final result = create();
    if (reviewId != null) result.reviewId = reviewId;
    return result;
  }

  GetReviewRequest._();

  factory GetReviewRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GetReviewRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GetReviewRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'reviewId')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetReviewRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetReviewRequest copyWith(void Function(GetReviewRequest) updates) =>
      super.copyWith((message) => updates(message as GetReviewRequest))
          as GetReviewRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GetReviewRequest create() => GetReviewRequest._();
  @$core.override
  GetReviewRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GetReviewRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GetReviewRequest>(create);
  static GetReviewRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get reviewId => $_getSZ(0);
  @$pb.TagNumber(1)
  set reviewId($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasReviewId() => $_has(0);
  @$pb.TagNumber(1)
  void clearReviewId() => $_clearField(1);
}

class GetReviewReply extends $pb.GeneratedMessage {
  factory GetReviewReply({
    ReviewSummary? meta,
    $core.String? body,
    $core.bool? truncated,
  }) {
    final result = create();
    if (meta != null) result.meta = meta;
    if (body != null) result.body = body;
    if (truncated != null) result.truncated = truncated;
    return result;
  }

  GetReviewReply._();

  factory GetReviewReply.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GetReviewReply.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GetReviewReply',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<ReviewSummary>(1, _omitFieldNames ? '' : 'meta',
        subBuilder: ReviewSummary.create)
    ..aOS(2, _omitFieldNames ? '' : 'body')
    ..aOB(3, _omitFieldNames ? '' : 'truncated')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetReviewReply clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetReviewReply copyWith(void Function(GetReviewReply) updates) =>
      super.copyWith((message) => updates(message as GetReviewReply))
          as GetReviewReply;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GetReviewReply create() => GetReviewReply._();
  @$core.override
  GetReviewReply createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GetReviewReply getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GetReviewReply>(create);
  static GetReviewReply? _defaultInstance;

  @$pb.TagNumber(1)
  ReviewSummary get meta => $_getN(0);
  @$pb.TagNumber(1)
  set meta(ReviewSummary value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasMeta() => $_has(0);
  @$pb.TagNumber(1)
  void clearMeta() => $_clearField(1);
  @$pb.TagNumber(1)
  ReviewSummary ensureMeta() => $_ensure(0);

  @$pb.TagNumber(2)
  $core.String get body => $_getSZ(1);
  @$pb.TagNumber(2)
  set body($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBody() => $_has(1);
  @$pb.TagNumber(2)
  void clearBody() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.bool get truncated => $_getBF(2);
  @$pb.TagNumber(3)
  set truncated($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasTruncated() => $_has(2);
  @$pb.TagNumber(3)
  void clearTruncated() => $_clearField(3);
}

/// Rewrite a draft's markdown body in place (review-fleet C14, the portal's GitHub-style edit).
/// `review_id` is untrusted wire input, looked up as a bound query arg; the new `body` is written
/// to the draft's own server-minted `draft_path` (confined under the fleet root, size-capped) —
/// never a wire-supplied path. Only an un-posted draft is editable: a draft whose status is
/// `posted` or `approved` is locked (an edit would diverge from what was posted).
class UpdateReviewRequest extends $pb.GeneratedMessage {
  factory UpdateReviewRequest({
    $core.String? reviewId,
    $core.String? body,
  }) {
    final result = create();
    if (reviewId != null) result.reviewId = reviewId;
    if (body != null) result.body = body;
    return result;
  }

  UpdateReviewRequest._();

  factory UpdateReviewRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory UpdateReviewRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'UpdateReviewRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'reviewId')
    ..aOS(2, _omitFieldNames ? '' : 'body')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpdateReviewRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpdateReviewRequest copyWith(void Function(UpdateReviewRequest) updates) =>
      super.copyWith((message) => updates(message as UpdateReviewRequest))
          as UpdateReviewRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static UpdateReviewRequest create() => UpdateReviewRequest._();
  @$core.override
  UpdateReviewRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static UpdateReviewRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<UpdateReviewRequest>(create);
  static UpdateReviewRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get reviewId => $_getSZ(0);
  @$pb.TagNumber(1)
  set reviewId($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasReviewId() => $_has(0);
  @$pb.TagNumber(1)
  void clearReviewId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get body => $_getSZ(1);
  @$pb.TagNumber(2)
  set body($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBody() => $_has(1);
  @$pb.TagNumber(2)
  void clearBody() => $_clearField(2);
}

/// The outcome of an UpdateReview. `status` is one of:
///   "updated"   — the body was rewritten.
///   "not_found" — no persisted draft for this `review_id`.
///   "locked"    — the draft is `posted`/`approved` and cannot be edited.
class UpdateReviewReply extends $pb.GeneratedMessage {
  factory UpdateReviewReply({
    $core.String? status,
  }) {
    final result = create();
    if (status != null) result.status = status;
    return result;
  }

  UpdateReviewReply._();

  factory UpdateReviewReply.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory UpdateReviewReply.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'UpdateReviewReply',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'status')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpdateReviewReply clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpdateReviewReply copyWith(void Function(UpdateReviewReply) updates) =>
      super.copyWith((message) => updates(message as UpdateReviewReply))
          as UpdateReviewReply;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static UpdateReviewReply create() => UpdateReviewReply._();
  @$core.override
  UpdateReviewReply createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static UpdateReviewReply getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<UpdateReviewReply>(create);
  static UpdateReviewReply? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get status => $_getSZ(0);
  @$pb.TagNumber(1)
  set status($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasStatus() => $_has(0);
  @$pb.TagNumber(1)
  void clearStatus() => $_clearField(1);
}

/// Operational self-diagnosis of a running fleet process (docs/design/doctor/): run
/// the same probe set as `agent doctor` over gRPC, so a portal or a remote operator
/// can ask "are your dependencies healthy?" without shelling into the box. No input.
class PreflightRequest extends $pb.GeneratedMessage {
  factory PreflightRequest() => create();

  PreflightRequest._();

  factory PreflightRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PreflightRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PreflightRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PreflightRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PreflightRequest copyWith(void Function(PreflightRequest) updates) =>
      super.copyWith((message) => updates(message as PreflightRequest))
          as PreflightRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PreflightRequest create() => PreflightRequest._();
  @$core.override
  PreflightRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PreflightRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PreflightRequest>(create);
  static PreflightRequest? _defaultInstance;
}

/// One probe's outcome. `status` is "ok" | "warn" | "fail" | "skipped". `detail` is a
/// status class or a trusted config value (an address/path) — NEVER a resolved secret
/// or a raw error body. `latency_ms` is the probe's wall-clock.
class PreflightProbe extends $pb.GeneratedMessage {
  factory PreflightProbe({
    $core.String? name,
    $core.String? status,
    $core.String? detail,
    $core.int? latencyMs,
  }) {
    final result = create();
    if (name != null) result.name = name;
    if (status != null) result.status = status;
    if (detail != null) result.detail = detail;
    if (latencyMs != null) result.latencyMs = latencyMs;
    return result;
  }

  PreflightProbe._();

  factory PreflightProbe.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PreflightProbe.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PreflightProbe',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'name')
    ..aOS(2, _omitFieldNames ? '' : 'status')
    ..aOS(3, _omitFieldNames ? '' : 'detail')
    ..aI(4, _omitFieldNames ? '' : 'latencyMs', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PreflightProbe clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PreflightProbe copyWith(void Function(PreflightProbe) updates) =>
      super.copyWith((message) => updates(message as PreflightProbe))
          as PreflightProbe;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PreflightProbe create() => PreflightProbe._();
  @$core.override
  PreflightProbe createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PreflightProbe getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PreflightProbe>(create);
  static PreflightProbe? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get name => $_getSZ(0);
  @$pb.TagNumber(1)
  set name($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasName() => $_has(0);
  @$pb.TagNumber(1)
  void clearName() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get status => $_getSZ(1);
  @$pb.TagNumber(2)
  set status($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasStatus() => $_has(1);
  @$pb.TagNumber(2)
  void clearStatus() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get detail => $_getSZ(2);
  @$pb.TagNumber(3)
  set detail($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasDetail() => $_has(2);
  @$pb.TagNumber(3)
  void clearDetail() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get latencyMs => $_getIZ(3);
  @$pb.TagNumber(4)
  set latencyMs($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasLatencyMs() => $_has(3);
  @$pb.TagNumber(4)
  void clearLatencyMs() => $_clearField(4);
}

/// The aggregate report. `ok` is the gate: true iff no probe failed (warn/skipped do
/// not fail). `probes` preserves the probe order.
class PreflightReply extends $pb.GeneratedMessage {
  factory PreflightReply({
    $core.bool? ok,
    $core.Iterable<PreflightProbe>? probes,
  }) {
    final result = create();
    if (ok != null) result.ok = ok;
    if (probes != null) result.probes.addAll(probes);
    return result;
  }

  PreflightReply._();

  factory PreflightReply.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PreflightReply.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PreflightReply',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'ok')
    ..pPM<PreflightProbe>(2, _omitFieldNames ? '' : 'probes',
        subBuilder: PreflightProbe.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PreflightReply clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PreflightReply copyWith(void Function(PreflightReply) updates) =>
      super.copyWith((message) => updates(message as PreflightReply))
          as PreflightReply;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PreflightReply create() => PreflightReply._();
  @$core.override
  PreflightReply createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PreflightReply getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PreflightReply>(create);
  static PreflightReply? _defaultInstance;

  @$pb.TagNumber(1)
  $core.bool get ok => $_getBF(0);
  @$pb.TagNumber(1)
  set ok($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasOk() => $_has(0);
  @$pb.TagNumber(1)
  void clearOk() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<PreflightProbe> get probes => $_getList(1);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
