import 'dart:convert';

import 'gen/agent/v1/graph.pb.dart';
import 'graph_json.dart';
import 'io/graph_platform.dart' as platform;

/// One named graph in the local library.
class GraphLibraryEntry {
  GraphLibraryEntry(this.name, this.graph);

  String name;
  CognitionGraph graph;
}

/// A browser-local (per-viewer) library of cognition graphs the user can switch
/// between, edit, and import/export. It is NOT the server's active document —
/// "Set active" (`GraphService.Put`) is what makes one of these the running
/// graph. Persistence is best-effort `localStorage` (see `io/graph_platform`);
/// on web with storage blocked it degrades to in-session memory.
class GraphLibrary {
  GraphLibrary(this.entries);

  static const _storageKey = 'agent_portal.graph_library.v1';

  final List<GraphLibraryEntry> entries;

  /// Load the library from storage. Never throws — a missing/corrupt blob
  /// yields an empty library so the caller can seed it (e.g. from the server).
  static GraphLibrary load() {
    final raw = platform.loadRaw(_storageKey);
    if (raw == null || raw.isEmpty) return GraphLibrary([]);
    try {
      final decoded = jsonDecode(raw);
      if (decoded is! List) return GraphLibrary([]);
      final out = <GraphLibraryEntry>[];
      for (final item in decoded) {
        if (item is! Map) continue;
        final name = item['name'];
        final graph = item['graph'];
        if (name is String && graph is Map) {
          out.add(GraphLibraryEntry(
            name,
            graphFromJson(graph.cast<String, dynamic>()),
          ));
        }
      }
      return GraphLibrary(_deduplicated(out));
    } catch (_) {
      return GraphLibrary([]);
    }
  }

  /// The name the server's active graph is added under.
  static const activeName = 'active (server)';

  /// Add the server's active [graph] unless an entry already holds the same
  /// graph. Returns the entry added, or null when it was already there.
  GraphLibraryEntry? addActive(CognitionGraph graph) {
    final want = canonicalGraphJson(graph);
    if (entries.any((e) => canonicalGraphJson(e.graph) == want)) return null;
    final entry = GraphLibraryEntry(uniqueName(activeName), graph);
    entries.add(entry);
    return entry;
  }

  /// Names are the list's widget keys, so they must be unique. A stored library
  /// can break that (it is untrusted, and older portals added the active graph
  /// again on every visit): drop entries that repeat an earlier entry's name and
  /// graph, and rename the rest.
  static List<GraphLibraryEntry> _deduplicated(List<GraphLibraryEntry> all) {
    final out = GraphLibrary([]);
    final seen = <String, Set<String>>{};
    for (final e in all) {
      final graphs = seen.putIfAbsent(e.name, () => {});
      if (!graphs.add(canonicalGraphJson(e.graph))) continue;
      out.entries.add(GraphLibraryEntry(out.uniqueName(e.name), e.graph));
    }
    return out.entries;
  }

  /// Persist the library (best effort).
  void save() {
    try {
      final blob = jsonEncode([
        for (final e in entries)
          {'name': e.name, 'graph': graphToJson(e.graph)},
      ]);
      platform.saveRaw(_storageKey, blob);
    } catch (_) {
      // Ignore — the in-memory list stays authoritative for the session.
    }
  }

  bool hasName(String name) => entries.any((e) => e.name == name);

  /// A name not already in the library, based on [base] (e.g. "graph 2").
  String uniqueName(String base) {
    if (!hasName(base)) return base;
    for (var i = 2;; i++) {
      final candidate = '$base $i';
      if (!hasName(candidate)) return candidate;
    }
  }
}
