import 'package:agent_portal/src/auth/capabilities.dart';
import 'package:agent_portal/src/pages/access_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../fake_gateway.dart';
import '../fakes/auth_service.dart';
import '../fakes/role_service.dart';
import '../recording.dart';
import 'robot.dart';

/// Robot for the **Access** page (security-hardening S22). Owns a [FakeGateway]
/// hosting a [FakeAuthService] (bindings, sessions) and a [FakeRoleService] (the
/// role catalog), plus the `PortalClients` the page dials; all are torn down in
/// `runAsync` automatically. Script the fakes before [load]; assert on [log].
class AccessRobot extends Robot {
  AccessRobot._(super.tester, this.gw, this.auth, this.roles);

  final FakeGateway gw;
  final FakeAuthService auth;
  final FakeRoleService roles;
  late final clients = gw.clients();

  RecordingLog get log => gw.log;

  static const authSvc = 'agent.v1.AuthService';
  static const roleSvc = 'agent.v1.RoleService';

  static Future<AccessRobot> create(WidgetTester tester) async {
    late FakeAuthService auth;
    late FakeRoleService roles;
    late FakeGateway gw;
    await tester.runAsync(() async {
      gw = await FakeGateway.start((log) {
        auth = FakeAuthService(log);
        roles = FakeRoleService(log);
        return [auth, roles];
      });
    });
    final robot = AccessRobot._(tester, gw, auth, roles);
    addTearDown(() => tester.runAsync(() async {
          await robot.clients.terminate();
          await gw.shutdown();
        }));
    return robot;
  }

  // ── predicates ─────────────────────────────────────────────────────────────
  bool get isError => exists('access.error.retry');
  bool get bindingsLoaded =>
      exists('access.binding.add') ||
      textShown('No role bindings in this tenant.') ||
      itemCount('access.binding.item.') > 0;
  bool get editorOpen => exists('access.binding.save');
  bool get roleEditorOpen => exists('access.role.save');

  bool textShown(String text) => find.textContaining(text).evaluate().isNotEmpty;

  int itemCount(String prefix) => find
      .byWidgetPredicate((w) =>
          w.key is ValueKey<String> &&
          (w.key as ValueKey<String>).value.startsWith(prefix))
      .evaluate()
      .length;

  bool fired(String rpc) => log.fired(rpc);
  int countOf(String rpc) => log.countOf(rpc);
  RecordedCall? last(String rpc) => log.last(rpc);

  // ── page lifecycle ─────────────────────────────────────────────────────────
  Future<void> load() async {
    await pumpPage(AccessPage(clients: clients));
    await pumpUntil(() => bindingsLoaded || isError,
        reason: 'bindings to settle');
  }

  /// [load] with the signed-in user's [caps] in scope.
  Future<void> loadWith(Capabilities caps) async {
    await pumpPage(
        CapabilityScope(capabilities: caps, child: AccessPage(clients: clients)));
    await pumpUntil(() => bindingsLoaded || isError,
        reason: 'bindings to settle');
  }

  Future<void> tapRetry(bool Function() until) =>
      tap('access.error.retry', until: until);

  // ── views ──────────────────────────────────────────────────────────────────
  Future<void> showBindings() => tap('access.view.bindings',
      until: () => bindingsLoaded || isError);

  Future<void> showRoles() => tap('access.view.roles',
      until: () => textShown('Built-in roles') || isError);

  Future<void> showSessions() => tap('access.view.sessions',
      until: () => exists('access.sessions.refresh') || isError);

  // ── bindings ───────────────────────────────────────────────────────────────
  Future<void> openAdd() =>
      tap('access.binding.add', until: () => editorOpen);

  Future<void> openEdit(String id) =>
      tap('access.binding.edit.$id', until: () => editorOpen);

  Future<void> fillBinding({
    String? id,
    String? subject,
    List<String> roles = const [],
    String? expires,
  }) async {
    if (id != null) await enterText('access.binding.field.id', id);
    if (subject != null) await enterText('access.binding.field.subject', subject);
    for (final r in roles) {
      await toggleRole(r);
    }
    if (expires != null) await enterText('access.binding.field.expires', expires);
  }

  Future<void> toggleRole(String role) async {
    await tester.ensureVisible(byKey('access.binding.role.$role'));
    await tester.pump();
    await tester.tap(byKey('access.binding.role.$role'));
    await tester.pump();
  }

  Future<void> chooseKind(String kind) async {
    await tester.tap(byKey('access.binding.field.kind'));
    await tester.pumpAndSettle();
    await tester.tap(find.text(kind).hitTestable().last);
    await tester.pumpAndSettle();
  }

  Future<void> toggleKeep() async {
    await tester.ensureVisible(byKey('access.binding.field.keep'));
    await tester.pump();
    await tester.tap(byKey('access.binding.field.keep'));
    await tester.pump();
  }

  /// Tap Save; waits for the saved binding's reload, or the editor's error.
  /// (A closing dialog's widgets linger until its exit animation runs on the
  /// fake clock, so "the dialog is gone" is not a usable signal.)
  Future<void> saveBinding() async {
    await tester.ensureVisible(byKey('access.binding.save'));
    await tester.pump();
    final before = countOf('$authSvc/ListBindings');
    await tap('access.binding.save',
        until: () =>
            countOf('$authSvc/ListBindings') > before ||
            exists('access.binding.editor.error'));
    // Not pumpAndSettle: the reload's spinner never settles on the fake clock.
    await tester.pump(const Duration(milliseconds: 300));
    if (!editorOpen) {
      await pumpUntil(() => bindingsLoaded, reason: 'reload after save');
    }
  }

  Future<void> cancelBinding() => _dismiss('access.binding.cancel');

  /// Tap a dialog button that makes no call, then run the exit animation.
  Future<void> _dismiss(String key) async {
    await tap(key);
    await tester.pumpAndSettle();
  }

  Future<void> openDelete(String id) => tap('access.binding.delete.$id',
      until: () => exists('access.binding.delete.confirm'));

  Future<void> confirmDelete() async {
    final before = countOf('$authSvc/ListBindings');
    await tap('access.binding.delete.confirm',
        until: () => countOf('$authSvc/ListBindings') > before);
  }

  Future<void> cancelDelete() => _dismiss('access.binding.delete.cancel');

  // ── roles ──────────────────────────────────────────────────────────────────
  Future<void> openAddRole() =>
      tap('access.role.add', until: () => roleEditorOpen);

  Future<void> openEditRole(String id) =>
      tap('access.role.edit.$id', until: () => roleEditorOpen);

  Future<void> saveRole() async {
    final before = countOf('$roleSvc/List');
    await tap('access.role.save',
        until: () =>
            countOf('$roleSvc/List') > before ||
            exists('access.role.editor.error'));
    await tester.pump(const Duration(milliseconds: 300));
    if (!roleEditorOpen) {
      await pumpUntil(() => textShown('Built-in roles'),
          reason: 'reload after save');
    }
  }

  Future<void> cancelRole() => _dismiss('access.role.cancel');

  Future<void> toggleCrosses() async {
    await tester.tap(byKey('access.role.field.crosses'));
    await tester.pump();
  }

  Future<void> openDeleteRole(String id) => tap('access.role.delete.$id',
      until: () => exists('access.role.delete.confirm'));

  // ── sessions ───────────────────────────────────────────────────────────────
  Future<void> openRevoke(String sid) => tap('access.session.revoke.$sid',
      until: () => exists('access.session.revoke.confirm'));

  /// Confirm the open dialog [confirmKey], then wait for the list to reload.
  Future<void> confirmAndReload(String confirmKey, String listRpc) async {
    final before = countOf(listRpc);
    await tap(confirmKey, until: () => countOf(listRpc) > before);
  }

  Future<void> cancelDialog(String cancelKey) => _dismiss(cancelKey);

  /// Advance the fake clock past the timers a completed RPC leaves behind (the
  /// SnackBar's auto-dismiss, grpc-dart's idle timeout), after any reload lands.
  Future<void> settle() async {
    await pumpUntil(
        () => find.byType(CircularProgressIndicator).evaluate().isEmpty,
        reason: 'reload to land');
    await tester.pump(const Duration(seconds: 5));
    await tester.pump(const Duration(minutes: 6));
    await tester.pumpAndSettle();
  }
}
