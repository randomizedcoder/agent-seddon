import '../testkit/spec.dart';

/// Browser sign-in spec (security-hardening S13b,
/// docs/design/security-hardening/06-portal-and-edge.md). The sign-in page and
/// the account strip at the foot of the navigation rail. The callback and
/// refresh rows drive [AuthState] through the real gate over the fake gateway.
const loginSpec = PageSpec('login', [
  // ── positive ─────────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'login.issuer',
    caseClass: CaseClass.positive,
    name: 'issuer_button_begins_sign_in',
    description: 'an issuer button calls Begin with an S256 challenge and this '
        "page's address, stores the state, and goes to the IdP",
    expectedRpc: 'agent.v1.AuthService/Begin',
  ),
  SpecRow(
    elementId: 'login.progress',
    caseClass: CaseClass.positive,
    name: 'progress_while_redirecting',
    description: 'a spinner replaces the buttons while the browser leaves',
    expectedRpc: 'agent.v1.AuthService/Begin',
  ),
  SpecRow(
    elementId: 'login.account',
    caseClass: CaseClass.positive,
    name: 'round_trip_sets_bearer',
    description: 'the IdP callback is exchanged; the shell appears and its '
        'calls carry the agent token',
    expectedRpc: 'agent.v1.AuthService/Exchange',
  ),
  SpecRow(
    elementId: 'login.error',
    caseClass: CaseClass.positive,
    name: 'idp_error_is_shown',
    description: 'an ?error= callback shows why, and nothing is exchanged',
    expectedRpc: 'agent.v1.AuthService/Issuers',
  ),
  SpecRow(
    elementId: 'login.retry',
    caseClass: CaseClass.positive,
    name: 'retry_relists_issuers',
    description: 'when the agent could not be asked, Try again asks again',
    expectedRpc: 'agent.v1.AuthService/Issuers',
  ),
  SpecRow(
    elementId: 'login.expired',
    caseClass: CaseClass.positive,
    name: 'expired_after_refused_refresh',
    description: 'a refused refresh ends the session with a notice',
    expectedRpc: 'agent.v1.AuthService/Refresh',
  ),
  SpecRow(
    elementId: 'login.signout',
    caseClass: CaseClass.positive,
    name: 'signout_revokes_and_returns_to_login',
    description: 'Sign out calls Logout with the bearer, forgets the session '
        'and shows the sign-in page',
    expectedRpc: 'agent.v1.AuthService/Logout',
  ),
  SpecRow(
    elementId: 'login.account',
    caseClass: CaseClass.positive,
    name: 'refresh_swaps_the_bearer',
    description: 'a refresh replaces the token the next call sends',
    expectedRpc: 'agent.v1.AuthService/Refresh',
  ),
  // ── negative ─────────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'login.issuer',
    caseClass: CaseClass.negative,
    name: 'unsigned_in_shell_hidden',
    description: 'signed out, the app is not mounted and fires no RPC',
    expectedRpc: 'agent.v1.AuthService/Issuers',
  ),
  SpecRow(
    elementId: 'login.error',
    caseClass: CaseClass.negative,
    name: 'exchange_refused_shows_error',
    description: 'a refused Exchange leaves the user signed out with a reason',
    expectedRpc: 'agent.v1.AuthService/Exchange',
  ),
  SpecRow(
    elementId: 'login.issuer',
    caseClass: CaseClass.negative,
    name: 'native_cannot_redirect',
    description: 'without a browser to redirect, no Begin is sent',
  ),
  SpecRow(
    elementId: 'login.retry',
    caseClass: CaseClass.negative,
    name: 'on_mode_with_no_issuers_explains',
    description: 'PORTAL_AUTH=on against an agent with no browser sign-in says so',
    expectedRpc: 'agent.v1.AuthService/Issuers',
  ),
  // ── corner ───────────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'login.account',
    caseClass: CaseClass.corner,
    name: 'auth_off_runs_anonymously',
    description: 'PORTAL_AUTH=off: no sign-in RPC, no account strip, no bearer',
  ),
  SpecRow(
    elementId: 'login.account',
    caseClass: CaseClass.corner,
    name: 'auto_mode_without_issuers_runs_anonymously',
    description: 'auto mode against an agent with no browser sign-in keeps '
        'working as before S13',
    expectedRpc: 'agent.v1.AuthService/Issuers',
  ),
  SpecRow(
    elementId: 'login.account',
    caseClass: CaseClass.corner,
    name: 'stored_session_resumes',
    description: 'a live stored session is confirmed with WhoAmI, not a new '
        'sign-in',
    expectedRpc: 'agent.v1.AuthService/WhoAmI',
  ),
  SpecRow(
    elementId: 'login.issuer',
    caseClass: CaseClass.corner,
    name: 'preferred_issuer_narrows_buttons',
    description: 'PORTAL_AUTH_ISSUER offers only that issuer when listed',
    expectedRpc: 'agent.v1.AuthService/Issuers',
  ),
  // ── boundary ─────────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'login.account',
    caseClass: CaseClass.boundary,
    name: 'refresh_one_minute_before_expiry',
    description: 'the refresh is scheduled 60 s before expires_at, and at once '
        'when less than a minute is left',
    expectedRpc: 'agent.v1.AuthService/Exchange',
  ),
  // ── adversarial ──────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'login.error',
    caseClass: CaseClass.adversarial,
    name: 'callback_state_mismatch_rejected',
    description: 'a callback whose state this tab did not store is never '
        'exchanged, and the stored state is spent',
    expectedRpc: 'agent.v1.AuthService/Issuers',
  ),
  SpecRow(
    elementId: 'login.error',
    caseClass: CaseClass.adversarial,
    name: 'callback_without_pending_rejected',
    description: 'a replayed or planted ?code&state with nothing stored is '
        'never exchanged',
    expectedRpc: 'agent.v1.AuthService/Issuers',
  ),
  SpecRow(
    elementId: 'login.error',
    caseClass: CaseClass.adversarial,
    name: 'idp_error_text_not_echoed',
    description: 'markup in ?error= is replaced by "unknown", never rendered',
    expectedRpc: 'agent.v1.AuthService/Issuers',
  ),
  SpecRow(
    elementId: 'login.account',
    caseClass: CaseClass.positive,
    name: 'native_cli_login_signs_in',
    description: 'native: the CLI token is confirmed with WhoAmI and every call carries it',
    expectedRpc: 'agent.v1.AuthService/WhoAmI',
  ),
  SpecRow(
    elementId: 'login.cli.hint',
    caseClass: CaseClass.positive,
    name: 'native_without_cli_login_explains',
    description: 'native, not signed in with the CLI: the hint says to run agent login',
    expectedRpc: 'agent.v1.AuthService/Issuers',
  ),
  SpecRow(
    elementId: 'login.retry',
    caseClass: CaseClass.positive,
    name: 'native_retry_runs_the_cli_again',
    description: 'native: Try again re-runs the CLI and signs in',
    expectedRpc: 'agent.v1.AuthService/WhoAmI',
  ),
  SpecRow(
    elementId: 'login.account',
    caseClass: CaseClass.corner,
    name: 'native_refresh_asks_the_cli',
    description: 'native: the refresh timer re-runs the CLI 20 s before expiry and swaps the bearer',
    expectedRpc: 'agent.v1.AuthService/WhoAmI',
  ),
  SpecRow(
    elementId: 'login.error',
    caseClass: CaseClass.negative,
    name: 'native_cli_token_refused_by_agent',
    description: 'native: a CLI token the agent refuses shows why; the shell stays hidden',
    expectedRpc: 'agent.v1.AuthService/WhoAmI',
  ),
  SpecRow(
    elementId: 'login.signout',
    caseClass: CaseClass.negative,
    name: 'native_signout_keeps_the_cli_session',
    description: 'native: signing out sends no Logout, so the terminal stays signed in',
  ),
  SpecRow(
    elementId: 'login.error',
    caseClass: CaseClass.adversarial,
    name: 'native_hostile_cli_output_refused',
    description: 'native: CLI output with a CR/LF in the token is refused before any call',
  ),
]);
