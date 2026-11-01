import 'package:agent_portal/src/theme/theme_editor.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'theme_robot.dart';

/// Robot for the theme editor (visual/03-user-themes.md#authoring): the
/// [ThemeEditorButton] under a [memoryThemes] controller, signed in to a
/// `FakeThemeService` through [signIn] as the theme picker robot does.
class ThemeEditorRobot extends ThemeRobot {
  ThemeEditorRobot(super.tester) {
    // Tall enough that the whole form and the preview are on screen.
    tester.view.physicalSize = const Size(1600, 2400);
  }

  @override
  Future<void> load() => pumpPage(const Center(child: ThemeEditorButton()));

  Future<void> open() async {
    await tester.tap(byKey('theme_editor.open'));
    await tester.pumpAndSettle();
  }

  /// The text field inside the keyed box [key].
  Finder field(String key) =>
      find.descendant(of: byKey(key), matching: find.byType(EditableText));

  Future<void> type(String key, String text) async {
    await tester.enterText(field(key), text);
    await tester.pump();
  }

  Future<void> colour(String name, String hex) => type('theme_editor.color.$name', hex);

  /// Tap [key] and let any RPC it fires answer, until [until] holds.
  Future<void> press(String key, {bool Function()? until}) async {
    await tester.ensureVisible(byKey(key));
    await tester.pump();
    await tap(key, until: until);
    await tester.pump();
  }

  Future<void> pressSegment(String label) async {
    await tester.tap(find.text(label));
    await tester.pump();
  }

  bool enabled(String key) {
    final w = tester.widget(byKey(key));
    return switch (w) {
      ButtonStyleButton b => b.onPressed != null,
      IconButton b => b.onPressed != null,
      _ => throw StateError('$key is not a button'),
    };
  }

  /// The text of the problem banner.
  String get problem => tester
      .widgetList<Text>(find.descendant(of: byKey('theme_editor.problem'), matching: find.byType(Text)))
      .map((t) => t.data ?? '')
      .join(' ');

  /// The ThemeData the preview sample is painted in.
  ThemeData get previewTheme => Theme.of(tester.element(find.descendant(
      of: byKey('theme_editor.preview'), matching: find.byType(Material)).first));

  /// The label of the contrast badge for [fg] on [bg].
  String badge(String fg, String bg) {
    final chip = tester.widget<Chip>(byKey('theme_editor.contrast.$fg.$bg'));
    return (chip.label as Text).data ?? '';
  }

  bool snack(String text) => find.textContaining(text).evaluate().isNotEmpty;
}
