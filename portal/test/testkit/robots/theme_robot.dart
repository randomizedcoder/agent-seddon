import 'package:agent_portal/src/theme/theme_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../themes.dart';
import 'robot.dart';

/// Robot for the theme pickers (visual/05-portal-integration.md): the nav-rail
/// menu and the Appearance cards, pumped together under a [memoryThemes]
/// controller whose saved values land in [saved]. No backend until increment 3.
class ThemeRobot extends Robot {
  ThemeRobot(super.tester, {Map<String, String>? saved, bool scoped = true})
      : saved = saved ?? <String, String>{} {
    if (scoped) themes = memoryThemes(this.saved);
  }

  final Map<String, String> saved;

  Future<void> load() => pumpPage(const SingleChildScrollView(
        child: Column(children: [ThemeMenuButton(), AppearanceSection()]),
      ));

  Future<void> pickFromMenu(String id) async {
    await tester.tap(byKey('theme.menu'));
    await tester.pumpAndSettle();
    await tester.tap(byKey('theme.menu.$id'));
    await tester.pumpAndSettle();
  }

  Future<void> pickCard(String id) async {
    await tester.tap(byKey('theme.card.$id'));
    await tester.pumpAndSettle();
  }

  /// The theme the app is rendering now.
  ThemeData get rendered => Theme.of(tester.element(byKey('settings.appearance')));
}
