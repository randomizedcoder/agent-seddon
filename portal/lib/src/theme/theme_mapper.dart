// `agent.v1.Theme` + a brightness → Flutter `ThemeData`
// (docs/design/visual/01-theme-model.md#mapping-to-flutter). Pure: the same
// theme always maps to the same `ThemeData`, so it is table-tested directly.
//
// A token the theme leaves unset keeps Material 3's default: no `shape` block
// means Material's own corner radii and elevation, `DENSITY_UNSPECIFIED` the
// platform's density, no `accessibility` block Material's focus and target
// sizes. That is what lets `prototype` render exactly as the portal did before
// themes existed.
import 'package:flutter/material.dart';

import '../gen/agent/v1/theme.pb.dart' as pb;
import '../gen/agent/v1/theme.pbenum.dart' as pbe;
import 'seddon_colors.dart';
import 'theme_validate.dart';

/// Families shipped as one variable font: a weight needs a `wght` variation.
const _variableFamilies = {'Inter'};

/// [theme] as a `ThemeData` for [want]. A theme without the requested palette
/// uses the one it has (tn3270 stays dark in light mode). [theme] must already
/// have passed [validateTheme]; see [mapThemeOrFallback] for untrusted input.
/// What `DENSITY_COMFORTABLE` maps to: 4px more than standard per step.
const roomyDensity = VisualDensity(horizontal: 1, vertical: 1);

ThemeData themeToThemeData(pb.Theme theme, Brightness want) {
  final useDark = want == Brightness.dark ? theme.hasDark() : !theme.hasLight();
  final palette = useDark ? theme.dark : theme.light;
  final brightness = useDark ? Brightness.dark : Brightness.light;
  final cs = _colorScheme(palette, brightness);
  final typo = theme.typography;
  final body = typo.bodyFamily.isEmpty ? null : typo.bodyFamily;
  final mono = typo.monoFamily.isEmpty ? 'monospace' : typo.monoFamily;

  final base = ThemeData(
    colorScheme: cs,
    useMaterial3: true,
    fontFamily: body,
    visualDensity: switch (theme.density) {
      pbe.Density.DENSITY_COMPACT => VisualDensity.compact,
      pbe.Density.DENSITY_STANDARD => VisualDensity.standard,
      // Roomier than standard. Flutter's own `VisualDensity.comfortable` is
      // (-1, -1), tighter than standard, the opposite of what the theme means.
      pbe.Density.DENSITY_COMFORTABLE => roomyDensity,
      _ => null,
    },
    scaffoldBackgroundColor:
        palette.background.isEmpty ? null : _c(palette.background),
    navigationRailTheme: palette.surfaceVariant.isEmpty
        ? null
        : NavigationRailThemeData(
            backgroundColor: _c(palette.surfaceVariant)),
  );

  final a11y = theme.accessibility;
  final ring = a11y.focusRingWidth == 0 ? 2.0 : a11y.focusRingWidth.toDouble();
  final focus = palette.focus.isEmpty ? null : _c(palette.focus);
  final minTarget =
      a11y.minTargetSize == 0 ? null : a11y.minTargetSize.toDouble();
  final shape = theme.hasShape() ? theme.shape : null;
  final radius = shape == null
      ? null
      : BorderRadius.circular(shape.cornerRadius.toDouble());
  final rounded =
      radius == null ? null : RoundedRectangleBorder(borderRadius: radius);
  final flat = shape != null && !shape.elevation;

  // One button style carries the shape, the focus ring and the target size, so
  // every button kind agrees.
  final button = (rounded == null && focus == null && minTarget == null)
      ? null
      : ButtonStyle(
          shape: rounded == null ? null : WidgetStatePropertyAll(rounded),
          side: focus == null
              ? null
              : WidgetStateProperty.resolveWith((s) =>
                  s.contains(WidgetState.focused)
                      ? BorderSide(color: focus, width: ring)
                      : null),
          minimumSize: minTarget == null
              ? null
              : WidgetStatePropertyAll(Size(minTarget, minTarget)),
          elevation: flat ? const WidgetStatePropertyAll(0) : null,
        );

  OutlineInputBorder? field(Color color, double width) => radius == null &&
          focus == null
      ? null
      : OutlineInputBorder(
          borderRadius: radius ?? const BorderRadius.all(Radius.circular(4)),
          borderSide: BorderSide(color: color, width: width));

  final reduceMotion = a11y.reduceMotion;
  return base.copyWith(
    textTheme: _textTheme(base, typo),
    materialTapTargetSize:
        minTarget == null ? null : MaterialTapTargetSize.padded,
    focusColor: focus?.withValues(alpha: 0.24),
    cardTheme: shape == null
        ? null
        : CardThemeData(
            shape: RoundedRectangleBorder(
              borderRadius: radius!,
              side: shape.borderWidth == 0
                  ? BorderSide.none
                  : BorderSide(
                      color: cs.outline, width: shape.borderWidth.toDouble()),
            ),
            elevation: flat ? 0 : null,
          ),
    dialogTheme: rounded == null ? null : DialogThemeData(shape: rounded),
    chipTheme: rounded == null ? null : ChipThemeData(shape: rounded),
    filledButtonTheme:
        button == null ? null : FilledButtonThemeData(style: button),
    elevatedButtonTheme:
        button == null ? null : ElevatedButtonThemeData(style: button),
    outlinedButtonTheme:
        button == null ? null : OutlinedButtonThemeData(style: button),
    textButtonTheme: button == null ? null : TextButtonThemeData(style: button),
    iconButtonTheme: (focus == null && minTarget == null)
        ? null
        : IconButtonThemeData(
            style: ButtonStyle(
            side: button?.side,
            minimumSize: button?.minimumSize,
          )),
    inputDecorationTheme: field(cs.outline, 1) == null
        ? null
        : InputDecorationTheme(
            border: field(cs.outline, 1),
            enabledBorder: field(cs.outline, 1),
            focusedBorder: field(focus ?? cs.primary, focus == null ? 2 : ring),
          ),
    pageTransitionsTheme: reduceMotion
        ? const PageTransitionsTheme(builders: {
            TargetPlatform.android: _NoTransition(),
            TargetPlatform.iOS: _NoTransition(),
            TargetPlatform.linux: _NoTransition(),
            TargetPlatform.macOS: _NoTransition(),
            TargetPlatform.windows: _NoTransition(),
            TargetPlatform.fuchsia: _NoTransition(),
          })
        : null,
    extensions: [_seddonColors(palette.semantic, cs, mono, a11y.statusIcons,
        (typo.scale == 0 || !typo.scale.isFinite) ? 1.0 : typo.scale)],
  );
}

/// [theme] mapped if it validates, else [fallback] (which must be valid, e.g.
/// the built-in prototype). [log] hears why a theme was refused.
ThemeData mapThemeOrFallback(
  pb.Theme theme,
  Brightness want, {
  required pb.Theme fallback,
  void Function(String)? log,
}) {
  try {
    validateTheme(theme);
    return themeToThemeData(theme, want);
  } on ThemeError catch (e) {
    log?.call('theme "${theme.id}" refused, using "${fallback.id}": $e');
    return themeToThemeData(fallback, want);
  }
}

Color _c(String hex) => Color(parseColor(hex));

ColorScheme _colorScheme(pb.Palette p, Brightness brightness) {
  Color? opt(String v) => v.isEmpty ? null : _c(v);
  // Roles the palette leaves unset come from the seed's tonal palette; a palette
  // without a seed names every role, and its primary seeds the rest
  // (tertiary, containers).
  final seed = opt(p.seed) ?? _c(p.primary);
  final derived = ColorScheme.fromSeed(seedColor: seed, brightness: brightness);
  final surface = opt(p.surface);
  final variant = opt(p.surfaceVariant);
  final scheme = derived.copyWith(
    primary: opt(p.primary),
    onPrimary: opt(p.onPrimary),
    secondary: opt(p.secondary),
    onSecondary: opt(p.onSecondary),
    surface: surface,
    onSurface: opt(p.onSurface),
    onSurfaceVariant: opt(p.onSurfaceVariant),
    outline: opt(p.outline),
    outlineVariant: opt(p.outline),
    error: opt(p.error),
    onError: opt(p.onError),
    // An explicit surface means the author chose flat colours: containers
    // follow it rather than a seed tint (a white Windows card stays white).
    surfaceContainerLowest: surface,
    surfaceContainerLow: surface,
    surfaceContainer: surface,
    surfaceContainerHigh: variant ?? surface,
    surfaceContainerHighest: variant ?? surface,
    surfaceTint: surface == null ? null : Colors.transparent,
  );
  // Without a seed, Material's tertiary is a hue-shifted accent the author
  // never chose (Windows blue → purple), so tertiary follows secondary.
  if (opt(p.seed) != null) return scheme;
  return scheme.copyWith(
    tertiary: scheme.secondary,
    onTertiary: scheme.onSecondary,
    tertiaryContainer: scheme.secondaryContainer,
    onTertiaryContainer: scheme.onSecondaryContainer,
  );
}

TextTheme _textTheme(ThemeData theme, pb.Typography t) {
  final base = theme.textTheme;
  // `ThemeData.textTheme` carries no sizes (MaterialApp merges the locale's
  // geometry in at build time), so scaling starts from that geometry.
  final scaled = (t.scale == 0 || t.scale == 1)
      ? base
      : theme.typography.englishLike
          .merge(base)
          .apply(fontSizeFactor: t.scale);
  final heading = t.headingFamily.isEmpty ? null : t.headingFamily;
  final body = t.bodyFamily.isEmpty ? null : t.bodyFamily;
  FontWeight? weight(int w) => w == 0 ? null : FontWeight.values[w ~/ 100 - 1];
  final bodyWeight = weight(t.bodyWeight);
  final headingWeight = weight(t.headingWeight);
  final spacing = t.letterSpacing;

  TextStyle? style(TextStyle? s, {required bool isHeading}) {
    if (s == null) return null;
    final family = (isHeading ? heading : null) ?? body;
    final w = (isHeading ? headingWeight : bodyWeight) ?? s.fontWeight;
    return s.copyWith(
      // Always inheriting, like Material's own: MaterialApp animates between
      // themes, and TextStyle.lerp throws when `inherit` differs.
      inherit: true,
      fontFamily: family,
      fontWeight: w,
      // Absolute, replacing Material's per-style spacing: what the author wrote.
      letterSpacing: spacing == 0 ? null : spacing,
      fontVariations: _variableFamilies.contains(family ?? s.fontFamily)
          ? [FontVariation.weight((w ?? FontWeight.w400).value.toDouble())]
          : null,
    );
  }

  TextStyle? h(TextStyle? s) => style(s, isHeading: true);
  TextStyle? b(TextStyle? s) => style(s, isHeading: false);
  return scaled.copyWith(
    displayLarge: h(scaled.displayLarge),
    displayMedium: h(scaled.displayMedium),
    displaySmall: h(scaled.displaySmall),
    headlineLarge: h(scaled.headlineLarge),
    headlineMedium: h(scaled.headlineMedium),
    headlineSmall: h(scaled.headlineSmall),
    titleLarge: h(scaled.titleLarge),
    titleMedium: h(scaled.titleMedium),
    titleSmall: h(scaled.titleSmall),
    bodyLarge: b(scaled.bodyLarge),
    bodyMedium: b(scaled.bodyMedium),
    bodySmall: b(scaled.bodySmall),
    labelLarge: b(scaled.labelLarge),
    labelMedium: b(scaled.labelMedium),
    labelSmall: b(scaled.labelSmall),
  );
}

SeddonColors _seddonColors(
    pb.SemanticColors s, ColorScheme cs, String mono, bool statusIcons,
    double textScale) {
  final fallback = SeddonColors.fallback(cs, monoFamily: mono);
  Color or(String v, Color d) => v.isEmpty ? d : _c(v);
  return SeddonColors(
    success: or(s.success, fallback.success),
    warning: or(s.warning, fallback.warning),
    info: or(s.info, fallback.info),
    muted: or(s.muted, fallback.muted),
    codeBackground: or(s.codeBackground, fallback.codeBackground),
    codeForeground: or(s.codeForeground, fallback.codeForeground),
    navTint: {for (final e in s.navTint.entries) e.key: _c(e.value)},
    graphEdge: {for (final e in s.graphEdge.entries) e.key: _c(e.value)},
    mono: TextStyle(
      fontFamily: mono,
      fontVariations: _variableFamilies.contains(mono)
          ? const [FontVariation('wght', 400)]
          : null,
    ),
    statusIcons: statusIcons,
    textScale: textScale,
  );
}

/// No page transition: `reduce_motion`.
class _NoTransition extends PageTransitionsBuilder {
  const _NoTransition();

  @override
  Widget buildTransitions<T>(PageRoute<T> route, BuildContext context,
          Animation<double> animation, Animation<double> secondaryAnimation,
          Widget child) =>
      child;
}
