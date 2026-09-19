import 'package:agent_portal/src/gen/agent/v1/common.pb.dart';
import 'package:agent_portal/src/gen/agent/v1/graph.pb.dart';
import 'package:agent_portal/src/graph_json.dart';
import 'package:fixnum/fixnum.dart';
import 'package:flutter_test/flutter_test.dart';

/// L0 pure-logic tests for the wire↔Dart/JSON bridge (`graph_json.dart`), the
/// client-side twin of the Rust `serde_json::Value` bridge (the same shape the
/// #418 parity test guards). Table-driven across the four case classes; the JSON
/// decode path is untrusted (hand-authored import files), so `adversarial_` cases
/// are mandatory.
void main() {
  group('jsonValueToDart / dartToJsonValue round-trip', () {
    // A plain Dart value survives dart→JsonValue→dart unchanged.
    final cases = <String, dynamic>{
      'positive_null': null,
      'positive_bool_true': true,
      'positive_bool_false': false,
      'positive_int': 42,
      'positive_negative_int': -7,
      'positive_double': 3.5,
      'positive_string': 'hello',
      'positive_list': [1, 2, 3],
      'positive_map': {'a': 1, 'b': 'two'},
      'positive_nested': {
        'k': [
          {'x': true},
          null,
          [1, 2]
        ]
      },
      'corner_empty_string': '',
      'corner_empty_list': <dynamic>[],
      'corner_empty_map': <String, dynamic>{},
      'boundary_zero': 0,
      'boundary_large_int': 9007199254740991, // 2^53 - 1, exact in a double
      'adversarial_unicode_key': {'ключ': '值', '🔑': 'emoji'},
      'adversarial_deep_nest': _deep(50),
    };
    cases.forEach((name, value) {
      test(name, () {
        expect(jsonValueToDart(dartToJsonValue(value)), value);
      });
    });
  });

  group('jsonValueToDart edge kinds not producible by the encoder', () {
    test('corner_unset_kind_decodes_null', () {
      expect(jsonValueToDart(JsonValue()), isNull);
    });
    test('positive_uint_decodes_int', () {
      expect(jsonValueToDart(JsonValue(uintValue: Int64(9))), 9);
    });
    test('adversarial_bignumber_non_numeric_returns_string', () {
      expect(jsonValueToDart(JsonValue(bigNumber: 'not-a-number')),
          'not-a-number');
    });
  });

  group('edge kind label mapping', () {
    test('positive_labels', () {
      expect(edgeKindLabel(GraphEdge_Kind.KIND_MAIN), 'main');
      expect(edgeKindLabel(GraphEdge_Kind.KIND_BACKGROUND), 'background');
      expect(edgeKindLabel(GraphEdge_Kind.KIND_CAPABILITY), 'capability');
    });
    test('negative_unknown_label_falls_back_to_main', () {
      expect(edgeKindFromLabel('nonsense'), GraphEdge_Kind.KIND_MAIN);
    });
    test('corner_case_insensitive_label', () {
      expect(edgeKindFromLabel('BACKGROUND'), GraphEdge_Kind.KIND_BACKGROUND);
    });
    test('boundary_unspecified_kind_labels_main', () {
      // The zero value is never selectable; it must still label to a valid word.
      expect(edgeKindLabel(GraphEdge_Kind.KIND_UNSPECIFIED), 'main');
    });
  });

  group('graphToJson / graphFromJson', () {
    test('positive_round_trip_preserves_structure', () {
      final g = CognitionGraph()
        ..version = 3
        ..nodes['n1'] = (GraphNode()
          ..type = 'llm'
          ..typeVersion = 2
          ..params = dartToJsonValue({'temp': 0.7}))
        ..nodes['n0'] = (GraphNode()
          ..type = 'input'
          ..typeVersion = 1)
        ..edges.add(GraphEdge(
            from: 'n0', to: 'n1', kind: GraphEdge_Kind.KIND_BACKGROUND));

      final json = graphToJson(g);
      // Node ids are sorted for diff-friendly files.
      expect((json['nodes'] as Map).keys.toList(), ['n0', 'n1']);
      expect(json['version'], 3);
      expect((json['nodes'] as Map)['n1']['params'], {'temp': 0.7});
      // A node with no params omits the key.
      expect((json['nodes'] as Map)['n0'].containsKey('params'), isFalse);

      final back = graphFromJson(json);
      expect(back.version, 3);
      expect(back.nodes['n1']!.type, 'llm');
      expect(back.nodes['n1']!.typeVersion, 2);
      expect(jsonValueToDart(back.nodes['n1']!.params), {'temp': 0.7});
      expect(back.edges.single.kind, GraphEdge_Kind.KIND_BACKGROUND);
    });

    test('negative_missing_fields_tolerated', () {
      final g = graphFromJson({});
      expect(g.version, 0);
      expect(g.nodes, isEmpty);
      expect(g.edges, isEmpty);
    });

    test('adversarial_wrong_types_ignored_not_thrown', () {
      // Hostile/hand-broken file: wrong types everywhere. Must not throw; the
      // server Validate/Put is the real gate.
      final g = graphFromJson({
        'version': 'not-a-number',
        'nodes': 'not-a-map',
        'edges': {'not': 'a-list'},
      });
      expect(g.version, 0);
      expect(g.nodes, isEmpty);
      expect(g.edges, isEmpty);
    });

    test('adversarial_partial_edge_defaults_empty_endpoints', () {
      final g = graphFromJson({
        'edges': [
          {'from': 123, 'kind': 42}, // wrong types → empty from/to, main kind
        ],
      });
      expect(g.edges.single.from, '');
      expect(g.edges.single.to, '');
      expect(g.edges.single.kind, GraphEdge_Kind.KIND_MAIN);
    });
  });
}

Map<String, dynamic> _deep(int depth) {
  dynamic v = 'leaf';
  for (var i = 0; i < depth; i++) {
    v = {'d$i': v};
  }
  return v as Map<String, dynamic>;
}
