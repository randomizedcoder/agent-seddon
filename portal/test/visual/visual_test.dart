import 'package:agent_portal/src/gen/agent/v1/review_fleet.pb.dart';
import 'package:fixnum/fixnum.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../testkit/builders.dart';
import '../testkit/robots/fleet_robot.dart';
import '../testkit/robots/launch_robot.dart';
import '../testkit/robots/prompts_robot.dart';
import '../testkit/themes.dart';

/// Layer-A **visual + a11y** suite (design 03b), gated as `portal-visual`.
///
/// Two guards the widget-breadth layer can't give:
///   * **a11y** — `meetsGuideline` asserts tap-target size, that every tappable
///     is labeled, and text contrast, so a regression that makes the UI
///     unusable (or unreadable) fails the build.
///   * **golden** — `matchesGoldenFile` pins each page's rendered pixels, catching
///     the "all-icons-white" / broken-layout class the RPC-level widget tests
///     cannot see. Goldens are generated and checked on the same pinned hermetic
///     Flutter (`versions.flutter`), so rendering is deterministic; regenerate
///     with `flutter test --update-goldens test/visual`.
///
/// Launch + Prompts are the representative slice (the template pages, as inc 3);
/// the pattern extends to the other pages and the {dark}×{narrow,wide}×{large
/// text} matrix — see STATUS.md.
void main() {
  group('a11y — meetsGuideline', () {
    testWidgets('launch loaded meets tap-target + labeled + contrast',
        (tester) async {
      final robot = LaunchRobot(tester);
      await robot.load();
      final handle = tester.ensureSemantics();
      await expectLater(tester, meetsGuideline(androidTapTargetGuideline));
      await expectLater(tester, meetsGuideline(labeledTapTargetGuideline));
      await expectLater(tester, meetsGuideline(textContrastGuideline));
      handle.dispose();
    });

    testWidgets('prompts loaded meets tap-target + labeled', (tester) async {
      final robot = await PromptsRobot.create(tester);
      robot.prompts.listResponse =
          promptList([promptEntry(id: 'sys', content: 'body')]);
      await robot.load();
      final handle = tester.ensureSemantics();
      await expectLater(tester, meetsGuideline(androidTapTargetGuideline));
      await expectLater(tester, meetsGuideline(labeledTapTargetGuideline));
      handle.dispose();
    });
  });

  group('golden — rendered pixels', () {
    testWidgets('launch (light)', (tester) async {
      final robot = LaunchRobot(tester);
      await robot.load();
      await expectLater(
        find.byType(MaterialApp),
        matchesGoldenFile('goldens/launch_light.png'),
      );
    });

    testWidgets('prompts (light)', (tester) async {
      final robot = await PromptsRobot.create(tester);
      robot.prompts.listResponse =
          promptList([promptEntry(id: 'sys', content: 'body')]);
      await robot.load();
      await expectLater(
        find.byType(MaterialApp),
        matchesGoldenFile('goldens/prompts_light.png'),
      );
    });
  });

  // Every built-in theme (visual/08-accessibility.md, 07-testing.md
  // "Accessibility tests"): Launch must meet contrast + labels + targets in each
  // theme and mode it supports; goldens pin how each theme renders Launch.
  final themed = [
    ('prototype', Brightness.light),
    ('prototype', Brightness.dark),
    ('tn3270', Brightness.dark),
    ('windows', Brightness.light),
    ('windows', Brightness.dark),
    ('macos', Brightness.light),
    ('macos', Brightness.dark),
    ('accessible', Brightness.light),
    ('accessible', Brightness.dark),
  ];
  group('themes — a11y per theme', () {
    for (final (id, b) in themed) {
      testWidgets('positive_text_contrast_${id}_${b.name}: launch meets every guideline',
          (tester) async {
        final robot = LaunchRobot(tester)..theme = builtinThemeData(id, b);
        await robot.load();
        final handle = tester.ensureSemantics();
        await expectLater(tester, meetsGuideline(textContrastGuideline));
        await expectLater(tester, meetsGuideline(labeledTapTargetGuideline));
        // WCAG 2.2 2.5.8 (AA): 24px for every theme; the accessible theme
        // promises 48px (visual/08-accessibility.md).
        await expectLater(tester, meetsGuideline(id == 'accessible'
            ? androidTapTargetGuideline
            : const MinimumTapTargetGuideline(
                size: Size(24, 24), link: 'https://www.w3.org/TR/WCAG22/#target-size-minimum')));
        handle.dispose();
      });
    }

    testWidgets(
        'positive_status_has_icon: a review status is an icon with a name, not colour alone',
        (tester) async {
      final robot = await FleetRobot.create(tester)
        ..theme = builtinThemeData('accessible');
      robot.fleet.listReviewsResponse = ListReviewsReply(reviews: [
        for (final s in ['drafted', 'approved', 'posted', 'superseded'])
          ReviewSummary(
              reviewId: s, repo: 'o__n', prNumber: Int64(1), status: s),
      ]);
      await robot.load();
      final handle = tester.ensureSemantics();
      for (final s in ['drafted', 'approved', 'posted', 'superseded']) {
        // The row merges its children, so the status is one word of its label.
        expect(find.bySemanticsLabel(RegExp('\\b$s\\b'), skipOffstage: false),
            findsWidgets,
            reason: s);
        // status_icons: the name is also on screen as text.
        expect(find.text(s), findsWidgets, reason: s);
      }
      handle.dispose();
      await robot.settle();
    });
  });

  group('themes — golden per theme', () {
    for (final (id, b) in themed) {
      testWidgets('launch ($id, ${b.name})', (tester) async {
        final robot = LaunchRobot(tester)..theme = builtinThemeData(id, b);
        await robot.load();
        await expectLater(
          find.byType(MaterialApp),
          matchesGoldenFile('goldens/themes/launch_${id}_${b.name}.png'),
        );
      });
    }
  });
}
