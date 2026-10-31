import 'package:agent_portal/src/gen/agent/v1/theme.pb.dart' as pb;
import 'package:agent_portal/src/theme/builtin_themes.dart';
import 'package:agent_portal/src/theme/theme_validate.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../testkit/themes.dart';

/// The Dart port of the theme validator against the Rust table's rules
/// (`crates/agent-theme/tests/validate_table.rs`): each row mutates a built-in
/// and expects success (null) or one [ThemeErrorKind].
void main() {
  ThemeErrorKind? run(pb.Theme t) {
    try {
      validateTheme(t);
      return null;
    } on ThemeError catch (e) {
      return e.kind;
    }
  }

  group('validateTheme', () {
    for (final id in builtinThemeIds) {
      test('positive_builtin_$id: the bundled $id theme validates', () {
        expect(run(builtinTheme(id)), isNull);
      });
    }

    // (base theme, mutation)
    final cases = <ThemeCase<(String, void Function(pb.Theme)), ThemeErrorKind?>>[
      ThemeCase('corner_light_only', 'a theme may omit its dark palette',
          ('windows', (t) => t.clearDark()), null),
      ThemeCase('corner_dark_only', 'a theme may omit its light palette',
          ('tn3270', (t) => t.clearLight()), null),
      ThemeCase('negative_no_palette', 'a theme needs at least one palette',
          ('tn3270', (t) => t.clearDark()), ThemeErrorKind.missing),
      ThemeCase('negative_missing_role_without_seed',
          'without a seed every role is required',
          ('windows', (t) => t.light.clearSurface()), ThemeErrorKind.missing),
      ThemeCase('corner_seed_fills_missing_pair', 'with a seed a whole pair may be unset',
          ('windows', (t) => t.light..clearPrimary()..clearOnPrimary()..seed = '#0067C0'), null),
      ThemeCase('negative_unknown_font', 'a family the portal does not ship',
          ('windows', (t) => t.typography.bodyFamily = 'Comic Sans MS'),
          ThemeErrorKind.unknownFont),
      ThemeCase('negative_low_contrast', 'grey text on white fails AA',
          ('windows', (t) => t.light.onSurface = '#AAAAAA'),
          ThemeErrorKind.contrast),
      ThemeCase('negative_accessible_below_aaa',
          'the accessible theme declares AAA, so 4.5:1 text is not enough',
          ('accessible', (t) => t.light.onSurface = '#767676'),
          ThemeErrorKind.contrast),
      ThemeCase('negative_bad_colour', 'a CSS colour name is refused',
          ('windows', (t) => t.light.primary = 'blue'), ThemeErrorKind.badColor),
      ThemeCase('negative_unknown_nav_tab', 'nav tints name a known tab',
          ('prototype', (t) => t.light.semantic.navTint['admin'] = '#000000'),
          ThemeErrorKind.unknownKey),
      ThemeCase('boundary_radius_0', 'square corners',
          ('windows', (t) => t.shape.cornerRadius = 0), null),
      ThemeCase('boundary_radius_32', 'the largest radius',
          ('windows', (t) => t.shape.cornerRadius = 32), null),
      ThemeCase('boundary_radius_33', 'one over the largest radius',
          ('windows', (t) => t.shape.cornerRadius = 33), ThemeErrorKind.outOfRange),
      ThemeCase('boundary_scale_0_75', 'the smallest text scale',
          ('windows', (t) => t.typography.scale = 0.75), null),
      ThemeCase('boundary_scale_1_5', 'the largest text scale',
          ('windows', (t) => t.typography.scale = 1.5), null),
      ThemeCase('boundary_scale_1_51', 'just over the largest text scale',
          ('windows', (t) => t.typography.scale = 1.51), ThemeErrorKind.outOfRange),
      ThemeCase('boundary_weight_900', 'the heaviest weight',
          ('windows', (t) => t.typography.bodyWeight = 900), null),
      ThemeCase('negative_weight_450', 'weights step by 100',
          ('windows', (t) => t.typography.bodyWeight = 450), ThemeErrorKind.outOfRange),
      ThemeCase('boundary_name_64', 'the longest name',
          ('windows', (t) => t.name = 'n' * 64), null),
      ThemeCase('boundary_name_65', 'one over the longest name',
          ('windows', (t) => t.name = 'n' * 65), ThemeErrorKind.badName),
      ThemeCase('negative_empty_name', 'a theme needs a name',
          ('windows', (t) => t.name = ''), ThemeErrorKind.badName),
      ThemeCase('adversarial_traversal_id', 'an id that walks out of the store',
          ('windows', (t) => t.id = '../../etc'), ThemeErrorKind.badId),
      ThemeCase('adversarial_leading_dash_id', 'an id that reads as a flag',
          ('windows', (t) => t.id = '-rf'), ThemeErrorKind.badId),
      ThemeCase('adversarial_font_url', 'a URL would make the browser fetch a font',
          ('windows', (t) => t.typography.monoFamily = 'https://evil/x.woff'),
          ThemeErrorKind.unknownFont),
      ThemeCase('adversarial_nan_scale', 'NaN is not a scale',
          ('windows', (t) => t.typography.scale = double.nan),
          ThemeErrorKind.outOfRange),
      ThemeCase('adversarial_inf_letter_spacing', 'infinity is not a spacing',
          ('windows', (t) => t.typography.letterSpacing = double.infinity),
          ThemeErrorKind.outOfRange),
      ThemeCase('adversarial_name_bidi', 'a right-to-left override in the name',
          ('windows', (t) => t.name = 'Win\u202Ewodn'), ThemeErrorKind.badName),
      ThemeCase('adversarial_name_newline', 'a newline in the name',
          ('windows', (t) => t.name = 'Win\ndows'), ThemeErrorKind.badName),
      ThemeCase('adversarial_zero_width_name', 'a zero-width space hides in a lookalike name',
          ('windows', (t) => t.name = 'Win\u200Bdows'), ThemeErrorKind.badName),
      ThemeCase('adversarial_bom_name', 'a byte-order mark in the name',
          ('windows', (t) => t.name = '\uFEFFWindows'), ThemeErrorKind.badName),
      ThemeCase('adversarial_arabic_letter_mark', 'U+061C reorders text like a bidi control',
          ('windows', (t) => t.name = 'abc\u061Cdef'), ThemeErrorKind.badName),
      ThemeCase('adversarial_line_separator', 'U+2028 breaks a line without a newline',
          ('windows', (t) => t.description = 'a\u2028ERROR x'), ThemeErrorKind.badName),
      ThemeCase('adversarial_soft_hyphen', 'an invisible soft hyphen in the name',
          ('windows', (t) => t.name = 'Mac\u00ADOS'), ThemeErrorKind.badName),
      ThemeCase('adversarial_tag_characters', 'invisible tag characters can smuggle text',
          ('windows', (t) => t.name = 'abc\u{E0041}\u{E0042}'), ThemeErrorKind.badName),
      ThemeCase('corner_unicode_name', 'letters from any script are fine',
          ('windows', (t) => t.name = 'Thème 東京 — ночь'), null),
      ThemeCase('adversarial_half_pair_on_seed',
          'hand-picked text on a seed-derived surface is a pair nobody checked',
          ('prototype', (t) => t.light.onSurface = '#FFFFFF'), ThemeErrorKind.missing),
      ThemeCase('corner_full_pair_on_seed', 'a seeded palette may write a pair in full',
          ('prototype', (t) => t.light..onPrimary = '#FFFFFF'..primary = '#3F51B5'), null),
      ThemeCase('adversarial_oversize_doc', 'a document over 64 KiB',
          ('windows', (t) => t.description = 'x' * (70 * 1024)),
          ThemeErrorKind.tooLarge),
      ThemeCase('adversarial_too_many_tints', 'more nav tints than tabs',
          ('prototype', (t) {
            for (var i = 0; i < 17; i++) {
              t.light.semantic.navTint['t$i'] = '#000000';
            }
          }), ThemeErrorKind.unknownKey),
    ];
    for (final c in cases) {
      test('${c.name}: ${c.description}', () {
        final (base, mutate) = c.input;
        final t = builtinTheme(base);
        mutate(t);
        expect(run(t), c.expected);
      });
    }

    test('positive_contrast_error_names_the_pair: the message says what to fix',
        () {
      final t = builtinTheme('windows')..light.onPrimary = '#FFFFFF';
      t.light.primary = '#007AFF';
      expect(
          () => validateTheme(t),
          throwsA(isA<ThemeError>().having((e) => e.message, 'message',
              'light: contrast: on_primary #FFFFFF on primary #007AFF is 4.01:1, needs 4.5:1 (AA)')));
    });
  });

  group('contrastRatio', () {
    final cases = <ThemeCase<(String, String), double>>[
      const ThemeCase('positive_black_white', 'maximum ratio', ('#000000', '#FFFFFF'), 21),
      const ThemeCase('positive_symmetric', 'order does not matter', ('#FFFFFF', '#000000'), 21),
      const ThemeCase('positive_macos_blue', 'white on Apple blue is below AA', ('#FFFFFF', '#007AFF'), 4.02),
      const ThemeCase('boundary_same_colour', 'minimum ratio', ('#777777', '#777777'), 1),
      const ThemeCase('corner_alpha_composited', '50% black over white is mid grey', ('#00000080', '#FFFFFF'), 4.00),
      const ThemeCase('corner_low_channel_branch', 'dark channels use the linear branch', ('#0A0A0A', '#000000'), 1.06),
      const ThemeCase('adversarial_transparent_text', 'invisible text has no contrast', ('#00000000', '#FFFFFF'), 1),
      const ThemeCase('adversarial_translucent_background', 'judged on the worst backdrop', ('#FFFFFF', '#00000000'), 1),
    ];
    for (final c in cases) {
      test('${c.name}: ${c.description}', () {
        final got = contrastRatio(parseColor(c.input.$1), parseColor(c.input.$2));
        expect(got, closeTo(c.expected, 0.01));
      });
    }
  });

  group('parseThemeJson', () {
    final cases = <ThemeCase<String, ThemeErrorKind?>>[
      const ThemeCase('positive_minimal', 'an object with known fields', '{"id":"x"}', null),
      const ThemeCase('negative_not_json', 'malformed JSON', '{"id":', ThemeErrorKind.parse),
      const ThemeCase('negative_array', 'a JSON array is not a theme', '[1]', ThemeErrorKind.parse),
      const ThemeCase('adversarial_wrong_shape', 'a palette that is a number', '{"light": 7}', ThemeErrorKind.parse),
      const ThemeCase('adversarial_unknown_field', 'unknown fields are refused', '{"script":"x"}', ThemeErrorKind.parse),
      ThemeCase('adversarial_huge', 'over 64 KiB before decoding', '"${'x' * 70000}"', ThemeErrorKind.tooLarge),
    ];
    for (final c in cases) {
      test('${c.name}: ${c.description}', () {
        ThemeErrorKind? got;
        try {
          parseThemeJson(c.input);
        } on ThemeError catch (e) {
          got = e.kind;
        }
        expect(got, c.expected);
      });
    }
  });
}
