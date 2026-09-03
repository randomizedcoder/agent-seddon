// This is a generated file - do not edit.
//
// Generated from agent/v1/digest.proto.

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

/// One ledger row. `kind` is a CLOSED set ("summary" | "facts" | "objective" |
/// "alternatives") — an unknown discriminator is rejected/skipped, never stored.
class Digest extends $pb.GeneratedMessage {
  factory Digest({
    $core.String? sessionId,
    $core.String? userId,
    $fixnum.Int64? seq,
    $core.String? kind,
    $core.String? text,
    $core.Iterable<$core.String>? keywords,
    $core.String? mode,
    $core.String? model,
    $fixnum.Int64? tsMs,
    $core.int? durationMs,
    $core.int? tokens,
  }) {
    final result = create();
    if (sessionId != null) result.sessionId = sessionId;
    if (userId != null) result.userId = userId;
    if (seq != null) result.seq = seq;
    if (kind != null) result.kind = kind;
    if (text != null) result.text = text;
    if (keywords != null) result.keywords.addAll(keywords);
    if (mode != null) result.mode = mode;
    if (model != null) result.model = model;
    if (tsMs != null) result.tsMs = tsMs;
    if (durationMs != null) result.durationMs = durationMs;
    if (tokens != null) result.tokens = tokens;
    return result;
  }

  Digest._();

  factory Digest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory Digest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Digest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'sessionId')
    ..aOS(2, _omitFieldNames ? '' : 'userId')
    ..a<$fixnum.Int64>(3, _omitFieldNames ? '' : 'seq', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOS(4, _omitFieldNames ? '' : 'kind')
    ..aOS(5, _omitFieldNames ? '' : 'text')
    ..pPS(6, _omitFieldNames ? '' : 'keywords')
    ..aOS(7, _omitFieldNames ? '' : 'mode')
    ..aOS(8, _omitFieldNames ? '' : 'model')
    ..a<$fixnum.Int64>(9, _omitFieldNames ? '' : 'tsMs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aI(10, _omitFieldNames ? '' : 'durationMs',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(11, _omitFieldNames ? '' : 'tokens', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Digest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Digest copyWith(void Function(Digest) updates) =>
      super.copyWith((message) => updates(message as Digest)) as Digest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static Digest create() => Digest._();
  @$core.override
  Digest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static Digest getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<Digest>(create);
  static Digest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get sessionId => $_getSZ(0);
  @$pb.TagNumber(1)
  set sessionId($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSessionId() => $_has(0);
  @$pb.TagNumber(1)
  void clearSessionId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get userId => $_getSZ(1);
  @$pb.TagNumber(2)
  set userId($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasUserId() => $_has(1);
  @$pb.TagNumber(2)
  void clearUserId() => $_clearField(2);

  /// The per-session agreed-response ordinal this digest belongs to.
  @$pb.TagNumber(3)
  $fixnum.Int64 get seq => $_getI64(2);
  @$pb.TagNumber(3)
  set seq($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasSeq() => $_has(2);
  @$pb.TagNumber(3)
  void clearSeq() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get kind => $_getSZ(3);
  @$pb.TagNumber(4)
  set kind($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasKind() => $_has(3);
  @$pb.TagNumber(4)
  void clearKind() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get text => $_getSZ(4);
  @$pb.TagNumber(5)
  set text($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasText() => $_has(4);
  @$pb.TagNumber(5)
  void clearText() => $_clearField(5);

  @$pb.TagNumber(6)
  $pb.PbList<$core.String> get keywords => $_getList(5);

  /// `TaskMode` at delivery time (labels / relevance hints).
  @$pb.TagNumber(7)
  $core.String get mode => $_getSZ(6);
  @$pb.TagNumber(7)
  set mode($core.String value) => $_setString(6, value);
  @$pb.TagNumber(7)
  $core.bool hasMode() => $_has(6);
  @$pb.TagNumber(7)
  void clearMode() => $_clearField(7);

  /// The model that produced the distillation (cost/quality attribution).
  @$pb.TagNumber(8)
  $core.String get model => $_getSZ(7);
  @$pb.TagNumber(8)
  set model($core.String value) => $_setString(7, value);
  @$pb.TagNumber(8)
  $core.bool hasModel() => $_has(7);
  @$pb.TagNumber(8)
  void clearModel() => $_clearField(8);

  @$pb.TagNumber(9)
  $fixnum.Int64 get tsMs => $_getI64(8);
  @$pb.TagNumber(9)
  set tsMs($fixnum.Int64 value) => $_setInt64(8, value);
  @$pb.TagNumber(9)
  $core.bool hasTsMs() => $_has(8);
  @$pb.TagNumber(9)
  void clearTsMs() => $_clearField(9);

  @$pb.TagNumber(10)
  $core.int get durationMs => $_getIZ(9);
  @$pb.TagNumber(10)
  set durationMs($core.int value) => $_setUnsignedInt32(9, value);
  @$pb.TagNumber(10)
  $core.bool hasDurationMs() => $_has(9);
  @$pb.TagNumber(10)
  void clearDurationMs() => $_clearField(10);

  @$pb.TagNumber(11)
  $core.int get tokens => $_getIZ(10);
  @$pb.TagNumber(11)
  set tokens($core.int value) => $_setUnsignedInt32(10, value);
  @$pb.TagNumber(11)
  $core.bool hasTokens() => $_has(10);
  @$pb.TagNumber(11)
  void clearTokens() => $_clearField(11);
}

class PutDigestRequest extends $pb.GeneratedMessage {
  factory PutDigestRequest({
    Digest? digest,
  }) {
    final result = create();
    if (digest != null) result.digest = digest;
    return result;
  }

  PutDigestRequest._();

  factory PutDigestRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PutDigestRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PutDigestRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<Digest>(1, _omitFieldNames ? '' : 'digest', subBuilder: Digest.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutDigestRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutDigestRequest copyWith(void Function(PutDigestRequest) updates) =>
      super.copyWith((message) => updates(message as PutDigestRequest))
          as PutDigestRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PutDigestRequest create() => PutDigestRequest._();
  @$core.override
  PutDigestRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PutDigestRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PutDigestRequest>(create);
  static PutDigestRequest? _defaultInstance;

  @$pb.TagNumber(1)
  Digest get digest => $_getN(0);
  @$pb.TagNumber(1)
  set digest(Digest value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasDigest() => $_has(0);
  @$pb.TagNumber(1)
  void clearDigest() => $_clearField(1);
  @$pb.TagNumber(1)
  Digest ensureDigest() => $_ensure(0);
}

class PutDigestResponse extends $pb.GeneratedMessage {
  factory PutDigestResponse() => create();

  PutDigestResponse._();

  factory PutDigestResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PutDigestResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PutDigestResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutDigestResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutDigestResponse copyWith(void Function(PutDigestResponse) updates) =>
      super.copyWith((message) => updates(message as PutDigestResponse))
          as PutDigestResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PutDigestResponse create() => PutDigestResponse._();
  @$core.override
  PutDigestResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PutDigestResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PutDigestResponse>(create);
  static PutDigestResponse? _defaultInstance;
}

/// A ledger read: digests for one session, ordered by `seq` ascending.
/// `kind` empty = all kinds; `since_seq` 0 = from the start; `keywords_any`
/// empty = no keyword prefilter; `limit` 0 = the server cap (also the ceiling —
/// a hostile limit cannot unbound a read).
class QueryDigestsRequest extends $pb.GeneratedMessage {
  factory QueryDigestsRequest({
    $core.String? sessionId,
    $core.String? kind,
    $fixnum.Int64? sinceSeq,
    $core.Iterable<$core.String>? keywordsAny,
    $core.int? limit,
  }) {
    final result = create();
    if (sessionId != null) result.sessionId = sessionId;
    if (kind != null) result.kind = kind;
    if (sinceSeq != null) result.sinceSeq = sinceSeq;
    if (keywordsAny != null) result.keywordsAny.addAll(keywordsAny);
    if (limit != null) result.limit = limit;
    return result;
  }

  QueryDigestsRequest._();

  factory QueryDigestsRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory QueryDigestsRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'QueryDigestsRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'sessionId')
    ..aOS(2, _omitFieldNames ? '' : 'kind')
    ..a<$fixnum.Int64>(
        3, _omitFieldNames ? '' : 'sinceSeq', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..pPS(4, _omitFieldNames ? '' : 'keywordsAny')
    ..aI(5, _omitFieldNames ? '' : 'limit', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  QueryDigestsRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  QueryDigestsRequest copyWith(void Function(QueryDigestsRequest) updates) =>
      super.copyWith((message) => updates(message as QueryDigestsRequest))
          as QueryDigestsRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static QueryDigestsRequest create() => QueryDigestsRequest._();
  @$core.override
  QueryDigestsRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static QueryDigestsRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<QueryDigestsRequest>(create);
  static QueryDigestsRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get sessionId => $_getSZ(0);
  @$pb.TagNumber(1)
  set sessionId($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSessionId() => $_has(0);
  @$pb.TagNumber(1)
  void clearSessionId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get kind => $_getSZ(1);
  @$pb.TagNumber(2)
  set kind($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasKind() => $_has(1);
  @$pb.TagNumber(2)
  void clearKind() => $_clearField(2);

  @$pb.TagNumber(3)
  $fixnum.Int64 get sinceSeq => $_getI64(2);
  @$pb.TagNumber(3)
  set sinceSeq($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasSinceSeq() => $_has(2);
  @$pb.TagNumber(3)
  void clearSinceSeq() => $_clearField(3);

  @$pb.TagNumber(4)
  $pb.PbList<$core.String> get keywordsAny => $_getList(3);

  @$pb.TagNumber(5)
  $core.int get limit => $_getIZ(4);
  @$pb.TagNumber(5)
  set limit($core.int value) => $_setUnsignedInt32(4, value);
  @$pb.TagNumber(5)
  $core.bool hasLimit() => $_has(4);
  @$pb.TagNumber(5)
  void clearLimit() => $_clearField(5);
}

class QueryDigestsResponse extends $pb.GeneratedMessage {
  factory QueryDigestsResponse({
    $core.Iterable<Digest>? digests,
  }) {
    final result = create();
    if (digests != null) result.digests.addAll(digests);
    return result;
  }

  QueryDigestsResponse._();

  factory QueryDigestsResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory QueryDigestsResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'QueryDigestsResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<Digest>(1, _omitFieldNames ? '' : 'digests',
        subBuilder: Digest.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  QueryDigestsResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  QueryDigestsResponse copyWith(void Function(QueryDigestsResponse) updates) =>
      super.copyWith((message) => updates(message as QueryDigestsResponse))
          as QueryDigestsResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static QueryDigestsResponse create() => QueryDigestsResponse._();
  @$core.override
  QueryDigestsResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static QueryDigestsResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<QueryDigestsResponse>(create);
  static QueryDigestsResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<Digest> get digests => $_getList(0);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
