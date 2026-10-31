import 'package:agent_portal/src/theme/builtin_themes.dart';
import 'package:agent_portal/src/theme/theme_controller.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../testkit/robots/theme_robot.dart';
import 'theme_spec.dart';

/// Layer-A widget tests for the theme pickers — iterates [themeSpec]; each row is
/// one test.
void main() {
  for (final row in themeSpec.rows) {
    testWidgets('theme ${row.label} — ${row.description}', (tester) async {
      switch (row.label) {
        case 'positive_menu_selects_theme':
          final robot = ThemeRobot(tester);
          await robot.load();
          await robot.pickFromMenu('windows');
          expect(robot.themes!.active.id, 'windows');
          expect(robot.saved[ThemePrefsStore.themeKey], 'windows');
          expect(robot.rendered.colorScheme.primary, const Color(0xFF0067C0));

        case 'negative_no_scope_renders_nothing':
          final robot = ThemeRobot(tester, scoped: false);
          await robot.load();
          expect(robot.exists('theme.menu'), isFalse);
          expect(robot.exists('settings.appearance'), isFalse);
          expect(tester.takeException(), isNull);

        case 'positive_card_selects_theme':
          final robot = ThemeRobot(tester);
          await robot.load();
          await robot.pickCard('tn3270');
          expect(robot.themes!.active.id, 'tn3270');
          expect(robot.rendered.brightness, Brightness.dark);
          expect(robot.rendered.scaffoldBackgroundColor, const Color(0xFF000000));

        case 'corner_selected_card_is_announced':
          final robot = ThemeRobot(tester);
          final handle = tester.ensureSemantics();
          await robot.load();
          expect(tester.getSemantics(find.bySemanticsLabel('Prototype theme')),
              isSemantics(isButton: true, isSelected: true, hasTapAction: true));
          expect(tester.getSemantics(find.bySemanticsLabel('Windows theme')),
              isSemantics(isButton: true, isSelected: false));
          handle.dispose();

        case 'boundary_every_builtin_has_a_card':
          final robot = ThemeRobot(tester);
          await robot.load();
          final ids = [
            for (final e in find.byWidgetPredicate((w) =>
                    w.key is ValueKey<String> &&
                    (w.key! as ValueKey<String>).value.startsWith('theme.card.'))
                .evaluate())
              (e.widget.key! as ValueKey<String>).value.substring('theme.card.'.length),
          ];
          expect(ids, builtinThemeIds);

        case 'adversarial_hostile_saved_id_ignored':
          final robot =
              ThemeRobot(tester, saved: {ThemePrefsStore.themeKey: '../../etc/passwd'});
          await robot.load();
          expect(robot.themes!.active.id, fallbackThemeId);
          expect(tester.takeException(), isNull);

        default:
          fail('no test body for row ${row.label}');
      }
    });
  }
}
