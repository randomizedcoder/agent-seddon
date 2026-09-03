// This is a generated file - do not edit.
//
// Generated from agent/v1/ast.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

import 'ast.pbenum.dart';
import 'search.pb.dart' as $1;

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'ast.pbenum.dart';

class Symbol extends $pb.GeneratedMessage {
  factory Symbol({
    $core.int? id,
    SymbolKind? kind,
    $core.String? name,
    $core.String? recv,
    $core.String? package,
    $core.String? file,
    $core.int? line,
    $core.bool? exported,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (kind != null) result.kind = kind;
    if (name != null) result.name = name;
    if (recv != null) result.recv = recv;
    if (package != null) result.package = package;
    if (file != null) result.file = file;
    if (line != null) result.line = line;
    if (exported != null) result.exported = exported;
    return result;
  }

  Symbol._();

  factory Symbol.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory Symbol.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Symbol',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aI(1, _omitFieldNames ? '' : 'id', fieldType: $pb.PbFieldType.OU3)
    ..aE<SymbolKind>(2, _omitFieldNames ? '' : 'kind',
        enumValues: SymbolKind.values)
    ..aOS(3, _omitFieldNames ? '' : 'name')
    ..aOS(4, _omitFieldNames ? '' : 'recv')
    ..aOS(5, _omitFieldNames ? '' : 'package')
    ..aOS(6, _omitFieldNames ? '' : 'file')
    ..aI(7, _omitFieldNames ? '' : 'line', fieldType: $pb.PbFieldType.OU3)
    ..aOB(8, _omitFieldNames ? '' : 'exported')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Symbol clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Symbol copyWith(void Function(Symbol) updates) =>
      super.copyWith((message) => updates(message as Symbol)) as Symbol;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static Symbol create() => Symbol._();
  @$core.override
  Symbol createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static Symbol getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<Symbol>(create);
  static Symbol? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get id => $_getIZ(0);
  @$pb.TagNumber(1)
  set id($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  @$pb.TagNumber(2)
  SymbolKind get kind => $_getN(1);
  @$pb.TagNumber(2)
  set kind(SymbolKind value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasKind() => $_has(1);
  @$pb.TagNumber(2)
  void clearKind() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get name => $_getSZ(2);
  @$pb.TagNumber(3)
  set name($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasName() => $_has(2);
  @$pb.TagNumber(3)
  void clearName() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get recv => $_getSZ(3);
  @$pb.TagNumber(4)
  set recv($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasRecv() => $_has(3);
  @$pb.TagNumber(4)
  void clearRecv() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get package => $_getSZ(4);
  @$pb.TagNumber(5)
  set package($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasPackage() => $_has(4);
  @$pb.TagNumber(5)
  void clearPackage() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.String get file => $_getSZ(5);
  @$pb.TagNumber(6)
  set file($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasFile() => $_has(5);
  @$pb.TagNumber(6)
  void clearFile() => $_clearField(6);

  @$pb.TagNumber(7)
  $core.int get line => $_getIZ(6);
  @$pb.TagNumber(7)
  set line($core.int value) => $_setUnsignedInt32(6, value);
  @$pb.TagNumber(7)
  $core.bool hasLine() => $_has(6);
  @$pb.TagNumber(7)
  void clearLine() => $_clearField(7);

  @$pb.TagNumber(8)
  $core.bool get exported => $_getBF(7);
  @$pb.TagNumber(8)
  set exported($core.bool value) => $_setBool(7, value);
  @$pb.TagNumber(8)
  $core.bool hasExported() => $_has(7);
  @$pb.TagNumber(8)
  void clearExported() => $_clearField(8);
}

/// How a query names a target symbol: by stable graph id, or by name + optional
/// package/receiver. Model-supplied strings are matched, never interpolated.
class SymbolRef extends $pb.GeneratedMessage {
  factory SymbolRef({
    $core.int? id,
    $core.String? name,
    $core.String? package,
    $core.String? recv,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (name != null) result.name = name;
    if (package != null) result.package = package;
    if (recv != null) result.recv = recv;
    return result;
  }

  SymbolRef._();

  factory SymbolRef.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SymbolRef.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SymbolRef',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aI(1, _omitFieldNames ? '' : 'id', fieldType: $pb.PbFieldType.OU3)
    ..aOS(2, _omitFieldNames ? '' : 'name')
    ..aOS(3, _omitFieldNames ? '' : 'package')
    ..aOS(4, _omitFieldNames ? '' : 'recv')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SymbolRef clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SymbolRef copyWith(void Function(SymbolRef) updates) =>
      super.copyWith((message) => updates(message as SymbolRef)) as SymbolRef;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SymbolRef create() => SymbolRef._();
  @$core.override
  SymbolRef createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SymbolRef getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<SymbolRef>(create);
  static SymbolRef? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get id => $_getIZ(0);
  @$pb.TagNumber(1)
  set id($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get name => $_getSZ(1);
  @$pb.TagNumber(2)
  set name($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasName() => $_has(1);
  @$pb.TagNumber(2)
  void clearName() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get package => $_getSZ(2);
  @$pb.TagNumber(3)
  set package($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasPackage() => $_has(2);
  @$pb.TagNumber(3)
  void clearPackage() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get recv => $_getSZ(3);
  @$pb.TagNumber(4)
  set recv($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasRecv() => $_has(3);
  @$pb.TagNumber(4)
  void clearRecv() => $_clearField(4);
}

class CallEdge extends $pb.GeneratedMessage {
  factory CallEdge({
    $core.int? callerId,
    $core.int? calleeId,
  }) {
    final result = create();
    if (callerId != null) result.callerId = callerId;
    if (calleeId != null) result.calleeId = calleeId;
    return result;
  }

  CallEdge._();

  factory CallEdge.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CallEdge.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CallEdge',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aI(1, _omitFieldNames ? '' : 'callerId', fieldType: $pb.PbFieldType.OU3)
    ..aI(2, _omitFieldNames ? '' : 'calleeId', fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CallEdge clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CallEdge copyWith(void Function(CallEdge) updates) =>
      super.copyWith((message) => updates(message as CallEdge)) as CallEdge;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CallEdge create() => CallEdge._();
  @$core.override
  CallEdge createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CallEdge getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<CallEdge>(create);
  static CallEdge? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get callerId => $_getIZ(0);
  @$pb.TagNumber(1)
  set callerId($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasCallerId() => $_has(0);
  @$pb.TagNumber(1)
  void clearCallerId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get calleeId => $_getIZ(1);
  @$pb.TagNumber(2)
  set calleeId($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCalleeId() => $_has(1);
  @$pb.TagNumber(2)
  void clearCalleeId() => $_clearField(2);
}

/// A subgraph result: nodes, the caller→callee edges among them, and the roots the
/// query started from.
class AstCallGraph extends $pb.GeneratedMessage {
  factory AstCallGraph({
    $core.Iterable<Symbol>? nodes,
    $core.Iterable<CallEdge>? edges,
    $core.Iterable<$core.int>? roots,
    $core.bool? truncated,
    $core.String? backend,
  }) {
    final result = create();
    if (nodes != null) result.nodes.addAll(nodes);
    if (edges != null) result.edges.addAll(edges);
    if (roots != null) result.roots.addAll(roots);
    if (truncated != null) result.truncated = truncated;
    if (backend != null) result.backend = backend;
    return result;
  }

  AstCallGraph._();

  factory AstCallGraph.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory AstCallGraph.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'AstCallGraph',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<Symbol>(1, _omitFieldNames ? '' : 'nodes', subBuilder: Symbol.create)
    ..pPM<CallEdge>(2, _omitFieldNames ? '' : 'edges',
        subBuilder: CallEdge.create)
    ..p<$core.int>(3, _omitFieldNames ? '' : 'roots', $pb.PbFieldType.KU3)
    ..aOB(4, _omitFieldNames ? '' : 'truncated')
    ..aOS(5, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AstCallGraph clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AstCallGraph copyWith(void Function(AstCallGraph) updates) =>
      super.copyWith((message) => updates(message as AstCallGraph))
          as AstCallGraph;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static AstCallGraph create() => AstCallGraph._();
  @$core.override
  AstCallGraph createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static AstCallGraph getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<AstCallGraph>(create);
  static AstCallGraph? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<Symbol> get nodes => $_getList(0);

  @$pb.TagNumber(2)
  $pb.PbList<CallEdge> get edges => $_getList(1);

  @$pb.TagNumber(3)
  $pb.PbList<$core.int> get roots => $_getList(2);

  @$pb.TagNumber(4)
  $core.bool get truncated => $_getBF(3);
  @$pb.TagNumber(4)
  set truncated($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasTruncated() => $_has(3);
  @$pb.TagNumber(4)
  void clearTruncated() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get backend => $_getSZ(4);
  @$pb.TagNumber(5)
  set backend($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasBackend() => $_has(4);
  @$pb.TagNumber(5)
  void clearBackend() => $_clearField(5);
}

/// One ordered call path (a chain of symbols).
class CallPath extends $pb.GeneratedMessage {
  factory CallPath({
    $core.Iterable<Symbol>? nodes,
  }) {
    final result = create();
    if (nodes != null) result.nodes.addAll(nodes);
    return result;
  }

  CallPath._();

  factory CallPath.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CallPath.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CallPath',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<Symbol>(1, _omitFieldNames ? '' : 'nodes', subBuilder: Symbol.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CallPath clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CallPath copyWith(void Function(CallPath) updates) =>
      super.copyWith((message) => updates(message as CallPath)) as CallPath;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CallPath create() => CallPath._();
  @$core.override
  CallPath createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CallPath getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<CallPath>(create);
  static CallPath? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<Symbol> get nodes => $_getList(0);
}

class AstCapabilities extends $pb.GeneratedMessage {
  factory AstCapabilities({
    $core.String? backend,
    $core.Iterable<$core.String>? languages,
    $core.Iterable<$core.String>? verbs,
    $core.bool? incremental,
  }) {
    final result = create();
    if (backend != null) result.backend = backend;
    if (languages != null) result.languages.addAll(languages);
    if (verbs != null) result.verbs.addAll(verbs);
    if (incremental != null) result.incremental = incremental;
    return result;
  }

  AstCapabilities._();

  factory AstCapabilities.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory AstCapabilities.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'AstCapabilities',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'backend')
    ..pPS(2, _omitFieldNames ? '' : 'languages')
    ..pPS(3, _omitFieldNames ? '' : 'verbs')
    ..aOB(4, _omitFieldNames ? '' : 'incremental')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AstCapabilities clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AstCapabilities copyWith(void Function(AstCapabilities) updates) =>
      super.copyWith((message) => updates(message as AstCapabilities))
          as AstCapabilities;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static AstCapabilities create() => AstCapabilities._();
  @$core.override
  AstCapabilities createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static AstCapabilities getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<AstCapabilities>(create);
  static AstCapabilities? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get backend => $_getSZ(0);
  @$pb.TagNumber(1)
  set backend($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasBackend() => $_has(0);
  @$pb.TagNumber(1)
  void clearBackend() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbList<$core.String> get languages => $_getList(1);

  @$pb.TagNumber(3)
  $pb.PbList<$core.String> get verbs => $_getList(2);

  @$pb.TagNumber(4)
  $core.bool get incremental => $_getBF(3);
  @$pb.TagNumber(4)
  set incremental($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasIncremental() => $_has(3);
  @$pb.TagNumber(4)
  void clearIncremental() => $_clearField(4);
}

class AstStatusRequest extends $pb.GeneratedMessage {
  factory AstStatusRequest({
    $core.String? backend,
  }) {
    final result = create();
    if (backend != null) result.backend = backend;
    return result;
  }

  AstStatusRequest._();

  factory AstStatusRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory AstStatusRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'AstStatusRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AstStatusRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AstStatusRequest copyWith(void Function(AstStatusRequest) updates) =>
      super.copyWith((message) => updates(message as AstStatusRequest))
          as AstStatusRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static AstStatusRequest create() => AstStatusRequest._();
  @$core.override
  AstStatusRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static AstStatusRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<AstStatusRequest>(create);
  static AstStatusRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get backend => $_getSZ(0);
  @$pb.TagNumber(1)
  set backend($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasBackend() => $_has(0);
  @$pb.TagNumber(1)
  void clearBackend() => $_clearField(1);
}

class AstStatusResponse extends $pb.GeneratedMessage {
  factory AstStatusResponse({
    $core.Iterable<$1.IndexStatus>? backends,
  }) {
    final result = create();
    if (backends != null) result.backends.addAll(backends);
    return result;
  }

  AstStatusResponse._();

  factory AstStatusResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory AstStatusResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'AstStatusResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<$1.IndexStatus>(1, _omitFieldNames ? '' : 'backends',
        subBuilder: $1.IndexStatus.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AstStatusResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AstStatusResponse copyWith(void Function(AstStatusResponse) updates) =>
      super.copyWith((message) => updates(message as AstStatusResponse))
          as AstStatusResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static AstStatusResponse create() => AstStatusResponse._();
  @$core.override
  AstStatusResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static AstStatusResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<AstStatusResponse>(create);
  static AstStatusResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<$1.IndexStatus> get backends => $_getList(0);
}

class AstCapabilitiesRequest extends $pb.GeneratedMessage {
  factory AstCapabilitiesRequest({
    $core.String? backend,
  }) {
    final result = create();
    if (backend != null) result.backend = backend;
    return result;
  }

  AstCapabilitiesRequest._();

  factory AstCapabilitiesRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory AstCapabilitiesRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'AstCapabilitiesRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AstCapabilitiesRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AstCapabilitiesRequest copyWith(
          void Function(AstCapabilitiesRequest) updates) =>
      super.copyWith((message) => updates(message as AstCapabilitiesRequest))
          as AstCapabilitiesRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static AstCapabilitiesRequest create() => AstCapabilitiesRequest._();
  @$core.override
  AstCapabilitiesRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static AstCapabilitiesRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<AstCapabilitiesRequest>(create);
  static AstCapabilitiesRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get backend => $_getSZ(0);
  @$pb.TagNumber(1)
  set backend($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasBackend() => $_has(0);
  @$pb.TagNumber(1)
  void clearBackend() => $_clearField(1);
}

class AstCapabilitiesResponse extends $pb.GeneratedMessage {
  factory AstCapabilitiesResponse({
    $core.Iterable<AstCapabilities>? backends,
  }) {
    final result = create();
    if (backends != null) result.backends.addAll(backends);
    return result;
  }

  AstCapabilitiesResponse._();

  factory AstCapabilitiesResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory AstCapabilitiesResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'AstCapabilitiesResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<AstCapabilities>(1, _omitFieldNames ? '' : 'backends',
        subBuilder: AstCapabilities.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AstCapabilitiesResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AstCapabilitiesResponse copyWith(
          void Function(AstCapabilitiesResponse) updates) =>
      super.copyWith((message) => updates(message as AstCapabilitiesResponse))
          as AstCapabilitiesResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static AstCapabilitiesResponse create() => AstCapabilitiesResponse._();
  @$core.override
  AstCapabilitiesResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static AstCapabilitiesResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<AstCapabilitiesResponse>(create);
  static AstCapabilitiesResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<AstCapabilities> get backends => $_getList(0);
}

class AstReindexRequest extends $pb.GeneratedMessage {
  factory AstReindexRequest({
    $core.String? backend,
  }) {
    final result = create();
    if (backend != null) result.backend = backend;
    return result;
  }

  AstReindexRequest._();

  factory AstReindexRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory AstReindexRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'AstReindexRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AstReindexRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AstReindexRequest copyWith(void Function(AstReindexRequest) updates) =>
      super.copyWith((message) => updates(message as AstReindexRequest))
          as AstReindexRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static AstReindexRequest create() => AstReindexRequest._();
  @$core.override
  AstReindexRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static AstReindexRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<AstReindexRequest>(create);
  static AstReindexRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get backend => $_getSZ(0);
  @$pb.TagNumber(1)
  set backend($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasBackend() => $_has(0);
  @$pb.TagNumber(1)
  void clearBackend() => $_clearField(1);
}

class FindSymbolRequest extends $pb.GeneratedMessage {
  factory FindSymbolRequest({
    $core.String? name,
    SymbolKind? kind,
    $core.String? package,
    $core.bool? exact,
    $fixnum.Int64? limit,
    $core.String? backend,
  }) {
    final result = create();
    if (name != null) result.name = name;
    if (kind != null) result.kind = kind;
    if (package != null) result.package = package;
    if (exact != null) result.exact = exact;
    if (limit != null) result.limit = limit;
    if (backend != null) result.backend = backend;
    return result;
  }

  FindSymbolRequest._();

  factory FindSymbolRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory FindSymbolRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'FindSymbolRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'name')
    ..aE<SymbolKind>(2, _omitFieldNames ? '' : 'kind',
        enumValues: SymbolKind.values)
    ..aOS(3, _omitFieldNames ? '' : 'package')
    ..aOB(4, _omitFieldNames ? '' : 'exact')
    ..a<$fixnum.Int64>(5, _omitFieldNames ? '' : 'limit', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOS(6, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FindSymbolRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  FindSymbolRequest copyWith(void Function(FindSymbolRequest) updates) =>
      super.copyWith((message) => updates(message as FindSymbolRequest))
          as FindSymbolRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static FindSymbolRequest create() => FindSymbolRequest._();
  @$core.override
  FindSymbolRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static FindSymbolRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<FindSymbolRequest>(create);
  static FindSymbolRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get name => $_getSZ(0);
  @$pb.TagNumber(1)
  set name($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasName() => $_has(0);
  @$pb.TagNumber(1)
  void clearName() => $_clearField(1);

  @$pb.TagNumber(2)
  SymbolKind get kind => $_getN(1);
  @$pb.TagNumber(2)
  set kind(SymbolKind value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasKind() => $_has(1);
  @$pb.TagNumber(2)
  void clearKind() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get package => $_getSZ(2);
  @$pb.TagNumber(3)
  set package($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasPackage() => $_has(2);
  @$pb.TagNumber(3)
  void clearPackage() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.bool get exact => $_getBF(3);
  @$pb.TagNumber(4)
  set exact($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasExact() => $_has(3);
  @$pb.TagNumber(4)
  void clearExact() => $_clearField(4);

  @$pb.TagNumber(5)
  $fixnum.Int64 get limit => $_getI64(4);
  @$pb.TagNumber(5)
  set limit($fixnum.Int64 value) => $_setInt64(4, value);
  @$pb.TagNumber(5)
  $core.bool hasLimit() => $_has(4);
  @$pb.TagNumber(5)
  void clearLimit() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.String get backend => $_getSZ(5);
  @$pb.TagNumber(6)
  set backend($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasBackend() => $_has(5);
  @$pb.TagNumber(6)
  void clearBackend() => $_clearField(6);
}

/// The shared symbol-list response for the symbol-producing verbs (FindSymbol,
/// Implementations, InterfaceOf).
class SymbolList extends $pb.GeneratedMessage {
  factory SymbolList({
    $core.Iterable<Symbol>? symbols,
    $core.String? backend,
  }) {
    final result = create();
    if (symbols != null) result.symbols.addAll(symbols);
    if (backend != null) result.backend = backend;
    return result;
  }

  SymbolList._();

  factory SymbolList.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SymbolList.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SymbolList',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<Symbol>(1, _omitFieldNames ? '' : 'symbols',
        subBuilder: Symbol.create)
    ..aOS(2, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SymbolList clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SymbolList copyWith(void Function(SymbolList) updates) =>
      super.copyWith((message) => updates(message as SymbolList)) as SymbolList;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SymbolList create() => SymbolList._();
  @$core.override
  SymbolList createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SymbolList getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SymbolList>(create);
  static SymbolList? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<Symbol> get symbols => $_getList(0);

  @$pb.TagNumber(2)
  $core.String get backend => $_getSZ(1);
  @$pb.TagNumber(2)
  set backend($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBackend() => $_has(1);
  @$pb.TagNumber(2)
  void clearBackend() => $_clearField(2);
}

class ImplementationsRequest extends $pb.GeneratedMessage {
  factory ImplementationsRequest({
    SymbolRef? iface,
    $core.String? backend,
  }) {
    final result = create();
    if (iface != null) result.iface = iface;
    if (backend != null) result.backend = backend;
    return result;
  }

  ImplementationsRequest._();

  factory ImplementationsRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ImplementationsRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ImplementationsRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<SymbolRef>(1, _omitFieldNames ? '' : 'iface',
        subBuilder: SymbolRef.create)
    ..aOS(2, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ImplementationsRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ImplementationsRequest copyWith(
          void Function(ImplementationsRequest) updates) =>
      super.copyWith((message) => updates(message as ImplementationsRequest))
          as ImplementationsRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ImplementationsRequest create() => ImplementationsRequest._();
  @$core.override
  ImplementationsRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ImplementationsRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ImplementationsRequest>(create);
  static ImplementationsRequest? _defaultInstance;

  @$pb.TagNumber(1)
  SymbolRef get iface => $_getN(0);
  @$pb.TagNumber(1)
  set iface(SymbolRef value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasIface() => $_has(0);
  @$pb.TagNumber(1)
  void clearIface() => $_clearField(1);
  @$pb.TagNumber(1)
  SymbolRef ensureIface() => $_ensure(0);

  @$pb.TagNumber(2)
  $core.String get backend => $_getSZ(1);
  @$pb.TagNumber(2)
  set backend($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBackend() => $_has(1);
  @$pb.TagNumber(2)
  void clearBackend() => $_clearField(2);
}

class InterfaceOfRequest extends $pb.GeneratedMessage {
  factory InterfaceOfRequest({
    SymbolRef? concreteType,
    $core.String? backend,
  }) {
    final result = create();
    if (concreteType != null) result.concreteType = concreteType;
    if (backend != null) result.backend = backend;
    return result;
  }

  InterfaceOfRequest._();

  factory InterfaceOfRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory InterfaceOfRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'InterfaceOfRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<SymbolRef>(1, _omitFieldNames ? '' : 'concreteType',
        subBuilder: SymbolRef.create)
    ..aOS(2, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  InterfaceOfRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  InterfaceOfRequest copyWith(void Function(InterfaceOfRequest) updates) =>
      super.copyWith((message) => updates(message as InterfaceOfRequest))
          as InterfaceOfRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static InterfaceOfRequest create() => InterfaceOfRequest._();
  @$core.override
  InterfaceOfRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static InterfaceOfRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<InterfaceOfRequest>(create);
  static InterfaceOfRequest? _defaultInstance;

  @$pb.TagNumber(1)
  SymbolRef get concreteType => $_getN(0);
  @$pb.TagNumber(1)
  set concreteType(SymbolRef value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasConcreteType() => $_has(0);
  @$pb.TagNumber(1)
  void clearConcreteType() => $_clearField(1);
  @$pb.TagNumber(1)
  SymbolRef ensureConcreteType() => $_ensure(0);

  @$pb.TagNumber(2)
  $core.String get backend => $_getSZ(1);
  @$pb.TagNumber(2)
  set backend($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBackend() => $_has(1);
  @$pb.TagNumber(2)
  void clearBackend() => $_clearField(2);
}

class CallersRequest extends $pb.GeneratedMessage {
  factory CallersRequest({
    SymbolRef? target,
    $core.int? hops,
    $core.String? backend,
  }) {
    final result = create();
    if (target != null) result.target = target;
    if (hops != null) result.hops = hops;
    if (backend != null) result.backend = backend;
    return result;
  }

  CallersRequest._();

  factory CallersRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CallersRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CallersRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<SymbolRef>(1, _omitFieldNames ? '' : 'target',
        subBuilder: SymbolRef.create)
    ..aI(2, _omitFieldNames ? '' : 'hops', fieldType: $pb.PbFieldType.OU3)
    ..aOS(3, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CallersRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CallersRequest copyWith(void Function(CallersRequest) updates) =>
      super.copyWith((message) => updates(message as CallersRequest))
          as CallersRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CallersRequest create() => CallersRequest._();
  @$core.override
  CallersRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CallersRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<CallersRequest>(create);
  static CallersRequest? _defaultInstance;

  @$pb.TagNumber(1)
  SymbolRef get target => $_getN(0);
  @$pb.TagNumber(1)
  set target(SymbolRef value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasTarget() => $_has(0);
  @$pb.TagNumber(1)
  void clearTarget() => $_clearField(1);
  @$pb.TagNumber(1)
  SymbolRef ensureTarget() => $_ensure(0);

  @$pb.TagNumber(2)
  $core.int get hops => $_getIZ(1);
  @$pb.TagNumber(2)
  set hops($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasHops() => $_has(1);
  @$pb.TagNumber(2)
  void clearHops() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get backend => $_getSZ(2);
  @$pb.TagNumber(3)
  set backend($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasBackend() => $_has(2);
  @$pb.TagNumber(3)
  void clearBackend() => $_clearField(3);
}

class CalleesRequest extends $pb.GeneratedMessage {
  factory CalleesRequest({
    SymbolRef? target,
    $core.int? hops,
    $core.String? backend,
  }) {
    final result = create();
    if (target != null) result.target = target;
    if (hops != null) result.hops = hops;
    if (backend != null) result.backend = backend;
    return result;
  }

  CalleesRequest._();

  factory CalleesRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CalleesRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CalleesRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<SymbolRef>(1, _omitFieldNames ? '' : 'target',
        subBuilder: SymbolRef.create)
    ..aI(2, _omitFieldNames ? '' : 'hops', fieldType: $pb.PbFieldType.OU3)
    ..aOS(3, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CalleesRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CalleesRequest copyWith(void Function(CalleesRequest) updates) =>
      super.copyWith((message) => updates(message as CalleesRequest))
          as CalleesRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CalleesRequest create() => CalleesRequest._();
  @$core.override
  CalleesRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CalleesRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<CalleesRequest>(create);
  static CalleesRequest? _defaultInstance;

  @$pb.TagNumber(1)
  SymbolRef get target => $_getN(0);
  @$pb.TagNumber(1)
  set target(SymbolRef value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasTarget() => $_has(0);
  @$pb.TagNumber(1)
  void clearTarget() => $_clearField(1);
  @$pb.TagNumber(1)
  SymbolRef ensureTarget() => $_ensure(0);

  @$pb.TagNumber(2)
  $core.int get hops => $_getIZ(1);
  @$pb.TagNumber(2)
  set hops($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasHops() => $_has(1);
  @$pb.TagNumber(2)
  void clearHops() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get backend => $_getSZ(2);
  @$pb.TagNumber(3)
  set backend($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasBackend() => $_has(2);
  @$pb.TagNumber(3)
  void clearBackend() => $_clearField(3);
}

class CallchainRequest extends $pb.GeneratedMessage {
  factory CallchainRequest({
    SymbolRef? from,
    SymbolRef? to,
    $core.int? maxPaths,
    $core.String? backend,
  }) {
    final result = create();
    if (from != null) result.from = from;
    if (to != null) result.to = to;
    if (maxPaths != null) result.maxPaths = maxPaths;
    if (backend != null) result.backend = backend;
    return result;
  }

  CallchainRequest._();

  factory CallchainRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CallchainRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CallchainRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<SymbolRef>(1, _omitFieldNames ? '' : 'from',
        subBuilder: SymbolRef.create)
    ..aOM<SymbolRef>(2, _omitFieldNames ? '' : 'to',
        subBuilder: SymbolRef.create)
    ..aI(3, _omitFieldNames ? '' : 'maxPaths', fieldType: $pb.PbFieldType.OU3)
    ..aOS(4, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CallchainRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CallchainRequest copyWith(void Function(CallchainRequest) updates) =>
      super.copyWith((message) => updates(message as CallchainRequest))
          as CallchainRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CallchainRequest create() => CallchainRequest._();
  @$core.override
  CallchainRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CallchainRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<CallchainRequest>(create);
  static CallchainRequest? _defaultInstance;

  @$pb.TagNumber(1)
  SymbolRef get from => $_getN(0);
  @$pb.TagNumber(1)
  set from(SymbolRef value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasFrom() => $_has(0);
  @$pb.TagNumber(1)
  void clearFrom() => $_clearField(1);
  @$pb.TagNumber(1)
  SymbolRef ensureFrom() => $_ensure(0);

  @$pb.TagNumber(2)
  SymbolRef get to => $_getN(1);
  @$pb.TagNumber(2)
  set to(SymbolRef value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasTo() => $_has(1);
  @$pb.TagNumber(2)
  void clearTo() => $_clearField(2);
  @$pb.TagNumber(2)
  SymbolRef ensureTo() => $_ensure(1);

  @$pb.TagNumber(3)
  $core.int get maxPaths => $_getIZ(2);
  @$pb.TagNumber(3)
  set maxPaths($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasMaxPaths() => $_has(2);
  @$pb.TagNumber(3)
  void clearMaxPaths() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get backend => $_getSZ(3);
  @$pb.TagNumber(4)
  set backend($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasBackend() => $_has(3);
  @$pb.TagNumber(4)
  void clearBackend() => $_clearField(4);
}

class CallchainResponse extends $pb.GeneratedMessage {
  factory CallchainResponse({
    $core.Iterable<CallPath>? paths,
    $core.String? backend,
  }) {
    final result = create();
    if (paths != null) result.paths.addAll(paths);
    if (backend != null) result.backend = backend;
    return result;
  }

  CallchainResponse._();

  factory CallchainResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CallchainResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CallchainResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<CallPath>(1, _omitFieldNames ? '' : 'paths',
        subBuilder: CallPath.create)
    ..aOS(2, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CallchainResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CallchainResponse copyWith(void Function(CallchainResponse) updates) =>
      super.copyWith((message) => updates(message as CallchainResponse))
          as CallchainResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CallchainResponse create() => CallchainResponse._();
  @$core.override
  CallchainResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CallchainResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<CallchainResponse>(create);
  static CallchainResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<CallPath> get paths => $_getList(0);

  @$pb.TagNumber(2)
  $core.String get backend => $_getSZ(1);
  @$pb.TagNumber(2)
  set backend($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBackend() => $_has(1);
  @$pb.TagNumber(2)
  void clearBackend() => $_clearField(2);
}

class BlastRadiusRequest extends $pb.GeneratedMessage {
  factory BlastRadiusRequest({
    $core.Iterable<$core.String>? changed,
    $core.int? hops,
    $core.String? backend,
  }) {
    final result = create();
    if (changed != null) result.changed.addAll(changed);
    if (hops != null) result.hops = hops;
    if (backend != null) result.backend = backend;
    return result;
  }

  BlastRadiusRequest._();

  factory BlastRadiusRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory BlastRadiusRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'BlastRadiusRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPS(1, _omitFieldNames ? '' : 'changed')
    ..aI(2, _omitFieldNames ? '' : 'hops', fieldType: $pb.PbFieldType.OU3)
    ..aOS(3, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BlastRadiusRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BlastRadiusRequest copyWith(void Function(BlastRadiusRequest) updates) =>
      super.copyWith((message) => updates(message as BlastRadiusRequest))
          as BlastRadiusRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static BlastRadiusRequest create() => BlastRadiusRequest._();
  @$core.override
  BlastRadiusRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static BlastRadiusRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<BlastRadiusRequest>(create);
  static BlastRadiusRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<$core.String> get changed => $_getList(0);

  @$pb.TagNumber(2)
  $core.int get hops => $_getIZ(1);
  @$pb.TagNumber(2)
  set hops($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasHops() => $_has(1);
  @$pb.TagNumber(2)
  void clearHops() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get backend => $_getSZ(2);
  @$pb.TagNumber(3)
  set backend($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasBackend() => $_has(2);
  @$pb.TagNumber(3)
  void clearBackend() => $_clearField(3);
}

class DependencyPathRequest extends $pb.GeneratedMessage {
  factory DependencyPathRequest({
    $core.String? fromPackage,
    $core.String? toPackage,
    $core.String? backend,
  }) {
    final result = create();
    if (fromPackage != null) result.fromPackage = fromPackage;
    if (toPackage != null) result.toPackage = toPackage;
    if (backend != null) result.backend = backend;
    return result;
  }

  DependencyPathRequest._();

  factory DependencyPathRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DependencyPathRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DependencyPathRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'fromPackage')
    ..aOS(2, _omitFieldNames ? '' : 'toPackage')
    ..aOS(3, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DependencyPathRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DependencyPathRequest copyWith(
          void Function(DependencyPathRequest) updates) =>
      super.copyWith((message) => updates(message as DependencyPathRequest))
          as DependencyPathRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DependencyPathRequest create() => DependencyPathRequest._();
  @$core.override
  DependencyPathRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DependencyPathRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DependencyPathRequest>(create);
  static DependencyPathRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get fromPackage => $_getSZ(0);
  @$pb.TagNumber(1)
  set fromPackage($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasFromPackage() => $_has(0);
  @$pb.TagNumber(1)
  void clearFromPackage() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get toPackage => $_getSZ(1);
  @$pb.TagNumber(2)
  set toPackage($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasToPackage() => $_has(1);
  @$pb.TagNumber(2)
  void clearToPackage() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get backend => $_getSZ(2);
  @$pb.TagNumber(3)
  set backend($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasBackend() => $_has(2);
  @$pb.TagNumber(3)
  void clearBackend() => $_clearField(3);
}

class DependencyPathResponse extends $pb.GeneratedMessage {
  factory DependencyPathResponse({
    $core.Iterable<$core.String>? packages,
    $core.String? backend,
  }) {
    final result = create();
    if (packages != null) result.packages.addAll(packages);
    if (backend != null) result.backend = backend;
    return result;
  }

  DependencyPathResponse._();

  factory DependencyPathResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DependencyPathResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DependencyPathResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPS(1, _omitFieldNames ? '' : 'packages')
    ..aOS(2, _omitFieldNames ? '' : 'backend')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DependencyPathResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DependencyPathResponse copyWith(
          void Function(DependencyPathResponse) updates) =>
      super.copyWith((message) => updates(message as DependencyPathResponse))
          as DependencyPathResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DependencyPathResponse create() => DependencyPathResponse._();
  @$core.override
  DependencyPathResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DependencyPathResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DependencyPathResponse>(create);
  static DependencyPathResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<$core.String> get packages => $_getList(0);

  @$pb.TagNumber(2)
  $core.String get backend => $_getSZ(1);
  @$pb.TagNumber(2)
  set backend($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBackend() => $_has(1);
  @$pb.TagNumber(2)
  void clearBackend() => $_clearField(2);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
