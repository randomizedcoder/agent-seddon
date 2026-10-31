import 'package:agent_portal/src/gen/agent/v1/theme.pb.dart' as pb;
import 'package:agent_portal/src/theme/theme_validate.dart';
import 'package:fixnum/fixnum.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../testkit/fakes/theme_service.dart';
import '../testkit/robots/theme_editor_robot.dart';
import '../testkit/themes.dart';
import 'theme_editor_spec.dart';

const _svc = 'agent.v1.ThemeService';

/// The request stored, as the server answers it.
pb.Theme _stored(pb.Theme r) => r.deepCopy()
  ..version = Int64(r.version.toInt() + 1)
  ..owner = 'alice';

/// Layer-A widget tests for the theme editor — iterates [themeEditorSpec]; each
/// row is one test.
void main() {
  for (final row in themeEditorSpec.rows) {
    testWidgets('theme_editor ${row.label} — ${row.description}', (tester) async {
      final robot = ThemeEditorRobot(tester);
      addTearDown(robot.dispose);

      /// Signed in, the built-in [base] active, the editor open on a copy.
      Future<void> openCopyOf(String base, {void Function(FakeThemeService)? arrange}) async {
        await robot.load();
        await robot.signIn(arrange: (s) {
          s.active = pb.ActiveTheme(
              theme: builtinTheme(base), source: pb.ActiveSource.ACTIVE_SOURCE_USER);
          s.putAnswer = _stored;
          arrange?.call(s);
        });
        await tester.pumpAndSettle();
        expect(robot.themes!.active.id, base);
        await robot.open();
      }

      /// Signed in with the user theme `ocean` active, the editor open on it.
      Future<void> openUserTheme() async {
        final ocean = userTheme('ocean');
        await robot.load();
        await robot.signIn(arrange: (s) {
          s.listResponse = pb.ListThemesResponse(themes: [ocean]);
          s.active = pb.ActiveTheme(theme: ocean, source: pb.ActiveSource.ACTIVE_SOURCE_USER);
        });
        await tester.pumpAndSettle();
        await robot.open();
      }

      bool fired(String m) => robot.gateway!.log.fired('$_svc/$m');
      pb.Theme putRequest() => robot.gateway!.log.last('$_svc/Put')!.request as pb.Theme;

      switch (row.label) {
        case 'positive_open_clones_builtin':
          await openCopyOf('prototype');
          expect(find.text('Edit a copy'), findsNothing, reason: 'the editor replaced the page');
          expect(robot.tester.widget<EditableText>(robot.field('theme_editor.id')).controller.text,
              'prototype-copy');
          expect(find.textContaining('Prototype (copy)'), findsWidgets);
          expect(robot.exists('theme_editor.delete'), isFalse, reason: 'nothing stored yet');

        case 'positive_name_edit_shows_in_title':
          await openCopyOf('windows');
          await robot.type('theme_editor.name', 'Harbour');
          expect(find.text('Theme editor — Harbour'), findsOneWidget);

        case 'positive_id_edit_is_saved':
          await openCopyOf('windows');
          await robot.type('theme_editor.id', 'harbour');
          await robot.press('theme_editor.save', until: () => fired('SetActive'));
          expect(putRequest().id, 'harbour');
          expect(robot.themes!.active.id, 'harbour');

        case 'positive_colour_edit_repaints_preview':
          await openCopyOf('windows');
          await robot.colour('primary', '#AA0000');
          expect(robot.previewTheme.colorScheme.primary, const Color(0xFFAA0000));
          expect(robot.themes!.light.colorScheme.primary, const Color(0xFF0067C0),
              reason: 'the app keeps its theme until Save');

        case 'positive_badges_show_ratios':
          await openCopyOf('windows');
          final badges = find.byWidgetPredicate((w) =>
              w.key is ValueKey<String> &&
              (w.key! as ValueKey<String>).value.startsWith('theme_editor.contrast.'));
          final windows = builtinTheme('windows');
          expect(badges.evaluate().length, paletteContrast(windows, windows.light).length);
          expect(robot.badge('on_surface', 'surface'), allOf(contains(':1'), endsWith('pass')));

        case 'positive_problem_names_the_rule':
          await openCopyOf('windows');
          expect(robot.problem, 'Ready to save');
          await robot.colour('on_surface', '#888888');
          expect(robot.badge('on_surface', 'surface'), endsWith('fail'));
          expect(robot.problem, contains('contrast'));
          expect(robot.problem, contains('on_surface'));
          expect(robot.enabled('theme_editor.save'), isFalse);

        case 'positive_save_puts_and_switches':
          await openCopyOf('windows');
          await robot.press('theme_editor.save', until: () => fired('SetActive'));
          expect(putRequest().id, 'windows-copy');
          expect(putRequest().builtin, isFalse);
          expect(robot.themes!.active.id, 'windows-copy');
          expect(robot.themes!.themes.map((t) => t.id), contains('windows-copy'));
          expect(robot.snack('Saved'), isTrue);
          expect(robot.exists('theme_editor.delete'), isTrue, reason: 'now stored');

        case 'positive_share_sends_tenant_visibility':
          await openCopyOf('windows');
          await robot.pressSegment('Shared with the tenant');
          await robot.press('theme_editor.save', until: () => fired('Put'));
          expect(putRequest().visibility, pb.Visibility.VISIBILITY_TENANT);

        case 'positive_dark_side_edits_dark_palette':
          await openCopyOf('windows');
          await robot.pressSegment('Dark');
          await robot.colour('primary', '#123456');
          expect(robot.previewTheme.brightness, Brightness.dark);
          expect(robot.previewTheme.colorScheme.primary, const Color(0xFF123456));
          await robot.pressSegment('Light');
          expect(robot.previewTheme.colorScheme.primary, const Color(0xFF0067C0));

        case 'positive_add_light_palette_to_tn3270_copy':
          await openCopyOf('tn3270');
          expect(robot.previewTheme.brightness, Brightness.dark, reason: 'opens on its only palette');
          await robot.pressSegment('Light');
          expect(robot.exists('theme_editor.color.primary'), isFalse);
          await robot.press('theme_editor.add_palette');
          expect(robot.exists('theme_editor.color.background'), isTrue);
          expect(
              tester.widget<EditableText>(robot.field('theme_editor.color.background')).controller.text,
              builtinTheme('tn3270').dark.background);

        case 'positive_remove_one_palette':
          await openCopyOf('windows');
          await robot.pressSegment('Dark');
          await robot.press('theme_editor.remove_palette');
          expect(robot.exists('theme_editor.remove_palette'), isFalse, reason: 'one palette left');
          expect(robot.previewTheme.brightness, Brightness.light);
          expect(robot.problem, 'Ready to save');

        case 'positive_font_pick_sets_body_family':
          await openCopyOf('windows');
          await tester.ensureVisible(robot.byKey('theme_editor.font.body'));
          await tester.tap(robot.byKey('theme_editor.font.body'));
          await tester.pumpAndSettle();
          // Hit-testable: the open menu's item, not a closed dropdown's hidden copy.
          await tester.tap(find.text('AtkinsonHyperlegibleNext').hitTestable().first);
          await tester.pumpAndSettle();
          expect(robot.previewTheme.textTheme.bodyMedium!.fontFamily, 'AtkinsonHyperlegibleNext');

        case 'positive_preview_uses_the_draft':
          await openCopyOf('tn3270');
          expect(robot.previewTheme.scaffoldBackgroundColor, const Color(0xFF000000));
          robot.themes!.select('windows');
          await tester.pumpAndSettle();
          expect(robot.previewTheme.scaffoldBackgroundColor, const Color(0xFF000000),
              reason: 'the app theme changing does not change the draft');

        case 'positive_import_loads_the_draft':
          await openCopyOf('windows', arrange: (s) {
            s.importResponse = pb.ImportThemeResponse(
                theme: userTheme('sea', name: 'Sea')
                  ..clearVersion()
                  ..clearOwner());
          });
          await robot.press('theme_editor.import');
          await tester.pumpAndSettle();
          await robot.type('theme_editor.import.text', 'id: "sea"\nname: "Sea"');
          await robot.press('theme_editor.import.ok', until: () => robot.snack('Imported'));
          expect(
              (robot.gateway!.log.last('$_svc/Import')!.request as pb.ImportThemeRequest).text,
              startsWith('id: "sea"'));
          expect(find.text('Theme editor — Sea'), findsOneWidget);
          expect(robot.problem, 'Ready to save');

        case 'positive_export_shows_text':
          await openCopyOf('windows', arrange: (s) {
            s.exportResponse = pb.ExportThemeResponse(text: 'id: "windows-copy"\nname: "Windows (copy)"\n');
          });
          await robot.press('theme_editor.export',
              until: () => robot.exists('theme_editor.export.text'));
          await tester.pumpAndSettle();
          final sent = robot.gateway!.log.last('$_svc/Export')!.request as pb.ExportThemeRequest;
          expect(sent.theme.id, 'windows-copy');
          expect(find.textContaining('name: "Windows (copy)"'), findsOneWidget);
          expect(robot.exists('theme_editor.export.copy'), isTrue);

        case 'positive_delete_removes_user_theme':
          await openUserTheme();
          await robot.press('theme_editor.delete');
          await tester.pumpAndSettle();
          await robot.press('theme_editor.delete.confirm', until: () => fired('SetActive'));
          await tester.pumpAndSettle();
          expect((robot.gateway!.log.last('$_svc/Delete')!.request as pb.DeleteThemeRequest).id,
              'ocean');
          expect(robot.themes!.active.id, 'prototype');
          expect(robot.themes!.themes.map((t) => t.id), isNot(contains('ocean')));
          expect(robot.exists('theme_editor.preview'), isFalse, reason: 'the editor closed');

        case 'positive_signed_out_previews_only':
          await robot.load();
          await robot.open();
          expect(robot.exists('theme_editor.offline'), isTrue);
          expect(robot.enabled('theme_editor.save'), isFalse);
          expect(robot.enabled('theme_editor.import'), isFalse);
          expect(robot.enabled('theme_editor.export'), isFalse);
          expect(robot.exists('theme_editor.preview'), isTrue);

        case 'negative_share_denied_says_why':
          await openCopyOf('windows', arrange: (s) {
            s.failing['Put'] = const GrpcError.permissionDenied('sharing needs (approve, theme)');
          });
          await robot.pressSegment('Shared with the tenant');
          await robot.press('theme_editor.save', until: () => robot.snack('Not allowed'));
          expect(robot.snack('sharing needs (approve, theme)'), isTrue);
          expect(robot.themes!.active.id, 'windows');
          expect(fired('SetActive'), isFalse);

        case 'negative_unparseable_import_keeps_draft':
          await openCopyOf('windows', arrange: (s) {
            s.failing['Import'] = const GrpcError.invalidArgument('line 1: expected a field name');
          });
          await robot.press('theme_editor.import');
          await tester.pumpAndSettle();
          await robot.type('theme_editor.import.text', '{{{');
          await robot.press('theme_editor.import.ok',
              until: () => robot.snack('expected a field name'));
          expect(find.text('Theme editor — Windows (copy)'), findsOneWidget);

        case 'corner_user_theme_edited_in_place':
          await openUserTheme();
          expect(robot.exists('theme_editor.delete'), isTrue);
          expect(
              tester
                  .widget<TextField>(find.descendant(
                      of: robot.byKey('theme_editor.id'), matching: find.byType(TextField)))
                  .enabled,
              isFalse,
              reason: 'a stored theme keeps its id');
          expect(find.text('Theme editor — Ocean'), findsOneWidget);

        case 'corner_stale_version_is_explained':
          await openCopyOf('windows', arrange: (s) {
            s.failing['Put'] = const GrpcError.aborted('version 3 is stale');
          });
          await robot.press('theme_editor.save', until: () => robot.snack('Someone else saved'));

        case 'boundary_cancel_sends_nothing':
          await openUserTheme();
          await robot.press('theme_editor.delete');
          await tester.pumpAndSettle();
          await robot.press('theme_editor.delete.cancel');
          await tester.pumpAndSettle();
          expect(fired('Delete'), isFalse);
          expect(robot.themes!.active.id, 'ocean');

        case 'boundary_over_cap_text_not_sent':
          await openCopyOf('windows');
          await robot.press('theme_editor.import');
          await tester.pumpAndSettle();
          await robot.type('theme_editor.import.text', 'x' * (maxThemeBytes + 1));
          await robot.press('theme_editor.import.ok', until: () => robot.snack('over 64 KiB'));
          expect(fired('Import'), isFalse);

        case 'adversarial_imported_problem_blocks_save':
          const problem = "`windows` is a built-in theme's id; give the copy its own id";
          await openCopyOf('windows', arrange: (s) {
            s.importResponse = pb.ImportThemeResponse(
                theme: builtinTheme('windows')..clearBuiltin(), problem: problem);
          });
          await robot.press('theme_editor.import');
          await tester.pumpAndSettle();
          await robot.type('theme_editor.import.text', 'id: "windows"');
          await robot.press('theme_editor.import.ok', until: () => robot.snack('problem to fix'));
          expect(robot.problem, problem);
          expect(robot.enabled('theme_editor.save'), isFalse);

        case 'adversarial_invalid_stored_theme_not_applied':
          await openCopyOf('windows', arrange: (s) {
            s.putAnswer = (r) => _stored(r)..light.primary = 'url(x)';
          });
          await robot.press('theme_editor.save', until: () => robot.snack('cannot use'));
          expect(robot.themes!.active.id, 'windows');
          expect(robot.themes!.themes.map((t) => t.id), isNot(contains('windows-copy')));
          expect(fired('SetActive'), isFalse);

        case 'adversarial_css_injection_colour_rejected':
          await openCopyOf('windows');
          await robot.colour('primary', 'red; background:url(x)');
          final text =
              tester.widget<EditableText>(robot.field('theme_editor.color.primary')).controller.text;
          expect(text.length, maxColorLen, reason: 'cut at the longest colour');
          expect(robot.problem, contains('primary'));
          expect(robot.enabled('theme_editor.save'), isFalse);
          expect(tester.takeException(), isNull);

        default:
          fail('no test for spec row ${row.label}');
      }
    });
  }
}
