import 'dart:convert';

import 'package:flutter/material.dart';

import '../clients.dart';
import '../gen/agent/v1/config.pb.dart';
import '../graph_json.dart';
import '../widgets/schema_form.dart';

/// The **Settings** tab: a schema-driven editor over the *entire* agent config
/// (`ConfigService`, on the `--serve-all` gateway). Unlike the Router tab, edits
/// here are written to `config/agent.toml` and take effect on the **next agent
/// restart** — a persistent banner surfaces the pending-change / restart-required
/// drift from `Status`.
///
/// Left pane: the ~45 config sections. Right pane: a [SchemaForm] built from the
/// section's schema + current values, with Validate / Save / Revert.
class SettingsPage extends StatefulWidget {
  final PortalClients clients;
  const SettingsPage({super.key, required this.clients});

  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage> {
  Map<String, dynamic> _definitions = {};
  Map<String, dynamic> _sectionRefs = {}; // section -> its property (ref) node
  Map<String, dynamic> _values = {}; // section -> values map
  // Per-section staged edits (full section map as last emitted by the form).
  final Map<String, Map<String, dynamic>> _staged = {};
  String? _selected;
  bool _loading = true;
  String? _serverError;

  // Restart-required drift.
  bool _restartRequired = false;
  int _pendingCount = 0;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  Future<void> _reload() async {
    setState(() {
      _loading = true;
      _serverError = null;
    });
    try {
      final schemaResp = await widget.clients.config.getSchema(GetSchemaRequest());
      final valuesResp = await widget.clients.config.getValues(GetValuesRequest());
      final schema = jsonValueToDart(schemaResp.schema) as Map<String, dynamic>;
      final values = jsonValueToDart(valuesResp.values) as Map<String, dynamic>;
      final props = (schema['properties'] as Map?)?.cast<String, dynamic>() ?? {};
      final defs = (schema['definitions'] as Map?)?.cast<String, dynamic>() ?? {};
      if (!mounted) return;
      setState(() {
        _definitions = defs;
        _sectionRefs = props;
        _values = values;
        _staged.clear();
        _selected ??= (props.keys.toList()..sort()).firstOrNull;
        _loading = false;
      });
      await _refreshStatus();
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _serverError = '$e';
        _loading = false;
      });
    }
  }

  Future<void> _refreshStatus() async {
    try {
      final s = await widget.clients.config.status(ConfigStatusRequest());
      if (!mounted) return;
      setState(() {
        _restartRequired = s.restartRequired;
        _pendingCount = s.pending.length;
      });
    } catch (_) {
      // Non-fatal: leave the last known status.
    }
  }

  void _snack(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  bool _sectionDirty(String section) {
    final staged = _staged[section];
    if (staged == null) return false;
    final orig = (_values[section] as Map?)?.cast<String, dynamic>() ?? {};
    return _edits(orig, staged, '$section.').isNotEmpty;
  }

  /// Diff original vs staged into dotted-path edits. Recurses into objects;
  /// arrays and scalars are atomic leaves (whole-array replacement — matches the
  /// backend, which rejects indexed array paths).
  List<ConfigEdit> _edits(
      Map<String, dynamic> orig, Map<String, dynamic> staged, String prefix) {
    final out = <ConfigEdit>[];
    for (final key in staged.keys) {
      final o = orig[key];
      final e = staged[key];
      if (o is Map<String, dynamic> && e is Map<String, dynamic>) {
        out.addAll(_edits(o, e, '$prefix$key.'));
      } else if (jsonEncode(o) != jsonEncode(e)) {
        out.add(ConfigEdit()
          ..path = '$prefix$key'
          ..value = dartToJsonValue(e));
      }
    }
    return out;
  }

  List<ConfigEdit> _sectionEdits(String section) {
    final staged = _staged[section];
    if (staged == null) return [];
    final orig = (_values[section] as Map?)?.cast<String, dynamic>() ?? {};
    return _edits(orig, staged, '$section.');
  }

  Future<void> _save(String section) async {
    final edits = _sectionEdits(section);
    if (edits.isEmpty) {
      _snack('No changes to save.');
      return;
    }
    try {
      final resp =
          await widget.clients.config.put(PutConfigRequest()..edits.addAll(edits));
      if (resp.issues.isNotEmpty) {
        _showIssues(resp.issues);
        return;
      }
      _snack('Written to agent.toml — takes effect on restart.');
      await _reload();
      setState(() => _selected = section);
    } catch (e) {
      _snack('Save failed: $e');
    }
  }

  Future<void> _validate(String section) async {
    final edits = _sectionEdits(section);
    if (edits.isEmpty) {
      _snack('No changes to validate.');
      return;
    }
    try {
      final resp = await widget.clients.config
          .validate(ValidateConfigRequest()..edits.addAll(edits));
      if (resp.issues.isEmpty) {
        _snack('Valid — ${edits.length} change(s) ready to save.');
      } else {
        _showIssues(resp.issues);
      }
    } catch (e) {
      _snack('Validate failed: $e');
    }
  }

  void _revert(String section) {
    setState(() => _staged.remove(section));
  }

  void _showIssues(List<ConfigIssue> issues) {
    showDialog<void>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('${issues.length} issue(s)'),
        content: SizedBox(
          width: 480,
          child: ListView(
            shrinkWrap: true,
            children: [
              for (final i in issues)
                ListTile(
                  dense: true,
                  leading: const Icon(Icons.error_outline),
                  title: Text(i.path.isEmpty ? '(document)' : i.path),
                  subtitle: Text('${i.code}: ${i.detail}'),
                ),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx),
            child: const Text('Close'),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) return const Center(child: CircularProgressIndicator());
    if (_serverError != null) {
      return _OfflineRetry(message: _serverError!, onRetry: _reload);
    }
    final sections = _sectionRefs.keys.toList()..sort();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _banner(context),
        const Divider(height: 1),
        Expanded(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              SizedBox(
                width: 240,
                child: ListView(
                  children: [
                    for (final s in sections)
                      ListTile(
                        dense: true,
                        selected: _selected == s,
                        title: Text(s),
                        trailing: _sectionDirty(s)
                            ? const Icon(Icons.circle, size: 10, color: Colors.orange)
                            : null,
                        onTap: () => setState(() => _selected = s),
                      ),
                  ],
                ),
              ),
              const VerticalDivider(width: 1),
              Expanded(child: _detail()),
            ],
          ),
        ),
      ],
    );
  }

  Widget _banner(BuildContext context) {
    final theme = Theme.of(context);
    if (_restartRequired || _pendingCount > 0) {
      return Material(
        color: theme.colorScheme.tertiaryContainer,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
          child: Row(
            children: [
              Icon(Icons.restart_alt,
                  size: 18, color: theme.colorScheme.onTertiaryContainer),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  '$_pendingCount change(s) saved to config — restart the agent to apply.',
                  style: TextStyle(color: theme.colorScheme.onTertiaryContainer),
                ),
              ),
            ],
          ),
        ),
      );
    }
    return Material(
      color: theme.colorScheme.surfaceContainerHighest,
      child: const Padding(
        padding: EdgeInsets.symmetric(horizontal: 16, vertical: 8),
        child: Row(
          children: [
            Icon(Icons.settings, size: 18),
            SizedBox(width: 8),
            Expanded(
              child: Text(
                  'Edits are written to config/agent.toml and take effect on the next agent restart.'),
            ),
          ],
        ),
      ),
    );
  }

  Widget _detail() {
    final section = _selected;
    if (section == null) return const Center(child: Text('Select a section'));
    final ref = _sectionRefs[section];
    if (ref is! Map<String, dynamic>) {
      return Center(child: Text('No schema for "$section"'));
    }
    final values =
        (_values[section] as Map?)?.cast<String, dynamic>() ?? <String, dynamic>{};
    final dirty = _sectionDirty(section);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.all(12),
          child: Row(
            children: [
              Text('[$section]',
                  style: Theme.of(context).textTheme.titleLarge),
              const Spacer(),
              TextButton.icon(
                onPressed: dirty ? () => _revert(section) : null,
                icon: const Icon(Icons.undo, size: 18),
                label: const Text('Revert'),
              ),
              const SizedBox(width: 8),
              OutlinedButton.icon(
                onPressed: dirty ? () => _validate(section) : null,
                icon: const Icon(Icons.rule, size: 18),
                label: const Text('Validate'),
              ),
              const SizedBox(width: 8),
              FilledButton.icon(
                onPressed: dirty ? () => _save(section) : null,
                icon: const Icon(Icons.save, size: 18),
                label: const Text('Save'),
              ),
            ],
          ),
        ),
        const Divider(height: 1),
        Expanded(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(16),
            child: SchemaForm(
              // Rebuild the form when switching sections.
              key: ValueKey(section),
              schema: ref,
              definitions: _definitions,
              values: _staged[section] ?? values,
              onChanged: (m) => setState(() => _staged[section] = m),
            ),
          ),
        ),
      ],
    );
  }
}

class _OfflineRetry extends StatelessWidget {
  final String message;
  final VoidCallback onRetry;
  const _OfflineRetry({required this.message, required this.onRetry});

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Icon(Icons.cloud_off, size: 48),
          const SizedBox(height: 12),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 32),
            child: Text('Not connected to the gateway.\n$message',
                textAlign: TextAlign.center),
          ),
          const SizedBox(height: 12),
          FilledButton(onPressed: onRetry, child: const Text('Retry')),
        ],
      ),
    );
  }
}

extension _FirstOrNull<E> on Iterable<E> {
  E? get firstOrNull {
    final it = iterator;
    return it.moveNext() ? it.current : null;
  }
}
