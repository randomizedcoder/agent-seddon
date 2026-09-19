import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../testkit/builders.dart';
import '../testkit/robots/launch_robot.dart';
import '../testkit/robots/prompts_robot.dart';

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
}
