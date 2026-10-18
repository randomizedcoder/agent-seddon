import '../testkit/spec.dart';

/// Forges page spec (forge-auth F5d1) — the tenant's GitHub App installs and
/// forge cards, and the Connect-GitHub round trip (`BeginInstall` → GitHub →
/// the portal → `CompleteInstall`). The return rows arrange what
/// `AuthState.start` sets aside, then mount the page. The `adversarial_` rows
/// are the point: a return whose `state` this tab is not waiting on, or that
/// is malformed, never sends its `code` anywhere, and an install URL off the
/// wire is only followed when it is https.
const forgesSpec = PageSpec('forges', [
  // ── load ─────────────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'forges.install.item',
    caseClass: CaseClass.positive,
    name: 'loaded_lists_render',
    description: 'both Lists fire; installs and cards render',
    expectedRpc: 'agent.v1.GithubInstallRegistryService/List',
  ),
  SpecRow(
    elementId: 'forges.install.empty',
    caseClass: CaseClass.positive,
    name: 'no_installs_explains',
    description: 'an empty install list says so',
    expectedRpc: 'agent.v1.GithubInstallRegistryService/List',
  ),
  SpecRow(
    elementId: 'forges.card.empty',
    caseClass: CaseClass.positive,
    name: 'no_cards_explains',
    description: 'an empty card list says so',
    expectedRpc: 'agent.v1.ForgeRegistryService/List',
  ),
  SpecRow(
    elementId: 'forges.install.item',
    caseClass: CaseClass.corner,
    name: 'loading_shows_spinner',
    description: 'before the Lists resolve, a spinner is shown',
    expectedRpc: 'agent.v1.GithubInstallRegistryService/List',
  ),
  SpecRow(
    elementId: 'forges.error.retry',
    caseClass: CaseClass.negative,
    name: 'list_error_shows_retry',
    description: 'a failed List shows the retry panel, no crash',
    expectedRpc: 'agent.v1.ForgeRegistryService/List',
  ),
  SpecRow(
    elementId: 'forges.error.retry',
    caseClass: CaseClass.positive,
    name: 'retry_recovers',
    description: 'Retry lists again and shows the loaded page',
    expectedRpc: 'agent.v1.GithubInstallRegistryService/List',
  ),
  // ── connect ──────────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'forges.github.connect',
    caseClass: CaseClass.positive,
    name: 'connect_begins_and_leaves',
    description: 'Connect GitHub calls BeginInstall, remembers its state and '
        'goes to the install URL',
    expectedRpc: 'agent.v1.GithubInstallRegistryService/BeginInstall',
  ),
  SpecRow(
    elementId: 'forges.github.connect',
    caseClass: CaseClass.negative,
    name: 'onboarding_not_configured_explains',
    description: 'FAILED_PRECONDITION from BeginInstall explains the missing '
        'config and navigates nowhere',
    expectedRpc: 'agent.v1.GithubInstallRegistryService/BeginInstall',
  ),
  SpecRow(
    elementId: 'forges.github.connect',
    caseClass: CaseClass.corner,
    name: 'native_cannot_redirect',
    description: 'without a browser redirect the page explains instead of '
        'navigating',
    expectedRpc: 'agent.v1.GithubInstallRegistryService/BeginInstall',
  ),
  SpecRow(
    elementId: 'forges.github.connect',
    caseClass: CaseClass.adversarial,
    name: 'hostile_install_url_not_followed',
    description: 'a non-https install URL off the wire is neither followed '
        'nor remembered',
    expectedRpc: 'agent.v1.GithubInstallRegistryService/BeginInstall',
  ),
  // ── GitHub's return ──────────────────────────────────────────────────────
  SpecRow(
    elementId: 'forges.banner',
    caseClass: CaseClass.positive,
    name: 'return_completes_install',
    description: 'a return carrying the pending state is relayed in '
        'CompleteInstall with the bearer, and the pending state is spent',
    expectedRpc: 'agent.v1.GithubInstallRegistryService/CompleteInstall',
  ),
  SpecRow(
    elementId: 'forges.banner.dismiss',
    caseClass: CaseClass.positive,
    name: 'dismiss_hides_banner',
    description: 'the banner closes',
  ),
  SpecRow(
    elementId: 'forges.banner',
    caseClass: CaseClass.negative,
    name: 'not_admin_is_opaque',
    description: 'PERMISSION_DENIED shows one opaque refusal',
    expectedRpc: 'agent.v1.GithubInstallRegistryService/CompleteInstall',
  ),
  SpecRow(
    elementId: 'forges.banner',
    caseClass: CaseClass.negative,
    name: 'claimed_by_another_tenant',
    description: 'ALREADY_EXISTS says another tenant holds the install',
    expectedRpc: 'agent.v1.GithubInstallRegistryService/CompleteInstall',
  ),
  SpecRow(
    elementId: 'forges.banner',
    caseClass: CaseClass.corner,
    name: 'org_approval_pending_sends_nothing',
    description: 'setup_action=request explains an owner must approve; no RPC',
  ),
  SpecRow(
    elementId: 'forges.banner',
    caseClass: CaseClass.boundary,
    name: 'expired_pending_sends_nothing',
    description: 'a pending state one second past its expiry is not relayed',
  ),
  SpecRow(
    elementId: 'forges.banner',
    caseClass: CaseClass.adversarial,
    name: 'forged_state_never_relayed',
    description: 'a return whose state this tab is not waiting on never sends '
        'its code, and still spends the pending state',
  ),
  SpecRow(
    elementId: 'forges.banner',
    caseClass: CaseClass.adversarial,
    name: 'return_without_pending_never_relayed',
    description: 'a return with no Connect GitHub behind it never sends its code',
  ),
  SpecRow(
    elementId: 'forges.banner',
    caseClass: CaseClass.adversarial,
    name: 'malformed_return_never_relayed',
    description: 'a traversal installation_id is refused before any RPC',
  ),
  SpecRow(
    elementId: 'forges.banner',
    caseClass: CaseClass.adversarial,
    name: 'code_and_state_never_displayed',
    description: 'a refused CompleteInstall never shows the code or the state',
    expectedRpc: 'agent.v1.GithubInstallRegistryService/CompleteInstall',
  ),
  // ── forge cards ──────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'forges.install.card',
    caseClass: CaseClass.positive,
    name: 'create_card_puts_ghapp_ref',
    description: 'Create forge card puts a github card minting as the install',
    expectedRpc: 'agent.v1.ForgeRegistryService/Put',
  ),
  SpecRow(
    elementId: 'forges.install.card',
    caseClass: CaseClass.corner,
    name: 'existing_card_disables_create',
    description: 'an install that already has a ghapp card offers no second one',
    expectedRpc: 'agent.v1.ForgeRegistryService/List',
  ),
  SpecRow(
    elementId: 'forges.install.card',
    caseClass: CaseClass.negative,
    name: 'viewer_sees_no_write_controls',
    description: 'without write/delete on forge_registry: no Connect, no '
        'create, no delete, switches disabled',
  ),
  SpecRow(
    elementId: 'forges.card.dialog',
    caseClass: CaseClass.positive,
    name: 'dialog_edits_card_id',
    description: 'the card id can be changed before Create',
    expectedRpc: 'agent.v1.ForgeRegistryService/Put',
  ),
  SpecRow(
    elementId: 'forges.card.dialog',
    caseClass: CaseClass.negative,
    name: 'cancel_writes_nothing',
    description: 'cancelling the dialog puts nothing',
  ),
  SpecRow(
    elementId: 'forges.card.dialog',
    caseClass: CaseClass.boundary,
    name: 'id_length_limit',
    description: 'a 64-character id is accepted, 65 is refused',
    expectedRpc: 'agent.v1.ForgeRegistryService/Put',
  ),
  SpecRow(
    elementId: 'forges.card.dialog',
    caseClass: CaseClass.adversarial,
    name: 'hostile_card_id_refused',
    description: 'a traversal id is refused in the dialog; nothing is put',
  ),
  SpecRow(
    elementId: 'forges.card.item',
    caseClass: CaseClass.positive,
    name: 'cards_render_reference',
    description: 'a card shows its kind and token reference',
    expectedRpc: 'agent.v1.ForgeRegistryService/List',
  ),
  SpecRow(
    elementId: 'forges.card.enabled',
    caseClass: CaseClass.positive,
    name: 'toggle_puts_flag',
    description: 'the switch puts the card with the flag flipped',
    expectedRpc: 'agent.v1.ForgeRegistryService/Put',
  ),
  SpecRow(
    elementId: 'forges.card.delete',
    caseClass: CaseClass.positive,
    name: 'delete_confirmed',
    description: 'delete asks first, then calls Delete',
    expectedRpc: 'agent.v1.ForgeRegistryService/Delete',
  ),
  SpecRow(
    elementId: 'forges.card.delete',
    caseClass: CaseClass.negative,
    name: 'delete_cancel_fires_nothing',
    description: 'cancelling the confirm dialog deletes nothing',
  ),
]);
