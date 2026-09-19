import '../testkit/spec.dart';

/// Router page spec (design 02) — the live model-router / provider-registry
/// control plane. Covers the four async states on the upstreams load, the
/// segmented sub-views, upstream CRUD (with the confirm-dialog gate on delete),
/// the health refresh, and the route-introspection form. Every key stem in
/// `router_page.dart` has a `positive_` row (families cover their dotted
/// extensions), so the completeness critic is satisfied; a `negative_cancel`
/// proves the delete dialog gates the RPC, and `adversarial_` rows drive the
/// URL / number fields.
const routerSpec = PageSpec('router', [
  // ── page load: the four async states ──────────────────────────────────────
  SpecRow(
    elementId: 'router.upstream.item',
    caseClass: CaseClass.positive,
    name: 'loaded_lists_render',
    description: 'List fires; the upstream cards render and are selectable',
    expectedRpc: 'agent.v1.ProviderRegistryService/List',
  ),
  SpecRow(
    elementId: 'router.upstream.item',
    caseClass: CaseClass.boundary,
    name: 'empty_list_no_items',
    description: 'an empty registry renders no cards and does not crash',
    expectedRpc: 'agent.v1.ProviderRegistryService/List',
  ),
  SpecRow(
    elementId: 'router.upstream.item',
    caseClass: CaseClass.corner,
    name: 'loading_shows_spinner',
    description: 'before List resolves, a progress spinner is shown',
    expectedRpc: 'agent.v1.ProviderRegistryService/List',
  ),
  SpecRow(
    elementId: 'router.error.retry',
    caseClass: CaseClass.negative,
    name: 'list_error_shows_retry',
    description: 'a failed List surfaces the offline retry panel, no crash',
    expectedRpc: 'agent.v1.ProviderRegistryService/List',
  ),
  SpecRow(
    elementId: 'router.error.retry',
    caseClass: CaseClass.positive,
    name: 'retry_recovers',
    description: 'tapping Retry re-lists and shows the loaded UI',
    expectedRpc: 'agent.v1.ProviderRegistryService/List',
  ),
  // ── segmented sub-views ───────────────────────────────────────────────────
  SpecRow(
    elementId: 'router.view.upstreams',
    caseClass: CaseClass.positive,
    name: 'switch_back_to_upstreams',
    description: 'selecting Upstreams returns to the CRUD view (local)',
  ),
  SpecRow(
    elementId: 'router.view.health',
    caseClass: CaseClass.positive,
    name: 'shows_health_table',
    description: 'selecting Health mounts the table and fires Health',
    expectedRpc: 'agent.v1.ProviderRegistryService/Health',
  ),
  SpecRow(
    elementId: 'router.view.route',
    caseClass: CaseClass.positive,
    name: 'shows_route_tester',
    description: 'selecting Route tester mounts the form; no RPC fires (local)',
  ),
  SpecRow(
    elementId: 'router.health.refresh',
    caseClass: CaseClass.positive,
    name: 'refresh_refires_health',
    description: 'Refresh re-fires Health and re-renders the table',
    expectedRpc: 'agent.v1.ProviderRegistryService/Health',
  ),
  // ── upstream CRUD ─────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'router.upstream.add',
    caseClass: CaseClass.positive,
    name: 'add_opens_blank_editor',
    description: 'Add selects a blank upstream and opens the New editor (local)',
  ),
  SpecRow(
    elementId: 'router.upstream.enable',
    caseClass: CaseClass.positive,
    name: 'toggle_fires_enable',
    description: 'flipping the switch fires Enable {id,enabled} then reloads',
    expectedRpc: 'agent.v1.ProviderRegistryService/Enable',
  ),
  SpecRow(
    elementId: 'router.upstream.delete',
    caseClass: CaseClass.positive,
    name: 'delete_opens_confirm_dialog',
    description: 'the delete icon opens the AlertDialog; no RPC fires yet',
  ),
  SpecRow(
    elementId: 'router.upstream.delete.confirm',
    caseClass: CaseClass.positive,
    name: 'confirm_fires_delete',
    description: 'confirming the dialog fires Delete {id} then reloads',
    expectedRpc: 'agent.v1.ProviderRegistryService/Delete',
  ),
  SpecRow(
    elementId: 'router.upstream.delete.cancel',
    caseClass: CaseClass.negative,
    name: 'cancel_fires_no_delete',
    description: 'cancelling the dialog fires NO Delete RPC (the gate holds)',
  ),
  SpecRow(
    elementId: 'router.upstream.save',
    caseClass: CaseClass.positive,
    name: 'save_fires_put_and_reloads',
    description: 'Save fires Put {edited card} then reloads',
    expectedRpc: 'agent.v1.ProviderRegistryService/Put',
  ),
  SpecRow(
    elementId: 'router.upstream.save',
    caseClass: CaseClass.negative,
    name: 'save_error_snacks',
    description: 'a failed Put shows a failure SnackBar, no crash, no reload',
    expectedRpc: 'agent.v1.ProviderRegistryService/Put',
  ),
  SpecRow(
    elementId: 'router.upstream.save',
    caseClass: CaseClass.adversarial,
    name: 'hostile_url_and_number_sent_safely',
    description: 'a hostile base_url is Put verbatim; garbage numbers → 0, no crash',
    expectedRpc: 'agent.v1.ProviderRegistryService/Put',
  ),
  SpecRow(
    elementId: 'router.upstream.field',
    caseClass: CaseClass.positive,
    name: 'edit_updates_fields',
    description: 'typing into the card fields updates them (local)',
  ),
  SpecRow(
    elementId: 'router.upstream.field.tier',
    caseClass: CaseClass.positive,
    name: 'tier_dropdown_saved',
    description: 'choosing a tier in the dropdown is carried into the Put',
    expectedRpc: 'agent.v1.ProviderRegistryService/Put',
  ),
  // ── route tester ──────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'router.route.run',
    caseClass: CaseClass.positive,
    name: 'route_returns_decision',
    description: 'Route fires and the decision card shows the chosen upstream',
    expectedRpc: 'agent.v1.ProviderRegistryService/Route',
  ),
  SpecRow(
    elementId: 'router.route.task_mode',
    caseClass: CaseClass.positive,
    name: 'task_mode_in_hint',
    description: 'the chosen task_mode is carried into the RouteRequest hint',
    expectedRpc: 'agent.v1.ProviderRegistryService/Route',
  ),
  SpecRow(
    elementId: 'router.route.role',
    caseClass: CaseClass.positive,
    name: 'role_in_hint',
    description: 'the chosen role is carried into the RouteRequest hint',
    expectedRpc: 'agent.v1.ProviderRegistryService/Route',
  ),
  SpecRow(
    elementId: 'router.route.tier',
    caseClass: CaseClass.positive,
    name: 'tier_in_hint',
    description: 'the chosen tier is carried into the RouteRequest hint',
    expectedRpc: 'agent.v1.ProviderRegistryService/Route',
  ),
  SpecRow(
    elementId: 'router.route.min_context',
    caseClass: CaseClass.positive,
    name: 'min_context_in_hint',
    description: 'a numeric min_context is parsed into the hint',
    expectedRpc: 'agent.v1.ProviderRegistryService/Route',
  ),
  SpecRow(
    elementId: 'router.route.min_context',
    caseClass: CaseClass.adversarial,
    name: 'garbage_numbers_parse_to_zero',
    description: 'non-numeric min_context / max_cost parse to 0, no crash',
    expectedRpc: 'agent.v1.ProviderRegistryService/Route',
  ),
  SpecRow(
    elementId: 'router.route.max_cost',
    caseClass: CaseClass.positive,
    name: 'max_cost_in_hint',
    description: 'a numeric max_cost is parsed into the hint',
    expectedRpc: 'agent.v1.ProviderRegistryService/Route',
  ),
  SpecRow(
    elementId: 'router.route.hint',
    caseClass: CaseClass.positive,
    name: 'override_in_hint',
    description: 'the override_upstream id is carried into the hint',
    expectedRpc: 'agent.v1.ProviderRegistryService/Route',
  ),
]);
