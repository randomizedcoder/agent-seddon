import '../testkit/spec.dart';

/// Fleet page spec (design 02, `docs/design/portal-gui-testing/02-test-spec.md`).
/// Every `Key('fleet.…')` stem scanned from `fleet_page.dart` has a `positive_`
/// row (so the completeness critic passes once the page is tabled), plus the four
/// async states, a `negative_unimplemented` "bare gateway" row, the detail-pane
/// NotFound/error states, the locked/dirty gates, and the two mandatory
/// `adversarial_` rows (double-click on Approve; inert remote image in a
/// model-authored draft body).
const fleetSpec = PageSpec('fleet', [
  // ── list load: the four async states + bare-gateway UNIMPLEMENTED ───────────
  SpecRow(
    elementId: 'fleet.review.item',
    caseClass: CaseClass.positive,
    name: 'loaded_lists_reviews',
    description: 'ListReviews + roster List fire; draft items render',
    expectedRpc: 'agent.v1.ReviewFleetService/ListReviews',
  ),
  SpecRow(
    elementId: 'fleet.review.item',
    caseClass: CaseClass.boundary,
    name: 'empty_list_no_items',
    description: 'an empty ListReviews renders the empty state, no items',
    expectedRpc: 'agent.v1.ReviewFleetService/ListReviews',
  ),
  SpecRow(
    elementId: 'fleet.review.item',
    caseClass: CaseClass.corner,
    name: 'loading_shows_spinner',
    description: 'before the RPCs resolve, a progress spinner is shown',
    expectedRpc: 'agent.v1.ReviewFleetService/ListReviews',
  ),
  SpecRow(
    elementId: 'fleet.error.retry',
    caseClass: CaseClass.negative,
    name: 'list_error_shows_retry',
    description: 'a failed ListReviews surfaces the offline retry panel',
    expectedRpc: 'agent.v1.ReviewFleetService/ListReviews',
  ),
  SpecRow(
    elementId: 'fleet.error.retry',
    caseClass: CaseClass.negative,
    name: 'unimplemented_not_connected',
    description: 'a bare gateway answering UNIMPLEMENTED → not-connected state',
    expectedRpc: 'agent.v1.ReviewFleetService/ListReviews',
  ),
  SpecRow(
    elementId: 'fleet.error.retry',
    caseClass: CaseClass.positive,
    name: 'retry_recovers',
    description: 'tapping Retry re-lists and shows the loaded UI',
    expectedRpc: 'agent.v1.ReviewFleetService/ListReviews',
  ),
  // ── filter bar ──────────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'fleet.filter.repo',
    caseClass: CaseClass.positive,
    name: 'filter_by_repo',
    description: 'submitting the repo field re-lists with {repo} in the request',
    expectedRpc: 'agent.v1.ReviewFleetService/ListReviews',
  ),
  SpecRow(
    elementId: 'fleet.filter.status',
    caseClass: CaseClass.positive,
    name: 'filter_by_status',
    description: 'choosing a status re-lists with {status} in the request',
    expectedRpc: 'agent.v1.ReviewFleetService/ListReviews',
  ),
  SpecRow(
    elementId: 'fleet.filter.refresh',
    caseClass: CaseClass.positive,
    name: 'refresh_relists',
    description: 'Refresh re-fires ListReviews',
    expectedRpc: 'agent.v1.ReviewFleetService/ListReviews',
  ),
  // ── sessions strip ──────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'fleet.session.reviewNow',
    caseClass: CaseClass.positive,
    name: 'review_now_opens_dialog',
    description: 'the roster Review-now button opens the PR-number dialog',
    expectedRpc: 'local',
  ),
  SpecRow(
    elementId: 'fleet.reviewNow.queue',
    caseClass: CaseClass.positive,
    name: 'review_now_queues',
    description: 'Queue fires ReviewNow {sessionId, prNumber}',
    expectedRpc: 'agent.v1.ReviewFleetService/ReviewNow',
  ),
  SpecRow(
    elementId: 'fleet.reviewNow.prNumber',
    caseClass: CaseClass.positive,
    name: 'review_now_pr_field_sent',
    description: 'the typed PR number rides the ReviewNow request',
    expectedRpc: 'agent.v1.ReviewFleetService/ReviewNow',
  ),
  SpecRow(
    elementId: 'fleet.reviewNow.cancel',
    caseClass: CaseClass.positive,
    name: 'review_now_cancel_no_rpc',
    description: 'Cancel dismisses the dialog and fires no ReviewNow',
    expectedRpc: 'local',
  ),
  SpecRow(
    elementId: 'fleet.reviewNow.prNumber',
    caseClass: CaseClass.adversarial,
    name: 'review_now_non_numeric_no_rpc',
    description: 'a non-numeric / non-positive PR is rejected — no ReviewNow',
    expectedRpc: 'local',
  ),
  SpecRow(
    elementId: 'fleet.session.enable',
    caseClass: CaseClass.positive,
    name: 'toggle_enable',
    description: 'flipping the switch fires SetEnabled {id, enabled}',
    expectedRpc: 'agent.v1.ReviewFleetService/SetEnabled',
  ),
  // ── detail pane ─────────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'fleet.review.item',
    caseClass: CaseClass.positive,
    name: 'select_loads_detail',
    description: 'selecting a draft fires GetReview {reviewId} and renders it',
    expectedRpc: 'agent.v1.ReviewFleetService/GetReview',
  ),
  SpecRow(
    elementId: 'fleet.detail.approve',
    caseClass: CaseClass.corner,
    name: 'unavailable_body',
    description: 'GetReview NotFound → unavailable state; Approve disabled',
    expectedRpc: 'agent.v1.ReviewFleetService/GetReview',
  ),
  SpecRow(
    elementId: 'fleet.detail.retry',
    caseClass: CaseClass.negative,
    name: 'detail_error_shows_retry',
    description: 'a failed GetReview surfaces the detail retry panel',
    expectedRpc: 'agent.v1.ReviewFleetService/GetReview',
  ),
  SpecRow(
    elementId: 'fleet.detail.retry',
    caseClass: CaseClass.positive,
    name: 'detail_retry_recovers',
    description: 'tapping detail Retry re-fetches and renders the body',
    expectedRpc: 'agent.v1.ReviewFleetService/GetReview',
  ),
  SpecRow(
    elementId: 'fleet.detail.mode',
    caseClass: CaseClass.positive,
    name: 'switch_to_edit',
    description: 'selecting Edit shows the raw-markdown editor',
    expectedRpc: 'local',
  ),
  SpecRow(
    elementId: 'fleet.detail.mode',
    caseClass: CaseClass.boundary,
    name: 'locked_disables_edit',
    description: 'a posted/approved draft locks Edit/Preview — view only',
    expectedRpc: 'local',
  ),
  SpecRow(
    elementId: 'fleet.detail.editor',
    caseClass: CaseClass.positive,
    name: 'edit_updates_field',
    description: 'typing in the editor updates the buffer and marks it dirty',
    expectedRpc: 'local',
  ),
  SpecRow(
    elementId: 'fleet.detail.save',
    caseClass: CaseClass.positive,
    name: 'save_updates_review',
    description: 'Save fires UpdateReview {reviewId, body}',
    expectedRpc: 'agent.v1.ReviewFleetService/UpdateReview',
  ),
  SpecRow(
    elementId: 'fleet.detail.save',
    caseClass: CaseClass.boundary,
    name: 'save_disabled_unless_dirty',
    description: 'Save is disabled until the buffer differs from disk',
    expectedRpc: 'local',
  ),
  SpecRow(
    elementId: 'fleet.detail.approve',
    caseClass: CaseClass.positive,
    name: 'approve_posts',
    description: 'Approve → confirm fires Approve {reviewId}; posted snack',
    expectedRpc: 'agent.v1.ReviewFleetService/Approve',
  ),
  SpecRow(
    elementId: 'fleet.approve.confirm',
    caseClass: CaseClass.positive,
    name: 'confirm_fires_approve',
    description: 'the confirm button is what dispatches Approve',
    expectedRpc: 'agent.v1.ReviewFleetService/Approve',
  ),
  SpecRow(
    elementId: 'fleet.approve.cancel',
    caseClass: CaseClass.positive,
    name: 'approve_cancel_no_rpc',
    description: 'Cancel dismisses the confirm dialog and fires no Approve',
    expectedRpc: 'local',
  ),
  SpecRow(
    elementId: 'fleet.detail.approve',
    caseClass: CaseClass.boundary,
    name: 'approve_gated_when_dirty',
    description: 'unsaved edits disable Approve (post the persisted body first)',
    expectedRpc: 'local',
  ),
  SpecRow(
    elementId: 'fleet.detail.approve',
    caseClass: CaseClass.adversarial,
    name: 'double_click_one_approve',
    description: 'a rapid double-click on confirm fires exactly one Approve',
    expectedRpc: 'agent.v1.ReviewFleetService/Approve',
  ),
  SpecRow(
    elementId: 'fleet.review.item',
    caseClass: CaseClass.adversarial,
    name: 'remote_image_is_inert',
    description: 'a remote image in the model-authored body never fetches',
    expectedRpc: 'agent.v1.ReviewFleetService/GetReview',
  ),
]);
