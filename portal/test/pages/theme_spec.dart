import '../testkit/spec.dart';

/// Theme picker spec (visual/05-portal-integration.md#choosing-a-theme): the
/// nav-rail menu (`theme.menu`) and the Appearance cards (`theme.card`). Signed
/// in, the choice lives on the server (`ThemeService`, visual/04) and browser
/// storage is a per-user cache.
const themeSpec = PageSpec('theme', [
  SpecRow(
    elementId: 'theme.menu',
    caseClass: CaseClass.positive,
    name: 'menu_selects_theme',
    description: 'picking Windows from the rail menu re-themes the app and saves the id',
  ),
  SpecRow(
    elementId: 'theme.menu',
    caseClass: CaseClass.negative,
    name: 'no_scope_renders_nothing',
    description: 'without a ThemeScope the pickers render nothing and do not throw',
  ),
  SpecRow(
    elementId: 'theme.card',
    caseClass: CaseClass.positive,
    name: 'card_selects_theme',
    description: 'tapping the 3270 card switches to its dark-only palette',
  ),
  SpecRow(
    elementId: 'theme.card',
    caseClass: CaseClass.corner,
    name: 'selected_card_is_announced',
    description: 'a screen reader hears the active card as a selected button',
  ),
  SpecRow(
    elementId: 'theme.card',
    caseClass: CaseClass.boundary,
    name: 'every_builtin_has_a_card',
    description: 'all five built-ins get a card, in the built-in order',
  ),
  SpecRow(
    elementId: 'theme.card',
    caseClass: CaseClass.adversarial,
    name: 'hostile_saved_id_ignored',
    description: 'a traversal string in storage selects nothing; Prototype is active',
  ),
  SpecRow(
    elementId: 'theme.menu',
    caseClass: CaseClass.positive,
    name: 'sign_in_loads_server_theme',
    description: "signing in paints the user's theme and mode from GetActive",
    expectedRpc: 'agent.v1.ThemeService/GetActive',
  ),
  SpecRow(
    elementId: 'theme.card',
    caseClass: CaseClass.positive,
    name: 'pick_saved_on_server',
    description: 'tapping a card while signed in sends SetActive with the id and mode',
    expectedRpc: 'agent.v1.ThemeService/SetActive',
  ),
  SpecRow(
    elementId: 'theme.card',
    caseClass: CaseClass.positive,
    name: 'user_themes_listed',
    description: "the tenant's shared and the user's private themes get cards after the built-ins",
    expectedRpc: 'agent.v1.ThemeService/List',
  ),
  SpecRow(
    elementId: 'theme.card',
    caseClass: CaseClass.negative,
    name: 'server_down_keeps_cache',
    description: "GetActive failing keeps the user's cached theme painted",
    expectedRpc: 'agent.v1.ThemeService/GetActive',
  ),
  SpecRow(
    elementId: 'theme.card',
    caseClass: CaseClass.adversarial,
    name: 'hostile_server_theme_rejected',
    description: 'an active or listed theme that fails validation is never painted or offered',
    expectedRpc: 'agent.v1.ThemeService/GetActive',
  ),
  SpecRow(
    elementId: 'theme.card',
    caseClass: CaseClass.adversarial,
    name: 'listed_theme_cannot_replace_builtin',
    description: "a listed user theme carrying a built-in's id does not replace the bundled card",
    expectedRpc: 'agent.v1.ThemeService/List',
  ),
]);
