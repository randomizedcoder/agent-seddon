// The theme rules, mirrored from the Rust validator (`crates/agent-theme`,
// docs/design/visual/06-security-and-validation.md). A theme is authored by
// another user, so the portal checks it again before applying it: values are
// rejected, never repaired. The colour and font rules share one case table with
// Rust (`test/themes/validation_cases.json`), so the two sides cannot drift.
import 'dart:math' as math;

import '../gen/agent/v1/theme.pb.dart' as pb;
import '../gen/agent/v1/theme.pbenum.dart' as pbe;

/// Why a theme was refused; the names match Rust's `ThemeErrorKind`.
enum ThemeErrorKind {
  tooLarge('TooLarge'),
  parse('Parse'),
  badId('BadId'),
  badName('BadName'),
  badColor('BadColor'),
  missing('Missing'),
  unknownKey('UnknownKey'),
  unknownFont('UnknownFont'),
  outOfRange('OutOfRange'),
  contrast('Contrast');

  const ThemeErrorKind(this.wire);

  /// The name in the shared case table and in Rust error messages.
  final String wire;
}

class ThemeError implements Exception {
  ThemeError(this.kind, this.message);

  final ThemeErrorKind kind;
  final String message;

  ThemeError at(String field) => ThemeError(kind, '$field: $message');

  @override
  String toString() => 'theme ${kind.wire}: $message';
}

/// The longest valid colour string (`#RRGGBBAA`).
const maxColorLen = 9;
const maxThemeBytes = 64 * 1024;
const maxNameChars = 64;
const maxDescriptionChars = 512;
const maxMapEntries = 16;
const maxIdLen = 64;

/// Bundled font families (`pubspec.yaml`) plus generic ones; same list as Rust.
const fontFamilies = [
  'SourceSerif4',
  'IBM3270',
  'OpenSans',
  'Inter',
  'AtkinsonHyperlegibleNext',
  'JetBrainsMono',
  'monospace',
  'sans-serif',
  'serif',
];

/// Nav-rail tab ids a `nav_tint` may name.
const navTabs = [
  'launch', 'prompts', 'graph', 'agent', 'router', 'fleet', 'forges', //
  'access', 'settings',
];

/// Graph edge kinds a `graph_edge` colour may name.
const edgeKinds = ['main', 'background', 'capability'];

final _hex = RegExp(r'^[0-9A-Fa-f]+$');

/// `#RRGGBB` or `#RRGGBBAA` → ARGB (`0xAARRGGBB`, Flutter's `Color(int)`).
int parseColor(String s) {
  if (s.isEmpty) throw ThemeError(ThemeErrorKind.missing, 'colour is empty');
  ThemeError bad() => ThemeError(
      ThemeErrorKind.badColor, '"${_preview(s)}" is not #RRGGBB or #RRGGBBAA');
  // Length first, so a huge string costs nothing. Dart counts UTF-16 units and
  // Rust bytes; both reject every non-ASCII string below on the hex check.
  if (s.length > maxColorLen || !s.startsWith('#')) throw bad();
  final hex = s.substring(1);
  if (!(hex.length == 6 || hex.length == 8) || !_hex.hasMatch(hex)) throw bad();
  final v = int.parse(hex, radix: 16);
  return hex.length == 6 ? 0xFF000000 | v : ((v & 0xFF) << 24) | (v >> 8);
}

/// A font family must be bundled or generic; empty means the theme default.
String checkFont(String family) {
  if (family.isEmpty || fontFamilies.contains(family)) return family;
  throw ThemeError(ThemeErrorKind.unknownFont,
      '"${_preview(family)}" is not a bundled font family');
}

double _channel(int argb, int shift) => ((argb >> shift) & 0xFF) / 255.0;

int _composite(int fg, int bg) {
  final a = _channel(fg, 24);
  int mix(int shift) =>
      ((_channel(fg, shift) * a + _channel(bg, shift) * (1 - a)) * 255)
          .round() <<
      shift;
  return 0xFF000000 | mix(16) | mix(8) | mix(0);
}

double _luminance(int argb) {
  double lin(double c) =>
      c <= 0.04045 ? c / 12.92 : math.pow((c + 0.055) / 1.055, 2.4).toDouble();
  return 0.2126 * lin(_channel(argb, 16)) +
      0.7152 * lin(_channel(argb, 8)) +
      0.0722 * lin(_channel(argb, 0));
}

/// WCAG 2.2 contrast of text [fg] on [bg]; a translucent [bg] is judged on
/// the worse of a white and a black backdrop, as in Rust.
double contrastRatio(int fg, int bg) {
  var worst = double.infinity;
  for (final backdrop in const [0xFFFFFFFF, 0xFF000000]) {
    final b = _composite(bg, backdrop);
    final la = _luminance(_composite(fg, b)), lb = _luminance(b);
    worst = math.min(worst, (math.max(la, lb) + 0.05) / (math.min(la, lb) + 0.05));
  }
  return worst;
}

String formatColor(int argb) => (argb >> 24) == 0xFF
    ? '#${(argb & 0xFFFFFF).toRadixString(16).padLeft(6, '0').toUpperCase()}'
    : '#${(((argb & 0xFFFFFF) << 8) | (argb >> 24)).toRadixString(16).padLeft(8, '0').toUpperCase()}';

class _Level {
  const _Level(this.name, this.text, this.accent, this.outline);
  final String name;
  final double text, accent;
  final double? outline;
}

const _aa = _Level('AA', 4.5, 3.0, null);
const _aaa = _Level('AAA', 7.0, 4.5, 3.0);
const _focus = 3.0;

/// Validate [t]; throws [ThemeError] naming the first rule it breaks.
void validateTheme(pb.Theme t) {
  final size = t.writeToBuffer().length;
  if (size > maxThemeBytes) {
    throw ThemeError(ThemeErrorKind.tooLarge, '$size bytes, cap $maxThemeBytes');
  }
  if (t.id.length > maxIdLen || !_safeSegment(t.id)) {
    throw ThemeError(ThemeErrorKind.badId,
        'id "${_preview(t.id)}" must be 1-$maxIdLen of [A-Za-z0-9._-], not starting with "-"');
  }
  _checkText('name', t.name, 1, maxNameChars);
  _checkText('author', t.author, 0, maxNameChars);
  _checkText('description', t.description, 0, maxDescriptionChars);
  final level = _level(t);
  if (!t.hasLight() && !t.hasDark()) {
    throw ThemeError(
        ThemeErrorKind.missing, 'a theme needs a light or a dark palette');
  }
  for (final (name, has, p) in [
    ('light', t.hasLight(), t.light),
    ('dark', t.hasDark(), t.dark),
  ]) {
    if (!has) continue;
    try {
      _checkPalette(p, level);
    } on ThemeError catch (e) {
      throw e.at(name);
    }
  }
  if (t.hasTypography()) {
    try {
      _checkTypography(t.typography);
    } on ThemeError catch (e) {
      throw e.at('typography');
    }
  }
  if (t.hasShape()) {
    _range('shape.corner_radius', t.shape.cornerRadius, 0, 32);
    _range('shape.border_width', t.shape.borderWidth, 0, 4);
  }
  // Enum ranges are Rust-only: Dart decodes an unknown enum number to the
  // default value, which the mapper treats as unset.
  if (t.hasAccessibility()) {
    _optionalRange(
        'accessibility.focus_ring_width', t.accessibility.focusRingWidth, 1, 6);
    _optionalRange(
        'accessibility.min_target_size', t.accessibility.minTargetSize, 40, 64);
  }
}

_Level _level(pb.Theme t) =>
    t.hasAccessibility() &&
            t.accessibility.contrast == pbe.ContrastLevel.CONTRAST_LEVEL_AAA
        ? _aaa
        : _aa;

void _checkPalette(pb.Palette p, _Level level) {
  int? opt(String field, String v) {
    if (v.isEmpty) return null;
    try {
      return parseColor(v);
    } on ThemeError catch (e) {
      throw e.at(field);
    }
  }

  final seed = opt('seed', p.seed);
  final c = <String, int>{};
  for (final (field, v) in [
    ('primary', p.primary),
    ('on_primary', p.onPrimary),
    ('secondary', p.secondary),
    ('on_secondary', p.onSecondary),
    ('surface', p.surface),
    ('on_surface', p.onSurface),
    ('background', p.background),
    ('surface_variant', p.surfaceVariant),
    ('on_surface_variant', p.onSurfaceVariant),
    ('outline', p.outline),
    ('error', p.error),
    ('on_error', p.onError),
    ('focus', p.focus),
  ]) {
    final argb = opt(field, v);
    if (argb != null) {
      c[field] = argb;
    } else if (seed == null) {
      throw ThemeError(
          ThemeErrorKind.missing, '$field is required without a seed');
    }
  }
  final sem = p.semantic;
  for (final (field, v) in [
    ('success', sem.success),
    ('warning', sem.warning),
    ('info', sem.info),
    ('muted', sem.muted),
    ('code_background', sem.codeBackground),
    ('code_foreground', sem.codeForeground),
  ]) {
    final argb = opt(field, v);
    if (argb != null) c[field] = argb;
  }
  _checkMap('semantic.nav_tint', sem.navTint, navTabs);
  _checkMap('semantic.graph_edge', sem.graphEdge, edgeKinds);

  // As in Rust: a pair is checked when both colours are written down, and a
  // core pair may not be half written (a hand-picked half against a
  // seed-derived half is a pair nobody checked). Rendered text is also covered
  // by the widget tests' `textContrastGuideline`.
  for (final (fg, bg) in _corePairs) {
    if (c.containsKey(fg) != c.containsKey(bg)) {
      final (set, unset) = c.containsKey(fg) ? (fg, bg) : (bg, fg);
      throw ThemeError(ThemeErrorKind.missing,
          '$set is set without $unset: set both or neither, so their contrast can be checked');
    }
  }
  final pairs = _pairsFor(level);
  for (final (fg, bg, min) in pairs) {
    final f = c[fg], b = c[bg];
    if (f == null || b == null) continue;
    final ratio = contrastRatio(f, b);
    if (ratio < min) {
      final shown = ((ratio * 100).floor() / 100).toStringAsFixed(2);
      throw ThemeError(
          ThemeErrorKind.contrast,
          'contrast: $fg ${formatColor(f)} on $bg ${formatColor(b)} is '
          '$shown:1, needs $min:1 (${level.name})');
    }
  }
}

/// Every colour pair whose contrast [validateTheme] checks, at [level]: text
/// pairs at the level's text ratio, the accent and outline at their UI ratios,
/// the focus ring at 3:1.
List<(String, String, double)> _pairsFor(_Level level) => [
      ('on_surface', 'surface', level.text),
      ('on_surface', 'background', level.text),
      ('on_surface_variant', 'surface_variant', level.text),
      ('on_primary', 'primary', level.text),
      ('on_secondary', 'secondary', level.text),
      ('on_error', 'error', level.text),
      ('error', 'surface', level.text),
      ('success', 'surface', level.text),
      ('warning', 'surface', level.text),
      ('info', 'surface', level.text),
      ('muted', 'surface', level.text),
      ('code_foreground', 'code_background', level.text),
      ('primary', 'surface', level.accent),
      ('focus', 'background', _focus),
      if (level.outline != null) ('outline', 'background', level.outline!),
    ];

/// One pair the validator checks, as the theme editor shows it.
class ContrastCheck {
  const ContrastCheck(this.fg, this.bg, this.min, this.ratio, this.level);

  final String fg, bg;

  /// The ratio the pair needs.
  final double min;

  /// The pair's ratio; null while either colour is unset or not a colour.
  final double? ratio;

  /// `AA` or `AAA`: the theme's contrast level.
  final String level;

  bool get passes => ratio != null && ratio! >= min;

  /// The ratio as the validator prints it: truncated, never rounded up to a pass.
  String get shown => ratio == null ? '–' : '${((ratio! * 100).floor() / 100).toStringAsFixed(2)}:1';
}

/// The contrast of each pair [validateTheme] checks in [p], a palette of [t]
/// (whose accessibility block sets the level). Unlike the validator it never
/// throws: a colour that is unset or does not parse gives a null ratio, so an
/// editor can show every pair while the author types.
List<ContrastCheck> paletteContrast(pb.Theme t, pb.Palette p) {
  final level = _level(t);
  int? colour(String v) {
    if (v.isEmpty) return null;
    try {
      return parseColor(v);
    } on ThemeError {
      return null;
    }
  }

  final sem = p.semantic;
  final c = <String, int?>{
    'primary': colour(p.primary),
    'on_primary': colour(p.onPrimary),
    'secondary': colour(p.secondary),
    'on_secondary': colour(p.onSecondary),
    'surface': colour(p.surface),
    'on_surface': colour(p.onSurface),
    'background': colour(p.background),
    'surface_variant': colour(p.surfaceVariant),
    'on_surface_variant': colour(p.onSurfaceVariant),
    'outline': colour(p.outline),
    'error': colour(p.error),
    'on_error': colour(p.onError),
    'focus': colour(p.focus),
    'success': colour(sem.success),
    'warning': colour(sem.warning),
    'info': colour(sem.info),
    'muted': colour(sem.muted),
    'code_background': colour(sem.codeBackground),
    'code_foreground': colour(sem.codeForeground),
  };
  return [
    for (final (fg, bg, min) in _pairsFor(level))
      ContrastCheck(
        fg,
        bg,
        min,
        c[fg] == null || c[bg] == null ? null : contrastRatio(c[fg]!, c[bg]!),
        level.name,
      ),
  ];
}

void _checkMap(String field, Map<String, String> map, List<String> allowed) {
  if (map.length > maxMapEntries) {
    throw ThemeError(ThemeErrorKind.unknownKey,
        '$field: ${map.length} entries, cap $maxMapEntries');
  }
  map.forEach((k, v) {
    if (!allowed.contains(k)) {
      throw ThemeError(ThemeErrorKind.unknownKey,
          '$field: unknown key "${_preview(k)}" (allowed: ${allowed.join(', ')})');
    }
    try {
      parseColor(v);
    } on ThemeError catch (e) {
      throw e.at('$field[$k]');
    }
  });
}

void _checkTypography(pb.Typography t) {
  for (final (field, family) in [
    ('body_family', t.bodyFamily),
    ('heading_family', t.headingFamily),
    ('mono_family', t.monoFamily),
  ]) {
    try {
      checkFont(family);
    } on ThemeError catch (e) {
      throw e.at(field);
    }
  }
  final s = t.scale;
  if (s != 0 && !(s.isFinite && s >= 0.75 && s <= 1.5)) {
    throw ThemeError(ThemeErrorKind.outOfRange, 'scale $s not in 0.75-1.5');
  }
  final ls = t.letterSpacing;
  if (!(ls.isFinite && ls >= -1.0 && ls <= 2.0)) {
    throw ThemeError(
        ThemeErrorKind.outOfRange, 'letter_spacing $ls not in -1.0-2.0');
  }
  for (final (field, w) in [
    ('body_weight', t.bodyWeight),
    ('heading_weight', t.headingWeight),
  ]) {
    if (w != 0 && !(w >= 100 && w <= 900 && w % 100 == 0)) {
      throw ThemeError(
          ThemeErrorKind.outOfRange, '$field $w not in 100-900 step 100');
    }
  }
}

final _segment = RegExp(r'^[A-Za-z0-9._-]+$');

bool _safeSegment(String s) =>
    s.isNotEmpty &&
    s != '.' &&
    s != '..' &&
    !s.startsWith('-') &&
    _segment.hasMatch(s);

// Every Unicode Cc, Cf, Zl and Zp code point, the same list as Rust's
// `is_control || is_invisible`: C0, DEL, C1, bidi controls, zero-width
// characters, BOM, soft hyphen, tags. Each can make a name read differently from
// what it is (Trojan Source) or hide in a lookalike.
final _control = RegExp(
    r'[\x00-\x1F\x7F-\x9F\u00AD\u0600-\u0605\u061C\u06DD\u070F'
    r'\u0890-\u0891\u08E2\u180E\u200B-\u200F\u2028-\u202E\u2060-\u2064'
    r'\u2066-\u206F\uFEFF\uFFF9-\uFFFB\u{110BD}\u{110CD}\u{13430}-\u{1343F}'
    r'\u{1BCA0}-\u{1BCA3}\u{1D173}-\u{1D17A}\u{E0001}\u{E0020}-\u{E007F}]',
    unicode: true);

/// Pairs written in full or not at all (Rust's `CORE_PAIRS`).
const _corePairs = [
  ('on_surface', 'surface'),
  ('on_surface', 'background'),
  ('on_surface_variant', 'surface_variant'),
  ('on_primary', 'primary'),
  ('on_secondary', 'secondary'),
  ('on_error', 'error'),
  ('code_foreground', 'code_background'),
  ('focus', 'background'),
];

void _checkText(String field, String s, int min, int max) {
  final n = s.runes.length;
  if (n < min || n > max) {
    throw ThemeError(
        ThemeErrorKind.badName, '$field is $n chars, allowed $min-$max');
  }
  final m = _control.firstMatch(s);
  if (m != null) {
    final code = m.group(0)!.codeUnitAt(0).toRadixString(16).toUpperCase();
    throw ThemeError(ThemeErrorKind.badName,
        '$field contains control character U+${code.padLeft(4, '0')}');
  }
}

void _range(String field, int v, int min, int max) {
  if (v < min || v > max) {
    throw ThemeError(ThemeErrorKind.outOfRange, '$field $v not in $min-$max');
  }
}

void _optionalRange(String field, int v, int min, int max) {
  if (v != 0) _range(field, v, min, max);
}

String _preview(String s) =>
    s.runes.length <= 16 ? s : '${String.fromCharCodes(s.runes.take(16))}…';
