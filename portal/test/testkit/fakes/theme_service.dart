import 'dart:async';

import 'package:agent_portal/src/gen/agent/v1/theme.pbgrpc.dart';
import 'package:grpc/grpc.dart';

import '../recording.dart';

/// In-process fake of `agent.v1.ThemeService` (the gateway seam the theme
/// pickers consume, visual/04). Every RPC records into the shared
/// [RecordingLog] and returns a scripted response; set [error] to make the next
/// call fail with a chosen status.
class FakeThemeService extends ThemeServiceBase {
  FakeThemeService(this._log);

  final RecordingLog _log;

  // Scripted responses — assign in a test's `arrange` step.
  ListThemesResponse listResponse = ListThemesResponse();
  Theme getResponse = Theme();
  Theme putResponse = Theme();

  /// When set, `Put` answers this function of the request instead of
  /// [putResponse] (e.g. the request stored, with a version).
  Theme Function(Theme request)? putAnswer;
  DeleteThemeResponse deleteResponse = DeleteThemeResponse();
  ImportThemeResponse importResponse = ImportThemeResponse();
  ExportThemeResponse exportResponse = ExportThemeResponse();

  /// What `GetActive` answers (the user's stored choice, resolved).
  ActiveTheme active = ActiveTheme();

  /// `SetActive`'s reply. When null (default) the fake stores the request as the
  /// new [active] with source USER — what the real server does — so a later
  /// `GetActive` sees it.
  ActiveTheme? setActiveResponse;

  /// When set, the next served RPC throws this instead of returning — cleared
  /// after it fires, so one injected fault affects exactly one call.
  GrpcError? error;

  /// RPCs (by method name, e.g. `GetActive`) that always fail with the given
  /// status — an agent missing one RPC, or one that is down.
  final Map<String, GrpcError> failing = {};

  /// When > zero, every served RPC waits this long before responding.
  Duration responseDelay = Duration.zero;

  Future<void> _guard(String method) async {
    final always = failing[method];
    if (always != null) throw always;
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
  Future<ListThemesResponse> list(ServiceCall call, ListThemesRequest request) async {
    _log.record('agent.v1.ThemeService/List', request);
    await _guard('List');
    return listResponse;
  }

  @override
  Future<Theme> get(ServiceCall call, GetThemeRequest request) async {
    _log.record('agent.v1.ThemeService/Get', request);
    await _guard('Get');
    return getResponse;
  }

  @override
  Future<Theme> put(ServiceCall call, Theme request) async {
    _log.record('agent.v1.ThemeService/Put', request);
    await _guard('Put');
    return putAnswer?.call(request) ?? putResponse;
  }

  @override
  Future<DeleteThemeResponse> delete(ServiceCall call, DeleteThemeRequest request) async {
    _log.record('agent.v1.ThemeService/Delete', request);
    await _guard('Delete');
    return deleteResponse;
  }

  @override
  Future<ActiveTheme> getActive(ServiceCall call, GetActiveThemeRequest request) async {
    _log.record('agent.v1.ThemeService/GetActive', request);
    await _guard('GetActive');
    return active;
  }

  @override
  Future<ActiveTheme> setActive(ServiceCall call, SetActiveThemeRequest request) async {
    _log.record('agent.v1.ThemeService/SetActive', request);
    await _guard('SetActive');
    final scripted = setActiveResponse;
    if (scripted != null) return scripted;
    active = ActiveTheme(
      theme: Theme(id: request.themeId),
      mode: request.mode,
      source: ActiveSource.ACTIVE_SOURCE_USER,
    );
    return active;
  }

  @override
  Future<ImportThemeResponse> import(ServiceCall call, ImportThemeRequest request) async {
    _log.record('agent.v1.ThemeService/Import', request);
    await _guard('Import');
    return importResponse;
  }

  @override
  Future<ExportThemeResponse> export(ServiceCall call, ExportThemeRequest request) async {
    _log.record('agent.v1.ThemeService/Export', request);
    await _guard('Export');
    return exportResponse;
  }
}
