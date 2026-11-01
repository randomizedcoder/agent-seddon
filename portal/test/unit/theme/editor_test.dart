import 'package:agent_portal/src/gen/agent/v1/theme.pb.dart' as pb;
import 'package:agent_portal/src/theme/theme_editor.dart';
import 'package:agent_portal/src/theme/theme_validate.dart';
import 'package:fixnum/fixnum.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../testkit/themes.dart';

/// The theme editor's pure helpers (visual/07-testing.md): every row has a
/// class-prefixed name, a description and the expected outcome.
void main() {
  group('freeId', () {
    const cases = <ThemeCase<(String, List<String>), String>>[
      ThemeCase('positive_free', 'an unused id is kept', ('ocean-copy', []), 'ocean-copy'),
      ThemeCase('negative_taken_gets_a_number', 'a taken id gets -2',
          ('ocean-copy', ['ocean-copy']), 'ocean-copy-2'),
      ThemeCase('corner_skips_taken_numbers', 'the first free number is used',
          ('a', ['a', 'a-2', 'a-3']), 'a-4'),
      ThemeCase('boundary_cut_to_the_cap', 'a 70-char base is cut to 64',
          ('xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx', []),
          'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'),
      ThemeCase('adversarial_suffix_fits_the_cap', 'a taken 64-char id gets a suffix and stays 64',
          (
            'yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy',
            ['yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy']
          ),
          'yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy-2'),
    ];
    for (final c in cases) {
      test('${c.name}: ${c.description}', () {
        final got = freeId(c.input.$1, c.input.$2);
        expect(got, c.expected, reason: c.description);
        expect(got.length, lessThanOrEqualTo(maxIdLen));
      });
    }
  });

  group('cloneForEdit', () {
    final stored = userTheme('ocean', visibility: pb.Visibility.VISIBILITY_TENANT);
    final cases = <ThemeCase<(pb.Theme, List<String>), (String, String)>>[
      ThemeCase('positive_builtin_copy', 'a built-in copy is a private, unsaved user theme',
          (builtinTheme('windows'), []), ('windows-copy', 'Windows (copy)')),
      ThemeCase('negative_taken_id', 'a second copy gets a numbered id',
          (builtinTheme('windows'), ['windows-copy']), ('windows-copy-2', 'Windows (copy)')),
      ThemeCase('corner_shared_user_theme', 'a copy of a shared theme starts private, with no owner',
          (stored, []), ('ocean-copy', 'Ocean (copy)')),
      ThemeCase('boundary_empty_source', 'an empty theme still gets a valid id and a name',
          (pb.Theme(), []), ('theme-copy', 'Theme (copy)')),
      ThemeCase('adversarial_long_name', 'a 64-char name stays within the name cap',
          (pb.Theme(id: 'x', name: 'n' * 64), []), ('x-copy', 'n' * 64)),
    ];
    for (final c in cases) {
      test('${c.name}: ${c.description}', () {
        final before = c.input.$1.deepCopy();
        final t = cloneForEdit(c.input.$1, c.input.$2);
        expect((t.id, t.name), c.expected, reason: c.description);
        expect(t.builtin, isFalse);
        expect(t.version, Int64.ZERO);
        expect(t.owner, isEmpty);
        expect(t.visibility, pb.Visibility.VISIBILITY_PRIVATE);
        expect(c.input.$1, before, reason: 'the source is not modified');
      });
    }

    test('positive_copy_of_a_valid_builtin_validates: the copy is ready to save', () {
      for (final id in ['prototype', 'tn3270', 'windows', 'macos', 'accessible']) {
        expect(draftProblem(cloneForEdit(builtinTheme(id), const [])), isNull, reason: id);
      }
    });
  });

  group('setColour / colourOf', () {
    final primary = editorColourFields.firstWhere((f) => f.name == 'primary');
    final code = editorColourFields.firstWhere((f) => f.name == 'code_background');
    final cases = <ThemeCase<(ColourField, String), String>>[
      ThemeCase('positive_palette_field', 'a palette field is set', (primary, '#112233'), '#112233'),
      ThemeCase('positive_semantic_field', 'a semantic field is set in `semantic`',
          (code, '#445566'), '#445566'),
      ThemeCase('boundary_empty_clears', 'an empty value clears the field', (primary, ''), ''),
      ThemeCase('adversarial_value_is_stored_not_trusted',
          'a hostile value is stored as text for the validator to refuse',
          (primary, 'url(x)'), 'url(x)'),
    ];
    for (final c in cases) {
      test('${c.name}: ${c.description}', () {
        final p = pb.Palette(primary: '#000000');
        setColour(p, c.input.$1, c.input.$2);
        expect(colourOf(p, c.input.$1), c.expected, reason: c.description);
      });
    }

    test('corner_every_field_round_trips: each editor field writes its own proto field', () {
      final p = pb.Palette();
      for (final (i, f) in editorColourFields.indexed) {
        setColour(p, f, '#0000${i.toRadixString(16).padLeft(2, '0')}');
      }
      for (final (i, f) in editorColourFields.indexed) {
        expect(colourOf(p, f), '#0000${i.toRadixString(16).padLeft(2, '0')}', reason: f.name);
      }
      expect(p.primary, '#000001');
      expect(p.semantic.codeForeground, '#000013');
    });
  });

  group('paletteContrast', () {
    final windows = builtinTheme('windows');
    final cases = <ThemeCase<pb.Palette Function(), (String, String, bool?)>>[
      ThemeCase('positive_builtin_passes', 'Windows body text on its surface passes',
          () => windows.light, ('on_surface', 'surface', true)),
      ThemeCase('negative_low_contrast_fails', 'grey on white fails',
          () => windows.light.deepCopy()..onSurface = '#888888', ('on_surface', 'surface', false)),
      ThemeCase('corner_unset_pair_has_no_ratio', 'a seed-only palette has no ratio for its pairs',
          () => pb.Palette(seed: '#3F51B5'), ('on_surface', 'surface', null)),
      ThemeCase('boundary_focus_needs_three', 'the focus ring is held to 3:1',
          () => windows.light, ('focus', 'background', true)),
      ThemeCase('adversarial_bad_colour_is_no_ratio', 'a colour that does not parse gives no ratio, not a throw',
          () => windows.light.deepCopy()..onSurface = 'red; x', ('on_surface', 'surface', null)),
    ];
    for (final c in cases) {
      test('${c.name}: ${c.description}', () {
        final checks = paletteContrast(windows, c.input());
        final (fg, bg, passes) = c.expected;
        final check = checks.firstWhere((k) => k.fg == fg && k.bg == bg);
        expect(passes == null ? check.ratio : check.passes, passes ?? isNull, reason: c.description);
      });
    }

    test('positive_agrees_with_the_validator: a palette passes every badge iff it validates', () {
      for (final id in ['tn3270', 'windows', 'macos', 'accessible']) {
        final t = builtinTheme(id);
        final p = t.hasLight() ? t.light : t.dark;
        expect(paletteContrast(t, p).where((c) => c.ratio != null).every((c) => c.passes), isTrue,
            reason: id);
      }
      final bad = builtinTheme('windows')..light.onSurface = '#888888';
      expect(draftProblem(bad), contains('contrast'));
      expect(paletteContrast(bad, bad.light).any((c) => c.ratio != null && !c.passes), isTrue);
    });

    test('boundary_shown_truncates: 4.499 shows as 4.49, never a rounded-up pass', () {
      const c = ContrastCheck('a', 'b', 4.5, 4.499, 'AA');
      expect(c.shown, '4.49:1');
      expect(c.passes, isFalse);
    });
  });
}
