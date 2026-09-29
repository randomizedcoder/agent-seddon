import 'package:agent_portal/src/auth/capabilities.dart';
import 'package:agent_portal/src/gen/agent/v1/auth.pb.dart';
import 'package:agent_portal/src/gen/agent/v1/role.pb.dart';
import 'package:fixnum/fixnum.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../testkit/robots/access_robot.dart';
import 'access_spec.dart';

/// Layer-A widget tests for the Access page (security-hardening S22): one test
/// per [accessSpec] row. Each arranges the fakes, drives the page through its
/// robot, then asserts the recorded RPC (method and decoded request) and what
/// the page shows.
void main() {
  const auth = AccessRobot.authSvc;
  const role = AccessRobot.roleSvc;

  RoleBinding binding({
    String id = 'b1',
    String kind = 'email',
    String subject = 'alice@example.com',
    List<String> roles = const ['viewer'],
    int expiresAt = 0,
  }) =>
      RoleBinding(
        id: id,
        subjectKind: kind,
        subject: subject,
        roles: roles,
        grantedBy: 'admin@example.com',
        expiresAt: Int64(expiresAt),
      );

  AuthSessionInfo session(String sid,
          {int lastSeen = 100, bool current = false, int revokedAt = 0}) =>
      AuthSessionInfo(
        sid: sid,
        tenant: 'example.com',
        subject: 'user:$sid',
        email: '$sid@example.com',
        clientKind: 'portal',
        issuer: 'google',
        createdAt: Int64(50),
        lastSeenAt: Int64(lastSeen),
        expiresAt: Int64(4000000000),
        revokedAt: Int64(revokedAt),
        revokeReason: revokedAt > 0 ? 'logout' : '',
        current: current,
      );

  RoleCard triager({bool crosses = false}) => RoleCard(
        id: 'triager',
        crossesTenants: crosses,
        pairs: [
          RolePermission(action: 'read', resourceType: 'fleet'),
          RolePermission(action: 'write', resourceType: 'fleet'),
        ],
      );

  int nowSecs() => DateTime.now().millisecondsSinceEpoch ~/ 1000;

  for (final row in accessSpec.rows) {
    testWidgets('access ${row.label} — ${row.description}', (tester) async {
      final robot = await AccessRobot.create(tester);
      final fake = robot.auth;
      final roles = robot.roles;
      fake.listBindingsResponse = ListBindingsResponse(bindings: [binding()]);

      PutBindingRequest put() =>
          robot.last('$auth/PutBinding')!.request as PutBindingRequest;
      RoleCard putRole() => robot.last('$role/Put')!.request as RoleCard;

      Future<void> addWith({
        String id = 'b2',
        String subject = 'bob@example.com',
        List<String> pick = const ['viewer'],
        String? expires,
      }) async {
        await robot.load();
        await robot.openAdd();
        await robot.fillBinding(
            id: id, subject: subject, roles: pick, expires: expires);
      }

      Future<void> addRoleWith(String id, String perms) async {
        roles.listResponse = RoleList(roles: [triager()]);
        await robot.load();
        await robot.showRoles();
        await robot.openAddRole();
        await robot.enterText('access.role.field.id', id);
        await robot.enterText('access.role.field.perms', perms);
      }

      switch (row.label) {
        // ── bindings list ─────────────────────────────────────────────────
        case 'positive_loaded_bindings_render':
          fake.listBindingsResponse = ListBindingsResponse(
              bindings: [binding(), binding(id: 'b0', subject: 'x.org', kind: 'domain')]);
          await robot.load();
          expect(robot.fired('$auth/ListBindings'), isTrue);
          expect(robot.itemCount('access.binding.item.'), 2);
          expect(robot.textShown('email: alice@example.com'), isTrue);
          expect(robot.textShown('domain: x.org'), isTrue);
          await robot.settle();

        case 'boundary_empty_tenant_no_bindings':
          fake.listBindingsResponse = ListBindingsResponse();
          await robot.load();
          expect(robot.itemCount('access.binding.item.'), 0);
          expect(robot.textShown('No role bindings in this tenant.'), isTrue);
          await robot.settle();

        case 'adversarial_hostile_long_subject_renders':
          final hostile = '\u202E${'w' * 319}';
          fake.listBindingsResponse =
              ListBindingsResponse(bindings: [binding(subject: hostile)]);
          await robot.load();
          expect(robot.exists('access.binding.item.b1'), isTrue);
          expect(tester.takeException(), isNull);
          await robot.settle();

        case 'negative_list_error_shows_retry':
          fake.listBindingsError = const GrpcError.unavailable('gateway down');
          await robot.load();
          expect(robot.isError, isTrue);
          await robot.settle();

        case 'positive_retry_recovers':
          fake.listBindingsError = const GrpcError.unavailable('gateway down');
          await robot.load();
          fake.listBindingsError = null;
          await robot.tapRetry(() => robot.bindingsLoaded);
          expect(robot.countOf('$auth/ListBindings'), 2);
          await robot.settle();

        case 'positive_offline_message_names_the_error':
          fake.listBindingsError = const GrpcError.unavailable('gateway down');
          await robot.load();
          expect(robot.textShown('Not connected to the gateway'), isTrue);
          expect(robot.textShown('gateway down'), isTrue);
          await robot.settle();

        case 'negative_permission_denied_explains':
          fake.listBindingsError = const GrpcError.permissionDenied('denied');
          await robot.load();
          expect(robot.textShown('You do not have permission'), isTrue);
          expect(robot.textShown('Not connected'), isFalse);
          await robot.settle();

        case 'positive_switch_back_to_bindings':
          await robot.load();
          await robot.showRoles();
          await robot.showBindings();
          expect(robot.countOf('$auth/ListBindings'), 2);
          await robot.settle();

        // ── binding editor ────────────────────────────────────────────────
        case 'positive_add_binding_puts_it':
          await addWith();
          await robot.saveBinding();
          final req = put();
          expect(req.binding.id, 'b2');
          expect(req.binding.subjectKind, 'email');
          expect(req.binding.subject, 'bob@example.com');
          expect(req.binding.roles, ['viewer']);
          expect(req.binding.expiresAt, Int64.ZERO);
          expect(req.keepSessions, isFalse);
          expect(robot.textShown('Saved binding b2'), isTrue);
          await robot.settle();

        case 'negative_viewer_sees_no_edit_controls':
          await robot.loadWith(Capabilities.of(const ['read:binding']));
          expect(robot.exists('access.binding.item.b1'), isTrue);
          expect(robot.exists('access.binding.add'), isFalse);
          expect(robot.exists('access.binding.edit.b1'), isFalse);
          expect(robot.exists('access.binding.delete.b1'), isFalse);
          await robot.settle();

        case 'positive_id_trimmed':
          await addWith(id: '  b2  ');
          await robot.saveBinding();
          expect(put().binding.id, 'b2');
          await robot.settle();

        case 'adversarial_traversal_id_refused':
          await addWith(id: '../x');
          await robot.saveBinding();
          expect(robot.exists('access.binding.editor.error'), isTrue);
          expect(robot.textShown('not ".."'), isTrue);
          expect(robot.fired('$auth/PutBinding'), isFalse);
          await robot.cancelBinding();
          await robot.settle();

        case 'positive_choose_domain_kind':
          await addWith(subject: 'example.com');
          await robot.chooseKind('domain');
          await robot.saveBinding();
          expect(put().binding.subjectKind, 'domain');
          expect(put().binding.subject, 'example.com');
          await robot.settle();

        case 'positive_subject_trimmed':
          await addWith(subject: '  bob@example.com ');
          await robot.saveBinding();
          expect(put().binding.subject, 'bob@example.com');
          await robot.settle();

        case 'boundary_subject_capped_at_320':
          await addWith(subject: 'a' * 321);
          await robot.saveBinding();
          expect(put().binding.subject.length, 320);
          await robot.settle();

        case 'adversarial_control_chars_refused':
          await addWith(subject: 'bob\u0007@example.com');
          await robot.saveBinding();
          expect(robot.textShown('control characters'), isTrue);
          expect(robot.fired('$auth/PutBinding'), isFalse);
          await robot.cancelBinding();
          await robot.settle();

        case 'positive_roles_sent_sorted':
          roles.listResponse = RoleList(roles: [triager()]);
          await addWith(pick: ['viewer', 'triager']);
          await robot.saveBinding();
          expect(put().binding.roles, ['triager', 'viewer']);
          await robot.settle();

        case 'corner_catalog_unavailable_offers_builtins':
          roles.listError = const GrpcError.unimplemented('no role store');
          await robot.load();
          expect(robot.fired('$role/List'), isTrue);
          expect(robot.isError, isFalse);
          await robot.openAdd();
          expect(robot.exists('access.binding.role.org_admin'), isTrue);
          expect(robot.exists('access.binding.role.viewer'), isTrue);
          await robot.cancelBinding();
          await robot.settle();

        case 'positive_expiry_days_sent':
          await addWith(expires: '30');
          final before = nowSecs();
          await robot.saveBinding();
          final at = put().binding.expiresAt.toInt();
          expect(at, greaterThanOrEqualTo(before + 30 * 86400));
          expect(at, lessThanOrEqualTo(nowSecs() + 30 * 86400));
          await robot.settle();

        case 'boundary_expiry_over_cap_refused':
          await addWith(expires: '3651');
          await robot.saveBinding();
          expect(robot.textShown('whole number of days'), isTrue);
          expect(robot.fired('$auth/PutBinding'), isFalse);
          await robot.cancelBinding();
          await robot.settle();

        case 'positive_keep_sessions_sent':
          await addWith();
          await robot.toggleKeep();
          await robot.saveBinding();
          expect(put().keepSessions, isTrue);
          await robot.settle();

        case 'positive_save_reports_signed_out_sessions':
          fake.putRevokedSessions = 2;
          await addWith();
          await robot.saveBinding();
          expect(robot.textShown('2 sessions signed out'), isTrue);
          await robot.settle();

        case 'positive_cancel_sends_nothing':
          await addWith();
          await robot.cancelBinding();
          expect(robot.editorOpen, isFalse);
          expect(robot.fired('$auth/PutBinding'), isFalse);
          await robot.settle();

        case 'positive_missing_role_shown':
          await addWith(pick: const []);
          await robot.saveBinding();
          expect(robot.textShown('Pick at least one role.'), isTrue);
          expect(robot.fired('$auth/PutBinding'), isFalse);
          await robot.cancelBinding();
          await robot.settle();

        case 'corner_last_admin_message_shown':
          fake.putBindingError = const GrpcError.failedPrecondition(
              'the change would leave the tenant with nobody who can manage '
              'role bindings');
          await addWith();
          await robot.saveBinding();
          expect(robot.editorOpen, isTrue);
          expect(robot.textShown('nobody who can manage'), isTrue);
          await robot.cancelBinding();
          await robot.settle();

        case 'adversarial_escalation_refusal_explained':
          fake.putBindingError = const GrpcError.permissionDenied('denied');
          await addWith(pick: ['operator']);
          await robot.saveBinding();
          expect(robot.textShown('You can grant only permissions you hold'), isTrue);
          await robot.cancelBinding();
          await robot.settle();

        case 'positive_edit_keeps_id_and_expiry':
          fake.listBindingsResponse =
              ListBindingsResponse(bindings: [binding(expiresAt: 2000000000)]);
          await robot.load();
          await robot.openEdit('b1');
          expect(
              tester.widget<TextField>(robot.byKey('access.binding.field.id')).enabled,
              isFalse);
          await robot.toggleRole('reviewer');
          await robot.saveBinding();
          expect(put().binding.id, 'b1');
          expect(put().binding.expiresAt, Int64(2000000000));
          expect(put().binding.roles, ['reviewer', 'viewer']);
          await robot.settle();

        // ── delete ────────────────────────────────────────────────────────
        case 'positive_delete_confirmed':
          await robot.load();
          await robot.openDelete('b1');
          await robot.confirmDelete();
          final req =
              robot.last('$auth/DeleteBinding')!.request as DeleteBindingRequest;
          expect(req.id, 'b1');
          expect(req.keepSessions, isFalse);
          expect(robot.textShown('Deleted binding b1'), isTrue);
          await robot.settle();

        case 'negative_delete_cancel_sends_nothing':
          await robot.load();
          await robot.openDelete('b1');
          await robot.cancelDelete();
          expect(robot.fired('$auth/DeleteBinding'), isFalse);
          await robot.settle();

        case 'positive_keep_checked_sent':
          await robot.load();
          await robot.openDelete('b1');
          await tester.tap(robot.byKey('access.binding.delete.keep'));
          await tester.pump();
          await robot.confirmDelete();
          final req =
              robot.last('$auth/DeleteBinding')!.request as DeleteBindingRequest;
          expect(req.keepSessions, isTrue);
          await robot.settle();

        // ── roles ─────────────────────────────────────────────────────────
        case 'positive_shows_roles':
          roles.listResponse = RoleList(roles: [triager()]);
          await robot.load();
          final before = robot.countOf('$role/List');
          await robot.showRoles();
          expect(robot.countOf('$role/List'), before + 1);
          expect(robot.exists('access.role.item.triager'), isTrue);
          expect(robot.textShown('org_admin'), isTrue);
          await robot.settle();

        case 'positive_cards_render_with_summary':
          roles.listResponse = RoleList(roles: [triager(crosses: true)]);
          await robot.load();
          await robot.showRoles();
          expect(robot.textShown('read:fleet, write:fleet · host-wide'), isTrue);
          await robot.settle();

        case 'corner_catalog_not_served':
          roles.listError = const GrpcError.unimplemented('no role store');
          await robot.load();
          await robot.showRoles();
          expect(robot.isError, isFalse);
          expect(robot.textShown('serves no role catalog'), isTrue);
          expect(robot.exists('access.role.add'), isFalse);
          await robot.settle();

        case 'positive_add_role_opens_editor':
          await robot.load();
          await robot.showRoles();
          await robot.openAddRole();
          expect(
              tester
                  .widget<TextField>(robot.byKey('access.role.field.id'))
                  .controller!
                  .text,
              isEmpty);
          await robot.cancelRole();
          await robot.settle();

        case 'negative_reader_sees_no_role_controls':
          roles.listResponse = RoleList(roles: [triager()]);
          await robot.loadWith(Capabilities.of(const ['read:binding', 'read:role']));
          await robot.showRoles();
          expect(robot.exists('access.role.item.triager'), isTrue);
          expect(robot.exists('access.role.add'), isFalse);
          expect(robot.exists('access.role.edit.triager'), isFalse);
          expect(robot.exists('access.role.delete.triager'), isFalse);
          await robot.settle();

        case 'positive_id_sent_trimmed':
          await addRoleWith('  labeler  ', 'read:review');
          await robot.saveRole();
          expect(putRole().id, 'labeler');
          expect(robot.textShown('Saved role labeler'), isTrue);
          await robot.settle();

        case 'adversarial_builtin_id_refused':
          await addRoleWith('operator', 'read:fleet');
          await robot.saveRole();
          expect(robot.textShown('is a built-in role'), isTrue);
          expect(robot.fired('$role/Put'), isFalse);
          await robot.cancelRole();
          await robot.settle();

        case 'positive_pairs_sent':
          await addRoleWith('labeler', 'read:fleet\nwrite:review');
          await robot.saveRole();
          final card = putRole();
          expect(
              [for (final p in card.pairs) '${p.action}:${p.resourceType}'],
              ['read:fleet', 'write:review']);
          expect(card.all, isFalse);
          expect(card.actionsOnAll, isEmpty);
          await robot.settle();

        case 'positive_host_wide_sent':
          await addRoleWith('labeler', 'read:fleet');
          await robot.toggleCrosses();
          await robot.saveRole();
          expect(putRole().crossesTenants, isTrue);
          await robot.settle();

        case 'positive_actions_on_all_sent':
          await addRoleWith('auditor', 'read:*, observe:*');
          await robot.saveRole();
          final card = putRole();
          expect(card.actionsOnAll, ['read', 'observe']);
          expect(card.pairs, isEmpty);
          expect(card.all, isFalse);
          await robot.settle();

        case 'positive_role_cancel_sends_nothing':
          await addRoleWith('labeler', 'read:fleet');
          await robot.cancelRole();
          expect(robot.fired('$role/Put'), isFalse);
          await robot.settle();

        case 'positive_mixed_shapes_refused':
          await addRoleWith('labeler', 'read:*\nwrite:fleet');
          await robot.saveRole();
          expect(robot.textShown('either action:*'), isTrue);
          expect(robot.fired('$role/Put'), isFalse);
          await robot.cancelRole();
          await robot.settle();

        case 'adversarial_server_refusal_shown':
          roles.putError = const GrpcError.invalidArgument('unknown action "fly"');
          await addRoleWith('labeler', 'fly:fleet');
          await robot.saveRole();
          expect(robot.textShown('Rejected: unknown action'), isTrue);
          await robot.cancelRole();
          await robot.settle();

        case 'positive_edit_prefills_permissions':
          roles.listResponse = RoleList(roles: [RoleCard(id: 'root', all: true)]);
          await robot.load();
          await robot.showRoles();
          await robot.openEditRole('root');
          expect(
              tester
                  .widget<TextField>(robot.byKey('access.role.field.perms'))
                  .controller!
                  .text,
              '*');
          await robot.saveRole();
          expect(putRole().id, 'root');
          expect(putRole().all, isTrue);
          await robot.settle();

        case 'positive_role_delete_confirmed':
          roles.listResponse = RoleList(roles: [triager()]);
          await robot.load();
          await robot.showRoles();
          await robot.openDeleteRole('triager');
          await robot.confirmAndReload('access.role.delete.confirm', '$role/List');
          expect((robot.last('$role/Delete')!.request as RoleRef).id, 'triager');
          expect(robot.textShown('Deleted role triager'), isTrue);
          await robot.settle();

        // ── sessions ──────────────────────────────────────────────────────
        case 'positive_shows_sessions':
          fake.listSessionsResponse =
              ListSessionsResponse(sessions: [session('s1'), session('s2')]);
          await robot.load();
          await robot.showSessions();
          expect(robot.fired('$auth/ListSessions'), isTrue);
          expect(robot.itemCount('access.session.item.'), 2);
          await robot.settle();

        case 'positive_sorted_by_last_seen':
          fake.listSessionsResponse = ListSessionsResponse(sessions: [
            session('s1', lastSeen: 100),
            session('s2', lastSeen: 200, current: true),
          ]);
          await robot.load();
          await robot.showSessions();
          expect(
              tester.getTopLeft(robot.byKey('access.session.item.s2')).dy,
              lessThan(tester.getTopLeft(robot.byKey('access.session.item.s1')).dy));
          expect(robot.textShown('(this session)'), isTrue);
          await robot.settle();

        case 'positive_revoke_confirmed':
          fake.listSessionsResponse = ListSessionsResponse(sessions: [session('s1')]);
          await robot.load();
          await robot.showSessions();
          await robot.openRevoke('s1');
          await robot.confirmAndReload(
              'access.session.revoke.confirm', '$auth/ListSessions');
          expect(
              (robot.last('$auth/RevokeSession')!.request as RevokeSessionRequest)
                  .sid,
              's1');
          expect(robot.textShown('Session revoked.'), isTrue);
          await robot.settle();

        case 'negative_revoke_cancel_sends_nothing':
          fake.listSessionsResponse = ListSessionsResponse(sessions: [session('s1')]);
          await robot.load();
          await robot.showSessions();
          await robot.openRevoke('s1');
          await robot.cancelDialog('access.session.revoke.cancel');
          expect(robot.fired('$auth/RevokeSession'), isFalse);
          await robot.settle();

        case 'corner_revoked_session_has_no_button':
          fake.listSessionsResponse =
              ListSessionsResponse(sessions: [session('s1', revokedAt: 150)]);
          await robot.load();
          await robot.showSessions();
          expect(robot.exists('access.session.item.s1'), isTrue);
          expect(robot.exists('access.session.revoke.s1'), isFalse);
          expect(robot.textShown('revoked (logout)'), isTrue);
          await robot.settle();

        case 'corner_own_session_warns':
          fake.listSessionsResponse =
              ListSessionsResponse(sessions: [session('s2', current: true)]);
          await robot.load();
          await robot.showSessions();
          await robot.openRevoke('s2');
          expect(robot.textShown('you will be signed out'), isTrue);
          await robot.cancelDialog('access.session.revoke.cancel');
          await robot.settle();

        case 'adversarial_revoke_denied_snack':
          fake.listSessionsResponse = ListSessionsResponse(sessions: [session('s1')]);
          fake.revokeSessionError = const GrpcError.permissionDenied('denied');
          await robot.load();
          await robot.showSessions();
          await robot.openRevoke('s1');
          await robot.tap('access.session.revoke.confirm',
              until: () => robot.textShown('Revoke failed'));
          expect(robot.textShown('Not allowed'), isTrue);
          await robot.settle();

        case 'positive_refresh_relists':
          fake.listSessionsResponse = ListSessionsResponse(sessions: [session('s1')]);
          await robot.load();
          await robot.showSessions();
          await robot.tap('access.sessions.refresh',
              until: () => robot.countOf('$auth/ListSessions') >= 2);
          await robot.settle();

        default:
          fail('no test body for row ${row.label}');
      }
    });
  }
}
