import 'package:agent_portal/src/gen/agent/v1/graph.pb.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../testkit/robots/graph_robot.dart';
import 'graph_spec.dart';

/// Layer-A widget tests for the Graph page — iterates [graphSpec]; each row is one
/// test. The Graph tab is mostly **local**: its library + node/edge edits persist
/// to the in-memory storage stub on the VM, and file import/export are inert, so
/// those outcomes are asserted via the widget tree; the server actions (`Validate`
/// / `Put` / `DescribeNodeTypes`) are asserted against the recorded wire calls.
void main() {
  for (final row in graphSpec.rows) {
    testWidgets('graph ${row.label} — ${row.description}', (tester) async {
      final robot = await GraphRobot.create(tester);
      final fake = robot.graph;

      switch (row.label) {
        // ── page init ─────────────────────────────────────────────────────────
        case 'positive_library_lists_seeded':
          robot.seed(
              [GraphRobot.sampleEntry('alpha'), GraphRobot.sampleEntry('beta')]);
          await robot.load();
          expect(robot.libraryItemCount, 2);
          expect(robot.exists('graph.library.item.alpha'), isTrue);
          expect(robot.log.fired('agent.v1.GraphService/DescribeNodeTypes'),
              isTrue);

        case 'corner_empty_seeds_examples':
          // Empty library → the page seeds the shipped example-graph assets.
          await robot.load();
          expect(robot.libraryItemCount, greaterThanOrEqualTo(1));
          expect(robot.log.fired('agent.v1.GraphService/DescribeNodeTypes'),
              isTrue);

        case 'boundary_loading_shows_spinner':
          robot.seed([GraphRobot.sampleEntry()]);
          fake.responseDelay = const Duration(milliseconds: 200);
          await robot.pumpLoading();
          expect(robot.showsSpinner, isTrue);
          expect(robot.isLoaded, isFalse);
          await robot.pumpUntil(() => robot.isLoaded, reason: 'drain load');

        case 'negative_offline_shows_banner':
          robot.seed([GraphRobot.sampleEntry()]);
          fake.error = const GrpcError.unavailable('gateway down');
          await robot.load();
          expect(robot.hasServerBanner, isTrue);

        case 'positive_retry_reprobes':
          robot.seed([GraphRobot.sampleEntry()]);
          fake.error = const GrpcError.unavailable('gateway down');
          await robot.load();
          expect(robot.hasServerBanner, isTrue);
          await robot.retry();
          expect(robot.hasServerBanner, isFalse);
          expect(
              robot.log.countOf('agent.v1.GraphService/DescribeNodeTypes'),
              greaterThanOrEqualTo(2));

        // ── library actions ───────────────────────────────────────────────────
        case 'positive_new_adds_entry':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.new');
          expect(robot.exists('graph.name.field'), isTrue);
          await robot.enterText('graph.name.field', 'brand new');
          await robot.tapLocal('graph.name.ok');
          expect(robot.exists('graph.library.item.brand new'), isTrue);

        case 'positive_name_field_accepts_text':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.new');
          await robot.enterText('graph.name.field', 'typed-name');
          expect(find.text('typed-name').evaluate().isNotEmpty, isTrue);

        case 'positive_name_ok_confirms':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.new');
          await robot.enterText('graph.name.field', 'okname');
          await robot.tapLocal('graph.name.ok');
          expect(robot.exists('graph.name.field'), isFalse); // dialog closed
          expect(robot.exists('graph.library.item.okname'), isTrue);

        case 'positive_name_cancel_dismisses':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.new');
          await robot.tapLocal('graph.name.cancel');
          expect(robot.exists('graph.name.field'), isFalse);
          expect(robot.libraryItemCount, 1); // nothing added

        case 'positive_import_button_present':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          expect(robot.exists('graph.import'), isTrue);
          await robot.tapLocal('graph.import'); // stub picker → no file
          expect(robot.isLoaded, isTrue);

        case 'adversarial_import_bad_json_safe':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          final before = robot.libraryItemCount;
          // On the VM the file picker is inert (returns null), so the parse path
          // is unreachable from the UI — tapping must be a safe no-op: no crash,
          // no library mutation, and (there is no Import RPC) no wire traffic.
          await robot.tapLocal('graph.import');
          expect(robot.libraryItemCount, before);
          expect(robot.isLoaded, isTrue);

        case 'positive_rename_updates_name':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.rename');
          await robot.enterText('graph.name.field', 'renamed');
          await robot.tapLocal('graph.name.ok');
          expect(robot.exists('graph.library.item.renamed'), isTrue);
          expect(robot.exists('graph.library.item.alpha'), isFalse);

        case 'positive_duplicate_copies_entry':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.tapLocal('graph.duplicate');
          expect(robot.exists('graph.library.item.alpha copy'), isTrue);
          expect(robot.libraryItemCount, 2);

        case 'positive_export_noop_safe':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.tapLocal('graph.export'); // inert download on the VM
          expect(robot.isLoaded, isTrue);

        case 'positive_delete_removes_entry':
          robot.seed(
              [GraphRobot.sampleEntry('alpha'), GraphRobot.sampleEntry('beta')]);
          await robot.load();
          await robot.tapLocal('graph.delete'); // removes the selected (alpha)
          expect(robot.exists('graph.library.item.alpha'), isFalse);
          expect(robot.libraryItemCount, 1);

        // ── server actions ────────────────────────────────────────────────────
        case 'positive_validate_shows_issues':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          fake.validateResponse = ValidateGraphResponse(
              issues: [GraphIssue(code: 'DUP', node: 'cfg', detail: 'bad')]);
          await robot.validate();
          final v = robot.log.last('agent.v1.GraphService/Validate');
          expect((v!.request as ValidateGraphRequest).graph.nodes.length, 2);
          await robot.pumpUntil(
              () => robot.exists('graph.validate.issues.close'),
              reason: 'issues dialog');
          expect(robot.exists('graph.validate.issues.close'), isTrue);
          await robot.tapLocal('graph.validate.issues.close');

        case 'positive_close_issues_dialog':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.validate(); // default: no issues → "Valid" dialog
          await robot.pumpUntil(
              () => robot.exists('graph.validate.issues.close'),
              reason: 'issues dialog');
          await robot.tapLocal('graph.validate.issues.close');
          expect(robot.exists('graph.validate.issues.close'), isFalse);

        case 'positive_set_active_puts':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.setActive();
          final p = robot.log.last('agent.v1.GraphService/Put');
          expect(p, isNotNull);
          expect((p!.request as PutGraphRequest).graph.nodes.length, 2);
          await robot.settle();

        case 'negative_set_active_rejected_snacks':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          fake.error = const GrpcError.invalidArgument('invalid document');
          await robot.setActive();
          await robot.pumpUntil(
              () => robot.snackContains('Set active rejected'),
              reason: 'rejection snack');
          expect(robot.isLoaded, isTrue); // no crash
          await robot.settle();

        // ── node editing ──────────────────────────────────────────────────────
        case 'positive_node_tiles_render':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          expect(robot.exists('graph.node.cfg'), isTrue);
          expect(robot.exists('graph.node.gen'), isTrue);

        case 'positive_add_node_appends':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.node.add');
          expect(robot.exists('graph.node.add.confirm'), isTrue);
          await robot.enterText('graph.node.add.id', 'newnode');
          await robot.tapLocal('graph.node.add.confirm');
          expect(robot.exists('graph.node.newnode'), isTrue);

        case 'negative_add_node_blocked_offline':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          fake.error = const GrpcError.unavailable('gateway down');
          await robot.load(); // node types unfetched
          await robot.tapLocal('graph.node.add');
          expect(robot.snackContains('No node types'), isTrue);
          expect(robot.exists('graph.node.add.confirm'), isFalse); // no dialog
          await robot.settle();

        case 'positive_add_node_id_field':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.node.add');
          expect(robot.exists('graph.node.add.id'), isTrue);
          await robot.tapLocal('graph.node.add.cancel');

        case 'positive_add_node_type_dropdown':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.node.add');
          expect(robot.exists('graph.node.add.type'), isTrue);
          await robot.tapLocal('graph.node.add.cancel');

        case 'corner_add_node_confirm_requires_id':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.node.add');
          await robot.tapLocal('graph.node.add.confirm'); // empty id
          expect(robot.exists('graph.node.add.confirm'), isTrue); // still open
          expect(find.text('required').evaluate().isNotEmpty, isTrue);
          await robot.tapLocal('graph.node.add.cancel');

        case 'positive_add_node_cancel_dismisses':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.node.add');
          await robot.tapLocal('graph.node.add.cancel');
          expect(robot.exists('graph.node.add.confirm'), isFalse);

        case 'positive_remove_node_and_dangling_edges':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          expect(robot.exists('graph.node.gen'), isTrue);
          expect(robot.exists('graph.edge.0'), isTrue); // gen → cfg
          await robot.tapLocal('graph.node.remove.gen');
          expect(robot.exists('graph.node.gen'), isFalse);
          expect(robot.exists('graph.node.cfg'), isTrue);
          expect(robot.exists('graph.edge.0'), isFalse); // dangling edge dropped

        case 'positive_toggle_form_to_raw':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.expandNode('cfg'); // simple schema → form editor
          expect(robot.exists('graph.node.params.toggle'), isTrue);
          expect(robot.exists('graph.node.params.field.critic'), isTrue);
          await robot.tapLocal('graph.node.params.toggle');
          expect(robot.exists('graph.node.params.raw'), isTrue);

        case 'positive_param_field_edits':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.expandNode('cfg');
          expect(robot.exists('graph.node.params.field.critic'), isTrue);
          await robot.enterText('graph.node.params.field.critic', 'glm');
          expect(find.text('glm').evaluate().isNotEmpty, isTrue);

        case 'positive_raw_editor_for_schemaless':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.expandNode('gen'); // no simple schema → raw editor
          expect(robot.exists('graph.node.params.raw'), isTrue);

        case 'adversarial_raw_giant_invalid_json':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.expandNode('gen');
          // Invalid JSON must be rejected inline (error text), never crash.
          await robot.enterText('graph.node.params.raw', '{not: valid json');
          expect(robot.exists('graph.node.params.raw'), isTrue);
          // A huge but valid payload is accepted without a crash.
          final huge = '{"k":"${'A' * 100000}"}';
          await robot.enterText('graph.node.params.raw', huge);
          expect(robot.exists('graph.node.params.raw'), isTrue);

        // ── edge editing ──────────────────────────────────────────────────────
        case 'positive_edge_tiles_render':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          expect(robot.exists('graph.edge.0'), isTrue);

        case 'positive_add_edge_appends':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.edge.add');
          await robot.enterText('graph.edge.add.from', 'cfg');
          await robot.enterText('graph.edge.add.to', 'gen');
          await robot.tapLocal('graph.edge.add.confirm');
          expect(robot.exists('graph.edge.1'), isTrue); // second edge added

        case 'positive_add_edge_from_field':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.edge.add');
          expect(robot.exists('graph.edge.add.from'), isTrue);
          await robot.tapLocal('graph.edge.add.cancel');

        case 'positive_add_edge_to_field':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.edge.add');
          expect(robot.exists('graph.edge.add.to'), isTrue);
          await robot.tapLocal('graph.edge.add.cancel');

        case 'positive_add_edge_kind_dropdown':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.edge.add');
          expect(robot.exists('graph.edge.add.kind'), isTrue);
          await robot.tapLocal('graph.edge.add.cancel');

        case 'corner_add_edge_confirm_requires_endpoints':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.edge.add');
          await robot.tapLocal('graph.edge.add.confirm'); // empty endpoints
          expect(robot.exists('graph.edge.add.confirm'), isTrue); // still open
          expect(robot.exists('graph.edge.1'), isFalse); // nothing added
          await robot.tapLocal('graph.edge.add.cancel');

        case 'positive_add_edge_cancel_dismisses':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          await robot.openDialog('graph.edge.add');
          await robot.tapLocal('graph.edge.add.cancel');
          expect(robot.exists('graph.edge.add.confirm'), isFalse);

        case 'positive_remove_edge':
          robot.seed([GraphRobot.sampleEntry('alpha')]);
          await robot.load();
          expect(robot.exists('graph.edge.0'), isTrue);
          await robot.tapLocal('graph.edge.remove.0');
          expect(robot.exists('graph.edge.0'), isFalse);

        default:
          fail('no test body for row ${row.label}');
      }
    });
  }
}
