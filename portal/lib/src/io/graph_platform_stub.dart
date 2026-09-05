// Fallback (and native desktop): no browser storage or file dialogs, so the
// library lives in memory for the session and file import/export are inert.
// Web is the primary target — see `graph_platform_web.dart`.

final Map<String, String> _mem = {};

String? loadRaw(String key) => _mem[key];

void saveRaw(String key, String value) => _mem[key] = value;

Future<void> downloadJson(String filename, String content) async {
  // No download surface off the web; the in-app library still holds the graph.
}

Future<String?> pickJsonFile() async => null;
