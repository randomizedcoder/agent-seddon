import 'package:agent_portal/src/auth/capabilities.dart';
import 'package:agent_portal/src/auth/install_callback.dart';
import 'package:agent_portal/src/gen/agent/v1/forge_registry.pb.dart';
import 'package:agent_portal/src/gen/agent/v1/github_install_registry.pb.dart';
import 'package:agent_portal/src/pages/forges_page.dart' show maxForgeIdLen;
import 'package:fixnum/fixnum.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../testkit/robots/forges_robot.dart';
import 'forges_spec.dart';

/// Layer-A widget tests for the Forges page — iterates [forgesSpec]; each row
/// is one test. Each arranges the fakes and the tab's storage, drives the page
/// through its robot, then asserts the recorded RPCs (method + decoded request)
/// and what the page shows.
void main() {
  const now = ForgesRobot.now;

  GithubInstall install(String id, {int repos = 2}) => GithubInstall(
        installationId: id,
        selectedRepos: [for (var i = 0; i < repos; i++) 'acme/r$i'],
        installedAt: Int64(now - 60),
      );

  ForgeCard card(String id, {String ref = 'ghapp:42', bool enabled = true}) =>
      ForgeCard(
        id: id,
        kind: 'github',
        enabled: enabled,
        tokenRef: ref,
        repoEncoding: 'owner_name',
      );

  Map<String, String> ret({
    String code = 'gh-code-123',
    String state = 'st-1',
    String id = '42',
    String action = 'install',
  }) =>
      {'code': code, 'state': state, 'installation_id': id, 'setup_action': action};

  for (final row in forgesSpec.rows) {
    testWidgets('forges ${row.label} — ${row.description}', (tester) async {
      switch (row.label) {
        // ── load ───────────────────────────────────────────────────────────
        case 'positive_loaded_lists_render':
          final robot = await ForgesRobot.create(tester);
          robot.installs.listResponse =
              GithubInstallList(installs: [install('42'), install('7', repos: 0)]);
          robot.forges.listResponse = ForgeList(forges: [card('github-42')]);
          await robot.load();
          expect(robot.exists('forges.install.item.42'), isTrue);
          expect(robot.exists('forges.install.item.7'), isTrue);
          expect(robot.exists('forges.card.item.github-42'), isTrue);
          expect(find.text('2 selected repos'), findsOneWidget);
          expect(robot.firedInstall('List'), isTrue);
          expect(robot.firedForge('List'), isTrue);
          expect(robot.firedInstall('CompleteInstall'), isFalse);

        case 'positive_no_installs_explains':
          final robot = await ForgesRobot.create(tester);
          await robot.load();
          expect(robot.exists('forges.install.empty'), isTrue);

        case 'positive_no_cards_explains':
          final robot = await ForgesRobot.create(tester);
          await robot.load();
          expect(robot.exists('forges.card.empty'), isTrue);

        case 'corner_loading_shows_spinner':
          final robot = await ForgesRobot.create(tester);
          await robot.pumpLoading();
          expect(robot.showsSpinner, isTrue);
          await robot.pumpUntil(() => robot.isLoaded, reason: 'drain load');

        case 'negative_list_error_shows_retry':
          final robot = await ForgesRobot.create(tester);
          robot.forges.listError = const GrpcError.unavailable('down');
          await robot.load();
          expect(robot.isError, isTrue);
          expect(find.textContaining('down'), findsOneWidget);

        case 'positive_retry_recovers':
          final robot = await ForgesRobot.create(tester);
          robot.forges.listError = const GrpcError.unavailable('down');
          await robot.load();
          robot.forges.listError = null;
          await robot.tap('forges.error.retry', until: () => robot.isLoaded);
          expect(robot.isError, isFalse);
          expect(robot.log.countOf('${ForgesRobot.installSvc}/List'), 2);

        // ── connect ────────────────────────────────────────────────────────
        case 'positive_connect_begins_and_leaves':
          final robot = await ForgesRobot.create(tester);
          await robot.load();
          await robot.tap('forges.github.connect',
              until: () => robot.platform.navigated.isNotEmpty);
          expect(robot.firedInstall('BeginInstall'), isTrue);
          expect(robot.platform.navigated.single,
              'https://github.com/apps/seddon/installations/new?state=st-1');
          expect(takeInstallPending(robot.platform, now), 'st-1');

        case 'negative_onboarding_not_configured_explains':
          final robot = await ForgesRobot.create(tester);
          robot.installs.beginError = const GrpcError.failedPrecondition('no slug');
          await robot.load();
          await robot.tap('forges.github.connect',
              until: () => robot.bannerText != null);
          expect(robot.bannerText, contains("isn't configured"));
          expect(robot.platform.navigated, isEmpty);
          expect(robot.platform.storage.containsKey(installPendingKey), isFalse);

        case 'corner_native_cannot_redirect':
          final robot = await ForgesRobot.create(tester, canRedirect: false);
          await robot.load();
          await robot.tap('forges.github.connect',
              until: () => robot.bannerText != null);
          expect(robot.firedInstall('BeginInstall'), isTrue);
          expect(robot.bannerText, contains('needs the web portal'));
          expect(robot.platform.navigated, isEmpty);

        case 'adversarial_hostile_install_url_not_followed':
          for (final url in [
            'javascript:alert(1)',
            'http://github.com/apps/x/installations/new',
            '/relative/path',
            'data:text/html,hi',
          ]) {
            final robot = await ForgesRobot.create(tester);
            robot.installs.beginResponse = BeginInstallReply(
                installUrl: url, state: 'st-1', expiresAt: Int64(now + 900));
            await robot.load();
            await robot.tap('forges.github.connect',
                until: () => robot.bannerText != null);
            expect(robot.platform.navigated, isEmpty, reason: url);
            expect(robot.platform.storage.containsKey(installPendingKey), isFalse,
                reason: url);
            expect(robot.bannerText, contains('will not follow'), reason: url);
          }

        // ── GitHub's return ────────────────────────────────────────────────
        case 'positive_return_completes_install':
          final robot = await ForgesRobot.create(tester);
          robot.arrangePending('st-1');
          robot.arrangeReturn(ret());
          await robot.load();
          await robot.pumpUntil(() => robot.bannerText != null);
          final req = robot.lastInstall('CompleteInstall')!.request
              as CompleteInstallRequest;
          expect(req.state, 'st-1');
          expect(req.code, 'gh-code-123');
          expect(req.installationId, '42');
          expect(
              robot.log.authorizationFor(
                  '/${ForgesRobot.installSvc}/CompleteInstall'),
              ['Bearer ${ForgesRobot.bearer}']);
          expect(robot.bannerText, contains('installation 42 is connected'));
          // Single use: neither the pending state nor the return remain.
          expect(robot.platform.storage.containsKey(installPendingKey), isFalse);
          expect(robot.platform.storage.containsKey(installCallbackKey), isFalse);

        case 'positive_dismiss_hides_banner':
          final robot = await ForgesRobot.create(tester);
          robot.arrangeReturn(ret(action: 'request', id: '', code: ''));
          await robot.load();
          await robot.pumpUntilFound('forges.banner');
          await robot.tap('forges.banner.dismiss');
          expect(robot.exists('forges.banner'), isFalse);

        case 'negative_not_admin_is_opaque':
          final robot = await ForgesRobot.create(tester);
          robot.installs.completeError = const GrpcError.permissionDenied('nope');
          robot.arrangePending('st-1');
          robot.arrangeReturn(ret());
          await robot.load();
          await robot.pumpUntil(() => robot.bannerText != null);
          expect(robot.bannerText, contains("didn't confirm you administer"));
          expect(robot.bannerText, isNot(contains('nope')));

        case 'negative_claimed_by_another_tenant':
          final robot = await ForgesRobot.create(tester);
          robot.installs.completeError = const GrpcError.alreadyExists('held');
          robot.arrangePending('st-1');
          robot.arrangeReturn(ret());
          await robot.load();
          await robot.pumpUntil(() => robot.bannerText != null);
          expect(robot.bannerText, contains('another tenant'));

        case 'corner_org_approval_pending_sends_nothing':
          final robot = await ForgesRobot.create(tester);
          robot.arrangePending('st-1');
          robot.arrangeReturn(ret(action: 'request', id: '', code: ''));
          await robot.load();
          await robot.pumpUntilFound('forges.banner');
          expect(robot.bannerText, contains('owner must approve'));
          expect(robot.firedInstall('CompleteInstall'), isFalse);

        case 'boundary_expired_pending_sends_nothing':
          final robot = await ForgesRobot.create(tester);
          robot.arrangePending('st-1', expiresAt: now - 1);
          robot.arrangeReturn(ret());
          await robot.load();
          await robot.pumpUntilFound('forges.banner');
          expect(robot.firedInstall('CompleteInstall'), isFalse);
          expect(robot.bannerText, contains('expired'));
          // …and exactly at expiry it is still good.
          expect(
              takeInstallPending(
                  robot.platform..write(installPendingKey, '{"state":"s","expires_at":$now}'),
                  now),
              's');

        case 'adversarial_forged_state_never_relayed':
          final robot = await ForgesRobot.create(tester);
          robot.arrangePending('st-1');
          robot.arrangeReturn(ret(state: 'forged'));
          await robot.load();
          await robot.pumpUntilFound('forges.banner');
          expect(robot.firedInstall('CompleteInstall'), isFalse);
          expect(robot.bannerText, contains("didn't start in this tab"));
          // The pending state is spent, so the real return can't follow it.
          expect(robot.platform.storage.containsKey(installPendingKey), isFalse);

        case 'adversarial_return_without_pending_never_relayed':
          final robot = await ForgesRobot.create(tester);
          robot.arrangeReturn(ret());
          await robot.load();
          await robot.pumpUntilFound('forges.banner');
          expect(robot.firedInstall('CompleteInstall'), isFalse);

        case 'adversarial_malformed_return_never_relayed':
          for (final bad in [
            ret(id: '../1'),
            ret(id: '1' * 21),
            ret(action: 'uninstall'),
            ret(state: 'a\r\nb'),
            ret(code: ''),
          ]) {
            final robot = await ForgesRobot.create(tester);
            robot.arrangePending(bad['state']!);
            robot.arrangeReturn(bad);
            await robot.load();
            await robot.pumpUntilFound('forges.banner');
            expect(robot.firedInstall('CompleteInstall'), isFalse, reason: '$bad');
          }

        case 'adversarial_code_and_state_never_displayed':
          final robot = await ForgesRobot.create(tester);
          robot.installs.completeError =
              const GrpcError.internal('gh-code-123 st-1');
          robot.arrangePending('st-1');
          robot.arrangeReturn(ret());
          await robot.load();
          await robot.pumpUntil(() => robot.bannerText != null);
          expect(robot.bannerText, isNot(contains('gh-code-123')));
          expect(robot.bannerText, isNot(contains('st-1')));

        // ── forge cards ────────────────────────────────────────────────────
        case 'positive_create_card_puts_ghapp_ref':
          final robot = await ForgesRobot.create(tester);
          robot.installs.listResponse = GithubInstallList(installs: [install('42')]);
          await robot.load();
          await robot.tap('forges.install.card.42');
          await robot.tap('forges.card.dialog.save',
              until: () => robot.firedForge('Put'));
          final put = robot.lastForge('Put')!.request as ForgeCard;
          expect(put.id, 'github-42');
          expect(put.kind, 'github');
          expect(put.enabled, isTrue);
          expect(put.tokenRef, 'ghapp:42');
          expect(put.repoEncoding, 'owner_name');
          expect(put.baseUrl, isEmpty);

        case 'corner_existing_card_disables_create':
          final robot = await ForgesRobot.create(tester);
          robot.installs.listResponse = GithubInstallList(installs: [install('42')]);
          robot.forges.listResponse = ForgeList(forges: [card('gh', ref: 'ghapp:42')]);
          await robot.load();
          await robot.tap('forges.install.card.42');
          expect(robot.exists('forges.card.dialog.id'), isFalse);
          expect(find.text('Card exists'), findsOneWidget);

        case 'negative_viewer_sees_no_write_controls':
          final robot = await ForgesRobot.create(tester);
          robot.installs.listResponse = GithubInstallList(installs: [install('42')]);
          robot.forges.listResponse = ForgeList(forges: [card('github-42')]);
          await robot.load(caps: Capabilities.of(['read:forge_registry']));
          expect(robot.exists('forges.github.connect'), isFalse);
          expect(robot.exists('forges.install.card.42'), isFalse);
          expect(robot.exists('forges.card.delete.github-42'), isFalse);
          await robot.tap('forges.card.enabled.github-42');
          expect(robot.firedForge('Put'), isFalse);

        case 'positive_dialog_edits_card_id':
          final robot = await ForgesRobot.create(tester);
          robot.installs.listResponse = GithubInstallList(installs: [install('42')]);
          await robot.load();
          await robot.tap('forges.install.card.42');
          await robot.enterText('forges.card.dialog.id', 'acme-gh');
          await robot.tap('forges.card.dialog.save',
              until: () => robot.firedForge('Put'));
          expect((robot.lastForge('Put')!.request as ForgeCard).id, 'acme-gh');

        case 'negative_cancel_writes_nothing':
          final robot = await ForgesRobot.create(tester);
          robot.installs.listResponse = GithubInstallList(installs: [install('42')]);
          await robot.load();
          await robot.tap('forges.install.card.42');
          await robot.tap('forges.card.dialog.cancel');
          expect(robot.exists('forges.card.dialog.id'), isFalse);
          expect(robot.firedForge('Put'), isFalse);

        case 'boundary_id_length_limit':
          final robot = await ForgesRobot.create(tester);
          robot.installs.listResponse = GithubInstallList(installs: [install('42')]);
          await robot.load();
          await robot.tap('forges.install.card.42');
          await robot.enterText('forges.card.dialog.id', 'a' * (maxForgeIdLen + 1));
          await robot.tap('forges.card.dialog.save');
          expect(robot.firedForge('Put'), isFalse);
          expect(robot.exists('forges.card.dialog.id'), isTrue);
          await robot.enterText('forges.card.dialog.id', 'a' * maxForgeIdLen);
          await robot.tap('forges.card.dialog.save',
              until: () => robot.firedForge('Put'));
          expect((robot.lastForge('Put')!.request as ForgeCard).id.length,
              maxForgeIdLen);

        case 'adversarial_hostile_card_id_refused':
          final robot = await ForgesRobot.create(tester);
          robot.installs.listResponse = GithubInstallList(installs: [install('42')]);
          await robot.load();
          await robot.tap('forges.install.card.42');
          for (final bad in ['../../x', 'a/b', '-lead', '.hidden', 'a..b', 'a b', '']) {
            await robot.enterText('forges.card.dialog.id', bad);
            await robot.tap('forges.card.dialog.save');
            expect(robot.exists('forges.card.dialog.id'), isTrue, reason: bad);
          }
          expect(robot.firedForge('Put'), isFalse);

        case 'positive_cards_render_reference':
          final robot = await ForgesRobot.create(tester);
          robot.forges.listResponse = ForgeList(forges: [card('github-42')]);
          await robot.load();
          expect(find.textContaining('github · ghapp:42'), findsOneWidget);

        case 'positive_toggle_puts_flag':
          final robot = await ForgesRobot.create(tester);
          robot.forges.listResponse = ForgeList(forges: [card('github-42')]);
          await robot.load();
          await robot.tap('forges.card.enabled.github-42',
              until: () => robot.firedForge('Put'));
          final put = robot.lastForge('Put')!.request as ForgeCard;
          expect(put.id, 'github-42');
          expect(put.enabled, isFalse);
          expect(put.tokenRef, 'ghapp:42');

        case 'positive_delete_confirmed':
          final robot = await ForgesRobot.create(tester);
          robot.forges.listResponse = ForgeList(forges: [card('github-42')]);
          await robot.load();
          await robot.tap('forges.card.delete.github-42');
          await robot.tap('forges.card.delete.confirm',
              until: () => robot.firedForge('Delete'));
          expect((robot.lastForge('Delete')!.request as ForgeRef).id, 'github-42');

        case 'negative_delete_cancel_fires_nothing':
          final robot = await ForgesRobot.create(tester);
          robot.forges.listResponse = ForgeList(forges: [card('github-42')]);
          await robot.load();
          await robot.tap('forges.card.delete.github-42');
          await robot.tap('forges.card.delete.cancel');
          expect(robot.firedForge('Delete'), isFalse);

        default:
          fail('no test body for row ${row.label}');
      }
    });
  }
}
