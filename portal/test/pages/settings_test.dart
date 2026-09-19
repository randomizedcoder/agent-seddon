import 'package:agent_portal/src/gen/agent/v1/config.pb.dart';
import 'package:agent_portal/src/graph_json.dart' show dartToJsonValue;
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../testkit/robots/settings_robot.dart';
import 'settings_spec.dart';

/// Layer-A widget tests for the Settings page — iterates [settingsSpec]; each row
/// is one test. Covers the four async load states (GetSchema + GetValues +
/// Status), a `positive_` row for every keyed element (section list, schema-form
/// fields, Validate / Save / Revert / Retry), the resilience matrix, and an
/// `adversarial_` schema-form row. Each row arranges the fake, drives the page via
/// its robot, then asserts the recorded RPC (method + decoded edits) and outcome.
void main() {
  // A small but valid draft-07 object schema: one rich section (`agent`, with a
  // string / bool / enum / integer / secret-string field) + a second (`pool`) so
  // the section list has more than one row.
  Map<String, dynamic> schemaMap() => {
        'definitions': {
          'Agent': {
            'type': 'object',
            'properties': {
              'name': {'type': 'string', 'description': 'agent name'},
              'enabled': {'type': 'boolean', 'description': 'is enabled'},
              'mode': {
                'enum': ['implement', 'review', 'triage'],
                'description': 'operating mode',
              },
              'max_iters': {'type': 'integer', 'description': 'iteration cap'},
              'api_key': {
                'type': 'string',
                'x-secret': true,
                'description': 'provider key',
              },
            },
          },
          'Pool': {
            'type': 'object',
            'properties': {
              'size': {'type': 'integer', 'description': 'pool size'},
            },
          },
        },
        'properties': {
          'agent': {r'$ref': '#/definitions/Agent'},
          'pool': {r'$ref': '#/definitions/Pool'},
        },
      };

  Map<String, dynamic> valuesMap() => {
        'agent': {
          'name': 'seddon',
          'enabled': true,
          'mode': 'implement',
          'max_iters': 8,
          'api_key': 'secret123',
        },
        'pool': {'size': 4},
      };

  ConfigSchema schema() => ConfigSchema(schema: dartToJsonValue(schemaMap()));
  ConfigValues values() => ConfigValues(values: dartToJsonValue(valuesMap()));

  /// Arrange the fake to serve the standard schema + values so the page loads
  /// into the editor with `agent` selected.
  void arrangeLoaded(SettingsRobot robot) {
    robot.config.schemaResponse = schema();
    robot.config.valuesResponse = values();
  }

  for (final row in settingsSpec.rows) {
    testWidgets('settings ${row.label} — ${row.description}', (tester) async {
      final robot = await SettingsRobot.create(tester);
      final fake = robot.config;

      switch (row.label) {
        case 'positive_loaded_lists_sections':
          arrangeLoaded(robot);
          await robot.load();
          expect(robot.isLoaded, isTrue);
          expect(robot.sectionItemCount, greaterThanOrEqualTo(2));
          expect(robot.sectionExists('agent'), isTrue);
          expect(robot.log.fired('agent.v1.ConfigService/GetSchema'), isTrue);
          expect(robot.log.fired('agent.v1.ConfigService/GetValues'), isTrue);
          expect(robot.log.fired('agent.v1.ConfigService/Status'), isTrue);
        case 'boundary_empty_schema_no_sections':
          fake.schemaResponse =
              ConfigSchema(schema: dartToJsonValue(<String, dynamic>{}));
          fake.valuesResponse =
              ConfigValues(values: dartToJsonValue(<String, dynamic>{}));
          await robot.load();
          expect(robot.isLoaded, isTrue);
          expect(robot.sectionItemCount, 0);
        case 'corner_loading_shows_spinner':
          arrangeLoaded(robot);
          fake.responseDelay = const Duration(milliseconds: 60);
          await robot.pumpLoading();
          expect(robot.showsSpinner, isTrue);
          expect(robot.isLoaded, isFalse);
          // Let the delayed load resolve to the editor; the trailing Status must
          // fire before the shared `quiesce()` (below) drains its response — an
          // in-flight call wedges `channel.shutdown()` at teardown.
          await robot.pumpUntil(
              () =>
                  robot.isLoaded &&
                  robot.log.fired('agent.v1.ConfigService/Status'),
              maxCycles: 300,
              reason: 'drain load');
        case 'negative_load_error_shows_retry':
          fake.error = const GrpcError.unavailable('gateway down');
          await robot.load();
          expect(robot.isError, isTrue);
        case 'positive_retry_recovers':
          fake.error = const GrpcError.unavailable('gateway down');
          await robot.load();
          expect(robot.isError, isTrue);
          arrangeLoaded(robot);
          await robot.tapRetry();
          expect(robot.isLoaded, isTrue);
          expect(robot.sectionExists('agent'), isTrue);
        case 'positive_form_renders_typed_fields':
          arrangeLoaded(robot);
          await robot.load();
          for (final k in ['name', 'enabled', 'mode', 'max_iters', 'api_key']) {
            expect(robot.fieldExists(k), isTrue, reason: 'field $k should render');
          }
        case 'boundary_secret_blank_keeps_value':
          arrangeLoaded(robot);
          await robot.load();
          // The masked secret field renders blank, but editing only a *non-secret*
          // field must NOT emit a spurious `api_key` edit that would overwrite the
          // stored secret with "" — the blank field keeps the original untouched.
          await robot.editField('name', 'renamed');
          await robot.tapValidate();
          final sv = robot.log.last('agent.v1.ConfigService/Validate');
          final paths =
              (sv!.request as ValidateConfigRequest).edits.map((e) => e.path);
          expect(paths, contains('agent.name'));
          expect(paths, isNot(contains('agent.api_key')),
              reason: 'a blank secret field must not overwrite the stored secret');
        case 'adversarial_bad_number_produces_no_edit':
          arrangeLoaded(robot);
          await robot.load();
          await robot.editField('max_iters', 'not-a-number');
          expect(robot.saveEnabled, isFalse,
              reason: 'unparseable integer must not stage an edit');
          expect(robot.validateEnabled, isFalse);
          expect(robot.log.fired('agent.v1.ConfigService/Put'), isFalse);
          expect(robot.log.fired('agent.v1.ConfigService/Validate'), isFalse);
        case 'positive_validate_sends_edits':
          arrangeLoaded(robot);
          await robot.load();
          await robot.editField('name', 'newname');
          expect(robot.validateEnabled, isTrue);
          await robot.tapValidate();
          final v = robot.log.last('agent.v1.ConfigService/Validate');
          expect(v, isNotNull);
          final vreq = v!.request as ValidateConfigRequest;
          expect(vreq.edits.length, 1);
          expect(vreq.edits.first.path, 'agent.name');
          expect(vreq.edits.first.value.stringValue, 'newname');
        case 'negative_validate_issues_dialog':
          arrangeLoaded(robot);
          fake.validateResponse = ValidateConfigResponse(issues: [
            ConfigIssue(
                path: 'agent.name', code: 'invalid', detail: 'too short'),
          ]);
          await robot.load();
          await robot.editField('name', 'x');
          await robot.tapValidate();
          await robot.pumpUntil(() => robot.dialogContains('too short'),
              reason: 'issues dialog');
          expect(robot.log.fired('agent.v1.ConfigService/Put'), isFalse);
        case 'positive_save_puts_and_reloads':
          arrangeLoaded(robot);
          await robot.load();
          await robot.editField('name', 'saved-name');
          await robot.tapSave();
          final put = robot.log.last('agent.v1.ConfigService/Put');
          expect(put, isNotNull);
          final preq = put!.request as PutConfigRequest;
          expect(preq.edits.length, 1);
          expect(preq.edits.first.path, 'agent.name');
          expect(preq.edits.first.value.stringValue, 'saved-name');
          // A successful Put reloads (a second GetSchema).
          await robot.waitForReload(2);
          expect(
              robot.log.countOf('agent.v1.ConfigService/GetSchema'), 2);
        case 'negative_save_error_snacks':
          arrangeLoaded(robot);
          await robot.load();
          await robot.editField('name', 'will-fail');
          fake.error = const GrpcError.permissionDenied('nope');
          await robot.tapSave();
          await robot.pumpUntil(() => robot.snackContains('Save failed'),
              reason: 'save-failed snack');
          expect(robot.isLoaded, isTrue); // no crash
          // Only the initial GetSchema — no reload after a failed Put.
          expect(robot.log.countOf('agent.v1.ConfigService/GetSchema'), 1);
        case 'positive_revert_discards_edits':
          arrangeLoaded(robot);
          await robot.load();
          await robot.editField('name', 'temp');
          expect(robot.saveEnabled, isTrue);
          expect(robot.revertEnabled, isTrue);
          await robot.tapRevert();
          expect(robot.saveEnabled, isFalse,
              reason: 'Revert clears the staged edit');
        default:
          fail('no test body for row ${row.label}');
      }

      // Drain any in-flight RPC response so the channel is idle before teardown
      // shuts it down (grpc-dart's shutdown() wedges on a mid-flight call).
      await robot.quiesce();
    });
  }
}
