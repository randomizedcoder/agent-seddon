// The colours a page needs beyond Material's `ColorScheme`, named by meaning
// (docs/design/visual/01-theme-model.md `SemanticColors`). Pages read them with
// `SeddonColors.of(context)` instead of hardcoding `Colors.green`, so every
// theme reaches every pixel.
import 'package:flutter/material.dart';

class SeddonColors extends ThemeExtension<SeddonColors> {
  const SeddonColors({
    required this.success,
    required this.warning,
    required this.info,
    required this.muted,
    required this.codeBackground,
    required this.codeForeground,
    required this.navTint,
    required this.graphEdge,
    required this.mono,
    required this.statusIcons,
    this.textScale = 1.0,
  });

  /// Posted reviews, healthy upstreams, a passing check.
  final Color success;

  /// Drafted reviews, unsaved changes, degraded.
  final Color warning;

  /// Approved reviews.
  final Color info;

  /// Superseded, disabled, unknown.
  final Color muted;
  final Color codeBackground;
  final Color codeForeground;

  /// Per nav-tab tint, keyed by tab id (`launch`, `fleet`, …); see [nav].
  final Map<String, Color> navTint;

  /// Per graph edge kind (`main`, `background`, `capability`); see [edge].
  final Map<String, Color> graphEdge;

  /// Code, logs and diffs: the theme's mono family.
  final TextStyle mono;

  /// Always pair a status colour with an icon and a label.
  final bool statusIcons;

  /// The theme's `typography.scale`. [sp] applies it to the few sizes a page
  /// sets by hand (chips, code panes, painter labels), which `TextTheme`
  /// scaling never reaches.
  final double textScale;

  /// [px] at the theme's text scale.
  double sp(double px) => px * textScale;

  /// Whether lists may be `dense`. A dense `ListTile` pins its text at 13/12px
  /// whatever the theme says, so a theme that enlarges text gets full tiles.
  bool get denseLists => textScale <= 1.1;

  /// The tints for a scheme with no semantic colours: every role falls back to
  /// a `ColorScheme` role, so a theme that names none still renders sensibly.
  factory SeddonColors.fallback(ColorScheme cs, {String monoFamily = 'monospace'}) =>
      SeddonColors(
        success: cs.primary,
        warning: cs.tertiary,
        info: cs.secondary,
        muted: cs.onSurfaceVariant,
        codeBackground: cs.surfaceContainerHighest,
        codeForeground: cs.onSurface,
        navTint: const {},
        graphEdge: const {},
        mono: TextStyle(fontFamily: monoFamily),
        statusIcons: false,
      );

  /// The theme's colours, or [SeddonColors.fallback] when the `ThemeData` has
  /// no extension (tests that pump a page under a plain `ThemeData`).
  static SeddonColors of(BuildContext context) {
    final theme = Theme.of(context);
    return theme.extension<SeddonColors>() ??
        SeddonColors.fallback(theme.colorScheme);
  }

  /// The nav tint for [tab], or [fallback] (the scheme's primary).
  Color nav(String tab, Color fallback) => navTint[tab] ?? fallback;

  /// The colour of a graph edge of [kind], or [fallback].
  Color edge(String kind, Color fallback) => graphEdge[kind] ?? fallback;

  @override
  SeddonColors copyWith({
    Color? success,
    Color? warning,
    Color? info,
    Color? muted,
    Color? codeBackground,
    Color? codeForeground,
    Map<String, Color>? navTint,
    Map<String, Color>? graphEdge,
    TextStyle? mono,
    bool? statusIcons,
    double? textScale,
  }) =>
      SeddonColors(
        success: success ?? this.success,
        warning: warning ?? this.warning,
        info: info ?? this.info,
        muted: muted ?? this.muted,
        codeBackground: codeBackground ?? this.codeBackground,
        codeForeground: codeForeground ?? this.codeForeground,
        navTint: navTint ?? this.navTint,
        graphEdge: graphEdge ?? this.graphEdge,
        mono: mono ?? this.mono,
        statusIcons: statusIcons ?? this.statusIcons,
        textScale: textScale ?? this.textScale,
      );

  @override
  SeddonColors lerp(SeddonColors? other, double t) {
    if (other == null) return this;
    Color mix(Color a, Color b) => Color.lerp(a, b, t)!;
    final near = t < 0.5 ? this : other;
    return SeddonColors(
      success: mix(success, other.success),
      warning: mix(warning, other.warning),
      info: mix(info, other.info),
      muted: mix(muted, other.muted),
      codeBackground: mix(codeBackground, other.codeBackground),
      codeForeground: mix(codeForeground, other.codeForeground),
      navTint: near.navTint,
      graphEdge: near.graphEdge,
      mono: TextStyle.lerp(mono, other.mono, t)!,
      statusIcons: near.statusIcons,
      textScale: near.textScale,
    );
  }
}
