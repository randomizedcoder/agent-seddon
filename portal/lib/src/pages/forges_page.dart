import 'package:flutter/material.dart';
import 'package:grpc/grpc.dart' show GrpcError, StatusCode;

import '../auth/auth_platform.dart';
import '../auth/capabilities.dart';
import '../auth/install_callback.dart';
import '../clients.dart';
import '../gen/agent/v1/forge_registry.pb.dart';
import '../gen/agent/v1/github_install_registry.pb.dart';

/// The **Forges** tab (forge-auth F5d1): this tenant's GitHub App installs and
/// git-host forge cards, over `GithubInstallRegistryService` and
/// `ForgeRegistryService` on the `--serve-all` gateway.
///
/// **Connect GitHub** runs the onboarding round trip: `BeginInstall` hands back
/// GitHub's install URL and a single-use `state`, which this tab remembers
/// before leaving. GitHub returns the browser to the portal with `?code=
/// &installation_id=&setup_action=&state=`; [AuthState.start] sets that aside
/// (it is not a sign-in return), and this page relays it in `CompleteInstall`
/// only when the `state` is the one this tab is waiting on — a forged or
/// replayed link never sends its `code` anywhere. The server then proves the
/// user administers the install before it may mint tokens.
///
/// An install becomes usable by the fleet through a forge card whose
/// `token_ref` is `ghapp:<installation id>`; **Create forge card** writes one.
class ForgesPage extends StatefulWidget {
  final PortalClients clients;

  /// Per-tab storage and navigation (the install `state`, the GitHub hop).
  final AuthPlatform platform;

  /// Unix seconds (tests pin it).
  final int Function() now;

  const ForgesPage({
    super.key,
    required this.clients,
    required this.platform,
    required this.now,
  });

  @override
  State<ForgesPage> createState() => _ForgesPageState();
}

/// How a banner reads: a finished install, a note, or a refusal.
enum _Tone { ok, info, error }

/// The longest forge-card id the dialog accepts.
const maxForgeIdLen = 64;

/// A forge-card id the agent's `safe_segment` would accept: ASCII letters,
/// digits, `.`, `_`, `-`; no leading `.`/`-`; no `..`. Checked here only to
/// explain early — the server validates again.
bool validForgeId(String s) =>
    s.isNotEmpty &&
    s.length <= maxForgeIdLen &&
    RegExp(r'^[A-Za-z0-9._-]+$').hasMatch(s) &&
    !s.startsWith('.') &&
    !s.startsWith('-') &&
    !s.contains('..');

/// Whether the portal may send the browser to [url]: `https`, or `http` to a
/// loopback host (a local test agent). The URL comes off the wire, so anything
/// else — `javascript:`, a relative path — is refused.
bool safeInstallUrl(String url) {
  final u = Uri.tryParse(url);
  if (u == null || !u.hasAuthority || u.host.isEmpty) return false;
  if (u.scheme == 'https') return true;
  return u.scheme == 'http' &&
      (u.host == '127.0.0.1' || u.host == 'localhost' || u.host == '[::1]' || u.host == '::1');
}

/// The `token_ref` a forge card uses to mint as installation [id].
String ghappRef(String id) => 'ghapp:$id';

class _ForgesPageState extends State<ForgesPage> {
  List<GithubInstall> _installs = [];
  List<ForgeCard> _cards = [];
  bool _loading = true;
  String? _serverError;
  bool _connecting = false;
  ({_Tone tone, String text})? _banner;

  @override
  void initState() {
    super.initState();
    _start();
  }

  Future<void> _start() async {
    final cb = takeStashedInstallCallback(widget.platform);
    if (cb != null) await _complete(cb);
    await _reload();
  }

  Future<void> _reload() async {
    setState(() {
      _loading = true;
      _serverError = null;
    });
    try {
      final results = await Future.wait([
        widget.clients.githubInstalls.list(GithubInstallListRequest()),
        widget.clients.forges.list(ForgeListRequest()),
      ]);
      if (!mounted) return;
      setState(() {
        _installs = (results[0] as GithubInstallList).installs.toList()
          ..sort((a, b) => a.installationId.compareTo(b.installationId));
        _cards = (results[1] as ForgeList).forges.toList()
          ..sort((a, b) => a.id.compareTo(b.id));
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _serverError = e is GrpcError ? (e.message ?? e.codeName) : '$e';
        _loading = false;
      });
    }
  }

  void _show(_Tone tone, String text) {
    if (!mounted) return;
    setState(() => _banner = (tone: tone, text: text));
  }

  void _snack(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  /// `BeginInstall`, remember its `state`, go to GitHub.
  Future<void> _connect() async {
    setState(() => _connecting = true);
    try {
      final begun =
          await widget.clients.githubInstalls.beginInstall(BeginInstallRequest());
      if (!safeInstallUrl(begun.installUrl) || begun.state.isEmpty) {
        _show(_Tone.error,
            'The agent returned an install link the portal will not follow.');
        return;
      }
      storeInstallPending(widget.platform, begun.state, begun.expiresAt.toInt());
      if (!widget.platform.canRedirect) {
        _show(_Tone.info,
            'Connecting GitHub needs the web portal (`nix run .#portal-web`).');
        return;
      }
      widget.platform.navigate(begun.installUrl);
    } on GrpcError catch (e) {
      _show(
          _Tone.error,
          switch (e.code) {
            StatusCode.failedPrecondition || StatusCode.unimplemented =>
              "GitHub App onboarding isn't configured on this agent "
                  '(`[github_app] slug` and the OAuth client).',
            StatusCode.unauthenticated => 'Sign in again to connect GitHub.',
            StatusCode.permissionDenied =>
              'Your role cannot connect GitHub (needs write on forge_registry).',
            StatusCode.resourceExhausted =>
              'Too many installs are in progress; try again in a few minutes.',
            _ => 'Could not start the GitHub install (${e.codeName}).',
          });
    } finally {
      if (mounted) setState(() => _connecting = false);
    }
  }

  /// Relay GitHub's return, but only for the `state` this tab is waiting on.
  Future<void> _complete(InstallCallback cb) async {
    final expected = takeInstallPending(widget.platform, widget.now());
    if (cb.awaitingApproval) {
      _show(_Tone.info,
          'An organization owner must approve the install. Connect again once '
          'it is approved.');
      return;
    }
    if (cb.malformed || expected == null || expected != cb.state) {
      _show(_Tone.error,
          "That GitHub install didn't start in this tab, or it expired. "
          'Connect again.');
      return;
    }
    try {
      final got = await widget.clients.githubInstalls.completeInstall(
        CompleteInstallRequest(
          state: cb.state,
          code: cb.code,
          installationId: cb.installationId,
        ),
      );
      _show(_Tone.ok,
          'GitHub installation ${got.installationId} is connected. Create a '
          'forge card for it to use it in the fleet.');
    } on GrpcError catch (e) {
      _show(
          _Tone.error,
          switch (e.code) {
            // Opaque on purpose, as the server is: a bad state and a failed
            // ownership proof look the same.
            StatusCode.permissionDenied =>
              "GitHub didn't confirm you administer that account, or the "
                  'install link expired. Connect again.',
            StatusCode.alreadyExists =>
              'That GitHub installation is connected to another tenant.',
            StatusCode.unauthenticated =>
              'Sign in again, then connect GitHub again.',
            StatusCode.failedPrecondition || StatusCode.unimplemented =>
              "GitHub App onboarding isn't configured on this agent.",
            _ => 'Connecting the GitHub installation failed (${e.codeName}).',
          });
    }
  }

  bool _hasCardFor(String id) => _cards.any((c) => c.tokenRef == ghappRef(id));

  Future<void> _createCard(GithubInstall i) async {
    final id = await showDialog<String>(
      context: context,
      builder: (ctx) => _CardIdDialog(initial: 'github-${i.installationId}'),
    );
    if (id == null) return;
    try {
      final saved = await widget.clients.forges.put(ForgeCard(
        id: id,
        kind: 'github',
        enabled: true,
        tokenRef: ghappRef(i.installationId),
        repoEncoding: 'owner_name',
      ));
      _snack('Forge card ${saved.id} created.');
      await _reload();
    } on GrpcError catch (e) {
      _snack('Create failed: ${e.message ?? e.codeName}');
    }
  }

  Future<void> _setEnabled(ForgeCard c, bool enabled) async {
    try {
      await widget.clients.forges.put(c.deepCopy()..enabled = enabled);
      _snack('${c.id} ${enabled ? "enabled" : "disabled"}.');
      await _reload();
    } on GrpcError catch (e) {
      _snack('Update failed: ${e.message ?? e.codeName}');
    }
  }

  Future<void> _delete(ForgeCard c) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Delete "${c.id}"?'),
        content: const Text(
            'Removes the forge card. Fleet sessions that use it stop working.'),
        actions: [
          TextButton(
            key: const Key('forges.card.delete.cancel'),
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            key: const Key('forges.card.delete.confirm'),
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await widget.clients.forges.delete(ForgeRef(id: c.id));
      _snack('${c.id} deleted.');
      await _reload();
    } on GrpcError catch (e) {
      _snack('Delete failed: ${e.message ?? e.codeName}');
    }
  }

  @override
  Widget build(BuildContext context) {
    final caps = CapabilityScope.of(context);
    final canWrite = caps.can('write', 'forge_registry');
    final canDelete = caps.can('delete', 'forge_registry');
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 16, 16, 8),
          child: Row(
            children: [
              Text('Forges', style: Theme.of(context).textTheme.titleLarge),
              const Spacer(),
              if (canWrite)
                FilledButton.icon(
                  key: const Key('forges.github.connect'),
                  onPressed: _connecting ? null : _connect,
                  icon: const Icon(Icons.link),
                  label: const Text('Connect GitHub'),
                ),
            ],
          ),
        ),
        if (_banner != null) _bannerView(_banner!),
        Expanded(child: _body(canWrite, canDelete)),
      ],
    );
  }

  Widget _bannerView(({_Tone tone, String text}) b) {
    final scheme = Theme.of(context).colorScheme;
    final (bg, icon) = switch (b.tone) {
      _Tone.ok => (scheme.primaryContainer, Icons.check_circle_outline),
      _Tone.info => (scheme.secondaryContainer, Icons.info_outline),
      _Tone.error => (scheme.errorContainer, Icons.error_outline),
    };
    return Material(
      color: bg,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
        child: Row(children: [
          Icon(icon, size: 18),
          const SizedBox(width: 8),
          Expanded(child: Text(b.text, key: const Key('forges.banner'))),
          IconButton(
            key: const Key('forges.banner.dismiss'),
            tooltip: 'Dismiss',
            onPressed: () => setState(() => _banner = null),
            icon: const Icon(Icons.close, size: 18),
          ),
        ]),
      ),
    );
  }

  Widget _body(bool canWrite, bool canDelete) {
    if (_loading) return const Center(child: CircularProgressIndicator());
    if (_serverError != null) {
      return Center(
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          const Icon(Icons.cloud_off, size: 48),
          const SizedBox(height: 12),
          Text('Could not load forges.\n$_serverError', textAlign: TextAlign.center),
          const SizedBox(height: 12),
          FilledButton(
            key: const Key('forges.error.retry'),
            onPressed: _reload,
            child: const Text('Retry'),
          ),
        ]),
      );
    }
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Text('GitHub installations', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 8),
        if (_installs.isEmpty)
          const Text('No GitHub App installations yet.',
              key: Key('forges.install.empty')),
        for (final i in _installs)
          Card(
            key: Key('forges.install.item.${i.installationId}'),
            child: ListTile(
              leading: const Icon(Icons.account_tree_outlined),
              title: Text('Installation ${i.installationId}'),
              subtitle: Text('${i.selectedRepos.length} selected repos'),
              trailing: canWrite
                  ? OutlinedButton(
                      key: Key('forges.install.card.${i.installationId}'),
                      onPressed: _hasCardFor(i.installationId)
                          ? null
                          : () => _createCard(i),
                      child: Text(_hasCardFor(i.installationId)
                          ? 'Card exists'
                          : 'Create forge card'),
                    )
                  : null,
            ),
          ),
        const SizedBox(height: 24),
        Text('Forge cards', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 8),
        if (_cards.isEmpty)
          const Text('No forge cards yet.', key: Key('forges.card.empty')),
        for (final c in _cards)
          Card(
            key: Key('forges.card.item.${c.id}'),
            child: ListTile(
              leading: const Icon(Icons.cloud_outlined),
              title: Text(c.id),
              subtitle: Text(
                  '${c.kind} · ${c.tokenRef}${c.baseUrl.isEmpty ? "" : " · ${c.baseUrl}"}'),
              trailing: Row(mainAxisSize: MainAxisSize.min, children: [
                Switch(
                  key: Key('forges.card.enabled.${c.id}'),
                  value: c.enabled,
                  onChanged: canWrite ? (v) => _setEnabled(c, v) : null,
                ),
                if (canDelete)
                  IconButton(
                    key: Key('forges.card.delete.${c.id}'),
                    tooltip: 'Delete',
                    onPressed: () => _delete(c),
                    icon: const Icon(Icons.delete_outline),
                  ),
              ]),
            ),
          ),
      ],
    );
  }
}

/// Asks for the new card's id, prefilled; refuses one the agent would.
class _CardIdDialog extends StatefulWidget {
  const _CardIdDialog({required this.initial});

  final String initial;

  @override
  State<_CardIdDialog> createState() => _CardIdDialogState();
}

class _CardIdDialogState extends State<_CardIdDialog> {
  late final TextEditingController _id = TextEditingController(text: widget.initial);
  String? _error;

  @override
  void dispose() {
    _id.dispose();
    super.dispose();
  }

  void _submit() {
    final id = _id.text.trim();
    if (!validForgeId(id)) {
      setState(() => _error =
          'Use letters, digits, ".", "_" or "-" (no leading "." or "-", no "..", '
          'at most $maxForgeIdLen).');
      return;
    }
    Navigator.pop(context, id);
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
        title: const Text('Create forge card'),
        content: TextField(
          key: const Key('forges.card.dialog.id'),
          controller: _id,
          autofocus: true,
          decoration: InputDecoration(labelText: 'Card id', errorText: _error),
          onSubmitted: (_) => _submit(),
        ),
        actions: [
          TextButton(
            key: const Key('forges.card.dialog.cancel'),
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel'),
          ),
          FilledButton(
            key: const Key('forges.card.dialog.save'),
            onPressed: _submit,
            child: const Text('Create'),
          ),
        ],
      );
}
