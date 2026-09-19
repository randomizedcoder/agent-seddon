import 'dart:async';

import 'package:agent_portal/src/gen/agent/v1/review_fleet.pbgrpc.dart';
import 'package:grpc/grpc.dart';

import '../recording.dart';

/// In-process fake of `agent.v1.ReviewFleetService` — the full `--serve-fleet`
/// seam the Fleet tab consumes (roster + review drafts + approve). Every RPC
/// records into the shared [RecordingLog] **before** it may throw (so
/// `log.fired(...)` holds on the error rows too), then returns a scripted
/// response. Responses default to empty messages so an un-scripted call still
/// gets a well-formed reply rather than a crash.
///
/// Fault injection mirrors the Prompts fake, with one addition the Fleet page
/// forces: the page's `_reload()` fires **two RPCs concurrently** (`ListReviews`
/// + the best-effort roster `List`), so a bare one-shot [error] would be consumed
/// by whichever wins the race. [errorMethod] scopes the one-shot fault to a
/// single method (the short name, e.g. `ListReviews`/`GetReview`), making the
/// error deterministic; leave it null to fail the next call whatever it is.
class FakeReviewFleetService extends ReviewFleetServiceBase {
  FakeReviewFleetService(this._log);

  final RecordingLog _log;

  static const _svc = 'agent.v1.ReviewFleetService';

  // ── scripted responses (assign in a test's arrange step) ───────────────────
  ListReviewsReply listReviewsResponse = ListReviewsReply();
  FleetSessionList sessionListResponse = FleetSessionList();
  GetReviewReply getReviewResponse = GetReviewReply();
  UpdateReviewReply updateReviewResponse = UpdateReviewReply();
  ApproveReply approveResponse = ApproveReply();
  ReviewNowReply reviewNowResponse = ReviewNowReply();
  FleetSession setEnabledResponse = FleetSession();
  // Unwired by the page today, but abstract on the base — implemented so the
  // service is concrete (and future-proof if a control wires them).
  FleetSession getResponse = FleetSession();
  FleetSession putResponse = FleetSession();
  FleetDeleteReply deleteResponse = FleetDeleteReply();
  PreflightReply preflightResponse = PreflightReply();

  /// When set, the next served RPC whose short method name matches [errorMethod]
  /// (or any call when [errorMethod] is null) throws this instead of returning —
  /// then both are cleared, so one injected fault affects exactly one call.
  GrpcError? error;
  String? errorMethod;

  /// When > zero, every served RPC waits this long before responding — the
  /// `slow(delay)` script for the loading / in-flight rows.
  Duration responseDelay = Duration.zero;

  /// Convenience: fail the next call to [method] with [e] (defaults to a
  /// realistic UNIMPLEMENTED from a bare-gateway fleet endpoint).
  void failNext(String method,
      [GrpcError e = const GrpcError.unimplemented('bare gateway')]) {
    error = e;
    errorMethod = method;
  }

  Future<void> _guard(String method) async {
    if (responseDelay > Duration.zero) {
      await Future<void>.delayed(responseDelay);
    }
    final e = error;
    if (e != null && (errorMethod == null || errorMethod == method)) {
      error = null;
      errorMethod = null;
      throw e;
    }
  }

  // ── roster control plane ───────────────────────────────────────────────────
  @override
  Future<FleetSessionList> list(
      ServiceCall call, FleetListRequest request) async {
    _log.record('$_svc/List', request);
    await _guard('List');
    return sessionListResponse;
  }

  @override
  Future<FleetSession> get(ServiceCall call, FleetSessionRef request) async {
    _log.record('$_svc/Get', request);
    await _guard('Get');
    return getResponse;
  }

  @override
  Future<FleetSession> put(ServiceCall call, FleetSession request) async {
    _log.record('$_svc/Put', request);
    await _guard('Put');
    return putResponse;
  }

  @override
  Future<FleetDeleteReply> delete(
      ServiceCall call, FleetSessionRef request) async {
    _log.record('$_svc/Delete', request);
    await _guard('Delete');
    return deleteResponse;
  }

  @override
  Future<FleetSession> setEnabled(
      ServiceCall call, FleetSetEnabledRequest request) async {
    _log.record('$_svc/SetEnabled', request);
    await _guard('SetEnabled');
    return setEnabledResponse;
  }

  @override
  Future<ReviewNowReply> reviewNow(
      ServiceCall call, ReviewNowRequest request) async {
    _log.record('$_svc/ReviewNow', request);
    await _guard('ReviewNow');
    return reviewNowResponse;
  }

  @override
  Future<ApproveReply> approve(ServiceCall call, ApproveRequest request) async {
    _log.record('$_svc/Approve', request);
    await _guard('Approve');
    return approveResponse;
  }

  // ── review drafts ──────────────────────────────────────────────────────────
  @override
  Future<ListReviewsReply> listReviews(
      ServiceCall call, ListReviewsRequest request) async {
    _log.record('$_svc/ListReviews', request);
    await _guard('ListReviews');
    return listReviewsResponse;
  }

  @override
  Future<GetReviewReply> getReview(
      ServiceCall call, GetReviewRequest request) async {
    _log.record('$_svc/GetReview', request);
    await _guard('GetReview');
    return getReviewResponse;
  }

  @override
  Future<UpdateReviewReply> updateReview(
      ServiceCall call, UpdateReviewRequest request) async {
    _log.record('$_svc/UpdateReview', request);
    await _guard('UpdateReview');
    return updateReviewResponse;
  }

  @override
  Future<PreflightReply> preflight(
      ServiceCall call, PreflightRequest request) async {
    _log.record('$_svc/Preflight', request);
    await _guard('Preflight');
    return preflightResponse;
  }
}
