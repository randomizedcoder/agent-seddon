import '../testkit/spec.dart';

/// Settings page spec (design 02) — the schema-driven whole-config editor over
/// `ConfigService`. Covers the four async load states (GetSchema + GetValues +
/// Status), a `positive_` row for every keyed element scanned from
/// `settings_page.dart` **and** `schema_form.dart`, the resilience matrix
/// (negative / slow), and a mandatory `adversarial_` row on a schema-form field.
///
/// Key stems covered (family stems cover their dotted extensions):
///   settings.section.item · settings.form.field · settings.validate ·
///   settings.save · settings.revert · settings.retry
const settingsSpec = PageSpec('settings', [
  // ── page load: the four async states ──────────────────────────────────────
  SpecRow(
    elementId: 'settings.section.item',
    caseClass: CaseClass.positive,
    name: 'loaded_lists_sections',
    description: 'GetSchema + GetValues + Status fire; section list renders',
    expectedRpc: 'agent.v1.ConfigService/GetSchema',
  ),
  SpecRow(
    elementId: 'settings.section.item',
    caseClass: CaseClass.boundary,
    name: 'empty_schema_no_sections',
    description: 'a schema with no properties renders no section items, no crash',
    expectedRpc: 'agent.v1.ConfigService/GetSchema',
  ),
  SpecRow(
    elementId: 'settings.section.item',
    caseClass: CaseClass.corner,
    name: 'loading_shows_spinner',
    description: 'before the load RPCs resolve, a progress spinner is shown',
    expectedRpc: 'agent.v1.ConfigService/GetSchema',
  ),
  SpecRow(
    elementId: 'settings.retry',
    caseClass: CaseClass.negative,
    name: 'load_error_shows_retry',
    description: 'a failed GetSchema surfaces the offline retry panel, no crash',
    expectedRpc: 'agent.v1.ConfigService/GetSchema',
  ),
  SpecRow(
    elementId: 'settings.retry',
    caseClass: CaseClass.positive,
    name: 'retry_recovers',
    description: 'tapping Retry re-loads and shows the editor',
    expectedRpc: 'agent.v1.ConfigService/GetSchema',
  ),
  // ── schema form ───────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'settings.form.field',
    caseClass: CaseClass.positive,
    name: 'form_renders_typed_fields',
    description: 'the selected section renders string / bool / enum / number fields',
  ),
  SpecRow(
    elementId: 'settings.form.field',
    caseClass: CaseClass.boundary,
    name: 'secret_blank_keeps_value',
    description: 'a blank masked secret field emits no edit — stored secret preserved',
  ),
  SpecRow(
    elementId: 'settings.form.field',
    caseClass: CaseClass.adversarial,
    name: 'bad_number_produces_no_edit',
    description: 'non-numeric text in an integer field is rejected, nothing staged',
  ),
  // ── validate / save / revert ──────────────────────────────────────────────
  SpecRow(
    elementId: 'settings.validate',
    caseClass: CaseClass.positive,
    name: 'validate_sends_edits',
    description: 'editing a field then Validate fires Validate {edits}',
    expectedRpc: 'agent.v1.ConfigService/Validate',
  ),
  SpecRow(
    elementId: 'settings.validate',
    caseClass: CaseClass.negative,
    name: 'validate_issues_dialog',
    description: 'Validate returning issues shows the issues dialog, no save',
    expectedRpc: 'agent.v1.ConfigService/Validate',
  ),
  SpecRow(
    elementId: 'settings.save',
    caseClass: CaseClass.positive,
    name: 'save_puts_and_reloads',
    description: 'Save fires Put {dotted edits} then reloads (a second GetSchema)',
    expectedRpc: 'agent.v1.ConfigService/Put',
  ),
  SpecRow(
    elementId: 'settings.save',
    caseClass: CaseClass.negative,
    name: 'save_error_snacks',
    description: 'a failed Put shows a failure SnackBar, no crash',
    expectedRpc: 'agent.v1.ConfigService/Put',
  ),
  SpecRow(
    elementId: 'settings.revert',
    caseClass: CaseClass.positive,
    name: 'revert_discards_edits',
    description: 'Revert clears the staged edit; the section is no longer dirty',
  ),
]);
