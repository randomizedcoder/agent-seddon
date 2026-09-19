import 'package:agent_portal/src/pages/prompts_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../fake_gateway.dart';
import '../fakes/prompt_service.dart';
import 'robot.dart';

/// Robot for the Prompts page. Owns a [FakeGateway] (started on an ephemeral
/// loopback port) and the `PortalClients` the page dials; both are torn down —
/// in `runAsync`, since socket close needs the real event loop — automatically.
/// Script responses via `gw.prompts` before calling [load].
class PromptsRobot extends Robot {
  PromptsRobot._(super.tester, this.gw, this.prompts);

  final FakeGateway gw;

  /// The `PromptService` fake (scriptable responses + fault/slow injection).
  final FakePromptService prompts;
  late final clients = gw.clients();

  static Future<PromptsRobot> create(WidgetTester tester) async {
    late FakeGateway gw;
    late FakePromptService prompts;
    await tester.runAsync(() async {
      gw = await FakeGateway.start((log) {
        prompts = FakePromptService(log);
        return [prompts];
      });
    });
    final robot = PromptsRobot._(tester, gw, prompts);
    addTearDown(() => tester.runAsync(() async {
          await robot.clients.shutdown();
          await gw.shutdown();
        }));
    return robot;
  }

  // ── async-state predicates ────────────────────────────────────────────────
  bool get isLoaded => exists('prompts.personality.dropdown');
  bool get isError => exists('prompts.error.retry');
  bool get showsSpinner =>
      find.byType(CircularProgressIndicator).evaluate().isNotEmpty;

  /// Pump the page and hold it in the initial **loading** state (one frame, no
  /// settle) — the RPCs are still in flight.
  Future<void> pumpLoading() => pumpPage(PromptsPage(clients: clients));

  /// Pump the page and settle to its terminal state (loaded or error).
  Future<void> load() async {
    await pumpPage(PromptsPage(clients: clients));
    await pumpUntil(() => isLoaded || isError, reason: 'prompts to settle');
  }

  // ── intent actions ────────────────────────────────────────────────────────
  Future<void> tapRetry() => tap('prompts.error.retry', until: () => isLoaded);

  Future<void> selectItem(String kindName, String id) =>
      tap('prompts.list.item.$kindName.$id');

  Future<void> editContent(String text) => enterText('prompts.editor', text);

  // Save/Delete: wait until the RPC is *recorded* (the fake records before it may
  // throw, so this signal holds for both the success and the error rows). The
  // success path also reloads (a second List) — the test waits for that itself.
  Future<void> save() =>
      tap('prompts.save', until: () => gw.log.fired('agent.v1.PromptService/Put'));

  Future<void> delete() => tap('prompts.delete',
      until: () => gw.log.fired('agent.v1.PromptService/Delete'));

  Future<void> waitForReload(int listCount) =>
      pumpUntil(() => gw.log.countOf('agent.v1.PromptService/List') >= listCount,
          reason: 'reload (List x$listCount)');

  Future<void> preview() =>
      tap('prompts.preview', until: () => exists('prompts.preview.close'));

  Future<void> closePreview() => tap('prompts.preview.close');

  Future<void> togglePersist() => tap('prompts.personality.persist');

  /// Open the personality dropdown (a local menu animation).
  Future<void> openPersonalityMenu() async {
    await tester.tap(byKey('prompts.personality.dropdown'));
    await tester.pumpAndSettle();
  }

  /// Tap the open menu's option by its visible [label] (e.g. `pi`). A
  /// `DropdownButton` renders every item twice — offstage in the button's
  /// IndexedStack (for sizing) and in the open menu route — so an unfiltered
  /// finder resolves to the offstage copy and the tap misses. The item `Key`
  /// sits on the (non-hit-testable) `DropdownMenuItem`, so we select the on-screen
  /// menu entry by its `Text` filtered with `.hitTestable()`. Local action; the
  /// switch RPC it fires progresses in the next settle.
  Future<void> tapPersonalityItem(String label) async {
    await tester.tap(find.text(label).hitTestable());
    await tester.pump();
  }

  /// Open the dropdown, choose [name], and settle the in-flight switch (and its
  /// confirmation SnackBar's auto-dismiss timer, so it doesn't outlive the test).
  Future<void> choosePersonality(String name) async {
    await openPersonalityMenu();
    await tapPersonalityItem(name);
    await pumpUntil(() => personalityDropdownEnabled, reason: 'switch to settle');
    await settleSnackbars();
  }

  /// Advance the fake clock past the two timers a completed switch leaves behind
  /// so neither leaks past the test (`!timersPending`): the SnackBar's ~4 s
  /// auto-dismiss timer, and grpc-dart's HTTP/2 **connection idle timeout** (5 min,
  /// scheduled once the connection goes idle after the last stream closes).
  Future<void> settleSnackbars() async {
    await tester.pump(const Duration(seconds: 5)); // SnackBar auto-dismiss
    await tester.pump(const Duration(minutes: 6)); // HTTP/2 idle close
    await tester.pumpAndSettle();
  }

  bool get personalityDropdownEnabled {
    final w = tester.widget<DropdownButton<String>>(
        byKey('prompts.personality.dropdown'));
    return w.onChanged != null;
  }

  bool snackContains(String text) =>
      find.textContaining(text).evaluate().isNotEmpty;

  int get listItemCount => find
      .byWidgetPredicate((w) =>
          w.key is ValueKey<String> &&
          (w.key as ValueKey<String>).value.startsWith('prompts.list.item.'))
      .evaluate()
      .length;
}
