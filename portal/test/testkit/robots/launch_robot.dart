import 'package:agent_portal/src/config.dart';
import 'package:agent_portal/src/pages/launcher_page.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'robot.dart';

/// Robot for the Launch page. The page has **no backend** — its cards open URLs
/// via `url_launcher`, a platform channel — so this robot mocks that channel to
/// record the opened URL and script success/failure (the `positive_open` /
/// `negative_launch_blocked` rows), proving the action is `local` (no gRPC).
class LaunchRobot extends Robot {
  LaunchRobot(super.tester, {this.config = const PortalConfig()}) {
    const channel = MethodChannel('plugins.flutter.io/url_launcher');
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(channel,
        (call) async {
      calls.add(call);
      if (call.method == 'launch' || call.method == 'launchUrl') {
        final args = call.arguments;
        final url = args is Map ? args['url'] as String? : args as String?;
        if (url != null) openedUrls.add(url);
        return launchSucceeds;
      }
      if (call.method == 'canLaunch') return true;
      return null;
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null));
  }

  final PortalConfig config;

  /// Every platform-channel call the page made — checked as a set to prove the
  /// action is browser-local (no gRPC ever fires; there is no gateway here).
  final List<MethodCall> calls = [];
  final List<String> openedUrls = [];

  /// Scripts whether `url_launcher` reports success; false drives the SnackBar.
  bool launchSucceeds = true;

  Future<void> load() => pumpPage(LauncherPage(config: config));

  Future<void> tapCard(String name) async {
    await tester.tap(byKey('launch.card.$name'));
    await tester.pumpAndSettle();
  }

  bool snackContains(String text) =>
      find.textContaining(text).evaluate().isNotEmpty;
}
