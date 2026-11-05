import '../testkit/spec.dart';

/// Launch page spec (design 02). The page has no backend — every action is
/// `local` (url_launcher). One key stem: `launch.card` (grafana/hyperdx/prometheus).
const launchSpec = PageSpec('launch', [
  SpecRow(
    elementId: 'launch.card',
    caseClass: CaseClass.positive,
    name: 'open',
    description: 'tapping a card opens its configured URL via url_launcher',
  ),
  SpecRow(
    elementId: 'launch.card',
    caseClass: CaseClass.negative,
    name: 'launch_blocked',
    description: 'when url_launcher reports failure, a SnackBar shows, no crash',
  ),
  SpecRow(
    elementId: 'launch.card',
    caseClass: CaseClass.corner,
    name: 'follows_page_host',
    description: 'a localhost URL opens on the host the portal was loaded from',
  ),
  SpecRow(
    elementId: 'launch.card',
    caseClass: CaseClass.boundary,
    name: 'empty_url_hidden',
    description: 'a UI whose URL is empty (not deployed) has no card',
  ),
]);
