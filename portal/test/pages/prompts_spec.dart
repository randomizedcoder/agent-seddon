import '../testkit/spec.dart';

/// Prompts page spec (design 02) — the rich template: the four async states,
/// the resilience matrix (`negative_`/slow), and a mandatory `adversarial_` row
/// for the free-text editor. Every key stem in `prompts_page.dart` has a
/// `positive_` row, so the completeness critic passes for this page.
const promptsSpec = PageSpec('prompts', [
  // ── page load: the four async states ──────────────────────────────────────
  SpecRow(
    elementId: 'prompts.list.item',
    caseClass: CaseClass.positive,
    name: 'loaded_lists_grouped',
    description: 'List + GetActivePersonality fire; entries render grouped',
    expectedRpc: 'agent.v1.PromptService/List',
  ),
  SpecRow(
    elementId: 'prompts.list.item',
    caseClass: CaseClass.boundary,
    name: 'empty_list_no_items',
    description: 'an empty list renders no items and does not crash',
    expectedRpc: 'agent.v1.PromptService/List',
  ),
  SpecRow(
    elementId: 'prompts.list.item',
    caseClass: CaseClass.corner,
    name: 'loading_shows_spinner',
    description: 'before the RPCs resolve, a progress spinner is shown',
    expectedRpc: 'agent.v1.PromptService/List',
  ),
  SpecRow(
    elementId: 'prompts.error.retry',
    caseClass: CaseClass.negative,
    name: 'list_error_shows_retry',
    description: 'a failed List surfaces the offline retry panel, no crash',
    expectedRpc: 'agent.v1.PromptService/List',
  ),
  SpecRow(
    elementId: 'prompts.error.retry',
    caseClass: CaseClass.positive,
    name: 'retry_recovers',
    description: 'tapping Retry re-lists and shows the loaded UI',
    expectedRpc: 'agent.v1.PromptService/List',
  ),
  // ── editor + CRUD ─────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'prompts.editor',
    caseClass: CaseClass.positive,
    name: 'edit_updates_field',
    description: 'typing updates the editor field',
  ),
  SpecRow(
    elementId: 'prompts.save',
    caseClass: CaseClass.positive,
    name: 'save_puts_and_reloads',
    description: 'Save fires Put {kind,id,content} then reloads',
    expectedRpc: 'agent.v1.PromptService/Put',
  ),
  SpecRow(
    elementId: 'prompts.save',
    caseClass: CaseClass.negative,
    name: 'save_error_snacks',
    description: 'a failed Put shows a failure SnackBar, no crash',
    expectedRpc: 'agent.v1.PromptService/Put',
  ),
  SpecRow(
    elementId: 'prompts.save',
    caseClass: CaseClass.adversarial,
    name: 'huge_content_is_sent_verbatim',
    description: 'a huge editor payload is Put without truncation or crash',
    expectedRpc: 'agent.v1.PromptService/Put',
  ),
  SpecRow(
    elementId: 'prompts.delete',
    caseClass: CaseClass.positive,
    name: 'delete_calls_delete',
    description: 'Reset/delete fires Delete {kind,id}',
    expectedRpc: 'agent.v1.PromptService/Delete',
  ),
  SpecRow(
    elementId: 'prompts.preview',
    caseClass: CaseClass.positive,
    name: 'preview_opens_dialog',
    description: 'Preview fires PreviewAssembled and shows the assembled dialog',
    expectedRpc: 'agent.v1.PromptService/PreviewAssembled',
  ),
  SpecRow(
    elementId: 'prompts.preview.close',
    caseClass: CaseClass.positive,
    name: 'close_dismisses_dialog',
    description: 'Close dismisses the preview dialog',
  ),
  // ── personality selector ──────────────────────────────────────────────────
  SpecRow(
    elementId: 'prompts.personality.dropdown',
    caseClass: CaseClass.positive,
    name: 'switch_sets_active',
    description: 'choosing a personality fires SetActivePersonality {id}',
    expectedRpc: 'agent.v1.PromptService/SetActivePersonality',
  ),
  SpecRow(
    elementId: 'prompts.personality.dropdown',
    caseClass: CaseClass.boundary,
    name: 'slow_switch_no_double_submit',
    description: 'while a switch is in flight the dropdown is disabled — one call',
    expectedRpc: 'agent.v1.PromptService/SetActivePersonality',
  ),
  SpecRow(
    elementId: 'prompts.personality.item',
    caseClass: CaseClass.positive,
    name: 'choose_named_base',
    description: 'selecting the "pi" item switches to that base',
    expectedRpc: 'agent.v1.PromptService/SetActivePersonality',
  ),
  SpecRow(
    elementId: 'prompts.personality.persist',
    caseClass: CaseClass.positive,
    name: 'persist_flag_sent',
    description: 'with persist checked, the next switch sends persist=true',
    expectedRpc: 'agent.v1.PromptService/SetActivePersonality',
  ),
]);
