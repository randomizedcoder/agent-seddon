// This is a generated file - do not edit.
//
// Generated from agent/v1/graph.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

import 'common.pb.dart' as $1;
import 'graph.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'graph.pbenum.dart';

/// One node instance: a registered type + schema version + parameters
/// (`agent-core GraphNode`). Schemas are never embedded in documents.
class GraphNode extends $pb.GeneratedMessage {
  factory GraphNode({
    $core.String? type,
    $core.int? typeVersion,
    $1.JsonValue? params,
  }) {
    final result = create();
    if (type != null) result.type = type;
    if (typeVersion != null) result.typeVersion = typeVersion;
    if (params != null) result.params = params;
    return result;
  }

  GraphNode._();

  factory GraphNode.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GraphNode.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GraphNode',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'type')
    ..aI(2, _omitFieldNames ? '' : 'typeVersion',
        fieldType: $pb.PbFieldType.OU3)
    ..aOM<$1.JsonValue>(3, _omitFieldNames ? '' : 'params',
        subBuilder: $1.JsonValue.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GraphNode clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GraphNode copyWith(void Function(GraphNode) updates) =>
      super.copyWith((message) => updates(message as GraphNode)) as GraphNode;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GraphNode create() => GraphNode._();
  @$core.override
  GraphNode createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GraphNode getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<GraphNode>(create);
  static GraphNode? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get type => $_getSZ(0);
  @$pb.TagNumber(1)
  set type($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasType() => $_has(0);
  @$pb.TagNumber(1)
  void clearType() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get typeVersion => $_getIZ(1);
  @$pb.TagNumber(2)
  set typeVersion($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasTypeVersion() => $_has(1);
  @$pb.TagNumber(2)
  void clearTypeVersion() => $_clearField(2);

  @$pb.TagNumber(3)
  $1.JsonValue get params => $_getN(2);
  @$pb.TagNumber(3)
  set params($1.JsonValue value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasParams() => $_has(2);
  @$pb.TagNumber(3)
  void clearParams() => $_clearField(3);
  @$pb.TagNumber(3)
  $1.JsonValue ensureParams() => $_ensure(2);
}

/// A typed edge. `KIND_UNSPECIFIED` is never valid — conversions reject it.
class GraphEdge extends $pb.GeneratedMessage {
  factory GraphEdge({
    $core.String? from,
    $core.String? to,
    GraphEdge_Kind? kind,
  }) {
    final result = create();
    if (from != null) result.from = from;
    if (to != null) result.to = to;
    if (kind != null) result.kind = kind;
    return result;
  }

  GraphEdge._();

  factory GraphEdge.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GraphEdge.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GraphEdge',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'from')
    ..aOS(2, _omitFieldNames ? '' : 'to')
    ..aE<GraphEdge_Kind>(3, _omitFieldNames ? '' : 'kind',
        enumValues: GraphEdge_Kind.values)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GraphEdge clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GraphEdge copyWith(void Function(GraphEdge) updates) =>
      super.copyWith((message) => updates(message as GraphEdge)) as GraphEdge;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GraphEdge create() => GraphEdge._();
  @$core.override
  GraphEdge createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GraphEdge getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<GraphEdge>(create);
  static GraphEdge? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get from => $_getSZ(0);
  @$pb.TagNumber(1)
  set from($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasFrom() => $_has(0);
  @$pb.TagNumber(1)
  void clearFrom() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get to => $_getSZ(1);
  @$pb.TagNumber(2)
  set to($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasTo() => $_has(1);
  @$pb.TagNumber(2)
  void clearTo() => $_clearField(2);

  @$pb.TagNumber(3)
  GraphEdge_Kind get kind => $_getN(2);
  @$pb.TagNumber(3)
  set kind(GraphEdge_Kind value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasKind() => $_has(2);
  @$pb.TagNumber(3)
  void clearKind() => $_clearField(3);
}

/// The document (`agent-core GraphDoc`): flat node map, stable human-chosen
/// ids, typed edges, zero layout (layout is a GUI-owned sidecar).
class CognitionGraph extends $pb.GeneratedMessage {
  factory CognitionGraph({
    $core.int? version,
    $core.Iterable<$core.MapEntry<$core.String, GraphNode>>? nodes,
    $core.Iterable<GraphEdge>? edges,
  }) {
    final result = create();
    if (version != null) result.version = version;
    if (nodes != null) result.nodes.addEntries(nodes);
    if (edges != null) result.edges.addAll(edges);
    return result;
  }

  CognitionGraph._();

  factory CognitionGraph.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory CognitionGraph.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'CognitionGraph',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aI(1, _omitFieldNames ? '' : 'version', fieldType: $pb.PbFieldType.OU3)
    ..m<$core.String, GraphNode>(2, _omitFieldNames ? '' : 'nodes',
        entryClassName: 'CognitionGraph.NodesEntry',
        keyFieldType: $pb.PbFieldType.OS,
        valueFieldType: $pb.PbFieldType.OM,
        valueCreator: GraphNode.create,
        valueDefaultOrMaker: GraphNode.getDefault,
        packageName: const $pb.PackageName('agent.v1'))
    ..pPM<GraphEdge>(3, _omitFieldNames ? '' : 'edges',
        subBuilder: GraphEdge.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CognitionGraph clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  CognitionGraph copyWith(void Function(CognitionGraph) updates) =>
      super.copyWith((message) => updates(message as CognitionGraph))
          as CognitionGraph;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static CognitionGraph create() => CognitionGraph._();
  @$core.override
  CognitionGraph createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static CognitionGraph getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<CognitionGraph>(create);
  static CognitionGraph? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get version => $_getIZ(0);
  @$pb.TagNumber(1)
  set version($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasVersion() => $_has(0);
  @$pb.TagNumber(1)
  void clearVersion() => $_clearField(1);

  @$pb.TagNumber(2)
  $pb.PbMap<$core.String, GraphNode> get nodes => $_getMap(1);

  @$pb.TagNumber(3)
  $pb.PbList<GraphEdge> get edges => $_getList(2);
}

/// A named, nominally-typed port on a node type (editor connection rules).
class NodePort extends $pb.GeneratedMessage {
  factory NodePort({
    $core.String? name,
    $core.String? kind,
  }) {
    final result = create();
    if (name != null) result.name = name;
    if (kind != null) result.kind = kind;
    return result;
  }

  NodePort._();

  factory NodePort.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory NodePort.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'NodePort',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'name')
    ..aOS(2, _omitFieldNames ? '' : 'kind')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  NodePort clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  NodePort copyWith(void Function(NodePort) updates) =>
      super.copyWith((message) => updates(message as NodePort)) as NodePort;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static NodePort create() => NodePort._();
  @$core.override
  NodePort createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static NodePort getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<NodePort>(create);
  static NodePort? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get name => $_getSZ(0);
  @$pb.TagNumber(1)
  set name($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasName() => $_has(0);
  @$pb.TagNumber(1)
  void clearName() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get kind => $_getSZ(1);
  @$pb.TagNumber(2)
  set kind($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasKind() => $_has(1);
  @$pb.TagNumber(2)
  void clearKind() => $_clearField(2);
}

/// Everything an editor needs to render one node type from data alone:
/// title/doc, typed ports, and a JSON Schema for the params form.
class NodeTypeSchema extends $pb.GeneratedMessage {
  factory NodeTypeSchema({
    $core.String? type,
    $core.int? typeVersion,
    $core.String? title,
    $core.String? doc,
    $core.Iterable<NodePort>? inputs,
    $core.Iterable<NodePort>? outputs,
    $1.JsonValue? paramsSchema,
  }) {
    final result = create();
    if (type != null) result.type = type;
    if (typeVersion != null) result.typeVersion = typeVersion;
    if (title != null) result.title = title;
    if (doc != null) result.doc = doc;
    if (inputs != null) result.inputs.addAll(inputs);
    if (outputs != null) result.outputs.addAll(outputs);
    if (paramsSchema != null) result.paramsSchema = paramsSchema;
    return result;
  }

  NodeTypeSchema._();

  factory NodeTypeSchema.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory NodeTypeSchema.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'NodeTypeSchema',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'type')
    ..aI(2, _omitFieldNames ? '' : 'typeVersion',
        fieldType: $pb.PbFieldType.OU3)
    ..aOS(3, _omitFieldNames ? '' : 'title')
    ..aOS(4, _omitFieldNames ? '' : 'doc')
    ..pPM<NodePort>(5, _omitFieldNames ? '' : 'inputs',
        subBuilder: NodePort.create)
    ..pPM<NodePort>(6, _omitFieldNames ? '' : 'outputs',
        subBuilder: NodePort.create)
    ..aOM<$1.JsonValue>(7, _omitFieldNames ? '' : 'paramsSchema',
        subBuilder: $1.JsonValue.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  NodeTypeSchema clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  NodeTypeSchema copyWith(void Function(NodeTypeSchema) updates) =>
      super.copyWith((message) => updates(message as NodeTypeSchema))
          as NodeTypeSchema;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static NodeTypeSchema create() => NodeTypeSchema._();
  @$core.override
  NodeTypeSchema createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static NodeTypeSchema getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<NodeTypeSchema>(create);
  static NodeTypeSchema? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get type => $_getSZ(0);
  @$pb.TagNumber(1)
  set type($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasType() => $_has(0);
  @$pb.TagNumber(1)
  void clearType() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get typeVersion => $_getIZ(1);
  @$pb.TagNumber(2)
  set typeVersion($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasTypeVersion() => $_has(1);
  @$pb.TagNumber(2)
  void clearTypeVersion() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get title => $_getSZ(2);
  @$pb.TagNumber(3)
  set title($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasTitle() => $_has(2);
  @$pb.TagNumber(3)
  void clearTitle() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get doc => $_getSZ(3);
  @$pb.TagNumber(4)
  set doc($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasDoc() => $_has(3);
  @$pb.TagNumber(4)
  void clearDoc() => $_clearField(4);

  @$pb.TagNumber(5)
  $pb.PbList<NodePort> get inputs => $_getList(4);

  @$pb.TagNumber(6)
  $pb.PbList<NodePort> get outputs => $_getList(5);

  /// JSON Schema (+ UI hints) for `GraphNode.params`.
  @$pb.TagNumber(7)
  $1.JsonValue get paramsSchema => $_getN(6);
  @$pb.TagNumber(7)
  set paramsSchema($1.JsonValue value) => $_setField(7, value);
  @$pb.TagNumber(7)
  $core.bool hasParamsSchema() => $_has(6);
  @$pb.TagNumber(7)
  void clearParamsSchema() => $_clearField(7);
  @$pb.TagNumber(7)
  $1.JsonValue ensureParamsSchema() => $_ensure(6);
}

/// One validation finding. `code` is a CLOSED set (see `agent-core
/// GraphIssueCode`) — an unknown code is rejected at conversion, never stored.
class GraphIssue extends $pb.GeneratedMessage {
  factory GraphIssue({
    $core.String? node,
    $core.String? code,
    $core.String? detail,
  }) {
    final result = create();
    if (node != null) result.node = node;
    if (code != null) result.code = code;
    if (detail != null) result.detail = detail;
    return result;
  }

  GraphIssue._();

  factory GraphIssue.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GraphIssue.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GraphIssue',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'node')
    ..aOS(2, _omitFieldNames ? '' : 'code')
    ..aOS(3, _omitFieldNames ? '' : 'detail')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GraphIssue clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GraphIssue copyWith(void Function(GraphIssue) updates) =>
      super.copyWith((message) => updates(message as GraphIssue)) as GraphIssue;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GraphIssue create() => GraphIssue._();
  @$core.override
  GraphIssue createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GraphIssue getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GraphIssue>(create);
  static GraphIssue? _defaultInstance;

  /// The offending node id / edge endpoint; "" = a document-level finding.
  @$pb.TagNumber(1)
  $core.String get node => $_getSZ(0);
  @$pb.TagNumber(1)
  set node($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasNode() => $_has(0);
  @$pb.TagNumber(1)
  void clearNode() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get code => $_getSZ(1);
  @$pb.TagNumber(2)
  set code($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCode() => $_has(1);
  @$pb.TagNumber(2)
  void clearCode() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get detail => $_getSZ(2);
  @$pb.TagNumber(3)
  set detail($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasDetail() => $_has(2);
  @$pb.TagNumber(3)
  void clearDetail() => $_clearField(3);
}

class GetGraphRequest extends $pb.GeneratedMessage {
  factory GetGraphRequest() => create();

  GetGraphRequest._();

  factory GetGraphRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GetGraphRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GetGraphRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetGraphRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetGraphRequest copyWith(void Function(GetGraphRequest) updates) =>
      super.copyWith((message) => updates(message as GetGraphRequest))
          as GetGraphRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GetGraphRequest create() => GetGraphRequest._();
  @$core.override
  GetGraphRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GetGraphRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GetGraphRequest>(create);
  static GetGraphRequest? _defaultInstance;
}

class GetGraphResponse extends $pb.GeneratedMessage {
  factory GetGraphResponse({
    CognitionGraph? graph,
  }) {
    final result = create();
    if (graph != null) result.graph = graph;
    return result;
  }

  GetGraphResponse._();

  factory GetGraphResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GetGraphResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GetGraphResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<CognitionGraph>(1, _omitFieldNames ? '' : 'graph',
        subBuilder: CognitionGraph.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetGraphResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetGraphResponse copyWith(void Function(GetGraphResponse) updates) =>
      super.copyWith((message) => updates(message as GetGraphResponse))
          as GetGraphResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GetGraphResponse create() => GetGraphResponse._();
  @$core.override
  GetGraphResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GetGraphResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GetGraphResponse>(create);
  static GetGraphResponse? _defaultInstance;

  @$pb.TagNumber(1)
  CognitionGraph get graph => $_getN(0);
  @$pb.TagNumber(1)
  set graph(CognitionGraph value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasGraph() => $_has(0);
  @$pb.TagNumber(1)
  void clearGraph() => $_clearField(1);
  @$pb.TagNumber(1)
  CognitionGraph ensureGraph() => $_ensure(0);
}

class PutGraphRequest extends $pb.GeneratedMessage {
  factory PutGraphRequest({
    CognitionGraph? graph,
  }) {
    final result = create();
    if (graph != null) result.graph = graph;
    return result;
  }

  PutGraphRequest._();

  factory PutGraphRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PutGraphRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PutGraphRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<CognitionGraph>(1, _omitFieldNames ? '' : 'graph',
        subBuilder: CognitionGraph.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutGraphRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutGraphRequest copyWith(void Function(PutGraphRequest) updates) =>
      super.copyWith((message) => updates(message as PutGraphRequest))
          as PutGraphRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PutGraphRequest create() => PutGraphRequest._();
  @$core.override
  PutGraphRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PutGraphRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PutGraphRequest>(create);
  static PutGraphRequest? _defaultInstance;

  @$pb.TagNumber(1)
  CognitionGraph get graph => $_getN(0);
  @$pb.TagNumber(1)
  set graph(CognitionGraph value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasGraph() => $_has(0);
  @$pb.TagNumber(1)
  void clearGraph() => $_clearField(1);
  @$pb.TagNumber(1)
  CognitionGraph ensureGraph() => $_ensure(0);
}

class PutGraphResponse extends $pb.GeneratedMessage {
  factory PutGraphResponse() => create();

  PutGraphResponse._();

  factory PutGraphResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PutGraphResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PutGraphResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutGraphResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutGraphResponse copyWith(void Function(PutGraphResponse) updates) =>
      super.copyWith((message) => updates(message as PutGraphResponse))
          as PutGraphResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PutGraphResponse create() => PutGraphResponse._();
  @$core.override
  PutGraphResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PutGraphResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PutGraphResponse>(create);
  static PutGraphResponse? _defaultInstance;
}

class ValidateGraphRequest extends $pb.GeneratedMessage {
  factory ValidateGraphRequest({
    CognitionGraph? graph,
  }) {
    final result = create();
    if (graph != null) result.graph = graph;
    return result;
  }

  ValidateGraphRequest._();

  factory ValidateGraphRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ValidateGraphRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ValidateGraphRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<CognitionGraph>(1, _omitFieldNames ? '' : 'graph',
        subBuilder: CognitionGraph.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ValidateGraphRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ValidateGraphRequest copyWith(void Function(ValidateGraphRequest) updates) =>
      super.copyWith((message) => updates(message as ValidateGraphRequest))
          as ValidateGraphRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ValidateGraphRequest create() => ValidateGraphRequest._();
  @$core.override
  ValidateGraphRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ValidateGraphRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ValidateGraphRequest>(create);
  static ValidateGraphRequest? _defaultInstance;

  @$pb.TagNumber(1)
  CognitionGraph get graph => $_getN(0);
  @$pb.TagNumber(1)
  set graph(CognitionGraph value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasGraph() => $_has(0);
  @$pb.TagNumber(1)
  void clearGraph() => $_clearField(1);
  @$pb.TagNumber(1)
  CognitionGraph ensureGraph() => $_ensure(0);
}

/// Typed findings; empty = valid. A broken document is a list of issues, not an
/// RPC error (the editor shows them inline while dragging).
class ValidateGraphResponse extends $pb.GeneratedMessage {
  factory ValidateGraphResponse({
    $core.Iterable<GraphIssue>? issues,
  }) {
    final result = create();
    if (issues != null) result.issues.addAll(issues);
    return result;
  }

  ValidateGraphResponse._();

  factory ValidateGraphResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ValidateGraphResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ValidateGraphResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<GraphIssue>(1, _omitFieldNames ? '' : 'issues',
        subBuilder: GraphIssue.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ValidateGraphResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ValidateGraphResponse copyWith(
          void Function(ValidateGraphResponse) updates) =>
      super.copyWith((message) => updates(message as ValidateGraphResponse))
          as ValidateGraphResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ValidateGraphResponse create() => ValidateGraphResponse._();
  @$core.override
  ValidateGraphResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ValidateGraphResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ValidateGraphResponse>(create);
  static ValidateGraphResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<GraphIssue> get issues => $_getList(0);
}

class DescribeNodeTypesRequest extends $pb.GeneratedMessage {
  factory DescribeNodeTypesRequest() => create();

  DescribeNodeTypesRequest._();

  factory DescribeNodeTypesRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DescribeNodeTypesRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DescribeNodeTypesRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DescribeNodeTypesRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DescribeNodeTypesRequest copyWith(
          void Function(DescribeNodeTypesRequest) updates) =>
      super.copyWith((message) => updates(message as DescribeNodeTypesRequest))
          as DescribeNodeTypesRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DescribeNodeTypesRequest create() => DescribeNodeTypesRequest._();
  @$core.override
  DescribeNodeTypesRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DescribeNodeTypesRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DescribeNodeTypesRequest>(create);
  static DescribeNodeTypesRequest? _defaultInstance;
}

class DescribeNodeTypesResponse extends $pb.GeneratedMessage {
  factory DescribeNodeTypesResponse({
    $core.Iterable<NodeTypeSchema>? nodeTypes,
  }) {
    final result = create();
    if (nodeTypes != null) result.nodeTypes.addAll(nodeTypes);
    return result;
  }

  DescribeNodeTypesResponse._();

  factory DescribeNodeTypesResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DescribeNodeTypesResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DescribeNodeTypesResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<NodeTypeSchema>(1, _omitFieldNames ? '' : 'nodeTypes',
        subBuilder: NodeTypeSchema.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DescribeNodeTypesResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DescribeNodeTypesResponse copyWith(
          void Function(DescribeNodeTypesResponse) updates) =>
      super.copyWith((message) => updates(message as DescribeNodeTypesResponse))
          as DescribeNodeTypesResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DescribeNodeTypesResponse create() => DescribeNodeTypesResponse._();
  @$core.override
  DescribeNodeTypesResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DescribeNodeTypesResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DescribeNodeTypesResponse>(create);
  static DescribeNodeTypesResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<NodeTypeSchema> get nodeTypes => $_getList(0);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
