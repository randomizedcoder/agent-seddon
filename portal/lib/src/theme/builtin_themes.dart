// The built-in themes, bundled as proto3 JSON in `assets/themes/` (generated
// from `config/themes/*.textproto`; agent-theme's parity test keeps them equal).
import 'dart:convert';

import 'package:flutter/services.dart';

import '../gen/agent/v1/theme.pb.dart' as pb;
import 'theme_validate.dart';

/// Picker order. `prototype` is the fallback whenever a theme cannot be used.
const builtinThemeIds = ['prototype', 'tn3270', 'windows', 'macos', 'accessible'];
const fallbackThemeId = 'prototype';

/// A theme from proto3 JSON. Malformed JSON, a wrong shape or an unknown field
/// is a [ThemeErrorKind.parse] error, never a crash: cached or user-authored
/// JSON is untrusted.
pb.Theme parseThemeJson(String text) {
  if (text.length > maxThemeBytes) {
    throw ThemeError(ThemeErrorKind.tooLarge,
        '${text.length} characters, cap $maxThemeBytes');
  }
  try {
    final json = jsonDecode(text);
    if (json is! Map<String, dynamic>) {
      throw const FormatException('not a JSON object');
    }
    return pb.Theme()..mergeFromProto3Json(json);
  } catch (e) {
    throw ThemeError(ThemeErrorKind.parse, '$e');
  }
}

/// Every built-in, parsed and validated, with `builtin` set. A broken asset is
/// a build bug (the Rust and Dart tests both load them), so it throws.
Future<List<pb.Theme>> loadBuiltinThemes(AssetBundle bundle) async => [
      for (final id in builtinThemeIds)
        _builtin(await bundle.loadString('assets/themes/$id.json')),
    ];

pb.Theme _builtin(String text) {
  final t = parseThemeJson(text)..builtin = true;
  validateTheme(t);
  return t;
}
