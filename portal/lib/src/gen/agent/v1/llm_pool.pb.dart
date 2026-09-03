// This is a generated file - do not edit.
//
// Generated from agent/v1/llm_pool.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

import 'common.pb.dart' as $1;
import 'llm_pool.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'llm_pool.pbenum.dart';

class PoolHealthRequest extends $pb.GeneratedMessage {
  factory PoolHealthRequest() => create();

  PoolHealthRequest._();

  factory PoolHealthRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PoolHealthRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PoolHealthRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PoolHealthRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PoolHealthRequest copyWith(void Function(PoolHealthRequest) updates) =>
      super.copyWith((message) => updates(message as PoolHealthRequest))
          as PoolHealthRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PoolHealthRequest create() => PoolHealthRequest._();
  @$core.override
  PoolHealthRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PoolHealthRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PoolHealthRequest>(create);
  static PoolHealthRequest? _defaultInstance;
}

class PoolMemberHealth extends $pb.GeneratedMessage {
  factory PoolMemberHealth({
    $core.String? name,
    $1.PoolTier? tier,
    $core.bool? alive,
    $core.int? consecutiveFailures,
    $core.int? lastProbeMs,
    $core.int? inFlight,
    $core.double? weight,
    $core.int? maxConcurrency,
    $core.bool? saturated,
    PoolMemberState? state,
    $core.int? latencyMsEwma,
  }) {
    final result = create();
    if (name != null) result.name = name;
    if (tier != null) result.tier = tier;
    if (alive != null) result.alive = alive;
    if (consecutiveFailures != null)
      result.consecutiveFailures = consecutiveFailures;
    if (lastProbeMs != null) result.lastProbeMs = lastProbeMs;
    if (inFlight != null) result.inFlight = inFlight;
    if (weight != null) result.weight = weight;
    if (maxConcurrency != null) result.maxConcurrency = maxConcurrency;
    if (saturated != null) result.saturated = saturated;
    if (state != null) result.state = state;
    if (latencyMsEwma != null) result.latencyMsEwma = latencyMsEwma;
    return result;
  }

  PoolMemberHealth._();

  factory PoolMemberHealth.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PoolMemberHealth.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PoolMemberHealth',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'name')
    ..aE<$1.PoolTier>(2, _omitFieldNames ? '' : 'tier',
        enumValues: $1.PoolTier.values)
    ..aOB(3, _omitFieldNames ? '' : 'alive')
    ..aI(4, _omitFieldNames ? '' : 'consecutiveFailures',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(5, _omitFieldNames ? '' : 'lastProbeMs',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(6, _omitFieldNames ? '' : 'inFlight', fieldType: $pb.PbFieldType.OU3)
    ..aD(7, _omitFieldNames ? '' : 'weight', fieldType: $pb.PbFieldType.OF)
    ..aI(8, _omitFieldNames ? '' : 'maxConcurrency',
        fieldType: $pb.PbFieldType.OU3)
    ..aOB(9, _omitFieldNames ? '' : 'saturated')
    ..aE<PoolMemberState>(10, _omitFieldNames ? '' : 'state',
        enumValues: PoolMemberState.values)
    ..aI(11, _omitFieldNames ? '' : 'latencyMsEwma',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PoolMemberHealth clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PoolMemberHealth copyWith(void Function(PoolMemberHealth) updates) =>
      super.copyWith((message) => updates(message as PoolMemberHealth))
          as PoolMemberHealth;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PoolMemberHealth create() => PoolMemberHealth._();
  @$core.override
  PoolMemberHealth createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PoolMemberHealth getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PoolMemberHealth>(create);
  static PoolMemberHealth? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get name => $_getSZ(0);
  @$pb.TagNumber(1)
  set name($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasName() => $_has(0);
  @$pb.TagNumber(1)
  void clearName() => $_clearField(1);

  @$pb.TagNumber(2)
  $1.PoolTier get tier => $_getN(1);
  @$pb.TagNumber(2)
  set tier($1.PoolTier value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasTier() => $_has(1);
  @$pb.TagNumber(2)
  void clearTier() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.bool get alive => $_getBF(2);
  @$pb.TagNumber(3)
  set alive($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasAlive() => $_has(2);
  @$pb.TagNumber(3)
  void clearAlive() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get consecutiveFailures => $_getIZ(3);
  @$pb.TagNumber(4)
  set consecutiveFailures($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasConsecutiveFailures() => $_has(3);
  @$pb.TagNumber(4)
  void clearConsecutiveFailures() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.int get lastProbeMs => $_getIZ(4);
  @$pb.TagNumber(5)
  set lastProbeMs($core.int value) => $_setUnsignedInt32(4, value);
  @$pb.TagNumber(5)
  $core.bool hasLastProbeMs() => $_has(4);
  @$pb.TagNumber(5)
  void clearLastProbeMs() => $_clearField(5);

  /// Live load + configured weight (GPU pool 01), additive. Both are clamped on
  /// receipt — a remote pool's reported load is untrusted.
  @$pb.TagNumber(6)
  $core.int get inFlight => $_getIZ(5);
  @$pb.TagNumber(6)
  set inFlight($core.int value) => $_setUnsignedInt32(5, value);
  @$pb.TagNumber(6)
  $core.bool hasInFlight() => $_has(5);
  @$pb.TagNumber(6)
  void clearInFlight() => $_clearField(6);

  @$pb.TagNumber(7)
  $core.double get weight => $_getN(6);
  @$pb.TagNumber(7)
  set weight($core.double value) => $_setFloat(6, value);
  @$pb.TagNumber(7)
  $core.bool hasWeight() => $_has(6);
  @$pb.TagNumber(7)
  void clearWeight() => $_clearField(7);

  /// Concurrency cap + current saturation (GPU pool 02), additive.
  @$pb.TagNumber(8)
  $core.int get maxConcurrency => $_getIZ(7);
  @$pb.TagNumber(8)
  set maxConcurrency($core.int value) => $_setUnsignedInt32(7, value);
  @$pb.TagNumber(8)
  $core.bool hasMaxConcurrency() => $_has(7);
  @$pb.TagNumber(8)
  void clearMaxConcurrency() => $_clearField(8);

  @$pb.TagNumber(9)
  $core.bool get saturated => $_getBF(8);
  @$pb.TagNumber(9)
  set saturated($core.bool value) => $_setBool(8, value);
  @$pb.TagNumber(9)
  $core.bool hasSaturated() => $_has(8);
  @$pb.TagNumber(9)
  void clearSaturated() => $_clearField(9);

  /// Graded liveness + smoothed latency (GPU pool 03), additive.
  @$pb.TagNumber(10)
  PoolMemberState get state => $_getN(9);
  @$pb.TagNumber(10)
  set state(PoolMemberState value) => $_setField(10, value);
  @$pb.TagNumber(10)
  $core.bool hasState() => $_has(9);
  @$pb.TagNumber(10)
  void clearState() => $_clearField(10);

  @$pb.TagNumber(11)
  $core.int get latencyMsEwma => $_getIZ(10);
  @$pb.TagNumber(11)
  set latencyMsEwma($core.int value) => $_setUnsignedInt32(10, value);
  @$pb.TagNumber(11)
  $core.bool hasLatencyMsEwma() => $_has(10);
  @$pb.TagNumber(11)
  void clearLatencyMsEwma() => $_clearField(11);
}

class PoolHealthReport extends $pb.GeneratedMessage {
  factory PoolHealthReport({
    $core.Iterable<PoolMemberHealth>? members,
  }) {
    final result = create();
    if (members != null) result.members.addAll(members);
    return result;
  }

  PoolHealthReport._();

  factory PoolHealthReport.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PoolHealthReport.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PoolHealthReport',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<PoolMemberHealth>(1, _omitFieldNames ? '' : 'members',
        subBuilder: PoolMemberHealth.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PoolHealthReport clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PoolHealthReport copyWith(void Function(PoolHealthReport) updates) =>
      super.copyWith((message) => updates(message as PoolHealthReport))
          as PoolHealthReport;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PoolHealthReport create() => PoolHealthReport._();
  @$core.override
  PoolHealthReport createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PoolHealthReport getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PoolHealthReport>(create);
  static PoolHealthReport? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<PoolMemberHealth> get members => $_getList(0);
}

class PoolCompleteRequest extends $pb.GeneratedMessage {
  factory PoolCompleteRequest({
    $1.CompletionRequest? req,
    $1.PoolTier? tier,
    $core.int? fanout,
  }) {
    final result = create();
    if (req != null) result.req = req;
    if (tier != null) result.tier = tier;
    if (fanout != null) result.fanout = fanout;
    return result;
  }

  PoolCompleteRequest._();

  factory PoolCompleteRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PoolCompleteRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PoolCompleteRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<$1.CompletionRequest>(1, _omitFieldNames ? '' : 'req',
        subBuilder: $1.CompletionRequest.create)
    ..aE<$1.PoolTier>(2, _omitFieldNames ? '' : 'tier',
        enumValues: $1.PoolTier.values)
    ..aI(3, _omitFieldNames ? '' : 'fanout', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PoolCompleteRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PoolCompleteRequest copyWith(void Function(PoolCompleteRequest) updates) =>
      super.copyWith((message) => updates(message as PoolCompleteRequest))
          as PoolCompleteRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PoolCompleteRequest create() => PoolCompleteRequest._();
  @$core.override
  PoolCompleteRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PoolCompleteRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PoolCompleteRequest>(create);
  static PoolCompleteRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $1.CompletionRequest get req => $_getN(0);
  @$pb.TagNumber(1)
  set req($1.CompletionRequest value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasReq() => $_has(0);
  @$pb.TagNumber(1)
  void clearReq() => $_clearField(1);
  @$pb.TagNumber(1)
  $1.CompletionRequest ensureReq() => $_ensure(0);

  @$pb.TagNumber(2)
  $1.PoolTier get tier => $_getN(1);
  @$pb.TagNumber(2)
  set tier($1.PoolTier value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasTier() => $_has(1);
  @$pb.TagNumber(2)
  void clearTier() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get fanout => $_getIZ(2);
  @$pb.TagNumber(3)
  set fanout($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasFanout() => $_has(2);
  @$pb.TagNumber(3)
  void clearFanout() => $_clearField(3);
}

/// One member's settled result. `ok=false` carries a class-only `error` (never a
/// raw body) and no `response`; the batch never fails.
class PoolMemberResult extends $pb.GeneratedMessage {
  factory PoolMemberResult({
    $core.String? member,
    $core.bool? ok,
    $core.String? error,
    $core.int? durationMs,
    $1.CompletionResponse? response,
  }) {
    final result = create();
    if (member != null) result.member = member;
    if (ok != null) result.ok = ok;
    if (error != null) result.error = error;
    if (durationMs != null) result.durationMs = durationMs;
    if (response != null) result.response = response;
    return result;
  }

  PoolMemberResult._();

  factory PoolMemberResult.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PoolMemberResult.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PoolMemberResult',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'member')
    ..aOB(2, _omitFieldNames ? '' : 'ok')
    ..aOS(3, _omitFieldNames ? '' : 'error')
    ..aI(4, _omitFieldNames ? '' : 'durationMs', fieldType: $pb.PbFieldType.OU3)
    ..aOM<$1.CompletionResponse>(5, _omitFieldNames ? '' : 'response',
        subBuilder: $1.CompletionResponse.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PoolMemberResult clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PoolMemberResult copyWith(void Function(PoolMemberResult) updates) =>
      super.copyWith((message) => updates(message as PoolMemberResult))
          as PoolMemberResult;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PoolMemberResult create() => PoolMemberResult._();
  @$core.override
  PoolMemberResult createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PoolMemberResult getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PoolMemberResult>(create);
  static PoolMemberResult? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get member => $_getSZ(0);
  @$pb.TagNumber(1)
  set member($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasMember() => $_has(0);
  @$pb.TagNumber(1)
  void clearMember() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.bool get ok => $_getBF(1);
  @$pb.TagNumber(2)
  set ok($core.bool value) => $_setBool(1, value);
  @$pb.TagNumber(2)
  $core.bool hasOk() => $_has(1);
  @$pb.TagNumber(2)
  void clearOk() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get error => $_getSZ(2);
  @$pb.TagNumber(3)
  set error($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasError() => $_has(2);
  @$pb.TagNumber(3)
  void clearError() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get durationMs => $_getIZ(3);
  @$pb.TagNumber(4)
  set durationMs($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasDurationMs() => $_has(3);
  @$pb.TagNumber(4)
  void clearDurationMs() => $_clearField(4);

  @$pb.TagNumber(5)
  $1.CompletionResponse get response => $_getN(4);
  @$pb.TagNumber(5)
  set response($1.CompletionResponse value) => $_setField(5, value);
  @$pb.TagNumber(5)
  $core.bool hasResponse() => $_has(4);
  @$pb.TagNumber(5)
  void clearResponse() => $_clearField(5);
  @$pb.TagNumber(5)
  $1.CompletionResponse ensureResponse() => $_ensure(4);
}

class PoolCompleteResponse extends $pb.GeneratedMessage {
  factory PoolCompleteResponse({
    $core.Iterable<PoolMemberResult>? results,
  }) {
    final result = create();
    if (results != null) result.results.addAll(results);
    return result;
  }

  PoolCompleteResponse._();

  factory PoolCompleteResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PoolCompleteResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PoolCompleteResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<PoolMemberResult>(1, _omitFieldNames ? '' : 'results',
        subBuilder: PoolMemberResult.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PoolCompleteResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PoolCompleteResponse copyWith(void Function(PoolCompleteResponse) updates) =>
      super.copyWith((message) => updates(message as PoolCompleteResponse))
          as PoolCompleteResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PoolCompleteResponse create() => PoolCompleteResponse._();
  @$core.override
  PoolCompleteResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PoolCompleteResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PoolCompleteResponse>(create);
  static PoolCompleteResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<PoolMemberResult> get results => $_getList(0);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
