import 'package:fixnum/fixnum.dart';
import 'package:flutter/material.dart';
import 'package:grpc/grpc.dart';

import '../access_model.dart';
import '../auth/capabilities.dart';
import '../clients.dart';
import '../gen/agent/v1/auth.pb.dart';
import '../gen/agent/v1/role.pb.dart';

/// The **Access** tab (security-hardening S22): who may do what in this tenant.
///
/// - **Bindings** (`AuthService` `ListBindings` / `PutBinding` / `DeleteBinding`):
///   grant roles to a subject, with an optional expiry. A change that takes roles
///   away signs the affected users out unless "keep their sessions" is on.
/// - **Roles** (`RoleService`): the built-in roles plus the operator-defined
///   cards; add, edit or delete a card with `write:role` / `delete:role`.
/// - **Sessions** (`ListSessions` / `RevokeSession`): every sign-in in the
///   tenant, revocable with `write:binding`.
///
/// Controls follow [CapabilityScope]; the server enforces every call.
class AccessPage extends StatefulWidget {
  final PortalClients clients;
  const AccessPage({super.key, required this.clients});

  @override
  State<AccessPage> createState() => _AccessPageState();
}

enum _View { bindings, roles, sessions }

class _AccessPageState extends State<AccessPage> {
  _View _view = _View.bindings;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SegmentedButton<_View>(
            segments: const [
              ButtonSegment(
                value: _View.bindings,
                icon: Icon(Icons.link),
                label: Text('Bindings', key: Key('access.view.bindings')),
              ),
              ButtonSegment(
                value: _View.roles,
                icon: Icon(Icons.badge_outlined),
                label: Text('Roles', key: Key('access.view.roles')),
              ),
              ButtonSegment(
                value: _View.sessions,
                icon: Icon(Icons.devices_outlined),
                label: Text('Sessions', key: Key('access.view.sessions')),
              ),
            ],
            selected: {_view},
            onSelectionChanged: (s) => setState(() => _view = s.first),
          ),
          const SizedBox(height: 12),
          Expanded(
            child: switch (_view) {
              _View.bindings => _BindingsView(clients: widget.clients),
              _View.roles => _RolesView(clients: widget.clients),
              _View.sessions => _SessionsView(clients: widget.clients),
            },
          ),
        ],
      ),
    );
  }
}

void _snack(BuildContext context, String msg) {
  ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
}

String _signedOut(int n) => switch (n) {
  0 => '',
  1 => '; 1 session signed out',
  _ => '; $n sessions signed out',
};

/// A confirm dialog; true when confirmed.
Future<bool> _confirm(
  BuildContext context, {
  required String title,
  required String body,
  required String confirmKey,
  required String cancelKey,
  required String action,
}) async {
  final ok = await showDialog<bool>(
    context: context,
    builder: (ctx) => AlertDialog(
      title: Text(title, maxLines: 2, overflow: TextOverflow.ellipsis),
      content: Text(body),
      actions: [
        TextButton(
          key: Key(cancelKey),
          onPressed: () => Navigator.pop(ctx, false),
          child: const Text('Cancel'),
        ),
        FilledButton(
          key: Key(confirmKey),
          onPressed: () => Navigator.pop(ctx, true),
          child: Text(action),
        ),
      ],
    ),
  );
  return ok == true;
}

/// The load-failure panel: a refusal explains itself, anything else offers a retry.
class _LoadError extends StatelessWidget {
  final Object error;
  final VoidCallback onRetry;
  const _LoadError({required this.error, required this.onRetry});

  @override
  Widget build(BuildContext context) {
    final denied =
        error is GrpcError &&
        (error as GrpcError).code == StatusCode.permissionDenied;
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(denied ? Icons.lock_outline : Icons.cloud_off, size: 48),
          const SizedBox(height: 12),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 32),
            child: Text(
              denied
                  ? 'You do not have permission to see this in your tenant.'
                  : 'Not connected to the gateway.\n${describeAccessError(error)}',
              key: const Key('access.error.message'),
              textAlign: TextAlign.center,
            ),
          ),
          const SizedBox(height: 12),
          FilledButton(
            key: const Key('access.error.retry'),
            onPressed: onRetry,
            child: const Text('Retry'),
          ),
        ],
      ),
    );
  }
}

// ── Bindings ────────────────────────────────────────────────────────────────

class _BindingsView extends StatefulWidget {
  final PortalClients clients;
  const _BindingsView({required this.clients});

  @override
  State<_BindingsView> createState() => _BindingsViewState();
}

class _BindingsViewState extends State<_BindingsView> {
  List<RoleBinding> _bindings = [];
  List<String> _roleNames = builtinRoles;
  bool _loading = true;
  Object? _error;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  Future<void> _reload() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final list = await widget.clients.auth.listBindings(
        ListBindingsRequest(),
      );
      // The role catalog only feeds the editor's choices: without it (not served,
      // or not readable) the built-in roles are still offered.
      var cards = <String>[];
      try {
        final roles = await widget.clients.roles.list(RoleListRequest());
        cards = [for (final r in roles.roles) r.id];
      } catch (_) {}
      if (!mounted) return;
      setState(() {
        _bindings = list.bindings.toList()
          ..sort((a, b) => a.id.compareTo(b.id));
        _roleNames = [
          ...builtinRoles,
          ...cards.where((c) => !builtinRoles.contains(c)),
        ];
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e;
        _loading = false;
      });
    }
  }

  Future<void> _edit(RoleBinding? existing) async {
    final saved = await showDialog<PutBindingResponse>(
      context: context,
      builder: (_) => _BindingEditor(
        clients: widget.clients,
        existing: existing,
        roleNames: _roleNames,
      ),
    );
    if (saved == null || !mounted) return;
    _snack(
      context,
      'Saved binding ${saved.binding.id}${_signedOut(saved.revokedSessions)}.',
    );
    await _reload();
  }

  Future<void> _delete(RoleBinding b) async {
    final keep = await showDialog<bool>(
      context: context,
      builder: (_) => _DeleteBindingDialog(binding: b),
    );
    if (keep == null || !mounted) return;
    try {
      final r = await widget.clients.auth.deleteBinding(
        DeleteBindingRequest()
          ..id = b.id
          ..keepSessions = keep,
      );
      if (!mounted) return;
      _snack(
        context,
        r.deleted
            ? 'Deleted binding ${b.id}${_signedOut(r.revokedSessions)}.'
            : 'Binding ${b.id} was already gone.',
      );
      await _reload();
    } catch (e) {
      if (mounted) _snack(context, 'Delete failed: ${describeAccessError(e)}');
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) return const Center(child: CircularProgressIndicator());
    if (_error != null) return _LoadError(error: _error!, onRetry: _reload);
    final caps = CapabilityScope.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Text(
              'Role bindings',
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const Spacer(),
            if (caps.can('write', 'binding'))
              FilledButton.icon(
                key: const Key('access.binding.add'),
                onPressed: () => _edit(null),
                icon: const Icon(Icons.add),
                label: const Text('Add binding'),
              ),
          ],
        ),
        const SizedBox(height: 8),
        if (_bindings.isEmpty)
          const Text('No role bindings in this tenant.')
        else
          Expanded(
            child: ListView(
              children: [
                for (final b in _bindings)
                  Card(
                    child: ListTile(
                      key: Key('access.binding.item.${b.id}'),
                      title: Text(
                        '${b.subjectKind}: ${b.subject}',
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                      subtitle: Text(
                        '${b.id} · ${b.roles.join(', ')} · expires '
                        '${formatUnix(b.expiresAt.toInt())}'
                        '${b.grantedBy.isEmpty ? '' : ' · by ${b.grantedBy}'}',
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                      ),
                      trailing: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          if (caps.can('write', 'binding'))
                            IconButton(
                              key: Key('access.binding.edit.${b.id}'),
                              tooltip: 'Edit',
                              icon: const Icon(Icons.edit_outlined),
                              onPressed: () => _edit(b),
                            ),
                          if (caps.can('delete', 'binding'))
                            IconButton(
                              key: Key('access.binding.delete.${b.id}'),
                              tooltip: 'Delete',
                              icon: const Icon(Icons.delete_outline),
                              onPressed: () => _delete(b),
                            ),
                        ],
                      ),
                    ),
                  ),
              ],
            ),
          ),
      ],
    );
  }
}

/// Delete confirmation; pops the "keep their sessions" choice, or null.
class _DeleteBindingDialog extends StatefulWidget {
  final RoleBinding binding;
  const _DeleteBindingDialog({required this.binding});

  @override
  State<_DeleteBindingDialog> createState() => _DeleteBindingDialogState();
}

class _DeleteBindingDialogState extends State<_DeleteBindingDialog> {
  bool _keep = false;

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(
        'Delete binding "${widget.binding.id}"?',
        maxLines: 2,
        overflow: TextOverflow.ellipsis,
      ),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text(
            'Its roles are taken away. By default the people it named '
            'are signed out now rather than at their next refresh.',
          ),
          CheckboxListTile(
            key: const Key('access.binding.delete.keep'),
            value: _keep,
            onChanged: (v) => setState(() => _keep = v ?? false),
            title: const Text('Keep their sessions'),
            controlAffinity: ListTileControlAffinity.leading,
          ),
        ],
      ),
      actions: [
        TextButton(
          key: const Key('access.binding.delete.cancel'),
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        FilledButton(
          key: const Key('access.binding.delete.confirm'),
          onPressed: () => Navigator.pop(context, _keep),
          child: const Text('Delete'),
        ),
      ],
    );
  }
}

/// Create or replace a binding. The `PutBinding` call runs here, so a refusal
/// leaves the draft on screen; pops the server's answer on success.
class _BindingEditor extends StatefulWidget {
  final PortalClients clients;
  final RoleBinding? existing;
  final List<String> roleNames;
  const _BindingEditor({
    required this.clients,
    required this.existing,
    required this.roleNames,
  });

  @override
  State<_BindingEditor> createState() => _BindingEditorState();
}

class _BindingEditorState extends State<_BindingEditor> {
  late final _id = TextEditingController(text: widget.existing?.id ?? '');
  late final _subject = TextEditingController(
    text: widget.existing?.subject ?? '',
  );
  final _expires = TextEditingController();
  late String _kind = widget.existing?.subjectKind ?? 'email';
  late final Set<String> _roles = {...?widget.existing?.roles};
  bool _keep = false;
  bool _expiryTouched = false;
  bool _saving = false;
  String? _error;

  @override
  void dispose() {
    _id.dispose();
    _subject.dispose();
    _expires.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final problem = checkBindingDraft(
      id: _id.text.trim(),
      subjectKind: _kind,
      subject: _subject.text.trim(),
      roles: _roles.toList(),
      expiresInDays: _expires.text,
    );
    if (problem != null) {
      setState(() => _error = problem);
      return;
    }
    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final expiresAt = _expiryTouched || widget.existing == null
        ? expiresAtFor(parseExpiryDays(_expires.text)!, now)
        : widget.existing!.expiresAt.toInt();
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      final r = await widget.clients.auth.putBinding(
        PutBindingRequest()
          ..binding = (RoleBinding()
            ..id = _id.text.trim()
            ..subjectKind = _kind
            ..subject = _subject.text.trim()
            ..roles.addAll(_roles.toList()..sort())
            ..expiresAt = Int64(expiresAt))
          ..keepSessions = _keep,
      );
      if (mounted) Navigator.pop(context, r);
    } catch (e) {
      if (mounted) {
        setState(() {
          _saving = false;
          _error = describeAccessError(e);
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final existing = widget.existing;
    final offered = {...widget.roleNames, ..._roles}.toList();
    return AlertDialog(
      title: Text(existing == null ? 'Add binding' : 'Edit binding'),
      content: SizedBox(
        width: 520,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              TextField(
                key: const Key('access.binding.field.id'),
                controller: _id,
                enabled: existing == null,
                decoration: const InputDecoration(labelText: 'Id'),
              ),
              DropdownButtonFormField<String>(
                key: const Key('access.binding.field.kind'),
                initialValue: subjectKinds.contains(_kind) ? _kind : null,
                decoration: const InputDecoration(labelText: 'Subject kind'),
                items: [
                  for (final k in subjectKinds)
                    DropdownMenuItem(value: k, child: Text(k)),
                ],
                onChanged: (v) => setState(() => _kind = v ?? _kind),
              ),
              TextField(
                key: const Key('access.binding.field.subject'),
                controller: _subject,
                maxLength: maxSubjectBytes,
                decoration: const InputDecoration(
                  labelText: 'Subject',
                  helperText: 'issuer/sub, an email, a domain, or an mTLS SAN',
                ),
              ),
              const SizedBox(height: 8),
              const Text('Roles'),
              Wrap(
                spacing: 6,
                runSpacing: 6,
                children: [
                  for (final r in offered)
                    FilterChip(
                      key: Key('access.binding.role.$r'),
                      label: Text(r),
                      selected: _roles.contains(r),
                      onSelected: (on) => setState(() {
                        on ? _roles.add(r) : _roles.remove(r);
                      }),
                    ),
                ],
              ),
              TextField(
                key: const Key('access.binding.field.expires'),
                controller: _expires,
                keyboardType: TextInputType.number,
                onChanged: (_) => _expiryTouched = true,
                decoration: InputDecoration(
                  labelText: 'Expires in days (blank or 0 = never)',
                  helperText: existing == null
                      ? null
                      : 'Now: ${formatUnix(existing.expiresAt.toInt())}; '
                            'leave blank to keep it',
                ),
              ),
              SwitchListTile(
                key: const Key('access.binding.field.keep'),
                value: _keep,
                onChanged: (v) => setState(() => _keep = v),
                title: const Text('Keep their sessions'),
                subtitle: const Text(
                  'Off: a change that takes roles away signs them out now.',
                ),
              ),
              if (_error != null)
                Text(
                  _error!,
                  key: const Key('access.binding.editor.error'),
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          key: const Key('access.binding.cancel'),
          onPressed: _saving ? null : () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        FilledButton(
          key: const Key('access.binding.save'),
          onPressed: _saving ? null : _save,
          child: const Text('Save'),
        ),
      ],
    );
  }
}

// ── Roles ───────────────────────────────────────────────────────────────────

class _RolesView extends StatefulWidget {
  final PortalClients clients;
  const _RolesView({required this.clients});

  @override
  State<_RolesView> createState() => _RolesViewState();
}

class _RolesViewState extends State<_RolesView> {
  List<RoleCard> _cards = [];
  bool _served = true;
  bool _loading = true;
  Object? _error;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  Future<void> _reload() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final list = await widget.clients.roles.list(RoleListRequest());
      if (!mounted) return;
      setState(() {
        _cards = list.roles.toList()..sort((a, b) => a.id.compareTo(b.id));
        _served = true;
        _loading = false;
      });
    } on GrpcError catch (e) {
      if (!mounted) return;
      setState(() {
        // No role store on this gateway: only the built-in roles apply.
        _served = e.code != StatusCode.unimplemented;
        _cards = [];
        _error = _served ? e : null;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e;
        _loading = false;
      });
    }
  }

  Future<void> _edit(RoleCard? existing) async {
    final saved = await showDialog<RoleCard>(
      context: context,
      builder: (_) => _RoleEditor(clients: widget.clients, existing: existing),
    );
    if (saved == null || !mounted) return;
    _snack(context, 'Saved role ${saved.id}.');
    await _reload();
  }

  Future<void> _delete(RoleCard card) async {
    final ok = await _confirm(
      context,
      title: 'Delete role "${card.id}"?',
      body: 'Bindings that name it stop granting it at their next refresh.',
      confirmKey: 'access.role.delete.confirm',
      cancelKey: 'access.role.delete.cancel',
      action: 'Delete',
    );
    if (!ok || !mounted) return;
    try {
      final r = await widget.clients.roles.delete(RoleRef()..id = card.id);
      if (!mounted) return;
      _snack(
        context,
        r.deleted
            ? 'Deleted role ${card.id}.'
            : 'Role ${card.id} was already gone.',
      );
      await _reload();
    } catch (e) {
      if (mounted) _snack(context, 'Delete failed: ${describeAccessError(e)}');
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) return const Center(child: CircularProgressIndicator());
    if (_error != null) return _LoadError(error: _error!, onRetry: _reload);
    final caps = CapabilityScope.of(context);
    return ListView(
      children: [
        Text('Built-in roles', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 6),
        Wrap(
          spacing: 6,
          runSpacing: 6,
          children: [for (final r in builtinRoles) Chip(label: Text(r))],
        ),
        const SizedBox(height: 16),
        Row(
          children: [
            Text(
              'Defined here',
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const Spacer(),
            if (_served && caps.can('write', 'role'))
              FilledButton.icon(
                key: const Key('access.role.add'),
                onPressed: () => _edit(null),
                icon: const Icon(Icons.add),
                label: const Text('Add role'),
              ),
          ],
        ),
        const SizedBox(height: 6),
        if (!_served)
          const Text(
            'This gateway serves no role catalog; only the built-in '
            'roles apply.',
          )
        else if (_cards.isEmpty)
          const Text('No roles defined here.')
        else
          for (final c in _cards)
            Card(
              child: ListTile(
                key: Key('access.role.item.${c.id}'),
                title: Text(c.id, maxLines: 1, overflow: TextOverflow.ellipsis),
                subtitle: Text(
                  '${summarizePermissions(c)}${c.crossesTenants ? ' · host-wide' : ''}',
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                ),
                trailing: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    if (caps.can('write', 'role'))
                      IconButton(
                        key: Key('access.role.edit.${c.id}'),
                        tooltip: 'Edit',
                        icon: const Icon(Icons.edit_outlined),
                        onPressed: () => _edit(c),
                      ),
                    if (caps.can('delete', 'role'))
                      IconButton(
                        key: Key('access.role.delete.${c.id}'),
                        tooltip: 'Delete',
                        icon: const Icon(Icons.delete_outline),
                        onPressed: () => _delete(c),
                      ),
                  ],
                ),
              ),
            ),
      ],
    );
  }
}

/// Create or replace a role card; `RoleService.Put` runs here and pops the
/// stored card on success.
class _RoleEditor extends StatefulWidget {
  final PortalClients clients;
  final RoleCard? existing;
  const _RoleEditor({required this.clients, required this.existing});

  @override
  State<_RoleEditor> createState() => _RoleEditorState();
}

class _RoleEditorState extends State<_RoleEditor> {
  late final _id = TextEditingController(text: widget.existing?.id ?? '');
  late final _perms = TextEditingController(
    text: widget.existing == null ? '' : formatPermissions(widget.existing!),
  );
  late bool _crosses = widget.existing?.crossesTenants ?? false;
  bool _saving = false;
  String? _error;

  @override
  void dispose() {
    _id.dispose();
    _perms.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final parsed = parseRoleCard(
      id: _id.text.trim(),
      crossesTenants: _crosses,
      permissions: _perms.text,
    );
    if (parsed.card == null) {
      setState(() => _error = parsed.error);
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      final stored = await widget.clients.roles.put(parsed.card!);
      if (mounted) Navigator.pop(context, stored);
    } catch (e) {
      if (mounted) {
        setState(() {
          _saving = false;
          _error = describeAccessError(e);
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(widget.existing == null ? 'Add role' : 'Edit role'),
      content: SizedBox(
        width: 520,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              TextField(
                key: const Key('access.role.field.id'),
                controller: _id,
                enabled: widget.existing == null,
                decoration: const InputDecoration(labelText: 'Id'),
              ),
              TextField(
                key: const Key('access.role.field.perms'),
                controller: _perms,
                minLines: 3,
                maxLines: 8,
                decoration: const InputDecoration(
                  labelText: 'Permissions',
                  helperText:
                      'One per line: action:resource (read:fleet), '
                      'action:* for every resource, or * for everything',
                ),
              ),
              SwitchListTile(
                key: const Key('access.role.field.crosses'),
                value: _crosses,
                onChanged: (v) => setState(() => _crosses = v),
                title: const Text('Host-wide (acts in every tenant)'),
              ),
              if (_error != null)
                Text(
                  _error!,
                  key: const Key('access.role.editor.error'),
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          key: const Key('access.role.cancel'),
          onPressed: _saving ? null : () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        FilledButton(
          key: const Key('access.role.save'),
          onPressed: _saving ? null : _save,
          child: const Text('Save'),
        ),
      ],
    );
  }
}

// ── Sessions ────────────────────────────────────────────────────────────────

class _SessionsView extends StatefulWidget {
  final PortalClients clients;
  const _SessionsView({required this.clients});

  @override
  State<_SessionsView> createState() => _SessionsViewState();
}

class _SessionsViewState extends State<_SessionsView> {
  List<AuthSessionInfo> _sessions = [];
  bool _loading = true;
  Object? _error;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  Future<void> _reload() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final list = await widget.clients.auth.listSessions(
        ListSessionsRequest(),
      );
      if (!mounted) return;
      setState(() {
        _sessions = list.sessions.toList()
          ..sort((a, b) => b.lastSeenAt.compareTo(a.lastSeenAt));
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e;
        _loading = false;
      });
    }
  }

  Future<void> _revoke(AuthSessionInfo s) async {
    final ok = await _confirm(
      context,
      title: 'Revoke this session?',
      body: s.current
          ? 'This is your own session: you will be signed out.'
          : '${_who(s)} is signed out of this session now.',
      confirmKey: 'access.session.revoke.confirm',
      cancelKey: 'access.session.revoke.cancel',
      action: 'Revoke',
    );
    if (!ok || !mounted) return;
    try {
      final r = await widget.clients.auth.revokeSession(
        RevokeSessionRequest()..sid = s.sid,
      );
      if (!mounted) return;
      _snack(
        context,
        r.revoked ? 'Session revoked.' : 'That session had already ended.',
      );
      await _reload();
    } catch (e) {
      if (mounted) _snack(context, 'Revoke failed: ${describeAccessError(e)}');
    }
  }

  static String _who(AuthSessionInfo s) =>
      s.email.isNotEmpty ? s.email : s.subject;

  @override
  Widget build(BuildContext context) {
    if (_loading) return const Center(child: CircularProgressIndicator());
    if (_error != null) return _LoadError(error: _error!, onRetry: _reload);
    final canRevoke = CapabilityScope.of(context).can('write', 'binding');
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Text('Sessions', style: Theme.of(context).textTheme.titleMedium),
            const Spacer(),
            IconButton(
              key: const Key('access.sessions.refresh'),
              tooltip: 'Refresh',
              icon: const Icon(Icons.refresh),
              onPressed: _reload,
            ),
          ],
        ),
        if (_sessions.isEmpty)
          const Text('No sessions in this tenant.')
        else
          Expanded(
            child: ListView(
              children: [
                for (final s in _sessions)
                  Card(
                    child: ListTile(
                      key: Key('access.session.item.${s.sid}'),
                      title: Text(
                        '${_who(s)}${s.current ? '  (this session)' : ''}',
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                      subtitle: Text(
                        '${s.clientKind} · ${s.issuer} · signed in '
                        '${formatUnix(s.createdAt.toInt())} · last seen '
                        '${formatUnix(s.lastSeenAt.toInt())} · '
                        '${s.revokedAt > 0 ? 'revoked (${s.revokeReason})' : 'expires ${formatUnix(s.expiresAt.toInt())}'}',
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                      ),
                      trailing: canRevoke && s.revokedAt == 0
                          ? IconButton(
                              key: Key('access.session.revoke.${s.sid}'),
                              tooltip: 'Revoke',
                              icon: const Icon(Icons.block),
                              onPressed: () => _revoke(s),
                            )
                          : null,
                    ),
                  ),
              ],
            ),
          ),
      ],
    );
  }
}
