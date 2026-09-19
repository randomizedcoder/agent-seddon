import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

import '../testkit/spec.dart';
import 'registry.dart';

/// The **completeness critic** (design 03) — the "no keyed element without a
/// test" guarantee, the GUI twin of the Rust side's "no silent caps" discipline.
///
/// It source-scans `portal/lib` for every `Key('…')` literal, reduces each to its
/// **stem** (the static prefix of an interpolated per-item key, e.g.
/// `prompts.list.item`), and fails if a **tabled** page (one listed in
/// [allSpecs]) has any key stem without a `positive_` spec row. Pages not yet
/// tabled are reported as *pending* — logged, never silently skipped — so inc 4's
/// job is simply to add each page to [allSpecs] and drive its pending count to 0.
void main() {
  // `Key('literal')` / `ValueKey('literal')`, single-quoted (the repo's style).
  final keyLiteral = RegExp(r'''(?:ValueKey|Key)\(\s*'([^']*)'\s*\)''');

  /// The static stem of a key literal: the prefix before the first interpolation
  /// (either `${…}` or `$ident`), with any trailing dot stripped. `prompts.save`
  /// → `prompts.save`; `prompts.list.item.${e.kind.name}.${e.id}` →
  /// `prompts.list.item`; `prompts.personality.item.$p` → `prompts.personality.item`.
  String stemOf(String raw) {
    final i = raw.indexOf(r'$');
    var s = i < 0 ? raw : raw.substring(0, i);
    while (s.endsWith('.')) {
      s = s.substring(0, s.length - 1);
    }
    return s;
  }

  String pageOf(String stem) => stem.split('.').first;

  /// A spec [elementId] covers a scanned [stem] if they are equal or the stem is
  /// a dot-boundary extension of the element family (e.g. the family
  /// `prompts.personality.item` covers the literal `prompts.personality.item.default`).
  bool covers(String elementId, String stem) =>
      stem == elementId || stem.startsWith('$elementId.');

  // Scan lib for all key stems, grouped by page. Only dotted, page-scoped keys
  // (a `.` present) — Flutter's own internal keys never match this shape.
  Map<String, Set<String>> scanKeyStems() {
    final byPage = <String, Set<String>>{};
    final dir = Directory('lib');
    for (final f in dir.listSync(recursive: true).whereType<File>()) {
      if (!f.path.endsWith('.dart')) continue;
      for (final m in keyLiteral.allMatches(f.readAsStringSync())) {
        final raw = m.group(1)!;
        if (!raw.contains('.')) continue; // skip non-namespaced keys
        final stem = stemOf(raw);
        byPage.putIfAbsent(pageOf(stem), () => <String>{}).add(stem);
      }
    }
    return byPage;
  }

  late final Map<String, Set<String>> stemsByPage = scanKeyStems();
  final tabledPages = {for (final s in allSpecs) s.page};

  test('critic: found some keyed elements to check', () {
    expect(stemsByPage, isNotEmpty,
        reason: 'the key scan found no namespaced Key() literals in lib/');
  });

  for (final spec in allSpecs) {
    test('critic: every ${spec.page} key has a positive_ spec row', () {
      final required = stemsByPage[spec.page] ?? <String>{};
      expect(required, isNotEmpty,
          reason: '${spec.page}: no keys scanned — wrong page prefix?');

      final positives = [
        for (final r in spec.rows)
          if (r.caseClass == CaseClass.positive) r.elementId,
      ];
      final missing = [
        for (final stem in required)
          if (!positives.any((e) => covers(e, stem))) stem,
      ]..sort();
      expect(missing, isEmpty,
          reason: '${spec.page}: these keyed elements have no positive_ row: '
              '$missing');

      // Hygiene: specs must not reference keys that no longer exist in lib.
      final dangling = [
        for (final e in spec.coveredElements)
          if (!required.any((stem) => covers(e, stem))) e,
      ]..sort();
      expect(dangling, isEmpty,
          reason: '${spec.page}: spec rows reference non-existent keys: '
              '$dangling');
    });
  }

  test('critic: pending (not-yet-tabled) pages are reported, not skipped', () {
    final pending = {
      for (final e in stemsByPage.entries)
        if (!tabledPages.contains(e.key)) e.key: e.value.length,
    };
    // Not a failure — this is the visible backlog inc 4 burns down. Assert it is
    // enumerable (no silent caps); when empty, every page is tabled.
    // ignore: avoid_print
    print('completeness critic — tabled: ${tabledPages.toList()..sort()}; '
        'pending pages (key stems): $pending');
    expect(pending, isA<Map<String, int>>());
  });
}
