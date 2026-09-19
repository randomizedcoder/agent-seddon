import 'package:agent_portal/src/gen/agent/v1/common.pb.dart';
import 'package:agent_portal/src/gen/agent/v1/upstream.pb.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../testkit/robots/router_robot.dart';
import 'router_spec.dart';

/// Layer-A widget tests for the Router page — iterates [routerSpec]; each row is
/// one test. Covers the four async states on the upstreams load, the segmented
/// sub-views, upstream CRUD (with the confirm-dialog gate on delete), the health
/// refresh, and the route-introspection form. Each row arranges the fake, drives
/// the page via its robot, then asserts the recorded RPC (method + decoded
/// request) and the widget outcome.
void main() {
  const svc = 'agent.v1.ProviderRegistryService';

  Upstream upstream({
    String id = 'u1',
    String model = 'gpt-x',
    String kind = 'openai-compat',
    bool enabled = true,
    PoolTier tier = PoolTier.POOL_TIER_MEDIUM,
  }) =>
      Upstream(id: id, model: model, kind: kind, enabled: enabled, tier: tier);

  UpstreamList list(List<Upstream> u) => UpstreamList(upstreams: u);

  for (final row in routerSpec.rows) {
    testWidgets('router ${row.label} — ${row.description}', (tester) async {
      final robot = await RouterRobot.create(tester);
      final fake = robot.providers;

      switch (row.label) {
        // ── page load: four async states ──────────────────────────────────
        case 'positive_loaded_lists_render':
          fake.listResponse = list([upstream(id: 'u1'), upstream(id: 'u2')]);
          await robot.load();
          expect(robot.isLoaded, isTrue);
          expect(robot.itemCount, 2);
          expect(robot.fired('List'), isTrue);
          await robot.selectItem('u1');
          expect(robot.exists('router.upstream.field.id'), isTrue);
          await robot.settle();

        case 'boundary_empty_list_no_items':
          fake.listResponse = list([]);
          await robot.load();
          expect(robot.isLoaded, isTrue);
          expect(robot.itemCount, 0);
          expect(find.text('No upstreams yet. Add one.'), findsOneWidget);
          await robot.settle();

        case 'corner_loading_shows_spinner':
          fake.listResponse = list([upstream()]);
          fake.responseDelay = const Duration(milliseconds: 200);
          await robot.pumpLoading();
          expect(robot.showsSpinner, isTrue);
          expect(robot.isLoaded, isFalse);
          await robot.pumpUntil(() => robot.isLoaded, reason: 'drain load');
          await robot.settle();

        case 'negative_list_error_shows_retry':
          fake.error = const GrpcError.unavailable('gateway down');
          await robot.load();
          expect(robot.isError, isTrue);
          await robot.settle();

        case 'positive_retry_recovers':
          fake.error = const GrpcError.unavailable('gateway down');
          await robot.load();
          expect(robot.isError, isTrue);
          fake.listResponse = list([upstream()]);
          await robot.tapRetry();
          expect(robot.isLoaded, isTrue);
          expect(robot.countOf('List'), 2);
          await robot.settle();

        // ── segmented sub-views ───────────────────────────────────────────
        case 'positive_switch_back_to_upstreams':
          fake.listResponse = list([upstream()]);
          fake.healthResponse = UpstreamHealthList(entries: [UpstreamHealth(id: 'u1')]);
          await robot.load();
          await robot.showHealth();
          await robot.showUpstreams();
          expect(robot.isLoaded, isTrue);
          expect(robot.itemCount, 1);
          await robot.settle();

        case 'positive_shows_health_table':
          fake.listResponse = list([upstream()]);
          fake.healthResponse = UpstreamHealthList(
              entries: [UpstreamHealth(id: 'kimi', inFlight: 3)]);
          await robot.load();
          await robot.showHealth();
          expect(robot.fired('Health'), isTrue);
          expect(find.text('kimi'), findsOneWidget);
          await robot.settle();

        case 'positive_shows_route_tester':
          fake.listResponse = list([upstream()]);
          await robot.load();
          await robot.showRoute();
          expect(robot.exists('router.route.run'), isTrue);
          expect(robot.fired('Route'), isFalse);
          await robot.settle();

        case 'positive_refresh_refires_health':
          fake.listResponse = list([upstream()]);
          fake.healthResponse =
              UpstreamHealthList(entries: [UpstreamHealth(id: 'u1')]);
          await robot.load();
          await robot.showHealth();
          expect(robot.countOf('Health'), 1);
          await robot.refreshHealth();
          expect(robot.countOf('Health'), greaterThanOrEqualTo(2));
          await robot.settle();

        // ── upstream CRUD ─────────────────────────────────────────────────
        case 'positive_add_opens_blank_editor':
          fake.listResponse = list([upstream()]);
          await robot.load();
          await robot.tapAdd();
          expect(robot.exists('router.upstream.field.id'), isTrue);
          expect(find.text('New upstream'), findsOneWidget);
          expect(robot.fired('Put'), isFalse);
          await robot.settle();

        case 'positive_toggle_fires_enable':
          fake.listResponse = list([upstream(id: 'u1', enabled: false)]);
          fake.enableResponse = upstream(id: 'u1', enabled: true);
          await robot.load();
          await robot.toggleEnable('u1');
          final en = robot.lastCall('Enable');
          expect((en!.request as UpstreamEnableRequest).id, 'u1');
          expect((en.request as UpstreamEnableRequest).enabled, isTrue);
          await robot.pumpUntil(() => robot.countOf('List') >= 2,
              reason: 'reload after enable');
          await robot.settle();

        case 'positive_delete_opens_confirm_dialog':
          fake.listResponse = list([upstream(id: 'u1')]);
          await robot.load();
          await robot.openDelete('u1');
          expect(robot.exists('router.upstream.delete.confirm'), isTrue);
          expect(robot.exists('router.upstream.delete.cancel'), isTrue);
          expect(robot.fired('Delete'), isFalse); // gate not yet crossed
          await robot.cancelDelete(); // dismiss so no route leaks
          await robot.settle();

        case 'positive_confirm_fires_delete':
          fake.listResponse = list([upstream(id: 'u1')]);
          fake.deleteResponse = UpstreamDeleteReply(deleted: true);
          await robot.load();
          await robot.openDelete('u1');
          await robot.confirmDelete();
          expect((robot.lastCall('Delete')!.request as UpstreamRef).id, 'u1');
          await robot.pumpUntil(() => robot.countOf('List') >= 2,
              reason: 'reload after delete');
          await robot.settle();

        case 'negative_cancel_fires_no_delete':
          fake.listResponse = list([upstream(id: 'u1')]);
          await robot.load();
          await robot.openDelete('u1');
          await robot.cancelDelete();
          expect(robot.exists('router.upstream.delete.confirm'), isFalse);
          expect(robot.fired('Delete'), isFalse); // cancel gates the RPC
          await robot.settle();

        case 'positive_save_fires_put_and_reloads':
          fake.listResponse = list([upstream(id: 'u1', model: 'old')]);
          await robot.load();
          await robot.selectItem('u1');
          await robot.editField('model', 'new-model');
          await robot.save();
          final put = robot.lastCall('Put')!.request as Upstream;
          expect(put.id, 'u1');
          expect(put.model, 'new-model');
          await robot.pumpUntil(() => robot.countOf('List') >= 2,
              reason: 'reload after save');
          await robot.settle();

        case 'negative_save_error_snacks':
          fake.listResponse = list([upstream(id: 'u1')]);
          await robot.load();
          await robot.selectItem('u1');
          fake.error = const GrpcError.permissionDenied('nope');
          await robot.save();
          await robot.pumpUntil(() => robot.snackContains('Save failed'),
              reason: 'save-failed snack');
          expect(robot.isLoaded, isTrue); // no crash
          expect(robot.countOf('List'), 1); // no reload
          await robot.settle();

        case 'adversarial_hostile_url_and_number_sent_safely':
          fake.listResponse = list([upstream()]);
          await robot.load();
          await robot.tapAdd();
          const hostileUrl =
              'http://evil.example/../../etc/passwd?x=<script># ';
          await robot.editField('id', 'evil');
          await robot.editField('baseUrl', hostileUrl);
          await robot.editField('maxConcurrency', 'not-a-number; DROP TABLE');
          await robot.save();
          final adv = robot.lastCall('Put')!.request as Upstream;
          expect(adv.baseUrl, hostileUrl); // sent verbatim, no sanitising
          expect(adv.maxConcurrency, 0); // garbage → 0, no crash
          await robot.pumpUntil(() => robot.countOf('List') >= 2,
              reason: 'reload after save');
          await robot.settle();

        case 'positive_edit_updates_fields':
          fake.listResponse = list([upstream(id: 'u1')]);
          await robot.load();
          await robot.selectItem('u1');
          await robot.editField('model', 'edited-model');
          expect(find.text('edited-model'), findsOneWidget);
          await robot.settle();

        case 'positive_tier_dropdown_saved':
          fake.listResponse = list([upstream()]);
          await robot.load();
          await robot.tapAdd();
          await robot.editField('id', 'tiered');
          await robot.chooseEditorTier('heavy');
          await robot.save();
          expect((robot.lastCall('Put')!.request as Upstream).tier,
              PoolTier.POOL_TIER_HEAVY);
          await robot.pumpUntil(() => robot.countOf('List') >= 2,
              reason: 'reload after save');
          await robot.settle();

        // ── route tester ──────────────────────────────────────────────────
        case 'positive_route_returns_decision':
          fake.listResponse = list([upstream()]);
          fake.routeResponse = RouteDecision(chosen: 'kimi', why: 'least-loaded');
          await robot.load();
          await robot.showRoute();
          await robot.runRoute();
          expect(robot.fired('Route'), isTrue);
          expect(find.textContaining('kimi'), findsWidgets);
          await robot.settle();

        case 'positive_task_mode_in_hint':
          fake.listResponse = list([upstream()]);
          await robot.load();
          await robot.showRoute();
          await robot.chooseTaskMode('implement');
          await robot.runRoute();
          expect((robot.lastCall('Route')!.request as RouteRequest).hint.taskMode,
              TaskMode.TASK_MODE_IMPLEMENT);
          await robot.settle();

        case 'positive_role_in_hint':
          fake.listResponse = list([upstream()]);
          await robot.load();
          await robot.showRoute();
          await robot.chooseRole('judge');
          await robot.runRoute();
          expect((robot.lastCall('Route')!.request as RouteRequest).hint.role,
              RouteRole.ROUTE_ROLE_JUDGE);
          await robot.settle();

        case 'positive_tier_in_hint':
          fake.listResponse = list([upstream()]);
          await robot.load();
          await robot.showRoute();
          await robot.chooseRouteTier('heavy');
          await robot.runRoute();
          expect((robot.lastCall('Route')!.request as RouteRequest).hint.tier,
              PoolTier.POOL_TIER_HEAVY);
          await robot.settle();

        case 'positive_min_context_in_hint':
          fake.listResponse = list([upstream()]);
          await robot.load();
          await robot.showRoute();
          await robot.setMinContext('4096');
          await robot.runRoute();
          expect(
              (robot.lastCall('Route')!.request as RouteRequest).hint.minContext,
              4096);
          await robot.settle();

        case 'adversarial_garbage_numbers_parse_to_zero':
          fake.listResponse = list([upstream()]);
          await robot.load();
          await robot.showRoute();
          await robot.setMinContext('not-a-number');
          await robot.setMaxCost('NaN!! -1e999');
          await robot.runRoute();
          final hint =
              (robot.lastCall('Route')!.request as RouteRequest).hint;
          expect(hint.minContext, 0); // garbage → 0, no crash
          expect(hint.maxCost, 0.0);
          await robot.settle();

        case 'positive_max_cost_in_hint':
          fake.listResponse = list([upstream()]);
          await robot.load();
          await robot.showRoute();
          await robot.setMaxCost('2.5');
          await robot.runRoute();
          expect((robot.lastCall('Route')!.request as RouteRequest).hint.maxCost,
              closeTo(2.5, 1e-6));
          await robot.settle();

        case 'positive_override_in_hint':
          fake.listResponse = list([upstream()]);
          await robot.load();
          await robot.showRoute();
          await robot.setOverride('force-me');
          await robot.runRoute();
          expect(
              (robot.lastCall('Route')!.request as RouteRequest)
                  .hint
                  .overrideUpstream,
              'force-me');
          await robot.settle();

        default:
          fail('no test body for row ${row.label}');
      }

      // The recorded set is exact: the page never dials a seam it should not.
      for (final m in robot.log.methods) {
        expect(m.startsWith('$svc/'), isTrue,
            reason: 'unexpected RPC to a foreign service: $m');
      }
    });
  }
}
