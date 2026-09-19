import '../pages/launch_spec.dart';
import '../pages/prompts_spec.dart';
import '../testkit/spec.dart';

/// Every page whose spec has been tabled. The completeness critic
/// (`coverage_test.dart`) requires full key coverage for each page listed here;
/// pages absent from this list are reported as *pending* (logged, not failed).
/// Inc 4 adds Graph / Agent / Router / Fleet / Settings here as it tables them —
/// when all seven are present, every keyed element in the app must have a
/// `positive_` spec row.
const allSpecs = <PageSpec>[launchSpec, promptsSpec];
