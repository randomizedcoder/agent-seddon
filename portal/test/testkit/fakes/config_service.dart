import 'dart:async';

import 'package:agent_portal/src/gen/agent/v1/config.pbgrpc.dart';
import 'package:grpc/grpc.dart';

import '../recording.dart';

/// In-process fake of `agent.v1.ConfigService` (the whole-config seam the
/// Settings tab consumes on the `--serve-all` gateway). Every RPC records into
/// the shared [RecordingLog] **before** it may throw — so `log.fired(...)` is a
/// valid "it happened" signal even on an injected-error row — then returns a
/// scripted response. Responses default to well-formed empty messages so an
/// un-scripted test still gets a reply rather than a crash.
class FakeConfigService extends ConfigServiceBase {
  FakeConfigService(this._log);

  final RecordingLog _log;

  // Scripted responses — assign in a test's `arrange` step.
  ConfigSchema schemaResponse = ConfigSchema();
  ConfigValues valuesResponse = ConfigValues();
  ConfigStatus statusResponse = ConfigStatus();
  ValidateConfigResponse validateResponse = ValidateConfigResponse();
  PutConfigResponse putResponse = PutConfigResponse();

  /// When set, the next served RPC throws this instead of returning — cleared
  /// after it fires, so one injected fault affects exactly one call (the load
  /// path's first RPC is `GetSchema`, so a single error drives the error state).
  GrpcError? error;

  /// When > zero, every served RPC waits this long before responding — the
  /// `slow(delay)` script for the loading / in-flight rows.
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
  Future<ConfigSchema> getSchema(
      ServiceCall call, GetSchemaRequest request) async {
    _log.record('agent.v1.ConfigService/GetSchema', request);
    await _guard();
    return schemaResponse;
  }

  @override
  Future<ConfigValues> getValues(
      ServiceCall call, GetValuesRequest request) async {
    _log.record('agent.v1.ConfigService/GetValues', request);
    await _guard();
    return valuesResponse;
  }

  @override
  Future<ValidateConfigResponse> validate(
      ServiceCall call, ValidateConfigRequest request) async {
    _log.record('agent.v1.ConfigService/Validate', request);
    await _guard();
    return validateResponse;
  }

  @override
  Future<PutConfigResponse> put(
      ServiceCall call, PutConfigRequest request) async {
    _log.record('agent.v1.ConfigService/Put', request);
    await _guard();
    return putResponse;
  }

  @override
  Future<ConfigStatus> status(
      ServiceCall call, ConfigStatusRequest request) async {
    _log.record('agent.v1.ConfigService/Status', request);
    await _guard();
    return statusResponse;
  }
}
