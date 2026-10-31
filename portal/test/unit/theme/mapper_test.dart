import 'package:agent_portal/src/gen/agent/v1/theme.pb.dart' as pb;
import 'package:agent_portal/src/theme/builtin_themes.dart';
import 'package:agent_portal/src/theme/seddon_colors.dart';
import 'package:agent_portal/src/theme/theme_mapper.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../testkit/themes.dart';

/// `themeToThemeData` (visual/07-testing.md `theme_to_themedata`): each row maps
/// a theme and checks the tokens that make it look like itself.
void main() {
  SeddonColors seddon(ThemeData d) => d.extension<SeddonColors>()!;
  double radiusOf(ThemeData d) =>
      ((d.cardTheme.shape! as RoundedRectangleBorder).borderRadius
              as BorderRadius)
          .topLeft
          .x;

  group('prototype is unchanged', () {
    for (final b in Brightness.values) {
      test('positive_prototype_unchanged_${b.name}: maps to the pre-themes ThemeData',
          () {
        final today = ThemeData(
          colorSchemeSeed: Colors.indigo,
          brightness: b,
          useMaterial3: true,
          fontFamily: 'SourceSerif4',
        );
        final got = themeToThemeData(builtinTheme('prototype'), b);
        expect(got.colorScheme, today.colorScheme);
        expect(got.textTheme, today.textTheme);
        expect(got.scaffoldBackgroundColor, today.scaffoldBackgroundColor);
        expect(got.visualDensity, today.visualDensity);
        expect(got.cardTheme, today.cardTheme);
        expect(got.filledButtonTheme, today.filledButtonTheme);
        expect(got.inputDecorationTheme, today.inputDecorationTheme);
        expect(got.navigationRailTheme, today.navigationRailTheme);
        expect(got.materialTapTargetSize, today.materialTapTargetSize);
        expect(got.focusColor, today.focusColor);
        // The colours pages used to hardcode.
        expect(seddon(got).success, const Color(0xFF4CAF50));
        expect(seddon(got).nav('fleet', Colors.black), const Color(0xFFC07A85));
        expect(seddon(got).edge('background', Colors.black),
            const Color(0xFFFF9800));
        expect(seddon(got).mono.fontFamily, 'monospace');
      });
    }
  });

  final cases = <ThemeCase<(pb.Theme, Brightness), void Function(ThemeData)>>[
    ThemeCase('positive_tn3270', 'the green screen',
        (builtinTheme('tn3270'), Brightness.dark), (d) {
      expect(d.scaffoldBackgroundColor, const Color(0xFF000000));
      expect(d.colorScheme.onSurface, const Color(0xFF33FF33));
      expect(d.textTheme.bodyMedium!.fontFamily, 'IBM3270');
      expect(d.textTheme.titleLarge!.fontFamily, 'IBM3270');
      expect(radiusOf(d), 0);
      expect(d.visualDensity, VisualDensity.compact);
      expect(d.cardTheme.elevation, 0);
    }),
    ThemeCase('positive_windows', 'white with a blue accent',
        (builtinTheme('windows'), Brightness.light), (d) {
      expect(d.scaffoldBackgroundColor, const Color(0xFFFFFFFF));
      expect(d.colorScheme.primary, const Color(0xFF0067C0));
      expect(d.colorScheme.surfaceContainerLow, const Color(0xFFFFFFFF));
      expect(radiusOf(d), 4);
      expect(d.textTheme.bodyMedium!.fontFamily, 'OpenSans');
    }),
    ThemeCase('positive_macos', 'rounded and comfortable',
        (builtinTheme('macos'), Brightness.light), (d) {
      expect(radiusOf(d), 10);
      expect(d.visualDensity, roomyDensity);
      expect(d.textTheme.bodyMedium!.letterSpacing, -0.2);
      // Inter is a variable font: the weight is also an axis value.
      expect(d.textTheme.bodyMedium!.fontVariations,
          [const FontVariation('wght', 400)]);
    }),
    ThemeCase('positive_accessible', 'big text, thick focus, big targets',
        (builtinTheme('accessible'), Brightness.light), (d) {
      final base = Typography.material2021().englishLike.bodyMedium!;
      expect(d.textTheme.bodyMedium!.fontSize, base.fontSize! * 1.25);
      expect(d.textTheme.titleLarge!.fontWeight, FontWeight.w700);
      expect(d.materialTapTargetSize, MaterialTapTargetSize.padded);
      expect(d.filledButtonTheme.style!.minimumSize!.resolve({}),
          const Size(48, 48));
      final side = d.filledButtonTheme.style!.side!
          .resolve({WidgetState.focused})!;
      expect(side.width, 3);
      expect(d.filledButtonTheme.style!.side!.resolve({}), isNull);
      expect(seddon(d).statusIcons, isTrue);
    }),
    ThemeCase('positive_seddon_colors', 'semantic tokens reach the extension',
        (builtinTheme('windows'), Brightness.light), (d) {
      expect(seddon(d).success, const Color(0xFF0F7B0F));
      expect(seddon(d).mono.fontFamily, 'JetBrainsMono');
    }),
    ThemeCase('corner_seedless_tertiary', 'a palette without a seed gets no hue-shifted tertiary',
        (builtinTheme('windows'), Brightness.dark), (d) {
      expect(d.colorScheme.tertiary, d.colorScheme.secondary);
      expect(d.colorScheme.tertiaryContainer, d.colorScheme.secondaryContainer);
    }),
    ThemeCase('positive_seeded_tertiary', 'a seeded palette keeps the tonal tertiary',
        (builtinTheme('prototype'), Brightness.light), (d) {
      expect(d.colorScheme.tertiary, isNot(d.colorScheme.secondary));
    }),
    ThemeCase('corner_light_only_in_dark_mode',
        'a light-only theme in dark mode uses its light palette',
        (builtinTheme('windows')..clearDark(), Brightness.dark), (d) {
      expect(d.brightness, Brightness.light);
      expect(d.scaffoldBackgroundColor, const Color(0xFFFFFFFF));
    }),
    ThemeCase('corner_dark_only_in_light_mode', 'tn3270 in light mode stays dark',
        (builtinTheme('tn3270'), Brightness.light), (d) {
      expect(d.brightness, Brightness.dark);
      expect(d.scaffoldBackgroundColor, const Color(0xFF000000));
    }),
    ThemeCase('corner_missing_nav_tint', 'a tab with no tint falls back to primary',
        (builtinTheme('windows'), Brightness.light), (d) {
      expect(seddon(d).nav('fleet', d.colorScheme.primary), d.colorScheme.primary);
    }),
    ThemeCase('corner_missing_semantic', 'a palette without semantic colours uses scheme roles',
        (builtinTheme('windows')..light.clearSemantic(), Brightness.light), (d) {
      expect(seddon(d).success, d.colorScheme.primary);
      expect(seddon(d).muted, d.colorScheme.onSurfaceVariant);
    }),
    ThemeCase('boundary_scale_0_75', 'the smallest scale reaches TextTheme',
        (builtinTheme('windows')..typography.scale = 0.75, Brightness.light), (d) {
      final base = Typography.material2021().englishLike.bodyMedium!;
      expect(d.textTheme.bodyMedium!.fontSize, base.fontSize! * 0.75);
    }),
    ThemeCase('boundary_scale_1_5', 'the largest scale reaches TextTheme',
        (builtinTheme('windows')..typography.scale = 1.5, Brightness.light), (d) {
      final base = Typography.material2021().englishLike.bodyMedium!;
      expect(d.textTheme.bodyMedium!.fontSize, base.fontSize! * 1.5);
    }),
    ThemeCase('positive_hand_sizes_scale', 'sizes a page sets by hand follow the theme scale',
        (builtinTheme('accessible'), Brightness.light), (d) {
      expect(seddon(d).textScale, 1.25);
      expect(seddon(d).sp(12), 15);
      expect(seddon(d).denseLists, isFalse,
          reason: 'a dense ListTile pins its text at 13/12px');
    }),
    ThemeCase('positive_unscaled_lists_stay_dense', 'a 1.0 theme keeps dense lists',
        (builtinTheme('prototype'), Brightness.light), (d) {
      expect(seddon(d).sp(11), 11);
      expect(seddon(d).denseLists, isTrue);
    }),
    ThemeCase('boundary_dense_at_1_1', 'the largest scale that keeps dense lists',
        (builtinTheme('windows')..typography.scale = 1.1, Brightness.light),
        (d) => expect(seddon(d).denseLists, isTrue)),
    ThemeCase('boundary_full_tiles_at_1_11', 'just over: full-size tiles',
        (builtinTheme('windows')..typography.scale = 1.11, Brightness.light),
        (d) => expect(seddon(d).denseLists, isFalse)),
    ThemeCase('corner_unset_scale', 'scale 0 (unset) means 1',
        (builtinTheme('windows')..typography.clearScale(), Brightness.light),
        (d) => expect(seddon(d).textScale, 1)),
    ThemeCase('corner_reduce_motion', 'reduce_motion removes page transitions',
        (builtinTheme('accessible'), Brightness.light), (d) {
      expect(d.pageTransitionsTheme.builders[TargetPlatform.linux],
          isNot(isA<FadeUpwardsPageTransitionsBuilder>()));
      expect(d.pageTransitionsTheme.builders[TargetPlatform.linux], isNotNull);
    }),
  ];
  group('themeToThemeData', () {
    for (final c in cases) {
      test('${c.name}: ${c.description}',
          () => c.expected(themeToThemeData(c.input.$1, c.input.$2)));
    }
  });

  group('switching themes animates', () {
    // MaterialApp tweens the old ThemeData into the new one; a TextStyle with a
    // different `inherit` makes TextStyle.lerp throw mid-switch.
    for (final from in builtinThemeIds) {
      for (final to in builtinThemeIds) {
        if (from == to) continue;
        test('corner_lerp_${from}_to_$to: ThemeData.lerp does not throw', () {
          final a = themeToThemeData(builtinTheme(from), Brightness.light);
          final b = themeToThemeData(builtinTheme(to), Brightness.light);
          expect(() => ThemeData.lerp(a, b, 0.5), returnsNormally);
        });
      }
    }
  });

  group('mapThemeOrFallback', () {
    final prototype = builtinTheme('prototype');
    final cases = <ThemeCase<pb.Theme, int>>[
      ThemeCase('positive_valid_theme_used', 'a valid theme is applied, nothing logged',
          builtinTheme('windows'), 0),
      ThemeCase('negative_invalid_falls_back',
          'a theme that fails validation renders as prototype and logs once',
          builtinTheme('windows')..typography.bodyFamily = 'nope', 1),
      ThemeCase('adversarial_empty_theme', 'an empty document falls back',
          pb.Theme(), 1),
    ];
    for (final c in cases) {
      test('${c.name}: ${c.description}', () {
        final logs = <String>[];
        final got = mapThemeOrFallback(c.input, Brightness.light,
            fallback: prototype, log: logs.add);
        expect(logs, hasLength(c.expected));
        final want = c.expected == 0
            ? themeToThemeData(c.input, Brightness.light)
            : themeToThemeData(prototype, Brightness.light);
        expect(got.colorScheme, want.colorScheme);
      });
    }
  });
}
