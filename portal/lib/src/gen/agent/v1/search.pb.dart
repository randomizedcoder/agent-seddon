// This is a generated file - do not edit.
//
// Generated from agent/v1/search.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

import 'search.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'search.pbenum.dart';

class SearchQuery extends $pb.GeneratedMessage {
  factory SearchQuery({
    $core.String? text,
    SearchMode? mode,
    $core.Iterable<$core.String>? pathGlobs,
    $core.String? lang,
    $fixnum.Int64? limit,
    $core.int? fuzzyDistance,
  }) {
    final result = create();
    if (text != null) result.text = text;
    if (mode != null) result.mode = mode;
    if (pathGlobs != null) result.pathGlobs.addAll(pathGlobs);
    if (lang != null) result.lang = lang;
    if (limit != null) result.limit = limit;
    if (fuzzyDistance != null) result.fuzzyDistance = fuzzyDistance;
    return result;
  }

  SearchQuery._();

  factory SearchQuery.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SearchQuery.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SearchQuery',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'text')
    ..aE<SearchMode>(2, _omitFieldNames ? '' : 'mode',
        enumValues: SearchMode.values)
    ..pPS(3, _omitFieldNames ? '' : 'pathGlobs')
    ..aOS(4, _omitFieldNames ? '' : 'lang')
    ..a<$fixnum.Int64>(5, _omitFieldNames ? '' : 'limit', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aI(6, _omitFieldNames ? '' : 'fuzzyDistance',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SearchQuery clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SearchQuery copyWith(void Function(SearchQuery) updates) =>
      super.copyWith((message) => updates(message as SearchQuery))
          as SearchQuery;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SearchQuery create() => SearchQuery._();
  @$core.override
  SearchQuery createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SearchQuery getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SearchQuery>(create);
  static SearchQuery? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get text => $_getSZ(0);
  @$pb.TagNumber(1)
  set text($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasText() => $_has(0);
  @$pb.TagNumber(1)
  void clearText() => $_clearField(1);

  @$pb.TagNumber(2)
  SearchMode get mode => $_getN(1);
  @$pb.TagNumber(2)
  set mode(SearchMode value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasMode() => $_has(1);
  @$pb.TagNumber(2)
  void clearMode() => $_clearField(2);

  @$pb.TagNumber(3)
  $pb.PbList<$core.String> get pathGlobs => $_getList(2);

  @$pb.TagNumber(4)
  $core.String get lang => $_getSZ(3);
  @$pb.TagNumber(4)
  set lang($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasLang() => $_has(3);
  @$pb.TagNumber(4)
  void clearLang() => $_clearField(4);

  @$pb.TagNumber(5)
  $fixnum.Int64 get limit => $_getI64(4);
  @$pb.TagNumber(5)
  set limit($fixnum.Int64 value) => $_setInt64(4, value);
  @$pb.TagNumber(5)
  $core.bool hasLimit() => $_has(4);
  @$pb.TagNumber(5)
  void clearLimit() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.int get fuzzyDistance => $_getIZ(5);
  @$pb.TagNumber(6)
  set fuzzyDistance($core.int value) => $_setUnsignedInt32(5, value);
  @$pb.TagNumber(6)
  $core.bool hasFuzzyDistance() => $_has(5);
  @$pb.TagNumber(6)
  void clearFuzzyDistance() => $_clearField(6);
}

class SearchHit extends $pb.GeneratedMessage {
  factory SearchHit({
    $core.String? path,
    $core.int? line,
    $core.int? colStart,
    $core.int? colEnd,
    $core.double? score,
    $core.String? snippet,
  }) {
    final result = create();
    if (path != null) result.path = path;
    if (line != null) result.line = line;
    if (colStart != null) result.colStart = colStart;
    if (colEnd != null) result.colEnd = colEnd;
    if (score != null) result.score = score;
    if (snippet != null) result.snippet = snippet;
    return result;
  }

  SearchHit._();

  factory SearchHit.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SearchHit.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SearchHit',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'path')
    ..aI(2, _omitFieldNames ? '' : 'line', fieldType: $pb.PbFieldType.OU3)
    ..aI(3, _omitFieldNames ? '' : 'colStart', fieldType: $pb.PbFieldType.OU3)
    ..aI(4, _omitFieldNames ? '' : 'colEnd', fieldType: $pb.PbFieldType.OU3)
    ..aD(5, _omitFieldNames ? '' : 'score', fieldType: $pb.PbFieldType.OF)
    ..aOS(6, _omitFieldNames ? '' : 'snippet')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SearchHit clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SearchHit copyWith(void Function(SearchHit) updates) =>
      super.copyWith((message) => updates(message as SearchHit)) as SearchHit;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SearchHit create() => SearchHit._();
  @$core.override
  SearchHit createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SearchHit getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<SearchHit>(create);
  static SearchHit? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get path => $_getSZ(0);
  @$pb.TagNumber(1)
  set path($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPath() => $_has(0);
  @$pb.TagNumber(1)
  void clearPath() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get line => $_getIZ(1);
  @$pb.TagNumber(2)
  set line($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasLine() => $_has(1);
  @$pb.TagNumber(2)
  void clearLine() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get colStart => $_getIZ(2);
  @$pb.TagNumber(3)
  set colStart($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasColStart() => $_has(2);
  @$pb.TagNumber(3)
  void clearColStart() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.int get colEnd => $_getIZ(3);
  @$pb.TagNumber(4)
  set colEnd($core.int value) => $_setUnsignedInt32(3, value);
  @$pb.TagNumber(4)
  $core.bool hasColEnd() => $_has(3);
  @$pb.TagNumber(4)
  void clearColEnd() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.double get score => $_getN(4);
  @$pb.TagNumber(5)
  set score($core.double value) => $_setFloat(4, value);
  @$pb.TagNumber(5)
  $core.bool hasScore() => $_has(4);
  @$pb.TagNumber(5)
  void clearScore() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.String get snippet => $_getSZ(5);
  @$pb.TagNumber(6)
  set snippet($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasSnippet() => $_has(5);
  @$pb.TagNumber(6)
  void clearSnippet() => $_clearField(6);
}

class SearchRequest extends $pb.GeneratedMessage {
  factory SearchRequest({
    SearchQuery? query,
    $core.String? backend,
  }) {
    final result = create();
    if (query != null) result.query = query;
    if (backend != null) result.backend = backend;
    return result;
  }

  SearchRequest._();

  factory SearchRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SearchRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SearchRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<SearchQuery>(1, _omitFieldNames ? '' : 'query',
        subBuilder: SearchQuery.create)
    ..aOS(2, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SearchRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SearchRequest copyWith(void Function(SearchRequest) updates) =>
      super.copyWith((message) => updates(message as SearchRequest))
          as SearchRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SearchRequest create() => SearchRequest._();
  @$core.override
  SearchRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SearchRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SearchRequest>(create);
  static SearchRequest? _defaultInstance;

  @$pb.TagNumber(1)
  SearchQuery get query => $_getN(0);
  @$pb.TagNumber(1)
  set query(SearchQuery value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasQuery() => $_has(0);
  @$pb.TagNumber(1)
  void clearQuery() => $_clearField(1);
  @$pb.TagNumber(1)
  SearchQuery ensureQuery() => $_ensure(0);

  /// Which backend to query. Empty ⇒ the server's default backend.
  @$pb.TagNumber(2)
  $core.String get backend => $_getSZ(1);
  @$pb.TagNumber(2)
  set backend($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBackend() => $_has(1);
  @$pb.TagNumber(2)
  void clearBackend() => $_clearField(2);
}

class SearchResponse extends $pb.GeneratedMessage {
  factory SearchResponse({
    $core.Iterable<SearchHit>? hits,
    $core.String? backend,
  }) {
    final result = create();
    if (hits != null) result.hits.addAll(hits);
    if (backend != null) result.backend = backend;
    return result;
  }

  SearchResponse._();

  factory SearchResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SearchResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SearchResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<SearchHit>(1, _omitFieldNames ? '' : 'hits',
        subBuilder: SearchHit.create)
    ..aOS(2, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SearchResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SearchResponse copyWith(void Function(SearchResponse) updates) =>
      super.copyWith((message) => updates(message as SearchResponse))
          as SearchResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SearchResponse create() => SearchResponse._();
  @$core.override
  SearchResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SearchResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SearchResponse>(create);
  static SearchResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<SearchHit> get hits => $_getList(0);

  /// The backend that served the response (echoed for attribution).
  @$pb.TagNumber(2)
  $core.String get backend => $_getSZ(1);
  @$pb.TagNumber(2)
  set backend($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBackend() => $_has(1);
  @$pb.TagNumber(2)
  void clearBackend() => $_clearField(2);
}

class SearchCapabilities extends $pb.GeneratedMessage {
  factory SearchCapabilities({
    $core.String? backend,
    $core.Iterable<SearchMode>? modes,
    $core.bool? contentSearch,
    $core.bool? scored,
    $core.bool? incremental,
    $core.int? maxConcurrentQueries,
  }) {
    final result = create();
    if (backend != null) result.backend = backend;
    if (modes != null) result.modes.addAll(modes);
    if (contentSearch != null) result.contentSearch = contentSearch;
    if (scored != null) result.scored = scored;
    if (incremental != null) result.incremental = incremental;
    if (maxConcurrentQueries != null)
      result.maxConcurrentQueries = maxConcurrentQueries;
    return result;
  }

  SearchCapabilities._();

  factory SearchCapabilities.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SearchCapabilities.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SearchCapabilities',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'backend')
    ..pc<SearchMode>(2, _omitFieldNames ? '' : 'modes', $pb.PbFieldType.KE,
        valueOf: SearchMode.valueOf,
        enumValues: SearchMode.values,
        defaultEnumValue: SearchMode.SEARCH_MODE_LITERAL)
    ..aOB(3, _omitFieldNames ? '' : 'contentSearch')
    ..aOB(4, _omitFieldNames ? '' : 'scored')
    ..aOB(5, _omitFieldNames ? '' : 'incremental')
    ..aI(6, _omitFieldNames ? '' : 'maxConcurrentQueries',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SearchCapabilities clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SearchCapabilities copyWith(void Function(SearchCapabilities) updates) =>
      super.copyWith((message) => updates(message as SearchCapabilities))
          as SearchCapabilities;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SearchCapabilities create() => SearchCapabilities._();
  @$core.override
  SearchCapabilities createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SearchCapabilities getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SearchCapabilities>(create);
  static SearchCapabilities? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get backend => $_getSZ(0);
  @$pb.TagNumber(1)
  set backend($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasBackend() => $_has(0);
  @$pb.TagNumber(1)
  void clearBackend() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<SearchMode> get modes => $_getList(1);

  @$pb.TagNumber(3)
  $core.bool get contentSearch => $_getBF(2);
  @$pb.TagNumber(3)
  set contentSearch($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasContentSearch() => $_has(2);
  @$pb.TagNumber(3)
  void clearContentSearch() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.bool get scored => $_getBF(3);
  @$pb.TagNumber(4)
  set scored($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasScored() => $_has(3);
  @$pb.TagNumber(4)
  void clearScored() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.bool get incremental => $_getBF(4);
  @$pb.TagNumber(5)
  set incremental($core.bool value) => $_setBool(4, value);
  @$pb.TagNumber(5)
  $core.bool hasIncremental() => $_has(4);
  @$pb.TagNumber(5)
  void clearIncremental() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.int get maxConcurrentQueries => $_getIZ(5);
  @$pb.TagNumber(6)
  set maxConcurrentQueries($core.int value) => $_setUnsignedInt32(5, value);
  @$pb.TagNumber(6)
  $core.bool hasMaxConcurrentQueries() => $_has(5);
  @$pb.TagNumber(6)
  void clearMaxConcurrentQueries() => $_clearField(6);
}

class SearchCapabilitiesRequest extends $pb.GeneratedMessage {
  factory SearchCapabilitiesRequest({
    $core.String? backend,
  }) {
    final result = create();
    if (backend != null) result.backend = backend;
    return result;
  }

  SearchCapabilitiesRequest._();

  factory SearchCapabilitiesRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SearchCapabilitiesRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SearchCapabilitiesRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SearchCapabilitiesRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SearchCapabilitiesRequest copyWith(
          void Function(SearchCapabilitiesRequest) updates) =>
      super.copyWith((message) => updates(message as SearchCapabilitiesRequest))
          as SearchCapabilitiesRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SearchCapabilitiesRequest create() => SearchCapabilitiesRequest._();
  @$core.override
  SearchCapabilitiesRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SearchCapabilitiesRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SearchCapabilitiesRequest>(create);
  static SearchCapabilitiesRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get backend => $_getSZ(0);
  @$pb.TagNumber(1)
  set backend($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasBackend() => $_has(0);
  @$pb.TagNumber(1)
  void clearBackend() => $_clearField(1);
}

class SearchCapabilitiesResponse extends $pb.GeneratedMessage {
  factory SearchCapabilitiesResponse({
    $core.Iterable<SearchCapabilities>? backends,
  }) {
    final result = create();
    if (backends != null) result.backends.addAll(backends);
    return result;
  }

  SearchCapabilitiesResponse._();

  factory SearchCapabilitiesResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SearchCapabilitiesResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SearchCapabilitiesResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<SearchCapabilities>(1, _omitFieldNames ? '' : 'backends',
        subBuilder: SearchCapabilities.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SearchCapabilitiesResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SearchCapabilitiesResponse copyWith(
          void Function(SearchCapabilitiesResponse) updates) =>
      super.copyWith(
              (message) => updates(message as SearchCapabilitiesResponse))
          as SearchCapabilitiesResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SearchCapabilitiesResponse create() => SearchCapabilitiesResponse._();
  @$core.override
  SearchCapabilitiesResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SearchCapabilitiesResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SearchCapabilitiesResponse>(create);
  static SearchCapabilitiesResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<SearchCapabilities> get backends => $_getList(0);
}

class IndexStatus extends $pb.GeneratedMessage {
  factory IndexStatus({
    IndexState? state,
    $fixnum.Int64? indexedFiles,
    $fixnum.Int64? lastIndexedMs,
    $core.String? manifestDigest,
    $core.String? backend,
  }) {
    final result = create();
    if (state != null) result.state = state;
    if (indexedFiles != null) result.indexedFiles = indexedFiles;
    if (lastIndexedMs != null) result.lastIndexedMs = lastIndexedMs;
    if (manifestDigest != null) result.manifestDigest = manifestDigest;
    if (backend != null) result.backend = backend;
    return result;
  }

  IndexStatus._();

  factory IndexStatus.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory IndexStatus.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'IndexStatus',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aE<IndexState>(1, _omitFieldNames ? '' : 'state',
        enumValues: IndexState.values)
    ..a<$fixnum.Int64>(
        2, _omitFieldNames ? '' : 'indexedFiles', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..a<$fixnum.Int64>(
        3, _omitFieldNames ? '' : 'lastIndexedMs', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOS(4, _omitFieldNames ? '' : 'manifestDigest')
    ..aOS(5, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  IndexStatus clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  IndexStatus copyWith(void Function(IndexStatus) updates) =>
      super.copyWith((message) => updates(message as IndexStatus))
          as IndexStatus;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static IndexStatus create() => IndexStatus._();
  @$core.override
  IndexStatus createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static IndexStatus getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<IndexStatus>(create);
  static IndexStatus? _defaultInstance;

  @$pb.TagNumber(1)
  IndexState get state => $_getN(0);
  @$pb.TagNumber(1)
  set state(IndexState value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasState() => $_has(0);
  @$pb.TagNumber(1)
  void clearState() => $_clearField(1);

  @$pb.TagNumber(2)
  $fixnum.Int64 get indexedFiles => $_getI64(1);
  @$pb.TagNumber(2)
  set indexedFiles($fixnum.Int64 value) => $_setInt64(1, value);
  @$pb.TagNumber(2)
  $core.bool hasIndexedFiles() => $_has(1);
  @$pb.TagNumber(2)
  void clearIndexedFiles() => $_clearField(2);

  @$pb.TagNumber(3)
  $fixnum.Int64 get lastIndexedMs => $_getI64(2);
  @$pb.TagNumber(3)
  set lastIndexedMs($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasLastIndexedMs() => $_has(2);
  @$pb.TagNumber(3)
  void clearLastIndexedMs() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get manifestDigest => $_getSZ(3);
  @$pb.TagNumber(4)
  set manifestDigest($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasManifestDigest() => $_has(3);
  @$pb.TagNumber(4)
  void clearManifestDigest() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get backend => $_getSZ(4);
  @$pb.TagNumber(5)
  set backend($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasBackend() => $_has(4);
  @$pb.TagNumber(5)
  void clearBackend() => $_clearField(5);
}

class StatusRequest extends $pb.GeneratedMessage {
  factory StatusRequest({
    $core.String? backend,
  }) {
    final result = create();
    if (backend != null) result.backend = backend;
    return result;
  }

  StatusRequest._();

  factory StatusRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory StatusRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'StatusRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  StatusRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  StatusRequest copyWith(void Function(StatusRequest) updates) =>
      super.copyWith((message) => updates(message as StatusRequest))
          as StatusRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static StatusRequest create() => StatusRequest._();
  @$core.override
  StatusRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static StatusRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<StatusRequest>(create);
  static StatusRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get backend => $_getSZ(0);
  @$pb.TagNumber(1)
  set backend($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasBackend() => $_has(0);
  @$pb.TagNumber(1)
  void clearBackend() => $_clearField(1);
}

class StatusResponse extends $pb.GeneratedMessage {
  factory StatusResponse({
    $core.Iterable<IndexStatus>? backends,
  }) {
    final result = create();
    if (backends != null) result.backends.addAll(backends);
    return result;
  }

  StatusResponse._();

  factory StatusResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory StatusResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'StatusResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<IndexStatus>(1, _omitFieldNames ? '' : 'backends',
        subBuilder: IndexStatus.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  StatusResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  StatusResponse copyWith(void Function(StatusResponse) updates) =>
      super.copyWith((message) => updates(message as StatusResponse))
          as StatusResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static StatusResponse create() => StatusResponse._();
  @$core.override
  StatusResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static StatusResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<StatusResponse>(create);
  static StatusResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<IndexStatus> get backends => $_getList(0);
}

class ReindexRequest extends $pb.GeneratedMessage {
  factory ReindexRequest({
    $core.String? backend,
  }) {
    final result = create();
    if (backend != null) result.backend = backend;
    return result;
  }

  ReindexRequest._();

  factory ReindexRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ReindexRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReindexRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReindexRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReindexRequest copyWith(void Function(ReindexRequest) updates) =>
      super.copyWith((message) => updates(message as ReindexRequest))
          as ReindexRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ReindexRequest create() => ReindexRequest._();
  @$core.override
  ReindexRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ReindexRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReindexRequest>(create);
  static ReindexRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get backend => $_getSZ(0);
  @$pb.TagNumber(1)
  set backend($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasBackend() => $_has(0);
  @$pb.TagNumber(1)
  void clearBackend() => $_clearField(1);
}

class ListFilesRequest extends $pb.GeneratedMessage {
  factory ListFilesRequest({
    $core.Iterable<$core.String>? globs,
    $core.String? backend,
  }) {
    final result = create();
    if (globs != null) result.globs.addAll(globs);
    if (backend != null) result.backend = backend;
    return result;
  }

  ListFilesRequest._();

  factory ListFilesRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ListFilesRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ListFilesRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPS(1, _omitFieldNames ? '' : 'globs')
    ..aOS(2, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListFilesRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListFilesRequest copyWith(void Function(ListFilesRequest) updates) =>
      super.copyWith((message) => updates(message as ListFilesRequest))
          as ListFilesRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ListFilesRequest create() => ListFilesRequest._();
  @$core.override
  ListFilesRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ListFilesRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ListFilesRequest>(create);
  static ListFilesRequest? _defaultInstance;

  /// Glob patterns to filter the indexed paths. Empty ⇒ every indexed file.
  @$pb.TagNumber(1)
  $pb.PbList<$core.String> get globs => $_getList(0);

  /// Which backend to list from. Empty ⇒ the server's default backend.
  @$pb.TagNumber(2)
  $core.String get backend => $_getSZ(1);
  @$pb.TagNumber(2)
  set backend($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBackend() => $_has(1);
  @$pb.TagNumber(2)
  void clearBackend() => $_clearField(2);
}

class ListFilesResponse extends $pb.GeneratedMessage {
  factory ListFilesResponse({
    $core.Iterable<$core.String>? paths,
    $core.String? backend,
  }) {
    final result = create();
    if (paths != null) result.paths.addAll(paths);
    if (backend != null) result.backend = backend;
    return result;
  }

  ListFilesResponse._();

  factory ListFilesResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ListFilesResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ListFilesResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPS(1, _omitFieldNames ? '' : 'paths')
    ..aOS(2, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListFilesResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListFilesResponse copyWith(void Function(ListFilesResponse) updates) =>
      super.copyWith((message) => updates(message as ListFilesResponse))
          as ListFilesResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ListFilesResponse create() => ListFilesResponse._();
  @$core.override
  ListFilesResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ListFilesResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ListFilesResponse>(create);
  static ListFilesResponse? _defaultInstance;

  /// Matching indexed paths, sorted + deduplicated by the backend.
  @$pb.TagNumber(1)
  $pb.PbList<$core.String> get paths => $_getList(0);

  /// The backend that served the response (echoed for attribution).
  @$pb.TagNumber(2)
  $core.String get backend => $_getSZ(1);
  @$pb.TagNumber(2)
  set backend($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBackend() => $_has(1);
  @$pb.TagNumber(2)
  void clearBackend() => $_clearField(2);
}

/// One increment of reindex progress (server-streamed by SearchService.Reindex).
class ReindexProgress extends $pb.GeneratedMessage {
  factory ReindexProgress({
    $fixnum.Int64? filesDone,
    $fixnum.Int64? filesTotal,
    $core.bool? done,
    $core.String? backend,
  }) {
    final result = create();
    if (filesDone != null) result.filesDone = filesDone;
    if (filesTotal != null) result.filesTotal = filesTotal;
    if (done != null) result.done = done;
    if (backend != null) result.backend = backend;
    return result;
  }

  ReindexProgress._();

  factory ReindexProgress.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ReindexProgress.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ReindexProgress',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..a<$fixnum.Int64>(
        1, _omitFieldNames ? '' : 'filesDone', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..a<$fixnum.Int64>(
        2, _omitFieldNames ? '' : 'filesTotal', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOB(3, _omitFieldNames ? '' : 'done')
    ..aOS(4, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReindexProgress clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ReindexProgress copyWith(void Function(ReindexProgress) updates) =>
      super.copyWith((message) => updates(message as ReindexProgress))
          as ReindexProgress;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ReindexProgress create() => ReindexProgress._();
  @$core.override
  ReindexProgress createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ReindexProgress getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ReindexProgress>(create);
  static ReindexProgress? _defaultInstance;

  @$pb.TagNumber(1)
  $fixnum.Int64 get filesDone => $_getI64(0);
  @$pb.TagNumber(1)
  set filesDone($fixnum.Int64 value) => $_setInt64(0, value);
  @$pb.TagNumber(1)
  $core.bool hasFilesDone() => $_has(0);
  @$pb.TagNumber(1)
  void clearFilesDone() => $_clearField(1);

  @$pb.TagNumber(2)
  $fixnum.Int64 get filesTotal => $_getI64(1);
  @$pb.TagNumber(2)
  set filesTotal($fixnum.Int64 value) => $_setInt64(1, value);
  @$pb.TagNumber(2)
  $core.bool hasFilesTotal() => $_has(1);
  @$pb.TagNumber(2)
  void clearFilesTotal() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.bool get done => $_getBF(2);
  @$pb.TagNumber(3)
  set done($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasDone() => $_has(2);
  @$pb.TagNumber(3)
  void clearDone() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get backend => $_getSZ(3);
  @$pb.TagNumber(4)
  set backend($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasBackend() => $_has(3);
  @$pb.TagNumber(4)
  void clearBackend() => $_clearField(4);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
