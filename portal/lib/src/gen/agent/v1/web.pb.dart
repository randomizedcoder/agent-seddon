// This is a generated file - do not edit.
//
// Generated from agent/v1/web.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

import 'web.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'web.pbenum.dart';

class WebFetchRequest extends $pb.GeneratedMessage {
  factory WebFetchRequest({
    $core.String? url,
    WebFormat? format,
    $fixnum.Int64? timeoutSecs,
    $fixnum.Int64? maxBytes,
    $core.int? maxRedirects,
  }) {
    final result = create();
    if (url != null) result.url = url;
    if (format != null) result.format = format;
    if (timeoutSecs != null) result.timeoutSecs = timeoutSecs;
    if (maxBytes != null) result.maxBytes = maxBytes;
    if (maxRedirects != null) result.maxRedirects = maxRedirects;
    return result;
  }

  WebFetchRequest._();

  factory WebFetchRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WebFetchRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WebFetchRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'url')
    ..aE<WebFormat>(2, _omitFieldNames ? '' : 'format',
        enumValues: WebFormat.values)
    ..a<$fixnum.Int64>(
        3, _omitFieldNames ? '' : 'timeoutSecs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..a<$fixnum.Int64>(
        4, _omitFieldNames ? '' : 'maxBytes', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aI(5, _omitFieldNames ? '' : 'maxRedirects',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebFetchRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebFetchRequest copyWith(void Function(WebFetchRequest) updates) =>
      super.copyWith((message) => updates(message as WebFetchRequest))
          as WebFetchRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WebFetchRequest create() => WebFetchRequest._();
  @$core.override
  WebFetchRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WebFetchRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WebFetchRequest>(create);
  static WebFetchRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get url => $_getSZ(0);
  @$pb.TagNumber(1)
  set url($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasUrl() => $_has(0);
  @$pb.TagNumber(1)
  void clearUrl() => $_clearField(1);

  @$pb.TagNumber(2)
  WebFormat get format => $_getN(1);
  @$pb.TagNumber(2)
  set format(WebFormat value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasFormat() => $_has(1);
  @$pb.TagNumber(2)
  void clearFormat() => $_clearField(2);

  @$pb.TagNumber(3)
  $fixnum.Int64 get timeoutSecs => $_getI64(2);
  @$pb.TagNumber(3)
  set timeoutSecs($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasTimeoutSecs() => $_has(2);
  @$pb.TagNumber(3)
  void clearTimeoutSecs() => $_clearField(3);

  /// Reject a body (declared or streamed) larger than this.
  @$pb.TagNumber(4)
  $fixnum.Int64 get maxBytes => $_getI64(3);
  @$pb.TagNumber(4)
  set maxBytes($fixnum.Int64 value) => $_setInt64(3, value);
  @$pb.TagNumber(4)
  $core.bool hasMaxBytes() => $_has(3);
  @$pb.TagNumber(4)
  void clearMaxBytes() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.int get maxRedirects => $_getIZ(4);
  @$pb.TagNumber(5)
  set maxRedirects($core.int value) => $_setUnsignedInt32(4, value);
  @$pb.TagNumber(5)
  $core.bool hasMaxRedirects() => $_has(4);
  @$pb.TagNumber(5)
  void clearMaxRedirects() => $_clearField(5);
}

class WebFetchResponse extends $pb.GeneratedMessage {
  factory WebFetchResponse({
    $core.String? finalUrl,
    $core.int? status,
    $core.String? contentType,
    WebFormat? format,
    $core.String? body,
    $fixnum.Int64? bytes,
  }) {
    final result = create();
    if (finalUrl != null) result.finalUrl = finalUrl;
    if (status != null) result.status = status;
    if (contentType != null) result.contentType = contentType;
    if (format != null) result.format = format;
    if (body != null) result.body = body;
    if (bytes != null) result.bytes = bytes;
    return result;
  }

  WebFetchResponse._();

  factory WebFetchResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WebFetchResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WebFetchResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'finalUrl')
    ..aI(2, _omitFieldNames ? '' : 'status', fieldType: $pb.PbFieldType.OU3)
    ..aOS(3, _omitFieldNames ? '' : 'contentType')
    ..aE<WebFormat>(4, _omitFieldNames ? '' : 'format',
        enumValues: WebFormat.values)
    ..aOS(5, _omitFieldNames ? '' : 'body')
    ..a<$fixnum.Int64>(6, _omitFieldNames ? '' : 'bytes', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebFetchResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebFetchResponse copyWith(void Function(WebFetchResponse) updates) =>
      super.copyWith((message) => updates(message as WebFetchResponse))
          as WebFetchResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WebFetchResponse create() => WebFetchResponse._();
  @$core.override
  WebFetchResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WebFetchResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WebFetchResponse>(create);
  static WebFetchResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get finalUrl => $_getSZ(0);
  @$pb.TagNumber(1)
  set finalUrl($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasFinalUrl() => $_has(0);
  @$pb.TagNumber(1)
  void clearFinalUrl() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get status => $_getIZ(1);
  @$pb.TagNumber(2)
  set status($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasStatus() => $_has(1);
  @$pb.TagNumber(2)
  void clearStatus() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get contentType => $_getSZ(2);
  @$pb.TagNumber(3)
  set contentType($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasContentType() => $_has(2);
  @$pb.TagNumber(3)
  void clearContentType() => $_clearField(3);

  @$pb.TagNumber(4)
  WebFormat get format => $_getN(3);
  @$pb.TagNumber(4)
  set format(WebFormat value) => $_setField(4, value);
  @$pb.TagNumber(4)
  $core.bool hasFormat() => $_has(3);
  @$pb.TagNumber(4)
  void clearFormat() => $_clearField(4);

  /// The raw decoded body, not yet reduced to `format`.
  @$pb.TagNumber(5)
  $core.String get body => $_getSZ(4);
  @$pb.TagNumber(5)
  set body($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasBody() => $_has(4);
  @$pb.TagNumber(5)
  void clearBody() => $_clearField(5);

  @$pb.TagNumber(6)
  $fixnum.Int64 get bytes => $_getI64(5);
  @$pb.TagNumber(6)
  set bytes($fixnum.Int64 value) => $_setInt64(5, value);
  @$pb.TagNumber(6)
  $core.bool hasBytes() => $_has(5);
  @$pb.TagNumber(6)
  void clearBytes() => $_clearField(6);
}

class WebSearchRequest extends $pb.GeneratedMessage {
  factory WebSearchRequest({
    $core.String? text,
    $core.int? limit,
    $core.int? freshnessDays,
    $core.String? backend,
  }) {
    final result = create();
    if (text != null) result.text = text;
    if (limit != null) result.limit = limit;
    if (freshnessDays != null) result.freshnessDays = freshnessDays;
    if (backend != null) result.backend = backend;
    return result;
  }

  WebSearchRequest._();

  factory WebSearchRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WebSearchRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WebSearchRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'text')
    ..aI(2, _omitFieldNames ? '' : 'limit', fieldType: $pb.PbFieldType.OU3)
    ..aI(3, _omitFieldNames ? '' : 'freshnessDays',
        fieldType: $pb.PbFieldType.OU3)
    ..aOS(4, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebSearchRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebSearchRequest copyWith(void Function(WebSearchRequest) updates) =>
      super.copyWith((message) => updates(message as WebSearchRequest))
          as WebSearchRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WebSearchRequest create() => WebSearchRequest._();
  @$core.override
  WebSearchRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WebSearchRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WebSearchRequest>(create);
  static WebSearchRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get text => $_getSZ(0);
  @$pb.TagNumber(1)
  set text($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasText() => $_has(0);
  @$pb.TagNumber(1)
  void clearText() => $_clearField(1);

  /// 0 ⇒ the backend/config default.
  @$pb.TagNumber(2)
  $core.int get limit => $_getIZ(1);
  @$pb.TagNumber(2)
  set limit($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasLimit() => $_has(1);
  @$pb.TagNumber(2)
  void clearLimit() => $_clearField(2);

  /// 0 ⇒ no restriction.
  @$pb.TagNumber(3)
  $core.int get freshnessDays => $_getIZ(2);
  @$pb.TagNumber(3)
  set freshnessDays($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasFreshnessDays() => $_has(2);
  @$pb.TagNumber(3)
  void clearFreshnessDays() => $_clearField(3);

  /// Per-query backend selector; unknown names fall back to the default.
  @$pb.TagNumber(4)
  $core.String get backend => $_getSZ(3);
  @$pb.TagNumber(4)
  set backend($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasBackend() => $_has(3);
  @$pb.TagNumber(4)
  void clearBackend() => $_clearField(4);
}

class WebSearchResult extends $pb.GeneratedMessage {
  factory WebSearchResult({
    $core.String? url,
    $core.String? title,
    $core.String? snippet,
    $core.double? score,
    $fixnum.Int64? publishedMs,
  }) {
    final result = create();
    if (url != null) result.url = url;
    if (title != null) result.title = title;
    if (snippet != null) result.snippet = snippet;
    if (score != null) result.score = score;
    if (publishedMs != null) result.publishedMs = publishedMs;
    return result;
  }

  WebSearchResult._();

  factory WebSearchResult.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WebSearchResult.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WebSearchResult',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'url')
    ..aOS(2, _omitFieldNames ? '' : 'title')
    ..aOS(3, _omitFieldNames ? '' : 'snippet')
    ..aD(4, _omitFieldNames ? '' : 'score', fieldType: $pb.PbFieldType.OF)
    ..a<$fixnum.Int64>(
        5, _omitFieldNames ? '' : 'publishedMs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebSearchResult clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebSearchResult copyWith(void Function(WebSearchResult) updates) =>
      super.copyWith((message) => updates(message as WebSearchResult))
          as WebSearchResult;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WebSearchResult create() => WebSearchResult._();
  @$core.override
  WebSearchResult createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WebSearchResult getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WebSearchResult>(create);
  static WebSearchResult? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get url => $_getSZ(0);
  @$pb.TagNumber(1)
  set url($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasUrl() => $_has(0);
  @$pb.TagNumber(1)
  void clearUrl() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get title => $_getSZ(1);
  @$pb.TagNumber(2)
  set title($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasTitle() => $_has(1);
  @$pb.TagNumber(2)
  void clearTitle() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get snippet => $_getSZ(2);
  @$pb.TagNumber(3)
  set snippet($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasSnippet() => $_has(2);
  @$pb.TagNumber(3)
  void clearSnippet() => $_clearField(3);

  /// Relevance in [0,1].
  @$pb.TagNumber(4)
  $core.double get score => $_getN(3);
  @$pb.TagNumber(4)
  set score($core.double value) => $_setFloat(3, value);
  @$pb.TagNumber(4)
  $core.bool hasScore() => $_has(3);
  @$pb.TagNumber(4)
  void clearScore() => $_clearField(4);

  @$pb.TagNumber(5)
  $fixnum.Int64 get publishedMs => $_getI64(4);
  @$pb.TagNumber(5)
  set publishedMs($fixnum.Int64 value) => $_setInt64(4, value);
  @$pb.TagNumber(5)
  $core.bool hasPublishedMs() => $_has(4);
  @$pb.TagNumber(5)
  void clearPublishedMs() => $_clearField(5);
}

class WebSearchResponse extends $pb.GeneratedMessage {
  factory WebSearchResponse({
    $core.Iterable<WebSearchResult>? results,
  }) {
    final result = create();
    if (results != null) result.results.addAll(results);
    return result;
  }

  WebSearchResponse._();

  factory WebSearchResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WebSearchResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WebSearchResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<WebSearchResult>(1, _omitFieldNames ? '' : 'results',
        subBuilder: WebSearchResult.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebSearchResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebSearchResponse copyWith(void Function(WebSearchResponse) updates) =>
      super.copyWith((message) => updates(message as WebSearchResponse))
          as WebSearchResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WebSearchResponse create() => WebSearchResponse._();
  @$core.override
  WebSearchResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WebSearchResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WebSearchResponse>(create);
  static WebSearchResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<WebSearchResult> get results => $_getList(0);
}

class WebCacheStatus extends $pb.GeneratedMessage {
  factory WebCacheStatus({
    WebCacheState? state,
  }) {
    final result = create();
    if (state != null) result.state = state;
    return result;
  }

  WebCacheStatus._();

  factory WebCacheStatus.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WebCacheStatus.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WebCacheStatus',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aE<WebCacheState>(1, _omitFieldNames ? '' : 'state',
        enumValues: WebCacheState.values)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebCacheStatus clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebCacheStatus copyWith(void Function(WebCacheStatus) updates) =>
      super.copyWith((message) => updates(message as WebCacheStatus))
          as WebCacheStatus;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WebCacheStatus create() => WebCacheStatus._();
  @$core.override
  WebCacheStatus createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WebCacheStatus getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WebCacheStatus>(create);
  static WebCacheStatus? _defaultInstance;

  @$pb.TagNumber(1)
  WebCacheState get state => $_getN(0);
  @$pb.TagNumber(1)
  set state(WebCacheState value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasState() => $_has(0);
  @$pb.TagNumber(1)
  void clearState() => $_clearField(1);
}

class WebSearchCapabilitiesRequest extends $pb.GeneratedMessage {
  factory WebSearchCapabilitiesRequest() => create();

  WebSearchCapabilitiesRequest._();

  factory WebSearchCapabilitiesRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WebSearchCapabilitiesRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WebSearchCapabilitiesRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebSearchCapabilitiesRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebSearchCapabilitiesRequest copyWith(
          void Function(WebSearchCapabilitiesRequest) updates) =>
      super.copyWith(
              (message) => updates(message as WebSearchCapabilitiesRequest))
          as WebSearchCapabilitiesRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WebSearchCapabilitiesRequest create() =>
      WebSearchCapabilitiesRequest._();
  @$core.override
  WebSearchCapabilitiesRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WebSearchCapabilitiesRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WebSearchCapabilitiesRequest>(create);
  static WebSearchCapabilitiesRequest? _defaultInstance;
}

class WebSearchCapabilities extends $pb.GeneratedMessage {
  factory WebSearchCapabilities({
    $core.String? backend,
    $core.bool? scored,
    $core.bool? freshness,
    $core.int? maxResults,
  }) {
    final result = create();
    if (backend != null) result.backend = backend;
    if (scored != null) result.scored = scored;
    if (freshness != null) result.freshness = freshness;
    if (maxResults != null) result.maxResults = maxResults;
    return result;
  }

  WebSearchCapabilities._();

  factory WebSearchCapabilities.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WebSearchCapabilities.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WebSearchCapabilities',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'backend')
    ..aOB(2, _omitFieldNames ? '' : 'scored')
    ..aOB(3, _omitFieldNames ? '' : 'freshness')
    ..aI(4, _omitFieldNames ? '' : 'maxResults', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebSearchCapabilities clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WebSearchCapabilities copyWith(
          void Function(WebSearchCapabilities) updates) =>
      super.copyWith((message) => updates(message as WebSearchCapabilities))
          as WebSearchCapabilities;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WebSearchCapabilities create() => WebSearchCapabilities._();
  @$core.override
  WebSearchCapabilities createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WebSearchCapabilities getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WebSearchCapabilities>(create);
  static WebSearchCapabilities? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get backend => $_getSZ(0);
  @$pb.TagNumber(1)
  set backend($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasBackend() => $_has(0);
  @$pb.TagNumber(1)
  void clearBackend() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.bool get scored => $_getBF(1);
  @$pb.TagNumber(2)
  set scored($core.bool value) => $_setBool(1, value);
  @$pb.TagNumber(2)
  $core.bool hasScored() => $_has(1);
  @$pb.TagNumber(2)
  void clearScored() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.bool get freshness => $_getBF(2);
  @$pb.TagNumber(3)
  set freshness($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasFreshness() => $_has(2);
  @$pb.TagNumber(3)
  void clearFreshness() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get maxResults => $_getIZ(3);
  @$pb.TagNumber(4)
  set maxResults($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasMaxResults() => $_has(3);
  @$pb.TagNumber(4)
  void clearMaxResults() => $_clearField(4);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
