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
  ActivePersonality activePersonality = ActivePersonality();

  /// When set, the next served RPC throws this instead of returning — cleared
  /// after it fires, so one injected fault affects exactly one call.
  GrpcError? error;

  void _guard() {
    final e = error;
    if (e != null) {
      error = null;
      throw e;
    }
  }

  @override
  Future<PromptList> list(ServiceCall call, PromptListRequest request) async {
    _log.record('agent.v1.PromptService/List', request);
    _guard();
    return listResponse;
  }

  @override
  Future<PromptEntry> get(ServiceCall call, PromptRef request) async {
    _log.record('agent.v1.PromptService/Get', request);
    _guard();
    return getResponse;
  }

  @override
  Future<PromptEntry> put(ServiceCall call, PromptEntry request) async {
    _log.record('agent.v1.PromptService/Put', request);
    _guard();
    return putResponse;
  }

  @override
  Future<DeleteReply> delete(ServiceCall call, PromptRef request) async {
    _log.record('agent.v1.PromptService/Delete', request);
    _guard();
    return deleteResponse;
  }

  @override
  Future<PromptList> select(ServiceCall call, PromptContext request) async {
    _log.record('agent.v1.PromptService/Select', request);
    _guard();
    return selectResponse;
  }

  @override
  Future<AssembledContext> previewAssembled(
      ServiceCall call, PreviewRequest request) async {
    _log.record('agent.v1.PromptService/PreviewAssembled', request);
    _guard();
    return previewResponse;
  }

  @override
  Future<ActivePersonality> getActivePersonality(
      ServiceCall call, GetActivePersonalityRequest request) async {
    _log.record('agent.v1.PromptService/GetActivePersonality', request);
    _guard();
    return activePersonality;
  }

  @override
  Future<ActivePersonality> setActivePersonality(
      ServiceCall call, SetActivePersonalityRequest request) async {
    _log.record('agent.v1.PromptService/SetActivePersonality', request);
    _guard();
    return activePersonality;
  }
}
