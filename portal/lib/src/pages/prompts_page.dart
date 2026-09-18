import 'package:flutter/material.dart';

import '../clients.dart';
import '../gen/agent/v1/prompt.pb.dart';

/// The Prompts editor: see + CRUD every prompt (system · context.d pre/post-pends ·
/// per-mode compaction lenses) over `PromptService`, plus a per-mode "preview
/// assembled" of the message list the model would see.
class PromptsPage extends StatefulWidget {
  const PromptsPage({super.key, required this.clients});

  final PortalClients clients;

  @override
  State<PromptsPage> createState() => _PromptsPageState();
}

class _PromptsPageState extends State<PromptsPage> {
  /// The closed set of personalities agent-seddon can run *as*, mirroring
  /// `agent_core::ALL_PERSONALITIES` (docs/design/prompts/07-personalities.md).
  /// `''` (the default base) is prepended as the first option in the selector.
  static const _personalities = [
    'agent-seddon',
    'pi',
    'hermes',
    'opencode',
    'codex',
  ];

  List<PromptEntry> _entries = [];
  PromptEntry? _selected;
  final _editor = TextEditingController();
  String? _error;
  bool _loading = true;

  /// The active personality (`''` ⇒ default base), and whether a live switch also
  /// persists it as the new-run default (docs/design/prompts/10-portal-selector.md).
  String _activePersonality = '';
  bool _persistPersonality = false;
  bool _switching = false;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  @override
  void dispose() {
    _editor.dispose();
    super.dispose();
  }

  Future<void> _reload() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final list = await widget.clients.prompts.list(PromptListRequest());
      final active = await widget.clients.prompts
          .getActivePersonality(GetActivePersonalityRequest());
      setState(() {
        _entries = list.entries;
        _activePersonality = active.id;
        _loading = false;
        // Keep the selection if it still exists, else clear.
        _selected = _entries.firstWhere(
          (e) => _selected != null &&
              e.kind == _selected!.kind &&
              e.id == _selected!.id,
          orElse: () => _entries.isNotEmpty ? _entries.first : PromptEntry(),
        );
        _editor.text = _selected?.content ?? '';
      });
    } catch (e) {
      setState(() {
        _loading = false;
        _error = '$e';
      });
    }
  }

  /// The store `System` entry for a personality id, if one has been imported —
  /// its version/provenance decorates the option. `null` for the default and for
  /// personalities resolved purely from files (no store row yet).
  PromptEntry? _personalityEntry(String id) {
    if (id.isEmpty) return null;
    for (final e in _entries) {
      if (e.kind == PromptKind.PROMPT_KIND_SYSTEM && e.id == id) return e;
    }
    return null;
  }

  Future<void> _setActivePersonality(String id) async {
    if (id == _activePersonality) return;
    setState(() => _switching = true);
    try {
      final res = await widget.clients.prompts.setActivePersonality(
          SetActivePersonalityRequest(id: id, persist: _persistPersonality));
      setState(() {
        _activePersonality = res.id;
        _switching = false;
      });
      final label = res.id.isEmpty ? 'default base' : res.id;
      _snack(_persistPersonality
          ? 'Personality "$label" applied live and saved as default.'
          : 'Personality "$label" applied live.');
    } catch (e) {
      setState(() => _switching = false);
      _snack('Switch failed: $e');
    }
  }

  void _select(PromptEntry e) {
    setState(() {
      _selected = e;
      _editor.text = e.content;
    });
  }

  Future<void> _save() async {
    final sel = _selected;
    if (sel == null) return;
    try {
      await widget.clients.prompts.put(PromptEntry(
        kind: sel.kind,
        id: sel.id,
        content: _editor.text,
      ));
      _snack('Saved ${_kindLabel(sel.kind)} ${sel.id}');
      await _reload();
    } catch (e) {
      _snack('Save failed: $e');
    }
  }

  Future<void> _delete() async {
    final sel = _selected;
    if (sel == null) return;
    try {
      final r = await widget.clients.prompts
          .delete(PromptRef(kind: sel.kind, id: sel.id));
      _snack(r.deleted ? 'Removed override' : 'Nothing to remove');
      await _reload();
    } catch (e) {
      _snack('Delete failed: $e');
    }
  }

  Future<void> _preview() async {
    final sel = _selected;
    final mode = (sel != null && sel.kind == PromptKind.PROMPT_KIND_MODE_LENS)
        ? sel.id
        : 'implement';
    try {
      final res = await widget.clients.prompts
          .previewAssembled(PreviewRequest(mode: mode, goal: 'do the task'));
      if (!mounted) return;
      showDialog<void>(
        context: context,
        builder: (_) => AlertDialog(
          title: Text('Assembled context ($mode)'),
          content: SizedBox(
            width: 560,
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                children: [
                  for (final m in res.messages) ...[
                    Text(m.role.toUpperCase(),
                        style: const TextStyle(fontWeight: FontWeight.bold)),
                    Text(m.content),
                    const Divider(),
                  ],
                ],
              ),
            ),
          ),
          actions: [
            TextButton(
                onPressed: () => Navigator.pop(context),
                child: const Text('Close')),
          ],
        ),
      );
    } catch (e) {
      _snack('Preview failed: $e');
    }
  }

  void _snack(String msg) {
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) return const Center(child: CircularProgressIndicator());
    if (_error != null) {
      return _ErrorRetry(message: _error!, onRetry: _reload);
    }
    return Column(
      children: [
        _selectorBar(),
        const Divider(height: 1),
        Expanded(
          child: Row(
            children: [
              SizedBox(width: 280, child: _list()),
              const VerticalDivider(width: 1),
              Expanded(child: _editorPane()),
            ],
          ),
        ),
      ],
    );
  }

  /// The personality selector — pick which named base the loop runs *as*, applied
  /// live (docs/design/prompts/10-portal-selector.md). Discovered from the closed
  /// set; a store-imported personality shows its version/provenance.
  Widget _selectorBar() {
    final theme = Theme.of(context);
    final active = _personalityEntry(_activePersonality);
    final subtitle = active != null
        ? 'v${active.version}${active.sourceRef.isEmpty ? "" : " · ${active.sourceRef}"}'
        : (_activePersonality.isEmpty
            ? 'the default base (from files/config)'
            : 'resolved from files');
    return Material(
      color: theme.colorScheme.tertiaryContainer,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
        child: Row(
          children: [
            Icon(Icons.badge_outlined,
                size: 18, color: theme.colorScheme.onTertiaryContainer),
            const SizedBox(width: 8),
            Text('Personality',
                style: TextStyle(
                    fontWeight: FontWeight.bold,
                    color: theme.colorScheme.onTertiaryContainer)),
            const SizedBox(width: 12),
            DropdownButton<String>(
              value: _activePersonality,
              onChanged: _switching
                  ? null
                  : (v) {
                      if (v != null) _setActivePersonality(v);
                    },
              items: [
                const DropdownMenuItem(value: '', child: Text('default base')),
                for (final p in _personalities)
                  DropdownMenuItem(value: p, child: Text(p)),
              ],
            ),
            const SizedBox(width: 12),
            if (_switching)
              const SizedBox(
                  width: 16,
                  height: 16,
                  child: CircularProgressIndicator(strokeWidth: 2))
            else
              Expanded(
                child: Text(subtitle,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                        color: theme.colorScheme.onTertiaryContainer
                            .withValues(alpha: 0.8))),
              ),
            const SizedBox(width: 8),
            Tooltip(
              message: 'Also write [agent] personality as the new-run default',
              child: Row(children: [
                Checkbox(
                  value: _persistPersonality,
                  onChanged: (v) =>
                      setState(() => _persistPersonality = v ?? false),
                ),
                Text('persist',
                    style:
                        TextStyle(color: theme.colorScheme.onTertiaryContainer)),
              ]),
            ),
          ],
        ),
      ),
    );
  }

  Widget _list() {
    // Group by kind, in a stable order.
    const order = [
      PromptKind.PROMPT_KIND_SYSTEM,
      PromptKind.PROMPT_KIND_PREPEND,
      PromptKind.PROMPT_KIND_APPEND,
      PromptKind.PROMPT_KIND_MODE_LENS,
    ];
    final items = <Widget>[];
    for (final kind in order) {
      final group = _entries.where((e) => e.kind == kind).toList();
      if (group.isEmpty) continue;
      items.add(Padding(
        padding: const EdgeInsets.fromLTRB(16, 16, 16, 4),
        child: Text(_kindLabel(kind).toUpperCase(),
            style: Theme.of(context).textTheme.labelSmall),
      ));
      for (final e in group) {
        final selected = identical(e, _selected) ||
            (_selected != null && e.kind == _selected!.kind && e.id == _selected!.id);
        items.add(ListTile(
          dense: true,
          selected: selected,
          title: Text(e.id.isEmpty ? '(system prompt)' : e.id),
          trailing: e.builtin
              ? const Tooltip(
                  message: 'serving the compiled/config default',
                  child: Icon(Icons.lock_outline, size: 16))
              : null,
          onTap: () => _select(e),
        ));
      }
    }
    return ListView(children: items);
  }

  Widget _editorPane() {
    final sel = _selected;
    if (sel == null) {
      return const Center(child: Text('Select a prompt'));
    }
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(children: [
            Text('${_kindLabel(sel.kind)} · ${sel.id.isEmpty ? "system" : sel.id}',
                style: Theme.of(context).textTheme.titleMedium),
            if (sel.builtin)
              const Padding(
                padding: EdgeInsets.only(left: 8),
                child: Chip(label: Text('default'), visualDensity: VisualDensity.compact),
              ),
          ]),
          const SizedBox(height: 8),
          Expanded(
            child: TextField(
              controller: _editor,
              maxLines: null,
              expands: true,
              textAlignVertical: TextAlignVertical.top,
              decoration: const InputDecoration(
                border: OutlineInputBorder(),
                alignLabelWithHint: true,
              ),
            ),
          ),
          const SizedBox(height: 8),
          Row(children: [
            FilledButton.icon(
                onPressed: _save,
                icon: const Icon(Icons.save),
                label: const Text('Save')),
            const SizedBox(width: 8),
            OutlinedButton.icon(
                onPressed: _delete,
                icon: const Icon(Icons.delete_outline),
                label: const Text('Reset / delete')),
            const Spacer(),
            TextButton.icon(
                onPressed: _preview,
                icon: const Icon(Icons.visibility),
                label: const Text('Preview assembled')),
          ]),
        ],
      ),
    );
  }

  static String _kindLabel(PromptKind k) {
    switch (k) {
      case PromptKind.PROMPT_KIND_SYSTEM:
        return 'System';
      case PromptKind.PROMPT_KIND_PREPEND:
        return 'Prepend';
      case PromptKind.PROMPT_KIND_APPEND:
        return 'Append';
      case PromptKind.PROMPT_KIND_MODE_LENS:
        return 'Mode lens';
      default:
        return 'Prompt';
    }
  }
}

class _ErrorRetry extends StatelessWidget {
  const _ErrorRetry({required this.message, required this.onRetry});
  final String message;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Icon(Icons.cloud_off, size: 40),
          const SizedBox(height: 8),
          Text('Not connected to the gateway.\n$message',
              textAlign: TextAlign.center),
          const SizedBox(height: 8),
          FilledButton(onPressed: onRetry, child: const Text('Retry')),
        ],
      ),
    );
  }
}
