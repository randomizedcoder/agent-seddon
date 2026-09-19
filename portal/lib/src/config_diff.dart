import 'dart:convert';

import 'gen/agent/v1/config.pb.dart';
import 'graph_json.dart' show dartToJsonValue;

/// Diff [orig] against [staged] into dotted-path [ConfigEdit]s under [prefix]
/// (e.g. `'agent.'`). The Settings tab stages a working copy of one config
/// section and calls this to compute the minimal set of changed paths to send
/// to `ConfigService.Put`.
///
/// Rules (kept identical to the backend's acceptance):
///   * Recurses into nested objects, extending the dotted path.
///   * Arrays and scalars are **atomic leaves** — a changed array is one
///     whole-value edit, never indexed paths (the backend rejects `foo[0]`).
///   * A key is an edit iff its `jsonEncode` differs from the original's; this
///     makes `1` vs `1.0`, key order, and nested changes compare correctly.
///   * Keys present only in [orig] are **not** deletions — the form never
///     removes keys, so it only ever emits changes for keys it staged.
///
/// Pure and side-effect free; the widget-level dirty check and Save both build
/// on it. Unit-tested in `test/unit/config_diff_test.dart`.
List<ConfigEdit> configEdits(
  Map<String, dynamic> orig,
  Map<String, dynamic> staged,
  String prefix,
) {
  final out = <ConfigEdit>[];
  for (final key in staged.keys) {
    final o = orig[key];
    final e = staged[key];
    if (o is Map<String, dynamic> && e is Map<String, dynamic>) {
      out.addAll(configEdits(o, e, '$prefix$key.'));
    } else if (jsonEncode(o) != jsonEncode(e)) {
      out.add(ConfigEdit()
        ..path = '$prefix$key'
        ..value = dartToJsonValue(e));
    }
  }
  return out;
}
