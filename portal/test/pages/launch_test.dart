import 'package:flutter_test/flutter_test.dart';

import '../testkit/robots/launch_robot.dart';
import 'launch_spec.dart';

/// Layer-A widget tests for the Launch page — iterates [launchSpec]; each row is
/// one test. The template for a `local` (no-backend) page: prove the action hits
/// only the platform channel (url_launcher), never a gRPC seam.
void main() {
  for (final row in launchSpec.rows) {
    testWidgets('launch ${row.label} — ${row.description}', (tester) async {
      final robot = LaunchRobot(tester);
      await robot.load();

      switch (row.label) {
        case 'positive_open':
          await robot.tapCard('grafana');
          expect(robot.openedUrls, ['http://localhost:3000']);
          // Exactly one launch call; nothing else (no gRPC — there is no gateway).
          expect(robot.calls.length, 1);
          break;

        case 'negative_launch_blocked':
          robot.launchSucceeds = false;
          await robot.tapCard('hyperdx');
          expect(robot.snackContains('Could not open'), isTrue);
          break;

        default:
          fail('no test body for row ${row.label}');
      }
    });
  }
}
