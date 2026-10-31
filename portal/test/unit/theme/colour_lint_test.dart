import 'dart:io';

import 'package:agent_portal/src/theme/theme_validate.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../testkit/themes.dart';

/// Pages and widgets take every colour from the theme, so a new theme reaches
/// every pixel. Fails on any `Colors.` / `Color(0x` literal outside
/// `test/themes/colour_allowlist.txt` (visual/07-testing.md).
final _literal = RegExp(r'\bColors\.[a-zA-Z]+|\bColor\(0x[0-9A-Fa-f]+\)');

/// `path:line: literal` for each literal in [files] not in [allow]
/// (`path:literal`). `//` comments are skipped.
List<String> colourLiterals(Map<String, String> files, Set<String> allow) => [
      for (final MapEntry(key: path, value: text) in files.entries)
        for (final (i, line) in text.split('\n').indexed)
          for (final m in _literal.allMatches(line.split('//').first))
            if (!allow.contains('$path:${m.group(0)}'))
              '$path:${i + 1}: ${m.group(0)}',
    ];

void main() {
  final cases = <ThemeCase<(Map<String, String>, Set<String>), int>>[
    const ThemeCase('negative_literal_found', 'a hardcoded colour is reported',
        ({'p.dart': 'final c = Colors.red;'}, {}), 1),
    const ThemeCase('negative_hex_literal_found', 'a hex Color is reported',
        ({'p.dart': 'const c = Color(0xFF00FF00);'}, {}), 1),
    const ThemeCase('corner_allowlisted', 'an allowlisted literal passes',
        ({'p.dart': 'Colors.transparent'}, {'p.dart:Colors.transparent'}), 0),
    const ThemeCase('corner_in_comment', 'a literal inside a // comment is ignored',
        ({'p.dart': 'x(); // was Colors.green'}, {}), 0),
    const ThemeCase('positive_theme_colour', 'a scheme role is not a literal',
        ({'p.dart': 'Theme.of(context).colorScheme.error'}, {}), 0),
    const ThemeCase('boundary_allowlist_is_per_file', 'an allowlist entry covers only its file',
        ({'q.dart': 'Colors.transparent'}, {'p.dart:Colors.transparent'}), 1),
  ];
  for (final c in cases) {
    test('${c.name}: ${c.description}',
        () => expect(colourLiterals(c.input.$1, c.input.$2), hasLength(c.expected)));
  }

  test('positive_clean_tree: lib/src/pages and lib/src/widgets have no literals', () {
    final allow = File('test/themes/colour_allowlist.txt')
        .readAsLinesSync()
        .map((l) => l.trim())
        .where((l) => l.isNotEmpty && !l.startsWith('#'))
        .toSet();
    final files = {
      for (final dir in ['lib/src/pages', 'lib/src/widgets'])
        for (final f in Directory(dir).listSync(recursive: true).whereType<File>())
          if (f.path.endsWith('.dart')) f.path: f.readAsStringSync(),
    };
    expect(files, isNotEmpty);
    expect(colourLiterals(files, allow), isEmpty);
  });

  test('positive_fonts_bundled: every allowlisted family is declared with its files', () {
    final pubspec = File('pubspec.yaml').readAsStringSync();
    for (final family in fontFamilies) {
      if (const {'monospace', 'sans-serif', 'serif'}.contains(family)) continue;
      expect(pubspec, contains('- family: $family\n'), reason: family);
    }
    for (final m in RegExp(r'- asset: (fonts/\S+)').allMatches(pubspec)) {
      expect(File(m.group(1)!).existsSync(), isTrue, reason: m.group(1));
    }
  });

  test('positive_builtin_fonts_allowlisted: built-ins only name bundled families', () {
    for (final t in builtinThemes()) {
      for (final f in [t.typography.bodyFamily, t.typography.headingFamily, t.typography.monoFamily]) {
        expect(() => checkFont(f), returnsNormally, reason: '${t.id}: $f');
      }
    }
  });
}
