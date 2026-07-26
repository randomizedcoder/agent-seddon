// This is a generated file - do not edit.
//
// Generated from agent/v1/scheduler.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

import 'scheduler.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'scheduler.pbenum.dart';

class SchedJobRef extends $pb.GeneratedMessage {
  factory SchedJobRef({
    $core.String? id,
  }) {
    final result = create();
    if (id != null) result.id = id;
    return result;
  }

  SchedJobRef._();

  factory SchedJobRef.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SchedJobRef.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SchedJobRef',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedJobRef clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedJobRef copyWith(void Function(SchedJobRef) updates) =>
      super.copyWith((message) => updates(message as SchedJobRef))
          as SchedJobRef;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SchedJobRef create() => SchedJobRef._();
  @$core.override
  SchedJobRef createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SchedJobRef getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SchedJobRef>(create);
  static SchedJobRef? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);
}

class SchedListRequest extends $pb.GeneratedMessage {
  factory SchedListRequest() => create();

  SchedListRequest._();

  factory SchedListRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SchedListRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SchedListRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedListRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedListRequest copyWith(void Function(SchedListRequest) updates) =>
      super.copyWith((message) => updates(message as SchedListRequest))
          as SchedListRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SchedListRequest create() => SchedListRequest._();
  @$core.override
  SchedListRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SchedListRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SchedListRequest>(create);
  static SchedListRequest? _defaultInstance;
}

class SchedScheduleRequest extends $pb.GeneratedMessage {
  factory SchedScheduleRequest({
    $core.String? spec,
    $core.String? goal,
  }) {
    final result = create();
    if (spec != null) result.spec = spec;
    if (goal != null) result.goal = goal;
    return result;
  }

  SchedScheduleRequest._();

  factory SchedScheduleRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SchedScheduleRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SchedScheduleRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'spec')
    ..aOS(2, _omitFieldNames ? '' : 'goal')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedScheduleRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedScheduleRequest copyWith(void Function(SchedScheduleRequest) updates) =>
      super.copyWith((message) => updates(message as SchedScheduleRequest))
          as SchedScheduleRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SchedScheduleRequest create() => SchedScheduleRequest._();
  @$core.override
  SchedScheduleRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SchedScheduleRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SchedScheduleRequest>(create);
  static SchedScheduleRequest? _defaultInstance;

  /// The schedule as written, e.g. "every 30m", "cron: 0 6 * * *", "in 45m".
  /// Parsed server-side: the parser is the scheduler's, and a spec it rejects
  /// must be rejected at scheduling time rather than silently never firing.
  @$pb.TagNumber(1)
  $core.String get spec => $_getSZ(0);
  @$pb.TagNumber(1)
  set spec($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSpec() => $_has(0);
  @$pb.TagNumber(1)
  void clearSpec() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get goal => $_getSZ(1);
  @$pb.TagNumber(2)
  set goal($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasGoal() => $_has(1);
  @$pb.TagNumber(2)
  void clearGoal() => $_clearField(2);
}

enum SchedSchedule_Kind { intervalSecs, cronExpr, onceAtMs, notSet }

/// Mirrors `agent_core::Schedule`. Carried explicitly rather than re-parsed from
/// `spec` on the client, so a client needs no copy of the cron parser — and can
/// therefore never disagree with the server about when a job fires.
class SchedSchedule extends $pb.GeneratedMessage {
  factory SchedSchedule({
    $fixnum.Int64? intervalSecs,
    $core.String? cronExpr,
    $fixnum.Int64? onceAtMs,
  }) {
    final result = create();
    if (intervalSecs != null) result.intervalSecs = intervalSecs;
    if (cronExpr != null) result.cronExpr = cronExpr;
    if (onceAtMs != null) result.onceAtMs = onceAtMs;
    return result;
  }

  SchedSchedule._();

  factory SchedSchedule.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SchedSchedule.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static const $core.Map<$core.int, SchedSchedule_Kind>
      _SchedSchedule_KindByTag = {
    1: SchedSchedule_Kind.intervalSecs,
    2: SchedSchedule_Kind.cronExpr,
    3: SchedSchedule_Kind.onceAtMs,
    0: SchedSchedule_Kind.notSet
  };
  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SchedSchedule',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..oo(0, [1, 2, 3])
    ..a<$fixnum.Int64>(
        1, _omitFieldNames ? '' : 'intervalSecs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOS(2, _omitFieldNames ? '' : 'cronExpr')
    ..a<$fixnum.Int64>(
        3, _omitFieldNames ? '' : 'onceAtMs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedSchedule clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedSchedule copyWith(void Function(SchedSchedule) updates) =>
      super.copyWith((message) => updates(message as SchedSchedule))
          as SchedSchedule;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SchedSchedule create() => SchedSchedule._();
  @$core.override
  SchedSchedule createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SchedSchedule getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SchedSchedule>(create);
  static SchedSchedule? _defaultInstance;

  @$pb.TagNumber(1)
  @$pb.TagNumber(2)
  @$pb.TagNumber(3)
  SchedSchedule_Kind whichKind() => _SchedSchedule_KindByTag[$_whichOneof(0)]!;
  @$pb.TagNumber(1)
  @$pb.TagNumber(2)
  @$pb.TagNumber(3)
  void clearKind() => $_clearField($_whichOneof(0));

  @$pb.TagNumber(1)
  $fixnum.Int64 get intervalSecs => $_getI64(0);
  @$pb.TagNumber(1)
  set intervalSecs($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasIntervalSecs() => $_has(0);
  @$pb.TagNumber(1)
  void clearIntervalSecs() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get cronExpr => $_getSZ(1);
  @$pb.TagNumber(2)
  set cronExpr($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCronExpr() => $_has(1);
  @$pb.TagNumber(2)
  void clearCronExpr() => $_clearField(2);

  @$pb.TagNumber(3)
  $fixnum.Int64 get onceAtMs => $_getI64(2);
  @$pb.TagNumber(3)
  set onceAtMs($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasOnceAtMs() => $_has(2);
  @$pb.TagNumber(3)
  void clearOnceAtMs() => $_clearField(3);
}

class SchedJob extends $pb.GeneratedMessage {
  factory SchedJob({
    $core.String? id,
    $core.String? spec,
    SchedSchedule? schedule,
    $core.String? goal,
    $fixnum.Int64? nextFireMs,
    $core.bool? enabled,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (spec != null) result.spec = spec;
    if (schedule != null) result.schedule = schedule;
    if (goal != null) result.goal = goal;
    if (nextFireMs != null) result.nextFireMs = nextFireMs;
    if (enabled != null) result.enabled = enabled;
    return result;
  }

  SchedJob._();

  factory SchedJob.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SchedJob.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SchedJob',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aOS(2, _omitFieldNames ? '' : 'spec')
    ..aOM<SchedSchedule>(3, _omitFieldNames ? '' : 'schedule',
        subBuilder: SchedSchedule.create)
    ..aOS(4, _omitFieldNames ? '' : 'goal')
    ..a<$fixnum.Int64>(
        5, _omitFieldNames ? '' : 'nextFireMs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOB(6, _omitFieldNames ? '' : 'enabled')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedJob clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedJob copyWith(void Function(SchedJob) updates) =>
      super.copyWith((message) => updates(message as SchedJob)) as SchedJob;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SchedJob create() => SchedJob._();
  @$core.override
  SchedJob createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SchedJob getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<SchedJob>(create);
  static SchedJob? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get spec => $_getSZ(1);
  @$pb.TagNumber(2)
  set spec($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasSpec() => $_has(1);
  @$pb.TagNumber(2)
  void clearSpec() => $_clearField(2);

  @$pb.TagNumber(3)
  SchedSchedule get schedule => $_getN(2);
  @$pb.TagNumber(3)
  set schedule(SchedSchedule value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasSchedule() => $_has(2);
  @$pb.TagNumber(3)
  void clearSchedule() => $_clearField(3);
  @$pb.TagNumber(3)
  SchedSchedule ensureSchedule() => $_ensure(2);

  @$pb.TagNumber(4)
  $core.String get goal => $_getSZ(3);
  @$pb.TagNumber(4)
  set goal($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasGoal() => $_has(3);
  @$pb.TagNumber(4)
  void clearGoal() => $_clearField(4);

  /// Absent for a spent one-shot.
  @$pb.TagNumber(5)
  $fixnum.Int64 get nextFireMs => $_getI64(4);
  @$pb.TagNumber(5)
  set nextFireMs($fixnum.Int64 value) => $_setInt64(4, value);
  @$pb.TagNumber(5)
  $core.bool hasNextFireMs() => $_has(4);
  @$pb.TagNumber(5)
  void clearNextFireMs() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.bool get enabled => $_getBF(5);
  @$pb.TagNumber(6)
  set enabled($core.bool value) => $_setBool(5, value);
  @$pb.TagNumber(6)
  $core.bool hasEnabled() => $_has(5);
  @$pb.TagNumber(6)
  void clearEnabled() => $_clearField(6);
}

class SchedJobList extends $pb.GeneratedMessage {
  factory SchedJobList({
    $core.Iterable<SchedJob>? jobs,
  }) {
    final result = create();
    if (jobs != null) result.jobs.addAll(jobs);
    return result;
  }

  SchedJobList._();

  factory SchedJobList.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SchedJobList.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SchedJobList',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<SchedJob>(1, _omitFieldNames ? '' : 'jobs',
        subBuilder: SchedJob.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedJobList clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedJobList copyWith(void Function(SchedJobList) updates) =>
      super.copyWith((message) => updates(message as SchedJobList))
          as SchedJobList;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SchedJobList create() => SchedJobList._();
  @$core.override
  SchedJobList createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SchedJobList getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SchedJobList>(create);
  static SchedJobList? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<SchedJob> get jobs => $_getList(0);
}

class SchedRun extends $pb.GeneratedMessage {
  factory SchedRun({
    $core.String? jobId,
    $fixnum.Int64? startedMs,
    $fixnum.Int64? finishedMs,
    SchedRunOutcome? outcome,
    $core.String? detail,
  }) {
    final result = create();
    if (jobId != null) result.jobId = jobId;
    if (startedMs != null) result.startedMs = startedMs;
    if (finishedMs != null) result.finishedMs = finishedMs;
    if (outcome != null) result.outcome = outcome;
    if (detail != null) result.detail = detail;
    return result;
  }

  SchedRun._();

  factory SchedRun.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SchedRun.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SchedRun',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'jobId')
    ..a<$fixnum.Int64>(
        2, _omitFieldNames ? '' : 'startedMs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..a<$fixnum.Int64>(
        3, _omitFieldNames ? '' : 'finishedMs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aE<SchedRunOutcome>(4, _omitFieldNames ? '' : 'outcome',
        enumValues: SchedRunOutcome.values)
    ..aOS(5, _omitFieldNames ? '' : 'detail')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedRun clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedRun copyWith(void Function(SchedRun) updates) =>
      super.copyWith((message) => updates(message as SchedRun)) as SchedRun;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SchedRun create() => SchedRun._();
  @$core.override
  SchedRun createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SchedRun getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<SchedRun>(create);
  static SchedRun? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get jobId => $_getSZ(0);
  @$pb.TagNumber(1)
  set jobId($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasJobId() => $_has(0);
  @$pb.TagNumber(1)
  void clearJobId() => $_clearField(1);

  @$pb.TagNumber(2)
  $fixnum.Int64 get startedMs => $_getI64(1);
  @$pb.TagNumber(2)
  set startedMs($fixnum.Int64 value) => $_setInt64(1, value);
  @$pb.TagNumber(2)
  $core.bool hasStartedMs() => $_has(1);
  @$pb.TagNumber(2)
  void clearStartedMs() => $_clearField(2);

  @$pb.TagNumber(3)
  $fixnum.Int64 get finishedMs => $_getI64(2);
  @$pb.TagNumber(3)
  set finishedMs($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasFinishedMs() => $_has(2);
  @$pb.TagNumber(3)
  void clearFinishedMs() => $_clearField(3);

  @$pb.TagNumber(4)
  SchedRunOutcome get outcome => $_getN(3);
  @$pb.TagNumber(4)
  set outcome(SchedRunOutcome value) => $_setField(4, value);
  @$pb.TagNumber(4)
  $core.bool hasOutcome() => $_has(3);
  @$pb.TagNumber(4)
  void clearOutcome() => $_clearField(4);

  /// Answer or error, truncated for storage.
  @$pb.TagNumber(5)
  $core.String get detail => $_getSZ(4);
  @$pb.TagNumber(5)
  set detail($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasDetail() => $_has(4);
  @$pb.TagNumber(5)
  void clearDetail() => $_clearField(5);
}

class SchedRunList extends $pb.GeneratedMessage {
  factory SchedRunList({
    $core.Iterable<SchedRun>? runs,
  }) {
    final result = create();
    if (runs != null) result.runs.addAll(runs);
    return result;
  }

  SchedRunList._();

  factory SchedRunList.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SchedRunList.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SchedRunList',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<SchedRun>(1, _omitFieldNames ? '' : 'runs',
        subBuilder: SchedRun.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedRunList clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedRunList copyWith(void Function(SchedRunList) updates) =>
      super.copyWith((message) => updates(message as SchedRunList))
          as SchedRunList;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SchedRunList create() => SchedRunList._();
  @$core.override
  SchedRunList createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SchedRunList getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SchedRunList>(create);
  static SchedRunList? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<SchedRun> get runs => $_getList(0);
}

class SchedCancelResponse extends $pb.GeneratedMessage {
  factory SchedCancelResponse({
    $core.bool? cancelled,
  }) {
    final result = create();
    if (cancelled != null) result.cancelled = cancelled;
    return result;
  }

  SchedCancelResponse._();

  factory SchedCancelResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SchedCancelResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SchedCancelResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'cancelled')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedCancelResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SchedCancelResponse copyWith(void Function(SchedCancelResponse) updates) =>
      super.copyWith((message) => updates(message as SchedCancelResponse))
          as SchedCancelResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SchedCancelResponse create() => SchedCancelResponse._();
  @$core.override
  SchedCancelResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SchedCancelResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SchedCancelResponse>(create);
  static SchedCancelResponse? _defaultInstance;

  /// False when no such job existed — not an error.
  @$pb.TagNumber(1)
  $core.bool get cancelled => $_getBF(0);
  @$pb.TagNumber(1)
  set cancelled($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasCancelled() => $_has(0);
  @$pb.TagNumber(1)
  void clearCancelled() => $_clearField(1);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
