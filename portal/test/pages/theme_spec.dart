import '../testkit/spec.dart';

/// Theme picker spec (visual/05-portal-integration.md#choosing-a-theme): the
/// nav-rail menu (`theme.menu`) and the Appearance cards (`theme.card`). Local
/// only — the choice is saved in browser storage until increment 3.
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
]);
