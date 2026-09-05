import 'package:flutter/material.dart';

import '../clients.dart';
import '../gen/agent/v1/common.pb.dart';
import '../gen/agent/v1/upstream.pb.dart';

/// The **Router** tab: a live CRUD editor over the model-router / provider
/// registry (`ProviderRegistryService`, `:50084`, hosted by the `--serve-all`
/// gateway). Unlike the Settings tab (which writes `config/agent.toml` and needs
/// a restart), everything here **applies immediately** — the `task-router`
/// re-reads the registry on its refresh interval.
///
/// Three views: the upstream **model cards** (add/edit/enable/delete), a
/// read-only **health** table, and a **route tester** that asks the router which
/// upstream a hypothetical request would pick and why.
class RouterPage extends StatefulWidget {
  final PortalClients clients;
  const RouterPage({super.key, required this.clients});

  @override
  State<RouterPage> createState() => _RouterPageState();
}

enum _View { upstreams, health, route }

class _RouterPageState extends State<RouterPage> {
  _View _view = _View.upstreams;

  // Upstreams view state.
  List<Upstream> _upstreams = [];
  Upstream? _selected;
  bool _loading = true;
  String? _serverError;

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
      final list = await widget.clients.providers.list(UpstreamListRequest());
      if (!mounted) return;
      setState(() {
        _upstreams = list.upstreams.toList();
        // Re-select by id so an edit/enable keeps the detail pane in place.
        final selId = _selected?.id;
        _selected = selId == null
            ? null
            : _upstreams.where((u) => u.id == selId).firstOrNull;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _serverError = '$e';
        _loading = false;
      });
    }
  }

  void _snack(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  Future<void> _toggleEnable(Upstream u, bool enabled) async {
    try {
      await widget.clients.providers.enable(
        UpstreamEnableRequest()
          ..id = u.id
          ..enabled = enabled,
      );
      _snack('${u.id} ${enabled ? "enabled" : "disabled"} — applied live.');
      await _reload();
    } catch (e) {
      _snack('Enable failed: $e');
    }
  }

  Future<void> _delete(Upstream u) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Delete "${u.id}"?'),
        content: const Text('Removes the upstream from the live registry.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await widget.clients.providers.delete(UpstreamRef()..id = u.id);
      if (_selected?.id == u.id) _selected = null;
      _snack('${u.id} deleted — applied live.');
      await _reload();
    } catch (e) {
      _snack('Delete failed: $e');
    }
  }

  Future<void> _save(Upstream draft) async {
    try {
      final saved = await widget.clients.providers.put(draft);
      _snack('${saved.id} saved — applied live.');
      setState(() => _selected = saved);
      await _reload();
    } catch (e) {
      _snack('Save failed: $e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // Live-apply banner — the semantic opposite of the Settings tab.
        Material(
          color: theme.colorScheme.tertiaryContainer,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
            child: Row(
              children: [
                Icon(Icons.bolt,
                    size: 18, color: theme.colorScheme.onTertiaryContainer),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    'Changes apply live — no restart needed.',
                    style: TextStyle(
                        color: theme.colorScheme.onTertiaryContainer),
                  ),
                ),
                SegmentedButton<_View>(
                  segments: const [
                    ButtonSegment(
                        value: _View.upstreams,
                        icon: Icon(Icons.dns_outlined),
                        label: Text('Upstreams')),
                    ButtonSegment(
                        value: _View.health,
                        icon: Icon(Icons.favorite_outline),
                        label: Text('Health')),
                    ButtonSegment(
                        value: _View.route,
                        icon: Icon(Icons.alt_route),
                        label: Text('Route tester')),
                  ],
                  selected: {_view},
                  onSelectionChanged: (s) => setState(() => _view = s.first),
                ),
              ],
            ),
          ),
        ),
        const Divider(height: 1),
        Expanded(child: _body()),
      ],
    );
  }

  Widget _body() {
    switch (_view) {
      case _View.upstreams:
        return _upstreamsView();
      case _View.health:
        return _HealthView(clients: widget.clients);
      case _View.route:
        return _RouteTester(clients: widget.clients);
    }
  }

  Widget _upstreamsView() {
    if (_loading) {
      return const Center(child: CircularProgressIndicator());
    }
    if (_serverError != null) {
      return _OfflineRetry(message: _serverError!, onRetry: _reload);
    }
    return Row(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SizedBox(
          width: 300,
          child: Column(
            children: [
              Padding(
                padding: const EdgeInsets.all(8),
                child: Row(
                  children: [
                    const Text('Upstreams',
                        style: TextStyle(fontWeight: FontWeight.bold)),
                    const Spacer(),
                    FilledButton.tonalIcon(
                      onPressed: () => setState(() => _selected = Upstream()),
                      icon: const Icon(Icons.add, size: 18),
                      label: const Text('Add'),
                    ),
                  ],
                ),
              ),
              const Divider(height: 1),
              Expanded(
                child: _upstreams.isEmpty
                    ? const Center(
                        child: Padding(
                          padding: EdgeInsets.all(16),
                          child: Text('No upstreams yet. Add one.',
                              textAlign: TextAlign.center),
                        ),
                      )
                    : ListView.builder(
                        itemCount: _upstreams.length,
                        itemBuilder: (ctx, i) {
                          final u = _upstreams[i];
                          return ListTile(
                            selected: _selected?.id == u.id,
                            title: Text(u.id,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis),
                            subtitle: Text(
                              '${u.model.isEmpty ? "—" : u.model} · ${u.kind}',
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                            ),
                            leading: _tierChip(u.tier),
                            trailing: Row(
                              mainAxisSize: MainAxisSize.min,
                              children: [
                                Switch(
                                  value: u.enabled,
                                  onChanged: (v) => _toggleEnable(u, v),
                                ),
                                IconButton(
                                  icon: const Icon(Icons.delete_outline),
                                  tooltip: 'Delete',
                                  onPressed: () => _delete(u),
                                ),
                              ],
                            ),
                            onTap: () => setState(() => _selected = u),
                          );
                        },
                      ),
              ),
            ],
          ),
        ),
        const VerticalDivider(width: 1),
        Expanded(
          child: _selected == null
              ? const Center(child: Text('Select or add an upstream'))
              : _UpstreamEditor(
                  // Key by id so switching selection rebuilds the form fresh.
                  key: ValueKey(_selected!.id),
                  initial: _selected!,
                  onSave: _save,
                ),
        ),
      ],
    );
  }

  Widget _tierChip(PoolTier tier) {
    final label = _enumLabel(tier.name, 'POOL_TIER_');
    return Chip(
      label: Text(label, style: const TextStyle(fontSize: 11)),
      visualDensity: VisualDensity.compact,
      materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
    );
  }
}

/// A typed editor over one [Upstream] card. All fields are concrete proto
/// fields, so this is hand-built (dropdowns for the `PoolTier` enum, switches
/// for the bools). `apiKeyRef` is a *reference* (`env:NAME` / `file:/path`),
/// never a secret — shown as plain text with a helper.
class _UpstreamEditor extends StatefulWidget {
  final Upstream initial;
  final Future<void> Function(Upstream) onSave;
  const _UpstreamEditor({super.key, required this.initial, required this.onSave});

  @override
  State<_UpstreamEditor> createState() => _UpstreamEditorState();
}

class _UpstreamEditorState extends State<_UpstreamEditor> {
  late final Map<String, TextEditingController> _text;
  late bool _enabled;
  late bool _insecureTls;
  late bool _supportsTools;
  late bool _supportsVision;
  late bool _supportsResponseFormat;
  late PoolTier _tier;

  @override
  void initState() {
    super.initState();
    final u = widget.initial;
    _text = {
      'id': TextEditingController(text: u.id),
      'kind': TextEditingController(text: u.kind),
      'baseUrl': TextEditingController(text: u.baseUrl),
      'model': TextEditingController(text: u.model),
      'apiKeyRef': TextEditingController(text: u.apiKeyRef),
      'version': TextEditingController(text: u.version),
      'tags': TextEditingController(text: u.tags.join(', ')),
      'maxRetries': TextEditingController(text: u.maxRetries.toString()),
      'contextWindow': TextEditingController(text: u.contextWindow.toString()),
      'maxOutputTokens':
          TextEditingController(text: u.maxOutputTokens.toString()),
      'maxConcurrency':
          TextEditingController(text: u.maxConcurrency.toString()),
      'weight': TextEditingController(text: u.weight.toString()),
      'inputCost': TextEditingController(text: u.inputCost.toString()),
      'outputCost': TextEditingController(text: u.outputCost.toString()),
    };
    _enabled = u.enabled;
    _insecureTls = u.insecureTls;
    _supportsTools = u.supportsTools;
    _supportsVision = u.supportsVision;
    _supportsResponseFormat = u.supportsResponseFormat;
    _tier = u.tier;
  }

  @override
  void dispose() {
    for (final c in _text.values) {
      c.dispose();
    }
    super.dispose();
  }

  int _intOf(String key) => int.tryParse(_text[key]!.text.trim()) ?? 0;
  double _doubleOf(String key) =>
      double.tryParse(_text[key]!.text.trim()) ?? 0.0;

  Upstream _build() {
    final u = Upstream()
      ..id = _text['id']!.text.trim()
      ..kind = _text['kind']!.text.trim()
      ..enabled = _enabled
      ..baseUrl = _text['baseUrl']!.text.trim()
      ..model = _text['model']!.text.trim()
      ..apiKeyRef = _text['apiKeyRef']!.text.trim()
      ..insecureTls = _insecureTls
      ..version = _text['version']!.text.trim()
      ..maxRetries = _intOf('maxRetries')
      ..contextWindow = _intOf('contextWindow')
      ..maxOutputTokens = _intOf('maxOutputTokens')
      ..supportsTools = _supportsTools
      ..supportsVision = _supportsVision
      ..supportsResponseFormat = _supportsResponseFormat
      ..inputCost = _doubleOf('inputCost')
      ..outputCost = _doubleOf('outputCost')
      ..tier = _tier
      ..weight = _doubleOf('weight')
      ..maxConcurrency = _intOf('maxConcurrency');
    final tags = _text['tags']!
        .text
        .split(',')
        .map((t) => t.trim())
        .where((t) => t.isNotEmpty);
    u.tags.addAll(tags);
    return u;
  }

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Row(
          children: [
            Text(
              widget.initial.id.isEmpty ? 'New upstream' : widget.initial.id,
              style: Theme.of(context).textTheme.titleLarge,
            ),
            const Spacer(),
            FilledButton.icon(
              onPressed: () => widget.onSave(_build()),
              icon: const Icon(Icons.save, size: 18),
              label: const Text('Save'),
            ),
          ],
        ),
        const SizedBox(height: 16),
        _field('id', 'id', helper: 'Unique id for this upstream'),
        _field('kind', 'kind',
            helper: 'Provider kind, e.g. openai-compat | anthropic'),
        _field('model', 'model'),
        _field('baseUrl', 'base_url'),
        _field('apiKeyRef', 'api_key_ref',
            helper:
                'A reference, e.g. env:OPENAI_API_KEY or file:/path — NOT the key itself'),
        _field('version', 'version'),
        _field('tags', 'tags', helper: 'Comma-separated capability tags'),
        const SizedBox(height: 8),
        DropdownButtonFormField<PoolTier>(
          initialValue: _tier,
          decoration: const InputDecoration(
              labelText: 'tier', border: OutlineInputBorder()),
          items: PoolTier.values
              .map((t) => DropdownMenuItem(
                  value: t, child: Text(_enumLabel(t.name, 'POOL_TIER_'))))
              .toList(),
          onChanged: (v) => setState(() => _tier = v ?? _tier),
        ),
        const SizedBox(height: 8),
        _numField('maxRetries', 'max_retries'),
        _numField('contextWindow', 'context_window'),
        _numField('maxOutputTokens', 'max_output_tokens'),
        _numField('maxConcurrency', 'max_concurrency'),
        _numField('weight', 'weight'),
        _numField('inputCost', 'input_cost'),
        _numField('outputCost', 'output_cost'),
        const SizedBox(height: 8),
        SwitchListTile(
          title: const Text('enabled'),
          value: _enabled,
          onChanged: (v) => setState(() => _enabled = v),
        ),
        SwitchListTile(
          title: const Text('insecure_tls'),
          value: _insecureTls,
          onChanged: (v) => setState(() => _insecureTls = v),
        ),
        SwitchListTile(
          title: const Text('supports_tools'),
          value: _supportsTools,
          onChanged: (v) => setState(() => _supportsTools = v),
        ),
        SwitchListTile(
          title: const Text('supports_vision'),
          value: _supportsVision,
          onChanged: (v) => setState(() => _supportsVision = v),
        ),
        SwitchListTile(
          title: const Text('supports_response_format'),
          value: _supportsResponseFormat,
          onChanged: (v) => setState(() => _supportsResponseFormat = v),
        ),
      ],
    );
  }

  Widget _field(String key, String label, {String? helper}) => Padding(
        padding: const EdgeInsets.only(bottom: 12),
        child: TextField(
          controller: _text[key],
          decoration: InputDecoration(
            labelText: label,
            helperText: helper,
            helperMaxLines: 2,
            border: const OutlineInputBorder(),
          ),
        ),
      );

  Widget _numField(String key, String label) => Padding(
        padding: const EdgeInsets.only(bottom: 12),
        child: TextField(
          controller: _text[key],
          keyboardType: const TextInputType.numberWithOptions(decimal: true),
          decoration: InputDecoration(
            labelText: label,
            border: const OutlineInputBorder(),
          ),
        ),
      );
}

/// Read-only live health table.
class _HealthView extends StatefulWidget {
  final PortalClients clients;
  const _HealthView({required this.clients});

  @override
  State<_HealthView> createState() => _HealthViewState();
}

class _HealthViewState extends State<_HealthView> {
  List<UpstreamHealth> _entries = [];
  bool _loading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final r = await widget.clients.providers.health(UpstreamHealthRequest());
      if (!mounted) return;
      setState(() {
        _entries = r.entries.toList();
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) return const Center(child: CircularProgressIndicator());
    if (_error != null) {
      return _OfflineRetry(message: _error!, onRetry: _refresh);
    }
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.all(8),
          child: Row(
            children: [
              const Text('Live health',
                  style: TextStyle(fontWeight: FontWeight.bold)),
              const Spacer(),
              OutlinedButton.icon(
                onPressed: _refresh,
                icon: const Icon(Icons.refresh, size: 18),
                label: const Text('Refresh'),
              ),
            ],
          ),
        ),
        const Divider(height: 1),
        Expanded(
          child: _entries.isEmpty
              ? const Center(child: Text('No health entries.'))
              : SingleChildScrollView(
                  scrollDirection: Axis.horizontal,
                  child: SingleChildScrollView(
                    child: DataTable(
                      columns: const [
                        DataColumn(label: Text('id')),
                        DataColumn(label: Text('state')),
                        DataColumn(label: Text('in-flight'), numeric: true),
                        DataColumn(label: Text('latency (ms)'), numeric: true),
                        DataColumn(label: Text('saturated')),
                        DataColumn(label: Text('failures'), numeric: true),
                      ],
                      rows: _entries
                          .map((h) => DataRow(cells: [
                                DataCell(Text(h.id)),
                                DataCell(Text(
                                    _enumLabel(h.state.name, 'POOL_MEMBER_STATE_'))),
                                DataCell(Text('${h.inFlight}')),
                                DataCell(Text('${h.latencyMsEwma}')),
                                DataCell(Icon(h.saturated
                                    ? Icons.warning_amber
                                    : Icons.check_circle_outline)),
                                DataCell(Text('${h.consecutiveFailures}')),
                              ]))
                          .toList(),
                    ),
                  ),
                ),
        ),
      ],
    );
  }
}

/// Ask the router which upstream a hypothetical request would pick, and why.
class _RouteTester extends StatefulWidget {
  final PortalClients clients;
  const _RouteTester({required this.clients});

  @override
  State<_RouteTester> createState() => _RouteTesterState();
}

class _RouteTesterState extends State<_RouteTester> {
  TaskMode _taskMode = TaskMode.TASK_MODE_UNSPECIFIED;
  RouteRole _role = RouteRole.ROUTE_ROLE_UNSPECIFIED;
  PoolTier _tier = PoolTier.POOL_TIER_UNSPECIFIED;
  final _minContext = TextEditingController(text: '0');
  final _maxCost = TextEditingController(text: '0');
  final _override = TextEditingController();
  RouteDecision? _decision;
  String? _error;

  @override
  void dispose() {
    _minContext.dispose();
    _maxCost.dispose();
    _override.dispose();
    super.dispose();
  }

  Future<void> _run() async {
    setState(() {
      _error = null;
      _decision = null;
    });
    try {
      final hint = RouteHint()
        ..taskMode = _taskMode
        ..role = _role
        ..tier = _tier
        ..minContext = int.tryParse(_minContext.text.trim()) ?? 0
        ..maxCost = double.tryParse(_maxCost.text.trim()) ?? 0.0
        ..overrideUpstream = _override.text.trim();
      final d = await widget.clients.providers.route(RouteRequest()..hint = hint);
      if (!mounted) return;
      setState(() => _decision = d);
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = '$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Text('Route tester', style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 8),
        const Text('Introspects the router — no request is dispatched.'),
        const SizedBox(height: 16),
        DropdownButtonFormField<TaskMode>(
          initialValue: _taskMode,
          decoration: const InputDecoration(
              labelText: 'task_mode', border: OutlineInputBorder()),
          items: TaskMode.values
              .map((v) => DropdownMenuItem(
                  value: v, child: Text(_enumLabel(v.name, 'TASK_MODE_'))))
              .toList(),
          onChanged: (v) => setState(() => _taskMode = v ?? _taskMode),
        ),
        const SizedBox(height: 12),
        DropdownButtonFormField<RouteRole>(
          initialValue: _role,
          decoration: const InputDecoration(
              labelText: 'role', border: OutlineInputBorder()),
          items: RouteRole.values
              .map((v) => DropdownMenuItem(
                  value: v, child: Text(_enumLabel(v.name, 'ROUTE_ROLE_'))))
              .toList(),
          onChanged: (v) => setState(() => _role = v ?? _role),
        ),
        const SizedBox(height: 12),
        DropdownButtonFormField<PoolTier>(
          initialValue: _tier,
          decoration: const InputDecoration(
              labelText: 'tier', border: OutlineInputBorder()),
          items: PoolTier.values
              .map((v) => DropdownMenuItem(
                  value: v, child: Text(_enumLabel(v.name, 'POOL_TIER_'))))
              .toList(),
          onChanged: (v) => setState(() => _tier = v ?? _tier),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: _minContext,
          keyboardType: TextInputType.number,
          decoration: const InputDecoration(
              labelText: 'min_context', border: OutlineInputBorder()),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: _maxCost,
          keyboardType: const TextInputType.numberWithOptions(decimal: true),
          decoration: const InputDecoration(
              labelText: 'max_cost', border: OutlineInputBorder()),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: _override,
          decoration: const InputDecoration(
              labelText: 'override_upstream',
              helperText: 'Force a specific upstream id (optional)',
              border: OutlineInputBorder()),
        ),
        const SizedBox(height: 16),
        FilledButton.icon(
          onPressed: _run,
          icon: const Icon(Icons.play_arrow),
          label: const Text('Route'),
        ),
        const SizedBox(height: 16),
        if (_error != null)
          Card(
            color: Theme.of(context).colorScheme.errorContainer,
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Text(_error!),
            ),
          ),
        if (_decision != null) _decisionCard(_decision!),
      ],
    );
  }

  Widget _decisionCard(RouteDecision d) => Card(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  const Icon(Icons.check_circle, color: Colors.green),
                  const SizedBox(width: 8),
                  Text('chosen: ${d.chosen.isEmpty ? "(none)" : d.chosen}',
                      style: const TextStyle(fontWeight: FontWeight.bold)),
                ],
              ),
              const SizedBox(height: 8),
              if (d.order.isNotEmpty) Text('fallback order: ${d.order.join(" → ")}'),
              if (d.rule.isNotEmpty) Text('matched rule: ${d.rule}'),
              if (d.why.isNotEmpty) ...[
                const SizedBox(height: 8),
                Text(d.why),
              ],
            ],
          ),
        ),
      );
}

/// Whole-view offline state with a retry, mirroring the other pages' pattern.
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

/// Strip a SCREAMING_SNAKE enum prefix and lower-case the remainder for display,
/// e.g. `POOL_TIER_HEAVY` + `POOL_TIER_` → `heavy`.
String _enumLabel(String name, String prefix) {
  var s = name;
  if (s.startsWith(prefix)) s = s.substring(prefix.length);
  return s.toLowerCase();
}

extension _FirstOrNull<E> on Iterable<E> {
  E? get firstOrNull {
    final it = iterator;
    return it.moveNext() ? it.current : null;
  }
}
