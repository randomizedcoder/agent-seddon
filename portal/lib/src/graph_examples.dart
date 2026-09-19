import 'dart:convert';

import 'package:flutter/services.dart' show rootBundle;

import 'graph_json.dart';
import 'graph_library.dart';

/// The cognition graphs shipped with the repo (`config/cognition/*.textproto`),
/// bundled as JSON assets (declared in `pubspec.yaml` and kept in sync with the
/// textproto by the `portal_examples_parity` test in `agent-graph`). They seed a
/// fresh [GraphLibrary] so the Graph tab demonstrates the feature out of the box
/// instead of showing an empty list. Each becomes an ordinary, fully-editable
/// entry — duplicate, edit, export, or **Set active**; deleting them all and
/// reloading re-seeds from these assets.
const _exampleAssets = <(String, String)>[
  ('simple (example)', 'assets/graphs/simple.json'),
  ('economical (example)', 'assets/graphs/economical.json'),
  ('intermediate (example)', 'assets/graphs/intermediate.json'),
  ('advanced (example)', 'assets/graphs/advanced.json'),
];

/// Load the bundled example graphs as library entries. Best-effort per file: a
/// missing or corrupt asset is skipped rather than failing the whole page — a
/// partial seed beats a broken Graph tab.
Future<List<GraphLibraryEntry>> loadExampleGraphs() async {
  final out = <GraphLibraryEntry>[];
  for (final (name, path) in _exampleAssets) {
    try {
      final raw = await rootBundle.loadString(path);
      final decoded = jsonDecode(raw);
      if (decoded is Map) {
        out.add(GraphLibraryEntry(
          name,
          graphFromJson(decoded.cast<String, dynamic>()),
        ));
      }
    } catch (_) {
      // Skip this asset; continue with the rest.
    }
  }
  return out;
}
