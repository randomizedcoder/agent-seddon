import 'dart:convert';
import 'dart:io';

import 'package:agent_portal/src/theme/theme_validate.dart';
import 'package:flutter_test/flutter_test.dart';

/// The colour and font tables shared with Rust (`crates/agent-theme/tests/
/// shared_cases.rs`): one file, two implementations, so a rule changed on one
/// side only fails the other side's test (visual/07-testing.md).
///
/// Read from the repo (`../test/themes/`) locally; the `portal-widget` check,
/// whose source is `portal/` alone, points `THEME_CASES` at its store copy.
void main() {
  final path = Platform.environment['THEME_CASES'] ??
      '../test/themes/validation_cases.json';
  final doc = jsonDecode(File(path).readAsStringSync()) as Map<String, dynamic>;

  String input(Map<String, dynamic> c) => c.containsKey('input')
      ? c['input'] as String
      : (c['prefix'] as String) + (c['repeat'] as String) * (c['count'] as int);

  /// `{"ok": v}` → v, `{"err": kind}` → the [ThemeErrorKind] wire name.
  Object outcome(Object? Function() run) {
    try {
      return {'ok': run()};
    } on ThemeError catch (e) {
      return {'err': e.kind.wire};
    }
  }

  void table(String name, Object? Function(String) run,
      Object? Function(Object?) okValue) {
    final cases = (doc[name] as List).cast<Map<String, dynamic>>();
    group(name, () {
      test('covers every case class', () {
        for (final prefix in [
          'positive_', 'negative_', 'boundary_', 'corner_', 'adversarial_', //
        ]) {
          expect(cases.any((c) => (c['name'] as String).startsWith(prefix)),
              isTrue,
              reason: '$name has no $prefix case');
        }
      });
      for (final c in cases) {
        test('${c['name']}: ${c['description']}', () {
          final want = c['expected'] as Map<String, dynamic>;
          final got = outcome(() => run(input(c)));
          final expected = want.containsKey('ok')
              ? {'ok': okValue(want['ok'])}
              : {'err': want['err']};
          expect(got, expected);
        });
      }
    });
  }

  table('parse_color', parseColor,
      (v) => int.parse((v as String).substring(2), radix: 16));
  table('font_family', checkFont, (v) => v);
}
