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
      return GraphLibrary(out);
    } catch (_) {
      return GraphLibrary([]);
    }
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
