import 'dart:async';

import 'package:agent_portal/src/gen/agent/v1/upstream.pbgrpc.dart';
import 'package:grpc/grpc.dart';

import '../recording.dart';

/// In-process fake of `agent.v1.ProviderRegistryService` (the model-router /
/// provider-registry control plane the Router tab consumes). Every RPC records
/// into the shared [RecordingLog] **before** it may throw — so `log.fired(...)`
/// is valid on the error rows too — then returns a scripted response.
///
/// Set [error] to make the *next* served call fail with a chosen status (the
/// resilience matrix); it is cleared after it fires, so one injected fault hits
/// exactly one call. Set [responseDelay] to hold every call in flight (the
/// "slow / loading" row). Responses default to empty messages so an un-scripted
/// test still gets a well-formed reply rather than a crash.
///
/// `GetPolicy` / `PutPolicy` are not yet wired by the page, but the abstract
/// base declares them, so they are implemented here (recording + scripted echo).
class FakeProviderRegistryService extends ProviderRegistryServiceBase {
  FakeProviderRegistryService(this._log);

  final RecordingLog _log;

  static const _svc = 'agent.v1.ProviderRegistryService';

  // Scripted responses — assign in a test's `arrange` step.
  UpstreamList listResponse = UpstreamList();
  Upstream getResponse = Upstream();
  Upstream putResponse = Upstream();
  UpstreamDeleteReply deleteResponse = UpstreamDeleteReply();
  Upstream enableResponse = Upstream();
  RoutePolicy getPolicyResponse = RoutePolicy();

  /// `PutPolicy`'s reply. When null (default) the fake echoes the request — the
  /// realistic server behaviour.
  RoutePolicy? putPolicyResponse;
  RouteDecision routeResponse = RouteDecision();
  UpstreamHealthList healthResponse = UpstreamHealthList();

  /// When set, the next served RPC throws this instead of returning — cleared
  /// after it fires, so one injected fault affects exactly one call.
  GrpcError? error;

  /// When > zero, every served RPC waits this long before responding — the
  /// `slow(delay)` script for the loading / in-flight resilience rows.
  Duration responseDelay = Duration.zero;

  Future<void> _guard() async {
    if (responseDelay > Duration.zero) {
      await Future<void>.delayed(responseDelay);
    }
    final e = error;
    if (e != null) {
      error = null;
      throw e;
    }
  }

  @override
  Future<UpstreamList> list(
      ServiceCall call, UpstreamListRequest request) async {
    _log.record('$_svc/List', request);
    await _guard();
    return listResponse;
  }

  @override
  Future<Upstream> get(ServiceCall call, UpstreamRef request) async {
    _log.record('$_svc/Get', request);
    await _guard();
    return getResponse;
  }

  @override
  Future<Upstream> put(ServiceCall call, Upstream request) async {
    _log.record('$_svc/Put', request);
    await _guard();
    // Echo the draft's id when the test did not script a saved card, so the
    // page's re-select-by-id keeps working with realistic server behaviour.
    if (!putResponse.hasId() && request.hasId()) return request;
    return putResponse;
  }

  @override
  Future<UpstreamDeleteReply> delete(
      ServiceCall call, UpstreamRef request) async {
    _log.record('$_svc/Delete', request);
    await _guard();
    return deleteResponse;
  }

  @override
  Future<Upstream> enable(
      ServiceCall call, UpstreamEnableRequest request) async {
    _log.record('$_svc/Enable', request);
    await _guard();
    return enableResponse;
  }

  @override
  Future<RoutePolicy> getPolicy(
      ServiceCall call, RoutePolicyRef request) async {
    _log.record('$_svc/GetPolicy', request);
    await _guard();
    return getPolicyResponse;
  }

  @override
  Future<RoutePolicy> putPolicy(ServiceCall call, RoutePolicy request) async {
    _log.record('$_svc/PutPolicy', request);
    await _guard();
    return putPolicyResponse ?? request;
  }

  @override
  Future<RouteDecision> route(ServiceCall call, RouteRequest request) async {
    _log.record('$_svc/Route', request);
    await _guard();
    return routeResponse;
  }

  @override
  Future<UpstreamHealthList> health(
      ServiceCall call, UpstreamHealthRequest request) async {
    _log.record('$_svc/Health', request);
    await _guard();
    return healthResponse;
  }
}
