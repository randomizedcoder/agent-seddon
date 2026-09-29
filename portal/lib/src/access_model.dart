import 'package:grpc/grpc.dart';

import 'gen/agent/v1/role.pb.dart';

/// The pure half of the **Access** tab (security-hardening S22): input checks,
/// the role-permission text format, and refusal wording. No widgets, so the unit
/// tables can drive every branch. The server re-checks everything; these checks
/// only give a faster, clearer answer.

/// The subject kinds a role binding can name (`RoleBinding.subject_kind`).
const subjectKinds = <String>['sub', 'email', 'domain', 'mtls_san'];

/// Built-in role names offered when binding (agent-core `BUILTIN_ROLES`, minus
/// the `reader` alias of `viewer`). The server still refuses what the caller may
/// not grant, such as the host-wide `operator` from a tenant admin.
const builtinRoles = <String>[
  'org_admin',
  'access_admin',
  'fleet_admin',
  'reviewer',
  'review_viewer',
  'agent_user',
  'viewer',
  'svc_fleet',
  'svc_seam',
  'operator',
];

/// Server limits (agent-grpc `server/auth/binding.rs`).
const maxSubjectBytes = 320;
const maxRolesPerBinding = 32;

/// Longest expiry the editor offers, in days (ten years).
const maxExpiryDays = 3650;

/// Most permission entries one role card may list in the editor.
const maxPermissionEntries = 64;

final _idPattern = RegExp(r'^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$');
final _control = RegExp(r'[\x00-\x1f\x7f]');

/// Why [id] cannot name a binding or role, or null when it can. Ids become a
/// storage path segment on the server.
String? checkId(String id) {
  if (id.isEmpty) return 'An id is required.';
  if (!_idPattern.hasMatch(id) || id.contains('..')) {
    return 'The id may use letters, digits, "_", "-" and "." (not "..") and '
        'must start with a letter or digit, at most 128 characters.';
  }
  return null;
}

/// Why this binding draft cannot be sent, or null when it can.
String? checkBindingDraft({
  required String id,
  required String subjectKind,
  required String subject,
  required List<String> roles,
  required String expiresInDays,
}) {
  final idProblem = checkId(id);
  if (idProblem != null) return idProblem;
  if (!subjectKinds.contains(subjectKind)) return 'Pick a subject kind.';
  if (subject.trim().isEmpty) return 'A subject is required.';
  if (subject.length > maxSubjectBytes) {
    return 'The subject is longer than $maxSubjectBytes characters.';
  }
  if (_control.hasMatch(subject)) {
    return 'The subject contains control characters.';
  }
  if (roles.isEmpty) return 'Pick at least one role.';
  if (roles.length > maxRolesPerBinding) {
    return 'At most $maxRolesPerBinding roles per binding.';
  }
  if (parseExpiryDays(expiresInDays) == null) {
    return 'Expiry is a whole number of days from 0 (never) to $maxExpiryDays.';
  }
  return null;
}

/// Days from now until expiry: blank or 0 means never. Null when not a whole
/// number in `0..maxExpiryDays`.
int? parseExpiryDays(String text) {
  final t = text.trim();
  if (t.isEmpty) return 0;
  if (!RegExp(r'^[0-9]{1,5}$').hasMatch(t)) return null;
  final days = int.parse(t);
  return days > maxExpiryDays ? null : days;
}

/// The `expires_at` (Unix seconds) for [days] from [nowSecs]; 0 means never.
int expiresAtFor(int days, int nowSecs) =>
    days == 0 ? 0 : nowSecs + days * 86400;

/// A role card's permissions as the editor shows them: `*` for everything, else
/// one `action:resource` per line (`action:*` for an action on every resource).
String formatPermissions(RoleCard card) {
  if (card.all) return '*';
  if (card.pairs.isNotEmpty) {
    return [
      for (final p in card.pairs) '${p.action}:${p.resourceType}',
    ].join('\n');
  }
  return [for (final a in card.actionsOnAll) '$a:*'].join('\n');
}

/// One line summary of a role card's grants.
String summarizePermissions(RoleCard card) {
  if (card.all) return 'everything';
  if (card.pairs.isNotEmpty) {
    return [
      for (final p in card.pairs) '${p.action}:${p.resourceType}',
    ].join(', ');
  }
  if (card.actionsOnAll.isEmpty) return 'nothing';
  return '${card.actionsOnAll.join(', ')} on every resource';
}

/// Parse the editor's permission text into a card, or say why not. The three
/// wire shapes cannot mix: `*` alone, only `action:*` lines, or only
/// `action:resource` lines. Names are checked by the server (an unknown action
/// or resource is refused there).
({RoleCard? card, String? error}) parseRoleCard({
  required String id,
  required bool crossesTenants,
  required String permissions,
}) {
  ({RoleCard? card, String? error}) fail(String why) =>
      (card: null, error: why);
  final idProblem = checkId(id);
  if (idProblem != null) return fail(idProblem);
  if (builtinRoles.contains(id) || id == 'reader') {
    return fail('"$id" is a built-in role; pick another id.');
  }
  final entries = [
    for (final raw in permissions.split(RegExp(r'[\n,]')))
      if (raw.trim().isNotEmpty) raw.trim(),
  ];
  if (entries.isEmpty) return fail('A role must grant something.');
  if (entries.length > maxPermissionEntries) {
    return fail('At most $maxPermissionEntries permission entries.');
  }
  final card = RoleCard(id: id, crossesTenants: crossesTenants);
  if (entries.contains('*')) {
    if (entries.length > 1) {
      return fail('"*" grants everything; list nothing else.');
    }
    return (card: card..all = true, error: null);
  }
  final pairs = <(String, String)>[];
  for (final e in entries) {
    final m = RegExp(r'^([a-z_]{1,32}):([a-z_]{1,32}|\*)$').firstMatch(e);
    if (m == null) {
      return fail('"$e" is not action:resource (e.g. read:fleet).');
    }
    pairs.add((m.group(1)!, m.group(2)!));
  }
  final onAll = pairs.where((p) => p.$2 == '*').length;
  if (onAll == pairs.length) {
    return (
      card: card..actionsOnAll.addAll([for (final p in pairs) p.$1]),
      error: null,
    );
  }
  if (onAll > 0) {
    return fail(
      'Use either action:* lines or action:resource lines, not both.',
    );
  }
  card.pairs.addAll([
    for (final p in pairs) RolePermission(action: p.$1, resourceType: p.$2),
  ]);
  return (card: card, error: null);
}

/// What to tell the user when an access-control call fails. Escalation,
/// host-wide and self-binding refusals all arrive as the same opaque
/// `PermissionDenied`, so the text lists the usual causes.
String describeAccessError(Object error) {
  if (error is GrpcError) {
    final message = error.message ?? '';
    switch (error.code) {
      case StatusCode.permissionDenied:
        return 'Not allowed. You can grant only permissions you hold, never a '
            'host-wide role unless you have one, and never to yourself.';
      case StatusCode.failedPrecondition:
        return message.isEmpty ? 'The server refused the change.' : message;
      case StatusCode.invalidArgument:
        return 'Rejected: ${message.isEmpty ? 'invalid input' : message}';
      case StatusCode.notFound:
        return 'It no longer exists; refresh the list.';
      case StatusCode.unimplemented:
        return 'This gateway does not serve that.';
      case StatusCode.unauthenticated:
        return 'Your sign-in has ended; sign in again.';
    }
    return message.isEmpty ? 'The call failed (code ${error.code}).' : message;
  }
  return '$error';
}

/// Unix seconds as `YYYY-MM-DD HH:MM` UTC; 0 is shown as [zero].
String formatUnix(int secs, {String zero = 'never'}) {
  if (secs <= 0) return zero;
  final t = DateTime.fromMillisecondsSinceEpoch(secs * 1000, isUtc: true);
  String two(int n) => n.toString().padLeft(2, '0');
  return '${t.year}-${two(t.month)}-${two(t.day)} ${two(t.hour)}:${two(t.minute)} UTC';
}
