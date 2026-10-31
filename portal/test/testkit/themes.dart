import 'dart:io';

import 'package:agent_portal/src/gen/agent/v1/theme.pb.dart' as pb;
import 'package:agent_portal/src/theme/builtin_themes.dart';
import 'package:agent_portal/src/theme/theme_controller.dart';
import 'package:agent_portal/src/theme/theme_mapper.dart';
import 'package:fixnum/fixnum.dart';
import 'package:flutter/material.dart';

/// The built-in themes read straight from `assets/themes/` (the test's cwd is
/// `portal/`), so a test needs no asset bundle.
pb.Theme builtinTheme(String id) {
  final t = parseThemeJson(File('assets/themes/$id.json').readAsStringSync());
  return t..builtin = true;
}

List<pb.Theme> builtinThemes() => [for (final id in builtinThemeIds) builtinTheme(id)];

/// [id] mapped to `ThemeData` for [b].
ThemeData builtinThemeData(String id, [Brightness b = Brightness.light]) =>
    themeToThemeData(builtinTheme(id), b);

/// One row of a theme table: the name carries the class prefix, the
/// description reads as a sentence in the test name (visual/07-testing.md).
class ThemeCase<I, O> {
  const ThemeCase(this.name, this.description, this.input, this.expected);
  final String name, description;
  final I input;
  final O expected;
}

/// A [ThemeController] over the built-ins, saving into [saved] (in memory).
ThemeController memoryThemes([Map<String, String>? saved]) {
  final mem = saved ?? <String, String>{};
  return ThemeController(
    themes: builtinThemes(),
    store: ThemePrefsStore(read: (k) => mem[k], write: (k, v) => mem[k] = v),
    log: (_) {},
  );
}

/// A user theme: a renamed copy of the Windows built-in, as `ThemeService.List`
/// would return it.
pb.Theme userTheme(String id,
        {String name = 'Ocean',
        pb.Visibility visibility = pb.Visibility.VISIBILITY_PRIVATE}) =>
    builtinTheme('windows').deepCopy()
      ..id = id
      ..name = name
      ..builtin = false
      ..owner = 'alice'
      ..version = Int64(1)
      ..visibility = visibility;
