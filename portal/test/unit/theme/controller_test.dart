import 'package:agent_portal/src/theme/theme_controller.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../testkit/themes.dart';

/// `ThemeController`: the saved choice is restored, a hostile saved value is
/// ignored, and picking a theme is remembered.
void main() {
  (ThemeController, Map<String, String>) make(Map<String, String> saved) {
    final mem = Map<String, String>.of(saved);
    final c = ThemeController(
      themes: builtinThemes(),
      store: ThemePrefsStore(read: (k) => mem[k], write: (k, v) => mem[k] = v),
      log: (_) {},
    );
    return (c, mem);
  }

  const theme = ThemePrefsStore.themeKey, mode = ThemePrefsStore.modeKey;

  final cases = <ThemeCase<Map<String, String>, (String, ThemeMode, bool)>>[
    const ThemeCase('positive_default', 'nothing saved: prototype, system mode, high contrast offered',
        {}, ('prototype', ThemeMode.system, true)),
    const ThemeCase('positive_restored', 'a saved theme and mode are restored',
        {theme: 'tn3270', mode: 'dark'}, ('tn3270', ThemeMode.dark, false)),
    const ThemeCase('negative_unknown_saved_id', 'a saved id that no longer exists falls back',
        {theme: 'gone'}, ('prototype', ThemeMode.system, true)),
    const ThemeCase('adversarial_traversal_saved_id', 'a hostile saved id is just unknown',
        {theme: '../../etc/passwd', mode: '<script>'}, ('prototype', ThemeMode.system, true)),
    ThemeCase('adversarial_huge_saved_value', 'a huge saved value is ignored',
        {theme: 'x' * 100000}, ('prototype', ThemeMode.system, true)),
  ];
  for (final c in cases) {
    test('${c.name}: ${c.description}', () {
      final (ctl, _) = make(c.input);
      final (id, m, hc) = c.expected;
      expect(ctl.active.id, id);
      expect(ctl.mode, m);
      expect(ctl.highContrastLight != null, hc);
    });
  }

  test('positive_select_persists: picking a theme applies and saves it', () {
    final (ctl, mem) = make({});
    var notified = 0;
    ctl.addListener(() => notified++);
    ctl.select('windows');
    expect(ctl.active.id, 'windows');
    expect(ctl.light.colorScheme.primary, const Color(0xFF0067C0));
    expect(mem[theme], 'windows');
    expect(ctl.highContrastLight, isNull, reason: 'the user chose a theme');
    expect(notified, 1);
  });

  test('negative_select_unknown: an unknown id changes nothing', () {
    final (ctl, mem) = make({});
    ctl.select('nope');
    expect(ctl.active.id, 'prototype');
    expect(mem.containsKey(theme), isFalse);
  });

  test('positive_mode_persists: the mode is saved by name', () {
    final (ctl, mem) = make({});
    ctl.setMode(ThemeMode.light);
    expect(ctl.mode, ThemeMode.light);
    expect(mem[mode], 'light');
  });
}
