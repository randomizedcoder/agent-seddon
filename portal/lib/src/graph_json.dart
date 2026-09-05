import 'package:fixnum/fixnum.dart';

import 'gen/agent/v1/common.pb.dart';
import 'gen/agent/v1/graph.pb.dart';

/// Conversions between the wire types and plain Dart/JSON.
///
/// The graph document uses agent-core's bespoke [JsonValue] for `params` (and
/// for the node-type `params_schema`), not `google.protobuf.Value`, so proto3
/// JSON would serialize it as verbose `{"stringValue": …}` forms. These helpers
/// project it to/from ordinary Dart values instead, which gives:
///   * clean, human-readable import/export files (see [graphToJson]); and
///   * a plain `Map`/scalar the schema-driven param forms can edit directly.
/// The mapping is the client-side twin of the Rust `serde_json::Value` bridge.

/// A [JsonValue] → plain Dart value (`null` / `bool` / `int` / `double` /
/// `String` / `List` / `Map<String, dynamic>`). An unset kind decodes to `null`.
dynamic jsonValueToDart(JsonValue v) {
  switch (v.whichKind()) {
    case JsonValue_Kind.nullValue:
    case JsonValue_Kind.notSet:
      return null;
    case JsonValue_Kind.boolValue:
      return v.boolValue;
    case JsonValue_Kind.intValue:
      return v.intValue.toInt();
    case JsonValue_Kind.uintValue:
      return v.uintValue.toInt();
    case JsonValue_Kind.doubleValue:
      return v.doubleValue;
    case JsonValue_Kind.stringValue:
      return v.stringValue;
    case JsonValue_Kind.bigNumber:
      // Beyond native range on the wire; hand back the exact decimal string.
      return num.tryParse(v.bigNumber) ?? v.bigNumber;
    case JsonValue_Kind.arrayValue:
      return v.arrayValue.values.map(jsonValueToDart).toList();
    case JsonValue_Kind.objectValue:
      return {
        for (final e in v.objectValue.fields.entries)
          e.key: jsonValueToDart(e.value),
      };
  }
}

/// A plain Dart value → [JsonValue] (the inverse of [jsonValueToDart]).
JsonValue dartToJsonValue(dynamic value) {
  if (value == null) {
    return JsonValue(nullValue: NullValue.NULL_VALUE);
  }
  if (value is bool) {
    return JsonValue(boolValue: value);
  }
  if (value is int) {
    return JsonValue(intValue: Int64(value));
  }
  if (value is double) {
    return JsonValue(doubleValue: value);
  }
  if (value is String) {
    return JsonValue(stringValue: value);
  }
  if (value is List) {
    return JsonValue(arrayValue: JsonArray(values: value.map(dartToJsonValue)));
  }
  if (value is Map) {
    return JsonValue(
      objectValue: JsonObject(
        fields: value.entries
            .map((e) => MapEntry('${e.key}', dartToJsonValue(e.value))),
      ),
    );
  }
  // Any other type (shouldn't occur from a JSON decode) stringifies safely.
  return JsonValue(stringValue: '$value');
}

/// Short, stable edge-kind labels for the clean JSON form and the UI.
const _kindToLabel = {
  GraphEdge_Kind.KIND_MAIN: 'main',
  GraphEdge_Kind.KIND_BACKGROUND: 'background',
  GraphEdge_Kind.KIND_CAPABILITY: 'capability',
};

String edgeKindLabel(GraphEdge_Kind k) => _kindToLabel[k] ?? 'main';

GraphEdge_Kind edgeKindFromLabel(String s) {
  switch (s.toLowerCase()) {
    case 'background':
      return GraphEdge_Kind.KIND_BACKGROUND;
    case 'capability':
      return GraphEdge_Kind.KIND_CAPABILITY;
    case 'main':
    default:
      return GraphEdge_Kind.KIND_MAIN;
  }
}

/// The selectable edge kinds (never the unspecified zero value).
const graphEdgeKinds = [
  GraphEdge_Kind.KIND_MAIN,
  GraphEdge_Kind.KIND_BACKGROUND,
  GraphEdge_Kind.KIND_CAPABILITY,
];

/// A [CognitionGraph] → a clean, ordered Dart map ready for `jsonEncode`:
/// `{version, nodes: {id: {type, typeVersion, params}}, edges: [{from,to,kind}]}`.
Map<String, dynamic> graphToJson(CognitionGraph g) {
  final nodes = <String, dynamic>{};
  // Sort node ids for deterministic, diff-friendly files.
  final ids = g.nodes.keys.toList()..sort();
  for (final id in ids) {
    final n = g.nodes[id]!;
    nodes[id] = {
      'type': n.type,
      'typeVersion': n.typeVersion,
      if (n.hasParams()) 'params': jsonValueToDart(n.params),
    };
  }
  return {
    'version': g.version,
    'nodes': nodes,
    'edges': [
      for (final e in g.edges)
        {'from': e.from, 'to': e.to, 'kind': edgeKindLabel(e.kind)},
    ],
  };
}

/// The inverse of [graphToJson]. Tolerant of missing fields so a hand-authored
/// file still loads; the server's `Validate`/`Put` is the real gate.
CognitionGraph graphFromJson(Map<String, dynamic> j) {
  final g = CognitionGraph();
  final version = j['version'];
  if (version is num) g.version = version.toInt();

  final nodes = j['nodes'];
  if (nodes is Map) {
    for (final e in nodes.entries) {
      final id = '${e.key}';
      final nv = e.value;
      final node = GraphNode();
      if (nv is Map) {
        if (nv['type'] is String) node.type = nv['type'] as String;
        final tv = nv['typeVersion'];
        if (tv is num) node.typeVersion = tv.toInt();
        if (nv.containsKey('params')) {
          node.params = dartToJsonValue(nv['params']);
        }
      }
      g.nodes[id] = node;
    }
  }

  final edges = j['edges'];
  if (edges is List) {
    for (final ev in edges) {
      if (ev is! Map) continue;
      g.edges.add(GraphEdge(
        from: ev['from'] is String ? ev['from'] as String : '',
        to: ev['to'] is String ? ev['to'] as String : '',
        kind: edgeKindFromLabel(
            ev['kind'] is String ? ev['kind'] as String : 'main'),
      ));
    }
  }
  return g;
}
