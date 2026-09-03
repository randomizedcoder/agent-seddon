// This is a generated file - do not edit.
//
// Generated from agent/v1/upstream.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

import 'common.pb.dart' as $1;
import 'llm_pool.pbenum.dart' as $2;

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

/// One routable upstream definition + metadata — the "model card". Mirrors
/// `agent_core::Upstream`. Persisted by the registry stores; `UpstreamHealth`
/// (live state) deliberately lives in a separate, never-persisted message.
class Upstream extends $pb.GeneratedMessage {
  factory Upstream({
    $core.String? id,
    $core.String? kind,
    $core.bool? enabled,
    $core.String? baseUrl,
    $core.String? model,
    $core.String? apiKeyRef,
    $core.bool? insecureTls,
    $core.String? version,
    $core.int? maxRetries,
    $core.int? contextWindow,
    $core.int? maxOutputTokens,
    $core.bool? supportsTools,
    $core.bool? supportsVision,
    $core.bool? supportsResponseFormat,
    $core.Iterable<$core.String>? tags,
    $core.double? inputCost,
    $core.double? outputCost,
    $1.PoolTier? tier,
    $core.double? weight,
    $core.int? maxConcurrency,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (kind != null) result.kind = kind;
    if (enabled != null) result.enabled = enabled;
    if (baseUrl != null) result.baseUrl = baseUrl;
    if (model != null) result.model = model;
    if (apiKeyRef != null) result.apiKeyRef = apiKeyRef;
    if (insecureTls != null) result.insecureTls = insecureTls;
    if (version != null) result.version = version;
    if (maxRetries != null) result.maxRetries = maxRetries;
    if (contextWindow != null) result.contextWindow = contextWindow;
    if (maxOutputTokens != null) result.maxOutputTokens = maxOutputTokens;
    if (supportsTools != null) result.supportsTools = supportsTools;
    if (supportsVision != null) result.supportsVision = supportsVision;
    if (supportsResponseFormat != null)
      result.supportsResponseFormat = supportsResponseFormat;
    if (tags != null) result.tags.addAll(tags);
    if (inputCost != null) result.inputCost = inputCost;
    if (outputCost != null) result.outputCost = outputCost;
    if (tier != null) result.tier = tier;
    if (weight != null) result.weight = weight;
    if (maxConcurrency != null) result.maxConcurrency = maxConcurrency;
    return result;
  }

  Upstream._();

  factory Upstream.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory Upstream.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Upstream',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aOS(2, _omitFieldNames ? '' : 'kind')
    ..aOB(3, _omitFieldNames ? '' : 'enabled')
    ..aOS(4, _omitFieldNames ? '' : 'baseUrl')
    ..aOS(5, _omitFieldNames ? '' : 'model')
    ..aOS(6, _omitFieldNames ? '' : 'apiKeyRef')
    ..aOB(7, _omitFieldNames ? '' : 'insecureTls')
    ..aOS(8, _omitFieldNames ? '' : 'version')
    ..aI(9, _omitFieldNames ? '' : 'maxRetries', fieldType: $pb.PbFieldType.OU3)
    ..aI(10, _omitFieldNames ? '' : 'contextWindow',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(11, _omitFieldNames ? '' : 'maxOutputTokens',
        fieldType: $pb.PbFieldType.OU3)
    ..aOB(12, _omitFieldNames ? '' : 'supportsTools')
    ..aOB(13, _omitFieldNames ? '' : 'supportsVision')
    ..aOB(14, _omitFieldNames ? '' : 'supportsResponseFormat')
    ..pPS(15, _omitFieldNames ? '' : 'tags')
    ..aD(16, _omitFieldNames ? '' : 'inputCost', fieldType: $pb.PbFieldType.OF)
    ..aD(17, _omitFieldNames ? '' : 'outputCost', fieldType: $pb.PbFieldType.OF)
    ..aE<$1.PoolTier>(18, _omitFieldNames ? '' : 'tier',
        enumValues: $1.PoolTier.values)
    ..aD(19, _omitFieldNames ? '' : 'weight', fieldType: $pb.PbFieldType.OF)
    ..aI(20, _omitFieldNames ? '' : 'maxConcurrency',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Upstream clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Upstream copyWith(void Function(Upstream) updates) =>
      super.copyWith((message) => updates(message as Upstream)) as Upstream;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static Upstream create() => Upstream._();
  @$core.override
  Upstream createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static Upstream getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<Upstream>(create);
  static Upstream? _defaultInstance;

  /// Identity. `id` is a path-safe segment (server-validated).
  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  /// "openai-compat" | "anthropic" | "grpc" | "" (resolve `id` as a registered
  /// provider name, like a `[[route.upstreams]]` entry with no endpoint).
  @$pb.TagNumber(2)
  $core.String get kind => $_getSZ(1);
  @$pb.TagNumber(2)
  set kind($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasKind() => $_has(1);
  @$pb.TagNumber(2)
  void clearKind() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.bool get enabled => $_getBF(2);
  @$pb.TagNumber(3)
  set enabled($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasEnabled() => $_has(2);
  @$pb.TagNumber(3)
  void clearEnabled() => $_clearField(3);

  /// Connection.
  @$pb.TagNumber(4)
  $core.String get baseUrl => $_getSZ(3);
  @$pb.TagNumber(4)
  set baseUrl($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasBaseUrl() => $_has(3);
  @$pb.TagNumber(4)
  void clearBaseUrl() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get model => $_getSZ(4);
  @$pb.TagNumber(5)
  set model($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasModel() => $_has(4);
  @$pb.TagNumber(5)
  void clearModel() => $_clearField(5);

  /// Kind-prefixed key reference: `env:NAME` or `file:/path` — NEVER the secret.
  @$pb.TagNumber(6)
  $core.String get apiKeyRef => $_getSZ(5);
  @$pb.TagNumber(6)
  set apiKeyRef($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasApiKeyRef() => $_has(5);
  @$pb.TagNumber(6)
  void clearApiKeyRef() => $_clearField(6);

  @$pb.TagNumber(7)
  $core.bool get insecureTls => $_getBF(6);
  @$pb.TagNumber(7)
  set insecureTls($core.bool value) => $_setBool(6, value);
  @$pb.TagNumber(7)
  $core.bool hasInsecureTls() => $_has(6);
  @$pb.TagNumber(7)
  void clearInsecureTls() => $_clearField(7);

  @$pb.TagNumber(8)
  $core.String get version => $_getSZ(7);
  @$pb.TagNumber(8)
  set version($core.String value) => $_setString(7, value);
  @$pb.TagNumber(8)
  $core.bool hasVersion() => $_has(7);
  @$pb.TagNumber(8)
  void clearVersion() => $_clearField(8);

  @$pb.TagNumber(9)
  $core.int get maxRetries => $_getIZ(8);
  @$pb.TagNumber(9)
  set maxRetries($core.int value) => $_setUnsignedInt32(8, value);
  @$pb.TagNumber(9)
  $core.bool hasMaxRetries() => $_has(8);
  @$pb.TagNumber(9)
  void clearMaxRetries() => $_clearField(9);

  /// Capabilities (the model card).
  @$pb.TagNumber(10)
  $core.int get contextWindow => $_getIZ(9);
  @$pb.TagNumber(10)
  set contextWindow($core.int value) => $_setUnsignedInt32(9, value);
  @$pb.TagNumber(10)
  $core.bool hasContextWindow() => $_has(9);
  @$pb.TagNumber(10)
  void clearContextWindow() => $_clearField(10);

  @$pb.TagNumber(11)
  $core.int get maxOutputTokens => $_getIZ(10);
  @$pb.TagNumber(11)
  set maxOutputTokens($core.int value) => $_setUnsignedInt32(10, value);
  @$pb.TagNumber(11)
  $core.bool hasMaxOutputTokens() => $_has(10);
  @$pb.TagNumber(11)
  void clearMaxOutputTokens() => $_clearField(11);

  @$pb.TagNumber(12)
  $core.bool get supportsTools => $_getBF(11);
  @$pb.TagNumber(12)
  set supportsTools($core.bool value) => $_setBool(11, value);
  @$pb.TagNumber(12)
  $core.bool hasSupportsTools() => $_has(11);
  @$pb.TagNumber(12)
  void clearSupportsTools() => $_clearField(12);

  @$pb.TagNumber(13)
  $core.bool get supportsVision => $_getBF(12);
  @$pb.TagNumber(13)
  set supportsVision($core.bool value) => $_setBool(12, value);
  @$pb.TagNumber(13)
  $core.bool hasSupportsVision() => $_has(12);
  @$pb.TagNumber(13)
  void clearSupportsVision() => $_clearField(13);

  @$pb.TagNumber(14)
  $core.bool get supportsResponseFormat => $_getBF(13);
  @$pb.TagNumber(14)
  set supportsResponseFormat($core.bool value) => $_setBool(13, value);
  @$pb.TagNumber(14)
  $core.bool hasSupportsResponseFormat() => $_has(13);
  @$pb.TagNumber(14)
  void clearSupportsResponseFormat() => $_clearField(14);

  @$pb.TagNumber(15)
  $pb.PbList<$core.String> get tags => $_getList(14);

  /// Economics / perf. Costs are per-Mtok hints (clamped non-negative on ingest).
  @$pb.TagNumber(16)
  $core.double get inputCost => $_getN(15);
  @$pb.TagNumber(16)
  set inputCost($core.double value) => $_setFloat(15, value);
  @$pb.TagNumber(16)
  $core.bool hasInputCost() => $_has(15);
  @$pb.TagNumber(16)
  void clearInputCost() => $_clearField(16);

  @$pb.TagNumber(17)
  $core.double get outputCost => $_getN(16);
  @$pb.TagNumber(17)
  set outputCost($core.double value) => $_setFloat(16, value);
  @$pb.TagNumber(17)
  $core.bool hasOutputCost() => $_has(16);
  @$pb.TagNumber(17)
  void clearOutputCost() => $_clearField(17);

  @$pb.TagNumber(18)
  $1.PoolTier get tier => $_getN(17);
  @$pb.TagNumber(18)
  set tier($1.PoolTier value) => $_setField(18, value);
  @$pb.TagNumber(18)
  $core.bool hasTier() => $_has(17);
  @$pb.TagNumber(18)
  void clearTier() => $_clearField(18);

  @$pb.TagNumber(19)
  $core.double get weight => $_getN(18);
  @$pb.TagNumber(19)
  set weight($core.double value) => $_setFloat(18, value);
  @$pb.TagNumber(19)
  $core.bool hasWeight() => $_has(18);
  @$pb.TagNumber(19)
  void clearWeight() => $_clearField(19);

  @$pb.TagNumber(20)
  $core.int get maxConcurrency => $_getIZ(19);
  @$pb.TagNumber(20)
  set maxConcurrency($core.int value) => $_setUnsignedInt32(19, value);
  @$pb.TagNumber(20)
  $core.bool hasMaxConcurrency() => $_has(19);
  @$pb.TagNumber(20)
  void clearMaxConcurrency() => $_clearField(20);
}

/// Live state of one upstream — runtime-only, NEVER persisted by any store.
class UpstreamHealth extends $pb.GeneratedMessage {
  factory UpstreamHealth({
    $core.String? id,
    $2.PoolMemberState? state,
    $core.int? inFlight,
    $core.int? latencyMsEwma,
    $core.bool? saturated,
    $core.int? consecutiveFailures,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (state != null) result.state = state;
    if (inFlight != null) result.inFlight = inFlight;
    if (latencyMsEwma != null) result.latencyMsEwma = latencyMsEwma;
    if (saturated != null) result.saturated = saturated;
    if (consecutiveFailures != null)
      result.consecutiveFailures = consecutiveFailures;
    return result;
  }

  UpstreamHealth._();

  factory UpstreamHealth.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory UpstreamHealth.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'UpstreamHealth',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aE<$2.PoolMemberState>(2, _omitFieldNames ? '' : 'state',
        enumValues: $2.PoolMemberState.values)
    ..aI(3, _omitFieldNames ? '' : 'inFlight', fieldType: $pb.PbFieldType.OU3)
    ..aI(4, _omitFieldNames ? '' : 'latencyMsEwma',
        fieldType: $pb.PbFieldType.OU3)
    ..aOB(5, _omitFieldNames ? '' : 'saturated')
    ..aI(6, _omitFieldNames ? '' : 'consecutiveFailures',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamHealth clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamHealth copyWith(void Function(UpstreamHealth) updates) =>
      super.copyWith((message) => updates(message as UpstreamHealth))
          as UpstreamHealth;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static UpstreamHealth create() => UpstreamHealth._();
  @$core.override
  UpstreamHealth createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static UpstreamHealth getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<UpstreamHealth>(create);
  static UpstreamHealth? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  @$pb.TagNumber(2)
  $2.PoolMemberState get state => $_getN(1);
  @$pb.TagNumber(2)
  set state($2.PoolMemberState value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasState() => $_has(1);
  @$pb.TagNumber(2)
  void clearState() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get inFlight => $_getIZ(2);
  @$pb.TagNumber(3)
  set inFlight($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasInFlight() => $_has(2);
  @$pb.TagNumber(3)
  void clearInFlight() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get latencyMsEwma => $_getIZ(3);
  @$pb.TagNumber(4)
  set latencyMsEwma($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasLatencyMsEwma() => $_has(3);
  @$pb.TagNumber(4)
  void clearLatencyMsEwma() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.bool get saturated => $_getBF(4);
  @$pb.TagNumber(5)
  set saturated($core.bool value) => $_setBool(4, value);
  @$pb.TagNumber(5)
  $core.bool hasSaturated() => $_has(4);
  @$pb.TagNumber(5)
  void clearSaturated() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.int get consecutiveFailures => $_getIZ(5);
  @$pb.TagNumber(6)
  set consecutiveFailures($core.int value) => $_setUnsignedInt32(5, value);
  @$pb.TagNumber(6)
  $core.bool hasConsecutiveFailures() => $_has(5);
  @$pb.TagNumber(6)
  void clearConsecutiveFailures() => $_clearField(6);
}

/// The `match` half of a rule: present conditions must all hold. Mirrors the
/// `[[route.rules]] match` TOML (02); typed enums here, so a typo'd role/mode is
/// impossible at this layer rather than a startup error.
class RouteMatch extends $pb.GeneratedMessage {
  factory RouteMatch({
    $1.TaskMode? taskMode,
    $1.RouteRole? role,
    $core.int? minContext,
  }) {
    final result = create();
    if (taskMode != null) result.taskMode = taskMode;
    if (role != null) result.role = role;
    if (minContext != null) result.minContext = minContext;
    return result;
  }

  RouteMatch._();

  factory RouteMatch.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RouteMatch.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RouteMatch',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aE<$1.TaskMode>(1, _omitFieldNames ? '' : 'taskMode',
        enumValues: $1.TaskMode.values)
    ..aE<$1.RouteRole>(2, _omitFieldNames ? '' : 'role',
        enumValues: $1.RouteRole.values)
    ..aI(3, _omitFieldNames ? '' : 'minContext', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RouteMatch clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RouteMatch copyWith(void Function(RouteMatch) updates) =>
      super.copyWith((message) => updates(message as RouteMatch)) as RouteMatch;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RouteMatch create() => RouteMatch._();
  @$core.override
  RouteMatch createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RouteMatch getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RouteMatch>(create);
  static RouteMatch? _defaultInstance;

  @$pb.TagNumber(1)
  $1.TaskMode get taskMode => $_getN(0);
  @$pb.TagNumber(1)
  set taskMode($1.TaskMode value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasTaskMode() => $_has(0);
  @$pb.TagNumber(1)
  void clearTaskMode() => $_clearField(1);

  @$pb.TagNumber(2)
  $1.RouteRole get role => $_getN(1);
  @$pb.TagNumber(2)
  set role($1.RouteRole value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasRole() => $_has(1);
  @$pb.TagNumber(2)
  void clearRole() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get minContext => $_getIZ(2);
  @$pb.TagNumber(3)
  set minContext($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasMinContext() => $_has(2);
  @$pb.TagNumber(3)
  void clearMinContext() => $_clearField(3);
}

/// The `prefer` half: how to order the survivors once a rule matches.
class RoutePrefer extends $pb.GeneratedMessage {
  factory RoutePrefer({
    $core.Iterable<$core.String>? tags,
    $1.PoolTier? tier,
    $core.Iterable<$core.String>? upstreams,
    $core.String? policy,
  }) {
    final result = create();
    if (tags != null) result.tags.addAll(tags);
    if (tier != null) result.tier = tier;
    if (upstreams != null) result.upstreams.addAll(upstreams);
    if (policy != null) result.policy = policy;
    return result;
  }

  RoutePrefer._();

  factory RoutePrefer.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RoutePrefer.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RoutePrefer',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPS(1, _omitFieldNames ? '' : 'tags')
    ..aE<$1.PoolTier>(2, _omitFieldNames ? '' : 'tier',
        enumValues: $1.PoolTier.values)
    ..pPS(3, _omitFieldNames ? '' : 'upstreams')
    ..aOS(4, _omitFieldNames ? '' : 'policy')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoutePrefer clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoutePrefer copyWith(void Function(RoutePrefer) updates) =>
      super.copyWith((message) => updates(message as RoutePrefer))
          as RoutePrefer;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RoutePrefer create() => RoutePrefer._();
  @$core.override
  RoutePrefer createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RoutePrefer getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RoutePrefer>(create);
  static RoutePrefer? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<$core.String> get tags => $_getList(0);

  @$pb.TagNumber(2)
  $1.PoolTier get tier => $_getN(1);
  @$pb.TagNumber(2)
  set tier($1.PoolTier value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasTier() => $_has(1);
  @$pb.TagNumber(2)
  void clearTier() => $_clearField(2);

  @$pb.TagNumber(3)
  $pb.PbList<$core.String> get upstreams => $_getList(2);

  /// Live-signal ordering policy: "" | "cost" | "latency" | "least-loaded".
  /// Validated on ingest; consumed by the registry-backed router (increment 04).
  @$pb.TagNumber(4)
  $core.String get policy => $_getSZ(3);
  @$pb.TagNumber(4)
  set policy($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasPolicy() => $_has(3);
  @$pb.TagNumber(4)
  void clearPolicy() => $_clearField(4);
}

class RouteRule extends $pb.GeneratedMessage {
  factory RouteRule({
    RouteMatch? match,
    RoutePrefer? prefer,
  }) {
    final result = create();
    if (match != null) result.match = match;
    if (prefer != null) result.prefer = prefer;
    return result;
  }

  RouteRule._();

  factory RouteRule.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RouteRule.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RouteRule',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<RouteMatch>(1, _omitFieldNames ? '' : 'match',
        subBuilder: RouteMatch.create)
    ..aOM<RoutePrefer>(2, _omitFieldNames ? '' : 'prefer',
        subBuilder: RoutePrefer.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RouteRule clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RouteRule copyWith(void Function(RouteRule) updates) =>
      super.copyWith((message) => updates(message as RouteRule)) as RouteRule;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RouteRule create() => RouteRule._();
  @$core.override
  RouteRule createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RouteRule getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<RouteRule>(create);
  static RouteRule? _defaultInstance;

  @$pb.TagNumber(1)
  RouteMatch get match => $_getN(0);
  @$pb.TagNumber(1)
  set match(RouteMatch value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasMatch() => $_has(0);
  @$pb.TagNumber(1)
  void clearMatch() => $_clearField(1);
  @$pb.TagNumber(1)
  RouteMatch ensureMatch() => $_ensure(0);

  @$pb.TagNumber(2)
  RoutePrefer get prefer => $_getN(1);
  @$pb.TagNumber(2)
  set prefer(RoutePrefer value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasPrefer() => $_has(1);
  @$pb.TagNumber(2)
  void clearPrefer() => $_clearField(2);
  @$pb.TagNumber(2)
  RoutePrefer ensurePrefer() => $_ensure(1);
}

/// The declarative routing policy: ordered rules + the default preference, plus
/// the router-wide breaker settings (0 ⇒ the built-in default).
class RoutePolicy extends $pb.GeneratedMessage {
  factory RoutePolicy({
    $core.Iterable<RouteRule>? rules,
    RoutePrefer? defaultPrefer,
    $core.int? failureThreshold,
    $core.int? cooldownSecs,
  }) {
    final result = create();
    if (rules != null) result.rules.addAll(rules);
    if (defaultPrefer != null) result.defaultPrefer = defaultPrefer;
    if (failureThreshold != null) result.failureThreshold = failureThreshold;
    if (cooldownSecs != null) result.cooldownSecs = cooldownSecs;
    return result;
  }

  RoutePolicy._();

  factory RoutePolicy.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RoutePolicy.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RoutePolicy',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<RouteRule>(1, _omitFieldNames ? '' : 'rules',
        subBuilder: RouteRule.create)
    ..aOM<RoutePrefer>(2, _omitFieldNames ? '' : 'defaultPrefer',
        subBuilder: RoutePrefer.create)
    ..aI(3, _omitFieldNames ? '' : 'failureThreshold',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(4, _omitFieldNames ? '' : 'cooldownSecs',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoutePolicy clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoutePolicy copyWith(void Function(RoutePolicy) updates) =>
      super.copyWith((message) => updates(message as RoutePolicy))
          as RoutePolicy;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RoutePolicy create() => RoutePolicy._();
  @$core.override
  RoutePolicy createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RoutePolicy getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RoutePolicy>(create);
  static RoutePolicy? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<RouteRule> get rules => $_getList(0);

  @$pb.TagNumber(2)
  RoutePrefer get defaultPrefer => $_getN(1);
  @$pb.TagNumber(2)
  set defaultPrefer(RoutePrefer value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasDefaultPrefer() => $_has(1);
  @$pb.TagNumber(2)
  void clearDefaultPrefer() => $_clearField(2);
  @$pb.TagNumber(2)
  RoutePrefer ensureDefaultPrefer() => $_ensure(1);

  @$pb.TagNumber(3)
  $core.int get failureThreshold => $_getIZ(2);
  @$pb.TagNumber(3)
  set failureThreshold($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasFailureThreshold() => $_has(2);
  @$pb.TagNumber(3)
  void clearFailureThreshold() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get cooldownSecs => $_getIZ(3);
  @$pb.TagNumber(4)
  set cooldownSecs($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasCooldownSecs() => $_has(3);
  @$pb.TagNumber(4)
  void clearCooldownSecs() => $_clearField(4);
}

/// The whole model-router config as ONE message: this is what a `*.textproto`
/// scenario file deserializes into at startup, and what the `file` storage
/// backend reads/writes (one format, hand-edited or control-plane-written).
class ModelRouterConfig extends $pb.GeneratedMessage {
  factory ModelRouterConfig({
    $core.Iterable<Upstream>? upstreams,
    RoutePolicy? policy,
  }) {
    final result = create();
    if (upstreams != null) result.upstreams.addAll(upstreams);
    if (policy != null) result.policy = policy;
    return result;
  }

  ModelRouterConfig._();

  factory ModelRouterConfig.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ModelRouterConfig.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ModelRouterConfig',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<Upstream>(1, _omitFieldNames ? '' : 'upstreams',
        subBuilder: Upstream.create)
    ..aOM<RoutePolicy>(2, _omitFieldNames ? '' : 'policy',
        subBuilder: RoutePolicy.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ModelRouterConfig clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ModelRouterConfig copyWith(void Function(ModelRouterConfig) updates) =>
      super.copyWith((message) => updates(message as ModelRouterConfig))
          as ModelRouterConfig;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ModelRouterConfig create() => ModelRouterConfig._();
  @$core.override
  ModelRouterConfig createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ModelRouterConfig getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ModelRouterConfig>(create);
  static ModelRouterConfig? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<Upstream> get upstreams => $_getList(0);

  @$pb.TagNumber(2)
  RoutePolicy get policy => $_getN(1);
  @$pb.TagNumber(2)
  set policy(RoutePolicy value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasPolicy() => $_has(1);
  @$pb.TagNumber(2)
  void clearPolicy() => $_clearField(2);
  @$pb.TagNumber(2)
  RoutePolicy ensurePolicy() => $_ensure(1);
}

/// What the router would pick for a hint, and why — pure introspection.
class RouteDecision extends $pb.GeneratedMessage {
  factory RouteDecision({
    $core.String? chosen,
    $core.Iterable<$core.String>? order,
    $core.String? rule,
    $core.String? why,
  }) {
    final result = create();
    if (chosen != null) result.chosen = chosen;
    if (order != null) result.order.addAll(order);
    if (rule != null) result.rule = rule;
    if (why != null) result.why = why;
    return result;
  }

  RouteDecision._();

  factory RouteDecision.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RouteDecision.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RouteDecision',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'chosen')
    ..pPS(2, _omitFieldNames ? '' : 'order')
    ..aOS(3, _omitFieldNames ? '' : 'rule')
    ..aOS(4, _omitFieldNames ? '' : 'why')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RouteDecision clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RouteDecision copyWith(void Function(RouteDecision) updates) =>
      super.copyWith((message) => updates(message as RouteDecision))
          as RouteDecision;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RouteDecision create() => RouteDecision._();
  @$core.override
  RouteDecision createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RouteDecision getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RouteDecision>(create);
  static RouteDecision? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get chosen => $_getSZ(0);
  @$pb.TagNumber(1)
  set chosen($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasChosen() => $_has(0);
  @$pb.TagNumber(1)
  void clearChosen() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<$core.String> get order => $_getList(1);

  @$pb.TagNumber(3)
  $core.String get rule => $_getSZ(2);
  @$pb.TagNumber(3)
  set rule($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasRule() => $_has(2);
  @$pb.TagNumber(3)
  void clearRule() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get why => $_getSZ(3);
  @$pb.TagNumber(4)
  set why($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasWhy() => $_has(3);
  @$pb.TagNumber(4)
  void clearWhy() => $_clearField(4);
}

class UpstreamListRequest extends $pb.GeneratedMessage {
  factory UpstreamListRequest() => create();

  UpstreamListRequest._();

  factory UpstreamListRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory UpstreamListRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'UpstreamListRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamListRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamListRequest copyWith(void Function(UpstreamListRequest) updates) =>
      super.copyWith((message) => updates(message as UpstreamListRequest))
          as UpstreamListRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static UpstreamListRequest create() => UpstreamListRequest._();
  @$core.override
  UpstreamListRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static UpstreamListRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<UpstreamListRequest>(create);
  static UpstreamListRequest? _defaultInstance;
}

class UpstreamList extends $pb.GeneratedMessage {
  factory UpstreamList({
    $core.Iterable<Upstream>? upstreams,
  }) {
    final result = create();
    if (upstreams != null) result.upstreams.addAll(upstreams);
    return result;
  }

  UpstreamList._();

  factory UpstreamList.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory UpstreamList.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'UpstreamList',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<Upstream>(1, _omitFieldNames ? '' : 'upstreams',
        subBuilder: Upstream.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamList clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamList copyWith(void Function(UpstreamList) updates) =>
      super.copyWith((message) => updates(message as UpstreamList))
          as UpstreamList;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static UpstreamList create() => UpstreamList._();
  @$core.override
  UpstreamList createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static UpstreamList getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<UpstreamList>(create);
  static UpstreamList? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<Upstream> get upstreams => $_getList(0);
}

class UpstreamRef extends $pb.GeneratedMessage {
  factory UpstreamRef({
    $core.String? id,
  }) {
    final result = create();
    if (id != null) result.id = id;
    return result;
  }

  UpstreamRef._();

  factory UpstreamRef.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory UpstreamRef.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'UpstreamRef',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamRef clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamRef copyWith(void Function(UpstreamRef) updates) =>
      super.copyWith((message) => updates(message as UpstreamRef))
          as UpstreamRef;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static UpstreamRef create() => UpstreamRef._();
  @$core.override
  UpstreamRef createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static UpstreamRef getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<UpstreamRef>(create);
  static UpstreamRef? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);
}

class UpstreamDeleteReply extends $pb.GeneratedMessage {
  factory UpstreamDeleteReply({
    $core.bool? deleted,
  }) {
    final result = create();
    if (deleted != null) result.deleted = deleted;
    return result;
  }

  UpstreamDeleteReply._();

  factory UpstreamDeleteReply.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory UpstreamDeleteReply.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'UpstreamDeleteReply',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'deleted')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamDeleteReply clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamDeleteReply copyWith(void Function(UpstreamDeleteReply) updates) =>
      super.copyWith((message) => updates(message as UpstreamDeleteReply))
          as UpstreamDeleteReply;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static UpstreamDeleteReply create() => UpstreamDeleteReply._();
  @$core.override
  UpstreamDeleteReply createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static UpstreamDeleteReply getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<UpstreamDeleteReply>(create);
  static UpstreamDeleteReply? _defaultInstance;

  @$pb.TagNumber(1)
  $core.bool get deleted => $_getBF(0);
  @$pb.TagNumber(1)
  set deleted($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasDeleted() => $_has(0);
  @$pb.TagNumber(1)
  void clearDeleted() => $_clearField(1);
}

class UpstreamEnableRequest extends $pb.GeneratedMessage {
  factory UpstreamEnableRequest({
    $core.String? id,
    $core.bool? enabled,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (enabled != null) result.enabled = enabled;
    return result;
  }

  UpstreamEnableRequest._();

  factory UpstreamEnableRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory UpstreamEnableRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'UpstreamEnableRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aOB(2, _omitFieldNames ? '' : 'enabled')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamEnableRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamEnableRequest copyWith(
          void Function(UpstreamEnableRequest) updates) =>
      super.copyWith((message) => updates(message as UpstreamEnableRequest))
          as UpstreamEnableRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static UpstreamEnableRequest create() => UpstreamEnableRequest._();
  @$core.override
  UpstreamEnableRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static UpstreamEnableRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<UpstreamEnableRequest>(create);
  static UpstreamEnableRequest? _defaultInstance;

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

class RoutePolicyRef extends $pb.GeneratedMessage {
  factory RoutePolicyRef() => create();

  RoutePolicyRef._();

  factory RoutePolicyRef.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RoutePolicyRef.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RoutePolicyRef',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoutePolicyRef clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoutePolicyRef copyWith(void Function(RoutePolicyRef) updates) =>
      super.copyWith((message) => updates(message as RoutePolicyRef))
          as RoutePolicyRef;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RoutePolicyRef create() => RoutePolicyRef._();
  @$core.override
  RoutePolicyRef createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RoutePolicyRef getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RoutePolicyRef>(create);
  static RoutePolicyRef? _defaultInstance;
}

class RouteRequest extends $pb.GeneratedMessage {
  factory RouteRequest({
    $1.RouteHint? hint,
  }) {
    final result = create();
    if (hint != null) result.hint = hint;
    return result;
  }

  RouteRequest._();

  factory RouteRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RouteRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RouteRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<$1.RouteHint>(1, _omitFieldNames ? '' : 'hint',
        subBuilder: $1.RouteHint.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RouteRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RouteRequest copyWith(void Function(RouteRequest) updates) =>
      super.copyWith((message) => updates(message as RouteRequest))
          as RouteRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RouteRequest create() => RouteRequest._();
  @$core.override
  RouteRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RouteRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RouteRequest>(create);
  static RouteRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $1.RouteHint get hint => $_getN(0);
  @$pb.TagNumber(1)
  set hint($1.RouteHint value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasHint() => $_has(0);
  @$pb.TagNumber(1)
  void clearHint() => $_clearField(1);
  @$pb.TagNumber(1)
  $1.RouteHint ensureHint() => $_ensure(0);
}

class UpstreamHealthRequest extends $pb.GeneratedMessage {
  factory UpstreamHealthRequest() => create();

  UpstreamHealthRequest._();

  factory UpstreamHealthRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory UpstreamHealthRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'UpstreamHealthRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamHealthRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamHealthRequest copyWith(
          void Function(UpstreamHealthRequest) updates) =>
      super.copyWith((message) => updates(message as UpstreamHealthRequest))
          as UpstreamHealthRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static UpstreamHealthRequest create() => UpstreamHealthRequest._();
  @$core.override
  UpstreamHealthRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static UpstreamHealthRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<UpstreamHealthRequest>(create);
  static UpstreamHealthRequest? _defaultInstance;
}

class UpstreamHealthList extends $pb.GeneratedMessage {
  factory UpstreamHealthList({
    $core.Iterable<UpstreamHealth>? entries,
  }) {
    final result = create();
    if (entries != null) result.entries.addAll(entries);
    return result;
  }

  UpstreamHealthList._();

  factory UpstreamHealthList.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory UpstreamHealthList.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'UpstreamHealthList',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<UpstreamHealth>(1, _omitFieldNames ? '' : 'entries',
        subBuilder: UpstreamHealth.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamHealthList clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UpstreamHealthList copyWith(void Function(UpstreamHealthList) updates) =>
      super.copyWith((message) => updates(message as UpstreamHealthList))
          as UpstreamHealthList;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static UpstreamHealthList create() => UpstreamHealthList._();
  @$core.override
  UpstreamHealthList createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static UpstreamHealthList getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<UpstreamHealthList>(create);
  static UpstreamHealthList? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<UpstreamHealth> get entries => $_getList(0);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
