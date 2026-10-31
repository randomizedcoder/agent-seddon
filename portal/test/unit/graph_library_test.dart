import 'package:agent_portal/src/gen/agent/v1/graph.pb.dart';
import 'package:agent_portal/src/graph_json.dart';
import 'package:agent_portal/src/graph_library.dart';
import 'package:agent_portal/src/io/graph_platform.dart' as platform;
import 'package:flutter_test/flutter_test.dart';

/// L0 tests for the browser-local graph library (`graph_library.dart`). On the
/// Dart VM the platform store is the in-memory stub, so save/load round-trips
/// deterministically. The stored blob is untrusted (survives across sessions and
/// could be hand-edited or corrupt), so `adversarial_` cases assert graceful
/// degradation to an empty library rather than a throw.
void main() {
  // The private storage key `graph_library.dart` uses; asserted here so a change
  // to it (which would silently orphan every user's saved library) fails a test.
  const storageKey = 'agent_portal.graph_library.v1';

  CognitionGraph graph(int version) => CognitionGraph()..version = version;

  setUp(() {
    // Reset the in-memory stub store so tests don't leak into each other.
    platform.saveRaw(storageKey, '');
  });

  group('save / load round-trip', () {
    test('positive_two_entries_preserved', () {
      GraphLibrary([
        GraphLibraryEntry('alpha', graph(1)),
        GraphLibraryEntry('beta', graph(2)),
      ]).save();

      final loaded = GraphLibrary.load();
      expect(loaded.entries.map((e) => e.name), ['alpha', 'beta']);
      expect(loaded.entries[0].graph.version, 1);
      expect(loaded.entries[1].graph.version, 2);
    });

    test('corner_empty_library_round_trips_empty', () {
      GraphLibrary([]).save();
      expect(GraphLibrary.load().entries, isEmpty);
    });
  });

  group('uniqueName / hasName', () {
    final lib = GraphLibrary([
      GraphLibraryEntry('graph', graph(1)),
      GraphLibraryEntry('graph 2', graph(1)),
    ]);
    test('positive_hasName', () {
      expect(lib.hasName('graph'), isTrue);
      expect(lib.hasName('missing'), isFalse);
    });
    test('corner_unique_base_returned_as_is', () {
      expect(lib.uniqueName('fresh'), 'fresh');
    });
    test('boundary_skips_existing_suffixes', () {
      // 'graph' and 'graph 2' taken → next free is 'graph 3'.
      expect(lib.uniqueName('graph'), 'graph 3');
    });
  });

  group('load tolerates a hostile/corrupt blob', () {
    test('negative_missing_blob_yields_empty', () {
      platform.saveRaw(storageKey, '');
      expect(GraphLibrary.load().entries, isEmpty);
    });
    test('adversarial_not_json_yields_empty', () {
      platform.saveRaw(storageKey, '}{not valid json');
      expect(GraphLibrary.load().entries, isEmpty);
    });
    test('adversarial_json_but_not_a_list_yields_empty', () {
      platform.saveRaw(storageKey, '{"name":"x"}');
      expect(GraphLibrary.load().entries, isEmpty);
    });
    test('adversarial_list_with_bad_items_skips_them', () {
      // Only the well-formed entry survives; wrong-typed items are dropped.
      platform.saveRaw(
        storageKey,
        '[42, {"name":123}, {"name":"ok","graph":{"version":5}}, {"nograph":"x"}]',
      );
      final loaded = GraphLibrary.load();
      expect(loaded.entries.map((e) => e.name), ['ok']);
      expect(loaded.entries.single.graph.version, 5);
    });
  });

  group('addActive and repeated names', () {
    CognitionGraph g(List<String> keys, {int version = 1}) =>
        CognitionGraph(version: version, nodes: [
          MapEntry(
              'n',
              GraphNode(
                  type: 't',
                  params: dartToJsonValue({for (final k in keys) k: 1}))),
        ]);
    const active = GraphLibrary.activeName;

    test('positive_add_active_once', () {
      // A new active graph is added under the active name, and only once.
      final lib = GraphLibrary([GraphLibraryEntry('alpha', graph(1))]);
      expect(lib.addActive(g(['a']))?.name, active);
      expect(lib.addActive(g(['a'])), isNull);
      expect(lib.entries, hasLength(2));
    });
    test('corner_reordered_params_match', () {
      // The same graph with its params in another key order is not re-added.
      final lib = GraphLibrary([GraphLibraryEntry('mine', g(['a', 'b']))]);
      expect(lib.addActive(g(['b', 'a'])), isNull);
      expect(lib.entries, hasLength(1));
    });
    test('boundary_changed_active_gets_a_free_name', () {
      // A different active graph is added beside the old one, not over it.
      final lib = GraphLibrary([GraphLibraryEntry(active, g(['a']))]);
      expect(lib.addActive(g(['a'], version: 2))?.name, '$active 2');
    });
    test('negative_same_graph_under_other_names_kept', () {
      // Only a repeated name with the same graph is dropped on load.
      GraphLibrary([
        GraphLibraryEntry('one', graph(1)),
        GraphLibraryEntry('two', graph(1)),
      ]).save();
      expect(GraphLibrary.load().entries.map((e) => e.name), ['one', 'two']);
    });
    test('adversarial_repeated_names_collapse_on_load', () {
      // 27 copies of one graph and one other graph, all under one name, as an
      // older portal left them: one copy stays, the other graph is renamed.
      GraphLibrary([
        for (var i = 0; i < 27; i++)
          GraphLibraryEntry(active, g(i.isEven ? ['a', 'b'] : ['b', 'a'])),
        GraphLibraryEntry(active, g(['a'], version: 2)),
        GraphLibraryEntry('alpha', graph(1)),
      ]).save();
      expect(GraphLibrary.load().entries.map((e) => e.name),
          [active, '$active 2', 'alpha']);
    });
  });
}
