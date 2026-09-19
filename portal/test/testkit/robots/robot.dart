import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

/// Base for the page robots (the Page-Object pattern, design 01). A robot wraps a
/// page's `find.byKey` interactions behind intent methods so tests read as user
/// intent and a renamed `Key` is fixed in exactly one place. Robots are shared by
/// Layer A (here) and Layer B (inc 7), since both address the same keys.
///
/// ## Real-loopback pumping recipe
///
/// The portal's pages dial the in-process fake gRPC gateway over the true VM
/// `channel_io` **loopback socket** (that realism is the design's key claim). But
/// `flutter_test` runs page code under a *fake* async clock, which does not
/// advance real socket I/O — so any RPC a page fires only progresses inside
/// [WidgetTester.runAsync]. This base centralises the recipe once:
///
///   * pump on a **desktop-sized surface** (the portal is a desktop layout; the
///     default 800×600 test window overflows its Rows);
///   * drive gRPC-triggering work + teardown inside `runAsync`;
///   * [pumpUntil] alternates a real-async step with a `pump()` so the widget tree
///     deterministically settles on the post-RPC state (no fixed sleeps).
abstract class Robot {
  Robot(this.tester) {
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
  }

  final WidgetTester tester;

  Finder byKey(String key) => find.byKey(Key(key));

  bool exists(String key) => byKey(key).evaluate().isNotEmpty;

  /// Pump [child] inside a minimal MaterialApp shell (Scaffold → ScaffoldMessenger
  /// for SnackBars). `pumpWidget` runs inside `runAsync` so the page's
  /// initState-triggered gRPC futures live on the **real** event loop (else they
  /// never resolve under the test's fake clock); [pumpUntil] then settles them.
  Future<void> pumpPage(Widget child) async {
    await tester.runAsync(() async {
      await tester.pumpWidget(MaterialApp(home: Scaffold(body: child)));
    });
    await tester.pump();
  }

  /// Advance real async (so pending gRPC completes) then rebuild, repeating until
  /// [done] holds or the budget runs out. Deterministic — no guessed sleeps.
  Future<void> pumpUntil(
    bool Function() done, {
    int maxCycles = 80,
    Duration step = const Duration(milliseconds: 10),
    String? reason,
  }) async {
    for (var i = 0; i < maxCycles; i++) {
      if (done()) return;
      await tester.runAsync(() => Future<void>.delayed(step));
      await tester.pump();
    }
    if (!done()) {
      throw StateError('pumpUntil timed out${reason == null ? '' : ': $reason'}');
    }
  }

  Future<void> pumpUntilFound(String key) =>
      pumpUntil(() => exists(key), reason: 'waiting for $key');

  Future<void> pumpUntilGone(String key) =>
      pumpUntil(() => !exists(key), reason: 'waiting for $key to disappear');

  /// Tap a key, advancing real async for any RPC the tap fires, then settle.
  Future<void> tap(String key, {bool Function()? until}) async {
    await tester.runAsync(() async {
      await tester.tap(byKey(key));
    });
    await tester.pump();
    if (until != null) await pumpUntil(until);
  }

  /// Enter [text] into a keyed field (no RPC), then rebuild.
  Future<void> enterText(String key, String text) async {
    await tester.enterText(byKey(key), text);
    await tester.pump();
  }

  /// Run [body] with real async active (for teardown that shuts down channels /
  /// servers, whose socket close needs the real event loop).
  Future<void> real(Future<void> Function() body) => tester.runAsync(body);
}
