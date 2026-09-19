import 'dart:async';

import 'package:agent_portal/src/gen/agent/v1/prompt.pbgrpc.dart';
import 'package:grpc/grpc.dart';

import '../recording.dart';

/// In-process fake of `agent.v1.PromptService` (the gateway seam the Prompts tab
/// consumes). Every RPC records into the shared [RecordingLog] and returns a
/// scripted response; set [error] to make the next call fail with a chosen
/// status (the resilience matrix). Responses default to empty messages so an
/// un-scripted test still gets a well-formed reply rather than a crash.
class FakePromptService extends PromptServiceBase {
  FakePromptService(this._log);

  final RecordingLog _log;

  // Scripted responses — assign in a test's `arrange` step.
  PromptList listResponse = PromptList();
  PromptEntry getResponse = PromptEntry();
  PromptEntry putResponse = PromptEntry();
  DeleteReply deleteResponse = DeleteReply();
  PromptList selectResponse = PromptList();
  AssembledContext previewResponse = AssembledContext();

  /// The active personality `GetActivePersonality` reports (the initial state);
  /// defaults to the empty id (the "default base").
  ActivePersonality activePersonality = ActivePersonality();

  /// `SetActivePersonality`'s reply. When null (default) the fake echoes the
  /// request id — the realistic server behaviour (it applies and returns the new
  /// active) — so a test need only script the *initial* [activePersonality].
  ActivePersonality? setActivePersonalityResponse;

  /// When set, the next served RPC throws this instead of returning — cleared
  /// after it fires, so one injected fault affects exactly one call.
  GrpcError? error;

  /// When > zero, every served RPC waits this long before responding — the
  /// `slow(delay)` script for the "slow / hung" resilience row (assert the
  /// loading state + the in-flight guard disables the button, no double-submit).
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
  Future<PromptList> list(ServiceCall call, PromptListRequest request) async {
    _log.record('agent.v1.PromptService/List', request);
    await _guard();
    return listResponse;
  }

  @override
  Future<PromptEntry> get(ServiceCall call, PromptRef request) async {
    _log.record('agent.v1.PromptService/Get', request);
    await _guard();
    return getResponse;
  }

  @override
  Future<PromptEntry> put(ServiceCall call, PromptEntry request) async {
    _log.record('agent.v1.PromptService/Put', request);
    await _guard();
    return putResponse;
  }

  @override
  Future<DeleteReply> delete(ServiceCall call, PromptRef request) async {
    _log.record('agent.v1.PromptService/Delete', request);
    await _guard();
    return deleteResponse;
  }

  @override
  Future<PromptList> select(ServiceCall call, PromptContext request) async {
    _log.record('agent.v1.PromptService/Select', request);
    await _guard();
    return selectResponse;
  }

  @override
  Future<AssembledContext> previewAssembled(
      ServiceCall call, PreviewRequest request) async {
    _log.record('agent.v1.PromptService/PreviewAssembled', request);
    await _guard();
    return previewResponse;
  }

  @override
  Future<ActivePersonality> getActivePersonality(
      ServiceCall call, GetActivePersonalityRequest request) async {
    _log.record('agent.v1.PromptService/GetActivePersonality', request);
    await _guard();
    return activePersonality;
  }

  @override
  Future<ActivePersonality> setActivePersonality(
      ServiceCall call, SetActivePersonalityRequest request) async {
    _log.record('agent.v1.PromptService/SetActivePersonality', request);
    await _guard();
    return setActivePersonalityResponse ?? (ActivePersonality()..id = request.id);
  }
}
