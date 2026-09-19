import '../pages/agent_spec.dart';
import '../pages/fleet_spec.dart';
import '../pages/graph_spec.dart';
import '../pages/launch_spec.dart';
import '../pages/prompts_spec.dart';
import '../pages/router_spec.dart';
import '../pages/settings_spec.dart';
import '../testkit/spec.dart';

/// Every page whose spec has been tabled. The completeness critic
/// (`coverage_test.dart`) requires full key coverage for each page listed here;
/// pages absent from this list are reported as *pending* (logged, not failed).
///
/// With inc 4 all seven portal pages are tabled, so the critic now enforces that
/// **every keyed element in the app has a `positive_` spec row** — a new element
/// cannot silently escape coverage.
const allSpecs = <PageSpec>[
  launchSpec,
  promptsSpec,
  graphSpec,
  agentSpec,
  routerSpec,
  fleetSpec,
  settingsSpec,
];
