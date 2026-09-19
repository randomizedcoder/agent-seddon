// Layer-B (`nix run .#portal-e2e`) integration-test driver bootstrap.
//
// The counterpart to the `integration_test/` suites: `flutter drive` runs this
// on the host while the suite runs in the browser, bridging the two over the
// web-driver protocol. `integrationDriver()` also writes the per-test JSON the
// suite hands back via `IntegrationTestWidgetsFlutterBinding.reportData`
// (consumed by the shell harness for the report + perf rows).
import 'package:integration_test/integration_test_driver.dart';

Future<void> main() => integrationDriver();
