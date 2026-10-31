import 'package:agent_portal/src/gen/agent/v1/theme.pb.dart' as pb;
import 'package:agent_portal/src/theme/builtin_themes.dart';
import 'package:agent_portal/src/theme/theme_controller.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../testkit/robots/theme_robot.dart';
import '../testkit/themes.dart';
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

        case 'positive_sign_in_loads_server_theme':
          final robot = ThemeRobot(tester);
          await robot.load();
          await robot.signIn(arrange: (s) => s.active = pb.ActiveTheme(
              theme: builtinTheme('tn3270'),
              mode: pb.ThemeMode.THEME_MODE_DARK,
              source: pb.ActiveSource.ACTIVE_SOURCE_USER));
          await tester.pumpAndSettle();
          expect(robot.themes!.active.id, 'tn3270');
          expect(robot.themes!.mode, ThemeMode.dark);
          expect(robot.rendered.scaffoldBackgroundColor, const Color(0xFF000000));
          expect(robot.calls, containsAll(['agent.v1.ThemeService/GetActive', 'agent.v1.ThemeService/List']));
          await robot.dispose();

        case 'positive_pick_saved_on_server':
          final robot = ThemeRobot(tester);
          await robot.load();
          await robot.signIn();
          await robot.pickCard('macos');
          await robot.waitFor('SetActive');
          final req = robot.gateway!.log.last('agent.v1.ThemeService/SetActive')!.request
              as pb.SetActiveThemeRequest;
          expect(req.themeId, 'macos');
          expect(req.mode, pb.ThemeMode.THEME_MODE_SYSTEM);
          expect(robot.themes!.active.id, 'macos');
          await robot.dispose();

        case 'positive_user_themes_listed':
          final robot = ThemeRobot(tester);
          await robot.load();
          await robot.signIn(arrange: (s) => s.listResponse = pb.ListThemesResponse(themes: [
                ...builtinThemes(),
                userTheme('ocean'),
                userTheme('team-dark', name: 'Team', visibility: pb.Visibility.VISIBILITY_TENANT),
              ]));
          await tester.pumpAndSettle();
          expect(robot.exists('theme.card.ocean'), isTrue);
          expect(robot.exists('theme.card.team-dark'), isTrue);
          expect(find.text('Private'), findsOneWidget);
          expect(find.text('Shared'), findsOneWidget);
          await robot.pickCard('ocean');
          expect(robot.themes!.active.id, 'ocean');
          await robot.waitFor('SetActive');
          await robot.dispose();

        case 'negative_server_down_keeps_cache':
          final robot = ThemeRobot(tester);
          final key = ThemePrefsStore.userKey('acme', 'alice');
          robot.saved[key] = '{"mode":"dark","theme":{"id":"windows"}}';
          await robot.load();
          await robot.signIn(
              arrange: (s) => s.failing['GetActive'] = const GrpcError.unavailable('agent down'));
          await tester.pumpAndSettle();
          expect(robot.themes!.active.id, 'windows');
          expect(robot.themes!.mode, ThemeMode.dark);
          await robot.dispose();

        case 'adversarial_hostile_server_theme_rejected':
          final robot = ThemeRobot(tester);
          await robot.load();
          final bad = userTheme('evil')..light.onSurface = 'red; background:url(x)';
          await robot.signIn(arrange: (s) {
            s.active = pb.ActiveTheme(theme: bad, source: pb.ActiveSource.ACTIVE_SOURCE_USER);
            s.listResponse = pb.ListThemesResponse(themes: [bad]);
          });
          await tester.pumpAndSettle();
          expect(robot.themes!.active.id, fallbackThemeId);
          expect(robot.exists('theme.card.evil'), isFalse);
          expect(tester.takeException(), isNull);
          await robot.dispose();

        case 'adversarial_listed_theme_cannot_replace_builtin':
          final robot = ThemeRobot(tester);
          await robot.load();
          await robot.signIn(arrange: (s) => s.listResponse = pb.ListThemesResponse(
              themes: [userTheme('prototype', name: 'Not the prototype')]));
          await tester.pumpAndSettle();
          expect(find.byKey(const Key('theme.card.prototype')), findsOneWidget);
          expect(find.text('Not the prototype'), findsNothing);
          await robot.dispose();

        default:
          fail('no test body for row ${row.label}');
      }
    });
  }
}
