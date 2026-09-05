// Web build: the graph library persists in `localStorage`, and import/export go
// through a Blob download and a file-input dialog. Every storage access is
// wrapped — a private window or blocked site-data makes reads/writes throw.
//
// `dart:html` is the transport `channel_web.dart` implicitly relies on too; the
// package:web migration is a separate, portal-wide change.
// ignore_for_file: deprecated_member_use, avoid_web_libraries_in_flutter
import 'dart:html' as html;

String? loadRaw(String key) {
  try {
    return html.window.localStorage[key];
  } catch (_) {
    // localStorage unavailable (private window, blocked site data) — start empty.
    return null;
  }
}

void saveRaw(String key, String value) {
  try {
    html.window.localStorage[key] = value;
  } catch (_) {
    // Best effort; the in-memory library still works for this session.
  }
}

Future<void> downloadJson(String filename, String content) async {
  final blob = html.Blob([content], 'application/json');
  final url = html.Url.createObjectUrlFromBlob(blob);
  try {
    html.AnchorElement(href: url)
      ..download = filename
      ..click();
  } finally {
    html.Url.revokeObjectUrl(url);
  }
}

Future<String?> pickJsonFile() async {
  final input = html.FileUploadInputElement()
    ..accept = '.json,application/json';
  input.click();
  await input.onChange.first;
  final files = input.files;
  if (files == null || files.isEmpty) return null;
  final reader = html.FileReader();
  reader.readAsText(files.first);
  await reader.onLoad.first;
  final result = reader.result;
  return result is String ? result : null;
}
