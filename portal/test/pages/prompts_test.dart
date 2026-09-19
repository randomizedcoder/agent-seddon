import 'package:agent_portal/src/gen/agent/v1/prompt.pb.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../testkit/builders.dart';
import '../testkit/robots/prompts_robot.dart';
import 'prompts_spec.dart';

/// Layer-A widget tests for the Prompts page — iterates [promptsSpec]; each row
/// is one test. The rich template: the four async states, the resilience matrix
/// (error / slow-no-double-submit), and an `adversarial_` free-text row. Each row
/// arranges the fake, drives the page via its robot, then asserts the recorded
/// RPC (method + decoded request) and the widget outcome.
void main() {
  // A canned System entry so the editor pane has a selection to Save/Delete.
  PromptEntry sysEntry() =>
      promptEntry(id: 'sys', content: 'orig', kind: PromptKind.PROMPT_KIND_SYSTEM);

  for (final row in promptsSpec.rows) {
    testWidgets('prompts ${row.label} — ${row.description}', (tester) async {
      final robot = await PromptsRobot.create(tester);
      final fake = robot.prompts;

      switch (row.label) {
        case 'positive_loaded_lists_grouped':
          fake.listResponse = promptList([sysEntry()]);
          await robot.load();
          expect(robot.isLoaded, isTrue);
          expect(robot.listItemCount, greaterThanOrEqualTo(1));
          expect(robot.gw.log.fired('agent.v1.PromptService/List'), isTrue);
          expect(
              robot.gw.log.fired('agent.v1.PromptService/GetActivePersonality'),
              isTrue);

        case 'boundary_empty_list_no_items':
          fake.listResponse = promptList([]);
          await robot.load();
          expect(robot.isLoaded, isTrue);
          expect(robot.listItemCount, 0);

        case 'corner_loading_shows_spinner':
          // Briefly hold the RPCs so the page is caught in its loading state,
          // then drain so no timer outlives the test.
          fake.listResponse = promptList([sysEntry()]);
          fake.responseDelay = const Duration(milliseconds: 200);
          await robot.pumpLoading();
          expect(robot.showsSpinner, isTrue);
          expect(robot.isLoaded, isFalse);
          await robot.pumpUntil(() => robot.isLoaded, reason: 'drain load');

        case 'negative_list_error_shows_retry':
          fake.error = const GrpcError.unavailable('gateway down');
          await robot.load();
          expect(robot.isError, isTrue);

        case 'positive_retry_recovers':
          fake.error = const GrpcError.unavailable('gateway down');
          await robot.load();
          expect(robot.isError, isTrue);
          fake.listResponse = promptList([sysEntry()]);
          await robot.tapRetry();
          expect(robot.isLoaded, isTrue);

        case 'positive_edit_updates_field':
          fake.listResponse = promptList([sysEntry()]);
          await robot.load();
          await robot.editContent('edited body');
          expect(find.text('edited body').evaluate().isNotEmpty, isTrue);

        case 'positive_save_puts_and_reloads':
          fake.listResponse = promptList([sysEntry()]);
          await robot.load();
          await robot.editContent('new body');
          await robot.save();
          final put = robot.gw.log.last('agent.v1.PromptService/Put');
          expect(put, isNotNull);
          final req = put!.request as PromptEntry;
          expect(req.kind, PromptKind.PROMPT_KIND_SYSTEM);
          expect(req.id, 'sys');
          expect(req.content, 'new body');
          // Reloaded after save (a second List).
          await robot.waitForReload(2);
          expect(robot.gw.log.countOf('agent.v1.PromptService/List'), 2);

        case 'negative_save_error_snacks':
          fake.listResponse = promptList([sysEntry()]);
          await robot.load();
          fake.error = const GrpcError.permissionDenied('nope');
          await robot.save();
          await robot.pumpUntil(() => robot.snackContains('Save failed'),
              reason: 'save-failed snack');
          expect(robot.isLoaded, isTrue); // no crash, no reload

        case 'adversarial_huge_content_is_sent_verbatim':
          fake.listResponse = promptList([sysEntry()]);
          await robot.load();
          final huge = 'A' * 200000;
          await robot.editContent(huge);
          await robot.save();
          final put = robot.gw.log.last('agent.v1.PromptService/Put');
          expect((put!.request as PromptEntry).content.length, 200000);
          await robot.waitForReload(2); // drain the post-save reload

        case 'positive_delete_calls_delete':
          fake.listResponse = promptList([sysEntry()]);
          fake.deleteResponse = DeleteReply()..deleted = true;
          await robot.load();
          await robot.delete();
          final del = robot.gw.log.last('agent.v1.PromptService/Delete');
          expect((del!.request as PromptRef).id, 'sys');
          expect((del.request as PromptRef).kind, PromptKind.PROMPT_KIND_SYSTEM);
          await robot.waitForReload(2); // drain the post-delete reload

        case 'positive_preview_opens_dialog':
          fake.listResponse = promptList([sysEntry()]);
          await robot.load();
          await robot.preview();
          expect(robot.exists('prompts.preview.close'), isTrue);
          final prev = robot.gw.log.last('agent.v1.PromptService/PreviewAssembled');
          final preq = prev!.request as PreviewRequest;
          expect(preq.mode, 'implement');
          expect(preq.goal, 'do the task');

        case 'positive_close_dismisses_dialog':
          fake.listResponse = promptList([sysEntry()]);
          await robot.load();
          await robot.preview();
          expect(robot.exists('prompts.preview.close'), isTrue);
          await robot.closePreview();
          expect(robot.exists('prompts.preview.close'), isFalse);

        case 'positive_switch_sets_active':
          fake.listResponse = promptList([sysEntry()]);
          await robot.load();
          await robot.choosePersonality('pi');
          final set =
              robot.gw.log.last('agent.v1.PromptService/SetActivePersonality');
          final sreq = set!.request as SetActivePersonalityRequest;
          expect(sreq.id, 'pi');
          expect(sreq.persist, isFalse);

        case 'boundary_slow_switch_no_double_submit':
          fake.listResponse = promptList([sysEntry()]);
          await robot.load();
          fake.responseDelay = const Duration(milliseconds: 300);
          await robot.openPersonalityMenu();
          await robot.tapPersonalityItem('pi');
          // In-flight: the dropdown is disabled (the `_switching` guard).
          expect(robot.personalityDropdownEnabled, isFalse);
          await robot.pumpUntil(() => robot.personalityDropdownEnabled);
          // Exactly one call despite the guard window.
          expect(
              robot.gw.log
                  .countOf('agent.v1.PromptService/SetActivePersonality'),
              1);
          await robot.settleSnackbars();

        case 'positive_choose_named_base':
          fake.listResponse = promptList([sysEntry()]);
          await robot.load();
          await robot.choosePersonality('pi');
          expect(robot.gw.log.fired('agent.v1.PromptService/SetActivePersonality'),
              isTrue);

        case 'positive_persist_flag_sent':
          fake.listResponse = promptList([sysEntry()]);
          await robot.load();
          await robot.togglePersist();
          await robot.choosePersonality('pi');
          final set =
              robot.gw.log.last('agent.v1.PromptService/SetActivePersonality');
          expect((set!.request as SetActivePersonalityRequest).persist, isTrue);

        default:
          fail('no test body for row ${row.label}');
      }
    });
  }
}
