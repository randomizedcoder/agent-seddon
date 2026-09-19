import 'package:agent_portal/src/gen/agent/v1/graph.pb.dart';
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
}
