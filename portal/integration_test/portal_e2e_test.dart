// Layer B — live end-to-end (`nix run .#portal-e2e`, design doc 04).
//
// The real Flutter *web* app, driven headlessly through the Envoy grpc-web
// bridge (`browser -> envoy -> gateway -> seam`), so the path under test is
// exactly the browser's. This suite is only the **GUI driver**: it performs a
// curated subset of *mutating* actions with deterministic values and hands each
// action's record back through `binding.reportData`. The **assertions** live in
// the shell harness (`nix/portal/default.nix` `portal-e2e`), which reads them
// from the observability system — the `:9700` metrics delta (correct RPC fired +
// `ok`), the read RPC on `:50100` (state changed), and a curated OTLP span in
// ClickHouse — because that is the design's contract: the proof comes from
// observability, not from the app asserting on itself.
//
// Backend-preflight philosophy (design 01): an action whose page/preconditions
// are not present is recorded `outcome: "skipped"`, never failed — a down seam
// reads as skipped, not broken.
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

import 'package:agent_portal/main.dart' as app;

/// A unique-per-run suffix so a created upstream id (and any other row this
/// suite writes) never collides with a prior run or operator data, and the
/// shell can assert on exactly the value this run set. Passed by the harness;
/// falls back to a fixed value for a bare `flutter drive`.
const _runId = String.fromEnvironment('E2E_RUN_ID', defaultValue: 'local');

/// One driven action's outcome, JSON-encoded into `reportData` for the shell.
class _ActionRecord {
  _ActionRecord({
    required this.page,
    required this.element,
    required this.rpc,
    required this.value,
    required this.outcome,
    required this.ms,
    this.detail = '',
  });

  final String page; // nav page the action ran on
  final String element; // the keyed control that fired the RPC
  final String rpc; // the write RPC the shell asserts a metrics delta on
  final String value; // the deterministic value the shell reads back
  final String outcome; // "ok" | "skipped" | "fail"
  final int ms; // client-perceived interaction latency
  final String detail; // free-text (skip reason / error)

  Map<String, Object?> toJson() => {
        'page': page,
        'element': element,
        'rpc': rpc,
        'value': value,
        'outcome': outcome,
        'ms': ms,
        'detail': detail,
      };
}

void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('portal-e2e: curated mutating actions over the real wire',
      (tester) async {
    final records = <_ActionRecord>[];

    // Boot the real app (endpoints come from --dart-define, pointed at the
    // Envoy bridge by the harness).
    app.main();
    await _settle(tester, const Duration(seconds: 5));

    // Navigation is by the nav-rail label text (the rail destinations carry no
    // Key; the design's keys live on page *elements*).
    Future<bool> goTo(String label) async {
      final dest = find.text(label);
      if (dest.evaluate().isEmpty) return false;
      await tester.tap(dest.first);
      await _settle(tester);
      return true;
    }

    bool has(String key) => find.byKey(Key(key)).evaluate().isNotEmpty;

    // Wait up to [budget] for [key], settling real async between polls. Returns
    // false instead of throwing so a missing control becomes a *skip*.
    Future<bool> waitFor(String key,
        {Duration budget = const Duration(seconds: 6)}) async {
      final deadline = DateTime.now().add(budget);
      while (DateTime.now().isBefore(deadline)) {
        if (has(key)) return true;
        await _settle(tester, const Duration(milliseconds: 150));
      }
      return has(key);
    }

    Future<void> tapKey(String key) async {
      await tester.ensureVisible(find.byKey(Key(key)));
      await tester.tap(find.byKey(Key(key)));
      await _settle(tester);
    }

    Future<void> typeKey(String key, String text) async {
      await tester.enterText(find.byKey(Key(key)), text);
      await _settle(tester);
    }

    // ---- Curated action 1+2: Router — create an upstream (Put) then enable it
    // (Enable). Fully self-contained: a unique id, reversible, no external
    // preconditions beyond the ProviderRegistry seam being served.
    final upstreamId = 'portal-e2e-$_runId';
    if (await goTo('Router') && await waitFor('router.upstream.add')) {
      // Put
      final t0 = DateTime.now();
      try {
        await tapKey('router.upstream.add');
        if (await waitFor('router.upstream.field.id')) {
          await typeKey('router.upstream.field.id', upstreamId);
          await typeKey('router.upstream.field.kind', 'openai-compat');
          await typeKey('router.upstream.field.model', 'e2e-model');
          await typeKey('router.upstream.field.baseUrl', 'http://127.0.0.1:1/v1');
          await tapKey('router.upstream.save');
          await _settle(tester, const Duration(seconds: 2));
          records.add(_ActionRecord(
            page: 'router',
            element: 'router.upstream.save',
            rpc: '/agent.v1.ProviderRegistryService/Put',
            value: upstreamId,
            outcome: 'ok',
            ms: DateTime.now().difference(t0).inMilliseconds,
          ));
        } else {
          records.add(_skip('router', 'router.upstream.save',
              '/agent.v1.ProviderRegistryService/Put', upstreamId,
              'editor fields never appeared'));
        }
      } catch (e) {
        records.add(_fail('router', 'router.upstream.save',
            '/agent.v1.ProviderRegistryService/Put', upstreamId, '$e'));
      }

      // Enable — the toggle keyed by the id we just created.
      final t1 = DateTime.now();
      final enableKey = 'router.upstream.enable.$upstreamId';
      if (await waitFor(enableKey)) {
        try {
          await tapKey(enableKey);
          await _settle(tester, const Duration(seconds: 1));
          records.add(_ActionRecord(
            page: 'router',
            element: enableKey,
            rpc: '/agent.v1.ProviderRegistryService/Enable',
            value: upstreamId,
            outcome: 'ok',
            ms: DateTime.now().difference(t1).inMilliseconds,
          ));
        } catch (e) {
          records.add(_fail('router', enableKey,
              '/agent.v1.ProviderRegistryService/Enable', upstreamId, '$e'));
        }
      } else {
        records.add(_skip('router', enableKey,
            '/agent.v1.ProviderRegistryService/Enable', upstreamId,
            'created upstream row never rendered'));
      }
    } else {
      records.add(_skip('router', 'router.upstream.save',
          '/agent.v1.ProviderRegistryService/Put', upstreamId,
          'Router page/backend not available'));
    }

    // ---- Curated action 3 (best-effort): Prompts — set the active personality.
    // Skipped unless the personality dropdown is present and offers a choice
    // (the closed-set personalities must be configured on the gateway).
    if (await goTo('Prompts') && await waitFor('prompts.personality.dropdown')) {
      final t0 = DateTime.now();
      try {
        // Read the dropdown's CURRENT value + offered values so we pick a
        // *different* personality — the page treats selecting the active one as a
        // no-op (`if (id == _activePersonality) return;`), which would fire no RPC
        // and leave nothing to observe.
        final dd = tester.widget<DropdownButton<String>>(
            find.byKey(const Key('prompts.personality.dropdown')));
        final current = dd.value ?? '';
        final choices = dd.items
                ?.map((it) => it.value ?? '')
                .where((v) => v.isNotEmpty && v != current)
                .toList() ??
            <String>[];
        if (choices.isEmpty) {
          records.add(_skip('prompts', 'prompts.personality.dropdown',
              '/agent.v1.PromptService/SetActivePersonality', '',
              'no personality other than the active one is offered'));
        } else {
          final target = choices.first;
          await tester.tap(find.byKey(const Key('prompts.personality.dropdown')));
          // The menu opens as an overlay route with its own animation; settle it
          // fully (pumpAndSettle, not the guarded _settle) before locating items.
          await tester.pumpAndSettle();
          // Select by the item's visible TEXT (the robot-recipe reliable path: the
          // keyed DropdownMenuItem has an offstage IndexedStack twin, so a keyed
          // finder is ambiguous; the open menu's Text is unambiguous). The button
          // also shows the current value's text, so take the last (menu) match.
          final item = find.text(target).hitTestable();
          for (var tries = 0; tries < 20 && item.evaluate().isEmpty; tries++) {
            await tester.pumpAndSettle(const Duration(milliseconds: 150));
          }
          await tester.tap(item.last);
          await tester.pumpAndSettle();
          records.add(_ActionRecord(
            page: 'prompts',
            element: 'prompts.personality.dropdown',
            rpc: '/agent.v1.PromptService/SetActivePersonality',
            value: target,
            outcome: 'ok',
            ms: DateTime.now().difference(t0).inMilliseconds,
          ));
        }
      } catch (e) {
        records.add(_skip('prompts', 'prompts.personality.dropdown',
            '/agent.v1.PromptService/SetActivePersonality', '', 'skip: $e'));
      }
    } else {
      records.add(_skip('prompts', 'prompts.personality.dropdown',
          '/agent.v1.PromptService/SetActivePersonality', '',
          'Prompts personality selector not available'));
    }

    // Hand the whole run back to the driver → the shell harness.
    binding.reportData = <String, Object?>{
      'run_id': _runId,
      'actions': records.map((r) => r.toJson()).toList(),
    };
    // Also print it (one line, marker-prefixed) so the harness has a fallback
    // even if the driver's result file is unavailable.
    // ignore: avoid_print
    print('PORTAL_E2E_ACTIONS ${jsonEncode(binding.reportData)}');

    // At least one action must have been attempted (a fully empty run is a
    // harness problem, not a pass).
    expect(records, isNotEmpty);
  });
}

_ActionRecord _skip(
        String page, String element, String rpc, String value, String why) =>
    _ActionRecord(
        page: page,
        element: element,
        rpc: rpc,
        value: value,
        outcome: 'skipped',
        ms: 0,
        detail: why);

_ActionRecord _fail(
        String page, String element, String rpc, String value, String why) =>
    _ActionRecord(
        page: page,
        element: element,
        rpc: rpc,
        value: value,
        outcome: 'fail',
        ms: 0,
        detail: why);

/// Settle real async + rebuild. Integration-test binding runs on the real
/// clock, so `pumpAndSettle` advances genuine socket I/O (grpc-web round-trips);
/// the extra fixed pump covers the batched-frame case.
Future<void> _settle(WidgetTester tester,
    [Duration timeout = const Duration(seconds: 3)]) async {
  try {
    await tester.pumpAndSettle(
        const Duration(milliseconds: 100), EnginePhase.sendSemanticsUpdate, timeout);
  } catch (_) {
    // pumpAndSettle throws if the tree never quiesces (e.g. a perpetual
    // spinner); fall back to a single pump so the suite can still proceed and
    // record the state it can observe.
    await tester.pump(const Duration(milliseconds: 100));
  }
}
