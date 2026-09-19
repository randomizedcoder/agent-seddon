import 'package:agent_portal/src/pages/fleet_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../fake_gateway.dart';
import '../fakes/review_fleet_service.dart';
import '../recording.dart';
import 'robot.dart';

/// Robot for the Fleet page. Owns a [FakeGateway] (started on an ephemeral
/// loopback port) with the [FakeReviewFleetService] registered as an extra seam,
/// and the `PortalClients` the page dials; both are torn down — in `runAsync`,
/// since socket close needs the real event loop — automatically. Script
/// responses via `robot.fleet` before calling [load]; assert against `robot.log`.
///
/// The fleet fake carries its **own** [RecordingLog] (the one built here and
/// shared with the fake), exposed as [log] — `gw.log` is the gateway's default
/// prompts log and is unused by these tests.
///
/// Two page quirks the robot papers over:
///   * the detail pane renders a `Markdown` scroll view, whose `RenderViewport`
///     trips `debugVisitOnstageChildren` when a finder walks the tree mid-frame —
///     so every probe here uses `skipOffstage: false` (which walks with
///     `visitChildren`, not the onstage variant);
///   * the Review-now dialog disposes its `TextEditingController` synchronously
///     when `showDialog` resolves, so animating the dialog *out* would rebuild a
///     disposed field — the dialog is closed with a single `pump(duration)` that
///     retires the route before the build phase (see [_closeDialog]).
class FleetRobot extends Robot {
  FleetRobot._(super.tester, this.gw, this.fleet, this.log);

  final FakeGateway gw;
  final FakeReviewFleetService fleet;
  final RecordingLog log;
  late final clients = gw.clients();

  static const _list = 'agent.v1.ReviewFleetService/ListReviews';
  static const _approve = 'agent.v1.ReviewFleetService/Approve';
  static const _update = 'agent.v1.ReviewFleetService/UpdateReview';
  static const _reviewNow = 'agent.v1.ReviewFleetService/ReviewNow';
  static const _setEnabled = 'agent.v1.ReviewFleetService/SetEnabled';

  static Future<FleetRobot> create(WidgetTester tester) async {
    late FakeReviewFleetService fleet;
    late FakeGateway gw;
    await tester.runAsync(() async {
      gw = await FakeGateway.start((log) {
        fleet = FakeReviewFleetService(log);
        return [fleet];
      });
    });
    final robot = FleetRobot._(tester, gw, fleet, gw.log);
    addTearDown(() => tester.runAsync(() async {
          await robot.clients.shutdown();
          await gw.shutdown();
        }));
    return robot;
  }

  // ── offstage-safe finders (avoid the Markdown viewport crash) ───────────────
  @override
  Finder byKey(String key) => find.byKey(Key(key), skipOffstage: false);

  @override
  bool exists(String key) => byKey(key).evaluate().isNotEmpty;

  bool textShown(String text) =>
      find.textContaining(text, skipOffstage: false).evaluate().isNotEmpty;

  // ── async-state predicates ────────────────────────────────────────────────
  /// The list settled to its loaded state (the two-pane body; the Sessions strip
  /// header is always present once loaded).
  bool get isListLoaded => textShown('Sessions (');
  bool get isError => exists('fleet.error.retry');
  bool get showsSpinner =>
      find.byType(CircularProgressIndicator, skipOffstage: false)
          .evaluate()
          .isNotEmpty;

  /// Detail-pane states (after a review is selected).
  bool get detailLoaded => exists('fleet.detail.mode');
  bool get detailError => exists('fleet.detail.retry');
  bool get detailUnavailable => textShown('No stored body');
  bool get detailSettled => detailLoaded || detailError || detailUnavailable;

  int get reviewItemCount => find
      .byWidgetPredicate(
          (w) =>
              w.key is ValueKey<String> &&
              (w.key as ValueKey<String>).value.startsWith('fleet.review.item.'),
          skipOffstage: false)
      .evaluate()
      .length;

  /// Is the keyed button currently enabled? Reads `ButtonStyleButton.enabled`
  /// (dynamic, so it works for FilledButton / .icon / .tonalIcon alike).
  bool buttonEnabled(String key) =>
      (tester.widget(byKey(key)) as dynamic).enabled as bool;

  /// Whether the mode `SegmentedButton` segment with visible [label] is enabled
  /// (a locked draft disables Edit/Preview). Reads `ButtonSegment.enabled`
  /// dynamically since `_DetailMode` is private to the page.
  bool segmentEnabled(String label) {
    final w = tester.widget(byKey('fleet.detail.mode'));
    for (final s in (w as dynamic).segments as List) {
      final lbl = (s as dynamic).label;
      if (lbl is Text && lbl.data == label) {
        return ((s as dynamic).enabled as bool?) ?? true;
      }
    }
    return false;
  }

  // ── page lifecycle ─────────────────────────────────────────────────────────
  /// Pump the page and hold it in the initial loading state (one frame).
  Future<void> pumpLoading() => pumpPage(FleetPage(clients: clients));

  /// Pump the page and settle to its terminal list state (loaded or error).
  Future<void> load() async {
    await pumpPage(FleetPage(clients: clients));
    await pumpUntil(() => isListLoaded || isError, reason: 'fleet list to settle');
  }

  /// The whole-view offline panel keys the `_OfflineRetry` widget, not its button
  /// — so the Retry button is addressed by its label text.
  Future<void> tapRetry() async {
    await tester.runAsync(() async {
      await tester.tap(find.text('Retry').hitTestable());
    });
    await tester.pump();
    await pumpUntil(() => isListLoaded || isError, reason: 'retry to settle');
  }

  // ── filter bar ─────────────────────────────────────────────────────────────
  Future<void> tapRefresh() {
    final before = log.countOf(_list);
    return tap('fleet.filter.refresh',
        until: () => log.countOf(_list) > before);
  }

  Future<void> submitRepo(String repo) async {
    final before = log.countOf(_list);
    await enterText('fleet.filter.repo', repo);
    await tester.runAsync(() async {
      await tester.testTextInput.receiveAction(TextInputAction.done);
    });
    await pumpUntil(() => log.countOf(_list) > before, reason: 'repo submit');
  }

  Future<void> selectStatus(String label) async {
    final before = log.countOf(_list);
    await tester.tap(byKey('fleet.filter.status'));
    await tester.pumpAndSettle(); // open the menu (no RPC, no text field)
    await tester.tap(find.text(label, skipOffstage: false).hitTestable());
    await tester.pump(); // selecting fires _reload — settle it via the RPC below
    await pumpUntil(() => log.countOf(_list) > before, reason: 'status filter');
  }

  // ── sessions strip ──────────────────────────────────────────────────────────
  Future<void> expandSessions() async {
    await tester.tap(find.textContaining('Sessions (', skipOffstage: false).first);
    await tester.pumpAndSettle();
  }

  /// Open the Review-now dialog. NOT `pumpAndSettle` — its autofocus TextField
  /// runs a periodic cursor-blink timer that never lets the tree go idle; a fixed
  /// pump past the enter transition is enough.
  Future<void> openReviewNow(String id) async {
    await tester.tap(byKey('fleet.session.reviewNow.$id'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
  }

  /// The Review-now dialog's page code disposes its `TextEditingController` the
  /// instant `showDialog` resolves (harmless in release — the disposed-use assert
  /// is debug-only — but it fires under the test's debug build). Animating the
  /// dialog *out* would rebuild the disposed field mid-transition and trip that
  /// assert, so once the dialog's button has popped it (and any RPC has been let
  /// fly on the real loop) we retire the whole tree with `pumpWidget` — an
  /// unmount, which never re-touches the disposed controller.
  Future<void> _drainReal({bool Function()? until}) => real(() async {
        for (var i = 0; i < 200; i++) {
          if (until != null && until()) return;
          await Future<void>.delayed(const Duration(milliseconds: 5));
        }
      });

  Future<void> _unmount() => tester.pumpWidget(const SizedBox());

  Future<void> queueReviewNow(String pr) async {
    await enterText('fleet.reviewNow.prNumber', pr);
    await tester.tap(byKey('fleet.reviewNow.queue'));
    await _drainReal(until: () => log.fired(_reviewNow));
    await _unmount();
  }

  Future<void> cancelReviewNow() async {
    await tester.tap(byKey('fleet.reviewNow.cancel'));
    await _drainReal();
    await _unmount();
  }

  Future<void> toggleEnable(String id) async {
    final before = log.countOf(_setEnabled);
    await tap('fleet.session.enable.$id',
        until: () => log.countOf(_setEnabled) > before);
  }

  // ── detail pane ─────────────────────────────────────────────────────────────
  Future<void> selectReview(String id) =>
      tap('fleet.review.item.$id', until: () => detailSettled);

  Future<void> selectMode(String label) async {
    await tester.tap(find.text(label, skipOffstage: false).hitTestable());
    await tester.pump();
  }

  Future<void> editBody(String text) => enterText('fleet.detail.editor', text);

  Future<void> save() {
    final before = log.countOf(_update);
    return tap('fleet.detail.save', until: () => log.countOf(_update) > before);
  }

  Future<void> tapDetailRetry() async {
    await tester.runAsync(() async {
      await tester.tap(find.text('Retry').hitTestable());
    });
    await tester.pump();
    await pumpUntil(() => detailSettled, reason: 'detail retry to settle');
  }

  // ── approve dialog (no controller — a plain confirm) ────────────────────────
  Future<void> openApprove() async {
    await tester.tap(byKey('fleet.detail.approve'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
  }

  Future<void> confirmApprove() async {
    final before = log.countOf(_approve);
    await tester.runAsync(() async {
      await tester.tap(byKey('fleet.approve.confirm'));
    });
    await tester.pump();
    await pumpUntil(() => log.countOf(_approve) > before, reason: 'approve fires');
  }

  Future<void> cancelApprove() async {
    await tester.tap(byKey('fleet.approve.cancel'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
  }

  /// Rapid double-tap of the confirm button with **no frame between the taps**:
  /// the first tap pops the dialog and dispatches Approve; the second lands on the
  /// still-mounted button before the route leaves. The guarantee under test is
  /// that `showDialog` resolves exactly once per `_approve`, so **one** Approve
  /// fires regardless (the second tap merely pops the route beneath). Follow with
  /// [unmountAndSettle] — the second pop can retire the page route, so the
  /// post-state is intentionally not asserted.
  Future<void> doubleConfirmApprove() async {
    await tester.tap(byKey('fleet.approve.confirm'), warnIfMissed: false);
    await tester.tap(byKey('fleet.approve.confirm'), warnIfMissed: false);
    await tester.pump();
    await pumpUntil(() => log.fired(_approve), reason: 'one approve to fire');
  }

  /// Let any in-flight RPC finish on the real loop, then retire the whole tree
  /// (an unmount, not an animated close) and drain the fake-clock timers — used
  /// where the page's post-state is deliberately torn (the double-click row).
  Future<void> unmountAndSettle() async {
    await _drainReal();
    await _unmount();
    await tester.pump(const Duration(seconds: 5));
    await tester.pump(const Duration(minutes: 6));
    await tester.pumpAndSettle();
  }

  // ── settle ──────────────────────────────────────────────────────────────────
  /// Advance the fake clock past trailing timers a completed action leaves: the
  /// SnackBar's ~4 s auto-dismiss and grpc-dart's HTTP/2 5-min idle-close (both
  /// leak as `!timersPending` otherwise).
  Future<void> settle() async {
    // Drain any in-flight reload/fetch on the REAL loop first: a lingering
    // `_loading` spinner is an infinite animation that `pumpAndSettle` can never
    // settle. `pumpUntil` advances real async, so the RPC completes and the
    // spinner clears before we drain the fake-clock timers.
    await pumpUntil(() => !showsSpinner, reason: 'spinner to clear');
    await tester.pump(const Duration(seconds: 5)); // SnackBar auto-dismiss
    await tester.pump(const Duration(minutes: 6)); // HTTP/2 idle close
    await tester.pumpAndSettle();
  }
}
