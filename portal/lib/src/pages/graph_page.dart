import 'dart:convert';

import 'package:flutter/material.dart';

import '../clients.dart';
import '../gen/agent/v1/graph.pb.dart';
import '../graph_json.dart';
import '../graph_library.dart';
import '../io/graph_platform.dart' as platform;

/// The reserved run-loop anchor endpoints an edge can attach to.
const _anchors = ['anchor.response', 'anchor.delivery', 'anchor.compaction'];

/// The Cognition Graph view: a browser-local **library** of graphs the user can
/// switch between, edit (nodes / edges / params, rendered from
/// `GraphService.DescribeNodeTypes`), validate, import/export as JSON, and push
/// to the running agent with **Set active** (`GraphService.Put`).
class GraphPage extends StatefulWidget {
  const GraphPage({super.key, required this.clients});

  final PortalClients clients;

  @override
  State<GraphPage> createState() => _GraphPageState();
}

class _GraphPageState extends State<GraphPage> {
  GraphLibrary _library = GraphLibrary([]);
  GraphLibraryEntry? _selected;

  /// node type → its schema (palette + param-form source of truth).
  final Map<String, NodeTypeSchema> _nodeTypes = {};

  /// Canonical JSON of the server's currently-active graph (for the badge).
  String? _activeJson;

  bool _loading = true;
  String? _serverError;

  @override
  void initState() {
    super.initState();
    _init();
  }

  Future<void> _init() async {
    _library = GraphLibrary.load();
    await _refreshServer(seedIfEmpty: true);
    setState(() {
      _selected = _library.entries.isNotEmpty ? _library.entries.first : null;
      _loading = false;
    });
  }

  /// Fetch node-type schemas (the reachability probe) and, best-effort, the
  /// active graph. A gateway that is down leaves the local library fully
  /// editable, just without live validate / set-active until Retry.
  Future<void> _refreshServer({bool seedIfEmpty = false}) async {
    // Node types are the "is the gateway reachable" probe — the palette + param
    // forms need them regardless of whether any graph is active yet.
    try {
      final types = await widget.clients.graph
          .describeNodeTypes(DescribeNodeTypesRequest());
      _nodeTypes
        ..clear()
        ..addEntries(types.nodeTypes.map((t) => MapEntry(t.type, t)));
      _serverError = null;
    } catch (e) {
      _serverError = '$e';
      return;
    }
    // The active graph is best effort: `Get` returns FAILED_PRECONDITION when no
    // document is stored yet (the default `.agent/graph.textproto` is absent) —
    // that means "nothing active", not an offline gateway. Leave the badge off.
    try {
      final active = await widget.clients.graph.get(GetGraphRequest());
      _activeJson = jsonEncode(graphToJson(active.graph));
      if (seedIfEmpty && _library.entries.isEmpty) {
        _library.entries
            .add(GraphLibraryEntry('active (server)', active.graph));
        _library.save();
      }
    } catch (_) {
      _activeJson = null;
    }
  }

  void _snack(String msg) {
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
    }
  }

  bool _isActive(GraphLibraryEntry e) =>
      _activeJson != null && jsonEncode(graphToJson(e.graph)) == _activeJson;

  // ---- library actions -----------------------------------------------------

  Future<void> _newGraph() async {
    final name = await _promptName('New graph', suggestion: 'graph');
    if (name == null) return;
    final entry = GraphLibraryEntry(
        _library.uniqueName(name), CognitionGraph(version: 1));
    setState(() {
      _library.entries.add(entry);
      _selected = entry;
    });
    _library.save();
  }

  Future<void> _duplicate(GraphLibraryEntry e) async {
    final copy = GraphLibraryEntry(
        _library.uniqueName('${e.name} copy'), e.graph.deepCopy());
    setState(() {
      _library.entries.add(copy);
      _selected = copy;
    });
    _library.save();
  }

  Future<void> _rename(GraphLibraryEntry e) async {
    final name = await _promptName('Rename graph', suggestion: e.name);
    if (name == null || name == e.name) return;
    setState(() => e.name = _library.uniqueName(name));
    _library.save();
  }

  void _delete(GraphLibraryEntry e) {
    setState(() {
      _library.entries.remove(e);
      if (_selected == e) {
        _selected = _library.entries.isNotEmpty ? _library.entries.first : null;
      }
    });
    _library.save();
  }

  Future<void> _import() async {
    try {
      final content = await platform.pickJsonFile();
      if (content == null) return;
      final decoded = jsonDecode(content);
      if (decoded is! Map) {
        _snack('Not a graph document (expected a JSON object)');
        return;
      }
      final graph = graphFromJson(decoded.cast<String, dynamic>());
      final entry = GraphLibraryEntry(_library.uniqueName('imported'), graph);
      setState(() {
        _library.entries.add(entry);
        _selected = entry;
      });
      _library.save();
      _snack('Imported "${entry.name}" — Validate before Set active');
    } catch (e) {
      _snack('Import failed: $e');
    }
  }

  Future<void> _export(GraphLibraryEntry e) async {
    final pretty =
        const JsonEncoder.withIndent('  ').convert(graphToJson(e.graph));
    final safe = e.name.replaceAll(RegExp(r'[^A-Za-z0-9._-]+'), '_');
    await platform.downloadJson('$safe.json', pretty);
  }

  // ---- server actions -------------------------------------------------------

  Future<void> _validate(GraphLibraryEntry e) async {
    try {
      final res = await widget.clients.graph
          .validate(ValidateGraphRequest(graph: e.graph));
      if (!mounted) return;
      _showIssues(res.issues);
    } catch (err) {
      _snack('Validate failed: $err');
    }
  }

  Future<void> _setActive(GraphLibraryEntry e) async {
    try {
      await widget.clients.graph.put(PutGraphRequest(graph: e.graph));
      await _refreshServer();
      setState(() {});
      _snack('"${e.name}" is now the active graph');
    } catch (err) {
      // Put rejects an invalid document wholesale with INVALID_ARGUMENT + the
      // first typed issues in the message — surface it.
      _snack('Set active rejected: $err');
    }
  }

  void _showIssues(List<GraphIssue> issues) {
    showDialog<void>(
      context: context,
      builder: (_) => AlertDialog(
        title: Text(
            issues.isEmpty ? 'Valid — no issues' : '${issues.length} issue(s)'),
        content: issues.isEmpty
            ? const Text(
                'The document passes validation and can be set active.')
            : SizedBox(
                width: 520,
                child: SingleChildScrollView(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      for (final i in issues)
                        ListTile(
                          dense: true,
                          leading: const Icon(Icons.error_outline,
                              color: Colors.red),
                          title: Text(i.code),
                          subtitle: Text([
                            if (i.node.isNotEmpty) 'node: ${i.node}',
                            if (i.detail.isNotEmpty) i.detail,
                          ].join('\n')),
                        ),
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
  }

  // ---- node / edge editing --------------------------------------------------

  Future<void> _addNode(GraphLibraryEntry e) async {
    if (_nodeTypes.isEmpty) {
      _snack('No node types (gateway unavailable) — Retry first');
      return;
    }
    final result = await showDialog<_NewNode>(
      context: context,
      builder: (_) => _AddNodeDialog(
        types: _nodeTypes.values.toList()
          ..sort((a, b) => a.type.compareTo(b.type)),
        existingIds: e.graph.nodes.keys.toSet(),
      ),
    );
    if (result == null) return;
    final schema = _nodeTypes[result.type];
    setState(() {
      e.graph.nodes[result.id] = GraphNode(
        type: result.type,
        typeVersion: schema?.typeVersion ?? 1,
      );
    });
    _library.save();
  }

  void _removeNode(GraphLibraryEntry e, String id) {
    setState(() {
      e.graph.nodes.remove(id);
      // Drop edges that dangle onto the removed node, for a clean document.
      e.graph.edges.removeWhere((edge) => edge.from == id || edge.to == id);
    });
    _library.save();
  }

  Future<void> _addEdge(GraphLibraryEntry e) async {
    final result = await showDialog<GraphEdge>(
      context: context,
      builder: (_) => _AddEdgeDialog(nodeIds: e.graph.nodes.keys.toList()),
    );
    if (result == null) return;
    setState(() => e.graph.edges.add(result));
    _library.save();
  }

  void _removeEdge(GraphLibraryEntry e, int index) {
    setState(() => e.graph.edges.removeAt(index));
    _library.save();
  }

  void _onParamsChanged(GraphLibraryEntry e, String nodeId, dynamic params) {
    final node = e.graph.nodes[nodeId];
    if (node == null) return;
    node.params = dartToJsonValue(params);
    _library.save();
  }

  // ---- build ---------------------------------------------------------------

  @override
  Widget build(BuildContext context) {
    if (_loading) return const Center(child: CircularProgressIndicator());
    return Row(
      children: [
        SizedBox(width: 280, child: _libraryPane()),
        const VerticalDivider(width: 1),
        Expanded(child: _editorPane()),
      ],
    );
  }

  Widget _libraryPane() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 16, 8, 8),
          child: Row(
            children: [
              Text('Library', style: Theme.of(context).textTheme.titleMedium),
              const Spacer(),
              IconButton(
                tooltip: 'New graph',
                icon: const Icon(Icons.add),
                onPressed: _newGraph,
              ),
              IconButton(
                tooltip: 'Import JSON',
                icon: const Icon(Icons.upload_file),
                onPressed: _import,
              ),
            ],
          ),
        ),
        if (_serverError != null)
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12),
            child: _ServerBanner(
              message: _serverError!,
              onRetry: () async {
                await _refreshServer();
                setState(() {});
              },
            ),
          ),
        Expanded(
          child: _library.entries.isEmpty
              ? const Center(
                  child: Text('No graphs yet.\nNew or Import.',
                      textAlign: TextAlign.center))
              : ListView(
                  children: [
                    for (final e in _library.entries)
                      ListTile(
                        dense: true,
                        selected: identical(e, _selected),
                        title: Text(e.name),
                        subtitle: Text(
                            '${e.graph.nodes.length} nodes · ${e.graph.edges.length} edges'),
                        trailing: _isActive(e)
                            ? const Tooltip(
                                message: 'the running agent\'s active graph',
                                child: Icon(Icons.check_circle,
                                    size: 18, color: Colors.green))
                            : null,
                        onTap: () => setState(() => _selected = e),
                      ),
                  ],
                ),
        ),
      ],
    );
  }

  Widget _editorPane() {
    final e = _selected;
    if (e == null) {
      return const Center(child: Text('Select or create a graph'));
    }
    final active = _isActive(e);
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Flexible(
                child: Text(e.name,
                    style: Theme.of(context).textTheme.titleLarge,
                    overflow: TextOverflow.ellipsis),
              ),
              if (active)
                const Padding(
                  padding: EdgeInsets.only(left: 8),
                  child: Chip(
                    label: Text('active'),
                    visualDensity: VisualDensity.compact,
                  ),
                ),
              const Spacer(),
              IconButton(
                  tooltip: 'Rename',
                  icon: const Icon(Icons.drive_file_rename_outline),
                  onPressed: () => _rename(e)),
              IconButton(
                  tooltip: 'Duplicate',
                  icon: const Icon(Icons.copy_all_outlined),
                  onPressed: () => _duplicate(e)),
              IconButton(
                  tooltip: 'Export JSON',
                  icon: const Icon(Icons.download),
                  onPressed: () => _export(e)),
              IconButton(
                  tooltip: 'Delete from library',
                  icon: const Icon(Icons.delete_outline),
                  onPressed: () => _delete(e)),
            ],
          ),
          const SizedBox(height: 8),
          Row(
            children: [
              OutlinedButton.icon(
                  onPressed: () => _validate(e),
                  icon: const Icon(Icons.fact_check_outlined),
                  label: const Text('Validate')),
              const SizedBox(width: 8),
              FilledButton.icon(
                  onPressed: () => _setActive(e),
                  icon: const Icon(Icons.play_circle_outline),
                  label: const Text('Set active')),
            ],
          ),
          const SizedBox(height: 12),
          Expanded(
            child: ListView(
              children: [
                _PreviewCard(graph: e.graph),
                const SizedBox(height: 16),
                _sectionHeader('Nodes', onAdd: () => _addNode(e)),
                if (e.graph.nodes.isEmpty)
                  const Padding(
                    padding: EdgeInsets.all(8),
                    child: Text('No nodes. Add one from the palette.'),
                  ),
                for (final id in (e.graph.nodes.keys.toList()..sort()))
                  _NodeTile(
                    id: id,
                    node: e.graph.nodes[id]!,
                    schema: _nodeTypes[e.graph.nodes[id]!.type],
                    onRemove: () => _removeNode(e, id),
                    onParamsChanged: (p) => _onParamsChanged(e, id, p),
                  ),
                const SizedBox(height: 16),
                _sectionHeader('Edges', onAdd: () => _addEdge(e)),
                if (e.graph.edges.isEmpty)
                  const Padding(
                    padding: EdgeInsets.all(8),
                    child: Text('No edges.'),
                  ),
                for (var i = 0; i < e.graph.edges.length; i++) _edgeTile(e, i),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _sectionHeader(String title, {required VoidCallback onAdd}) {
    return Row(
      children: [
        Text(title, style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(width: 8),
        IconButton(
          icon: const Icon(Icons.add_circle_outline, size: 20),
          onPressed: onAdd,
          tooltip: 'Add $title',
        ),
      ],
    );
  }

  Widget _edgeTile(GraphLibraryEntry e, int i) {
    final edge = e.graph.edges[i];
    return ListTile(
      dense: true,
      leading: _EdgeKindChip(kind: edge.kind),
      title: Text('${edge.from}  →  ${edge.to}'),
      trailing: IconButton(
        icon: const Icon(Icons.remove_circle_outline, size: 20),
        onPressed: () => _removeEdge(e, i),
      ),
    );
  }

  Future<String?> _promptName(String title, {required String suggestion}) {
    final controller = TextEditingController(text: suggestion);
    return showDialog<String>(
      context: context,
      builder: (_) => AlertDialog(
        title: Text(title),
        content: TextField(
          controller: controller,
          autofocus: true,
          decoration: const InputDecoration(labelText: 'Name'),
          onSubmitted: (v) => Navigator.pop(context, v.trim()),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(context),
              child: const Text('Cancel')),
          FilledButton(
              onPressed: () => Navigator.pop(context, controller.text.trim()),
              child: const Text('OK')),
        ],
      ),
    ).then((v) => (v == null || v.isEmpty) ? null : v);
  }
}

// ---- widgets ---------------------------------------------------------------

class _ServerBanner extends StatelessWidget {
  const _ServerBanner({required this.message, required this.onRetry});
  final String message;
  final Future<void> Function() onRetry;

  @override
  Widget build(BuildContext context) {
    return Card(
      color: Theme.of(context).colorScheme.errorContainer,
      child: Padding(
        padding: const EdgeInsets.all(8),
        child: Row(
          children: [
            const Icon(Icons.cloud_off, size: 18),
            const SizedBox(width: 8),
            const Expanded(
                child:
                    Text('Gateway offline — validate / set-active disabled')),
            TextButton(onPressed: onRetry, child: const Text('Retry')),
          ],
        ),
      ),
    );
  }
}

class _EdgeKindChip extends StatelessWidget {
  const _EdgeKindChip({required this.kind});
  final GraphEdge_Kind kind;

  @override
  Widget build(BuildContext context) {
    final label = edgeKindLabel(kind);
    final color = switch (kind) {
      GraphEdge_Kind.KIND_MAIN => Colors.blue,
      GraphEdge_Kind.KIND_BACKGROUND => Colors.orange,
      GraphEdge_Kind.KIND_CAPABILITY => Colors.purple,
      _ => Colors.grey,
    };
    return Chip(
      label: Text(label, style: const TextStyle(fontSize: 11)),
      backgroundColor: color.withValues(alpha: 0.15),
      side: BorderSide(color: color.withValues(alpha: 0.4)),
      visualDensity: VisualDensity.compact,
      materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
    );
  }
}

/// One node row: type/version header, remove, and a schema-guided param editor.
class _NodeTile extends StatelessWidget {
  const _NodeTile({
    required this.id,
    required this.node,
    required this.schema,
    required this.onRemove,
    required this.onParamsChanged,
  });

  final String id;
  final GraphNode node;
  final NodeTypeSchema? schema;
  final VoidCallback onRemove;
  final ValueChanged<dynamic> onParamsChanged;

  @override
  Widget build(BuildContext context) {
    return Card(
      child: ExpansionTile(
        title: Text(id, style: const TextStyle(fontWeight: FontWeight.bold)),
        subtitle: Text('${node.type} · v${node.typeVersion}'),
        trailing: IconButton(
          icon: const Icon(Icons.remove_circle_outline, size: 20),
          tooltip: 'Remove node',
          onPressed: onRemove,
        ),
        childrenPadding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
        children: [
          if (schema != null && schema!.doc.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(bottom: 8),
              child: Text(schema!.doc,
                  style: Theme.of(context).textTheme.bodySmall),
            ),
          _ParamsEditor(
            node: node,
            schema: schema,
            onChanged: onParamsChanged,
          ),
        ],
      ),
    );
  }
}

/// Renders `params` from the node type's JSON Schema when the schema is simple
/// (all scalar properties), else a raw-JSON escape hatch. Either way it hands
/// back a plain Dart value via [onChanged].
class _ParamsEditor extends StatefulWidget {
  const _ParamsEditor({
    required this.node,
    required this.schema,
    required this.onChanged,
  });

  final GraphNode node;
  final NodeTypeSchema? schema;
  final ValueChanged<dynamic> onChanged;

  @override
  State<_ParamsEditor> createState() => _ParamsEditorState();
}

class _ParamsEditorState extends State<_ParamsEditor> {
  late Map<String, dynamic> _values;
  late TextEditingController _raw;
  String? _rawError;
  bool _useRaw = false;

  @override
  void initState() {
    super.initState();
    final current = widget.node.hasParams()
        ? jsonValueToDart(widget.node.params)
        : <String, dynamic>{};
    _values = current is Map ? current.cast<String, dynamic>() : {};
    _raw = TextEditingController(
        text: const JsonEncoder.withIndent('  ').convert(_values));
    _useRaw = _schemaProps() == null;
  }

  @override
  void dispose() {
    _raw.dispose();
    super.dispose();
  }

  /// The scalar `properties` map if the schema is simple, else null.
  Map<String, dynamic>? _schemaProps() {
    final s = widget.schema;
    if (s == null || !s.hasParamsSchema()) return null;
    final decoded = jsonValueToDart(s.paramsSchema);
    if (decoded is! Map) return null;
    final props = decoded['properties'];
    if (props is! Map) return null;
    // Only drive the form when every property is a scalar we can render.
    for (final p in props.values) {
      if (p is! Map) return null;
      final t = p['type'];
      if (t is! String ||
          !const ['string', 'integer', 'number', 'boolean'].contains(t)) {
        return null;
      }
    }
    return props.cast<String, dynamic>();
  }

  void _emitFromValues() =>
      widget.onChanged(Map<String, dynamic>.from(_values));

  void _emitFromRaw() {
    try {
      final decoded = jsonDecode(_raw.text);
      setState(() => _rawError = null);
      widget.onChanged(decoded);
    } catch (e) {
      setState(() => _rawError = 'Invalid JSON: $e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final props = _schemaProps();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        if (props != null)
          Row(
            children: [
              const Text('params'),
              const Spacer(),
              TextButton.icon(
                icon: Icon(_useRaw ? Icons.list_alt : Icons.data_object,
                    size: 16),
                label: Text(_useRaw ? 'Form' : 'Raw JSON'),
                onPressed: () {
                  setState(() {
                    if (_useRaw) {
                      // leaving raw → adopt parsed values into the form
                      try {
                        final d = jsonDecode(_raw.text);
                        if (d is Map) _values = d.cast<String, dynamic>();
                        _rawError = null;
                      } catch (_) {}
                    } else {
                      _raw.text =
                          const JsonEncoder.withIndent('  ').convert(_values);
                    }
                    _useRaw = !_useRaw;
                  });
                },
              ),
            ],
          ),
        if (props != null && !_useRaw)
          ...props.entries.map((p) => _field(p.key, p.value as Map))
        else
          _rawEditor(),
      ],
    );
  }

  Widget _rawEditor() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        TextField(
          controller: _raw,
          maxLines: null,
          minLines: 3,
          style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
          decoration: InputDecoration(
            border: const OutlineInputBorder(),
            labelText: 'params (JSON)',
            errorText: _rawError,
          ),
          onChanged: (_) => _emitFromRaw(),
        ),
      ],
    );
  }

  Widget _field(String name, Map spec) {
    final type = spec['type'] as String;
    final enumVals = spec['enum'];
    final desc = spec['description'];

    // Enum → dropdown.
    if (enumVals is List && enumVals.isNotEmpty) {
      final current = _values[name]?.toString();
      final options = enumVals.map((e) => e.toString()).toList();
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: 4),
        child: InputDecorator(
          decoration: InputDecoration(
              labelText: name, helperText: desc is String ? desc : null),
          child: DropdownButtonHideUnderline(
            child: DropdownButton<String>(
              isExpanded: true,
              value: options.contains(current) ? current : null,
              items: [
                for (final o in options)
                  DropdownMenuItem(value: o, child: Text(o)),
              ],
              onChanged: (v) {
                setState(() => _values[name] = v);
                _emitFromValues();
              },
            ),
          ),
        ),
      );
    }

    if (type == 'boolean') {
      final v = _values[name] == true;
      return SwitchListTile(
        contentPadding: EdgeInsets.zero,
        title: Text(name),
        subtitle: desc is String ? Text(desc) : null,
        value: v,
        onChanged: (nv) {
          setState(() => _values[name] = nv);
          _emitFromValues();
        },
      );
    }

    final numeric = type == 'integer' || type == 'number';
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: TextFormField(
        initialValue: _values[name]?.toString() ?? '',
        keyboardType: numeric
            ? const TextInputType.numberWithOptions(decimal: true)
            : TextInputType.text,
        decoration: InputDecoration(
          labelText: name,
          helperText: desc is String ? desc : null,
          border: const OutlineInputBorder(),
        ),
        onChanged: (text) {
          dynamic value = text;
          if (numeric) {
            value = type == 'integer' ? int.tryParse(text) : num.tryParse(text);
          }
          setState(() {
            if (text.isEmpty) {
              _values.remove(name);
            } else {
              _values[name] = value ?? text;
            }
          });
          _emitFromValues();
        },
      ),
    );
  }
}

class _NewNode {
  _NewNode(this.id, this.type);
  final String id;
  final String type;
}

class _AddNodeDialog extends StatefulWidget {
  const _AddNodeDialog({required this.types, required this.existingIds});
  final List<NodeTypeSchema> types;
  final Set<String> existingIds;

  @override
  State<_AddNodeDialog> createState() => _AddNodeDialogState();
}

class _AddNodeDialogState extends State<_AddNodeDialog> {
  final _id = TextEditingController();
  String? _type;
  String? _error;

  @override
  void initState() {
    super.initState();
    _type = widget.types.isNotEmpty ? widget.types.first.type : null;
  }

  @override
  void dispose() {
    _id.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Add node'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          TextField(
            controller: _id,
            autofocus: true,
            decoration: InputDecoration(
              labelText: 'Node id',
              helperText: 'stable, unique, ≤64 chars (no anchor. prefix)',
              errorText: _error,
            ),
          ),
          const SizedBox(height: 12),
          DropdownButtonFormField<String>(
            initialValue: _type,
            decoration: const InputDecoration(labelText: 'Type'),
            items: [
              for (final t in widget.types)
                DropdownMenuItem(
                  value: t.type,
                  child:
                      Text(t.title.isEmpty ? t.type : '${t.type} — ${t.title}'),
                ),
            ],
            onChanged: (v) => setState(() => _type = v),
          ),
        ],
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel')),
        FilledButton(
          onPressed: () {
            final id = _id.text.trim();
            if (id.isEmpty) {
              setState(() => _error = 'required');
              return;
            }
            if (widget.existingIds.contains(id)) {
              setState(() => _error = 'already exists');
              return;
            }
            if (_type == null) return;
            Navigator.pop(context, _NewNode(id, _type!));
          },
          child: const Text('Add'),
        ),
      ],
    );
  }
}

class _AddEdgeDialog extends StatefulWidget {
  const _AddEdgeDialog({required this.nodeIds});
  final List<String> nodeIds;

  @override
  State<_AddEdgeDialog> createState() => _AddEdgeDialogState();
}

class _AddEdgeDialogState extends State<_AddEdgeDialog> {
  final _from = TextEditingController();
  final _to = TextEditingController();
  GraphEdge_Kind _kind = GraphEdge_Kind.KIND_MAIN;

  @override
  void dispose() {
    _from.dispose();
    _to.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final suggestions = [..._anchors, ...widget.nodeIds].join(', ');
    return AlertDialog(
      title: const Text('Add edge'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          TextField(
            controller: _from,
            autofocus: true,
            decoration: const InputDecoration(
              labelText: 'from',
              helperText:
                  'a node id, an anchor, or (capability) a resource name',
            ),
          ),
          const SizedBox(height: 8),
          TextField(
            controller: _to,
            decoration: const InputDecoration(labelText: 'to'),
          ),
          const SizedBox(height: 8),
          Align(
            alignment: Alignment.centerLeft,
            child: Text('available: $suggestions',
                style: Theme.of(context).textTheme.bodySmall),
          ),
          const SizedBox(height: 12),
          DropdownButtonFormField<GraphEdge_Kind>(
            initialValue: _kind,
            decoration: const InputDecoration(labelText: 'kind'),
            items: [
              for (final k in graphEdgeKinds)
                DropdownMenuItem(value: k, child: Text(edgeKindLabel(k))),
            ],
            onChanged: (v) => setState(() => _kind = v ?? _kind),
          ),
        ],
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel')),
        FilledButton(
          onPressed: () {
            final from = _from.text.trim();
            final to = _to.text.trim();
            if (from.isEmpty || to.isEmpty) return;
            Navigator.pop(context, GraphEdge(from: from, to: to, kind: _kind));
          },
          child: const Text('Add'),
        ),
      ],
    );
  }
}

/// A compact, read-only layered drawing of the graph — legibility, not editing.
class _PreviewCard extends StatelessWidget {
  const _PreviewCard({required this.graph});
  final CognitionGraph graph;

  @override
  Widget build(BuildContext context) {
    return Card(
      child: SizedBox(
        height: 220,
        width: double.infinity,
        child: graph.nodes.isEmpty
            ? const Center(child: Text('Empty graph'))
            : CustomPaint(
                painter: _GraphPainter(
                  graph: graph,
                  color: Theme.of(context).colorScheme.primary,
                  textColor: Theme.of(context).colorScheme.onSurface,
                ),
              ),
      ),
    );
  }
}

class _GraphPainter extends CustomPainter {
  _GraphPainter({
    required this.graph,
    required this.color,
    required this.textColor,
  });

  final CognitionGraph graph;
  final Color color;
  final Color textColor;

  @override
  void paint(Canvas canvas, Size size) {
    final ids = graph.nodes.keys.toList()..sort();
    // Layered layout: depth = longest MAIN path among nodes.
    final depth = {for (final id in ids) id: 0};
    for (var pass = 0; pass < ids.length; pass++) {
      for (final e in graph.edges) {
        if (e.kind == GraphEdge_Kind.KIND_MAIN &&
            depth.containsKey(e.from) &&
            depth.containsKey(e.to)) {
          final d = depth[e.from]! + 1;
          if (d > depth[e.to]!) depth[e.to] = d;
        }
      }
    }
    final maxDepth =
        depth.values.isEmpty ? 0 : depth.values.reduce((a, b) => a > b ? a : b);
    final columns = <int, List<String>>{};
    for (final id in ids) {
      columns.putIfAbsent(depth[id]!, () => []).add(id);
    }

    const boxW = 120.0;
    const boxH = 34.0;
    final colGap = maxDepth == 0
        ? 0.0
        : (size.width - boxW - 24) / (maxDepth == 0 ? 1 : maxDepth);
    final centers = <String, Offset>{};
    for (final entry in columns.entries) {
      final col = entry.key;
      final list = entry.value;
      final x = 12 + (maxDepth == 0 ? (size.width - boxW) / 2 : col * colGap);
      final rowGap = size.height / (list.length + 1);
      for (var i = 0; i < list.length; i++) {
        centers[list[i]] = Offset(x + boxW / 2, rowGap * (i + 1));
      }
    }

    // Edges first (behind boxes).
    for (final e in graph.edges) {
      final a = centers[e.from];
      final b = centers[e.to];
      if (a == null || b == null) continue; // anchor/capability endpoints
      final paint = Paint()
        ..strokeWidth = 1.5
        ..color = switch (e.kind) {
          GraphEdge_Kind.KIND_MAIN => Colors.blue,
          GraphEdge_Kind.KIND_BACKGROUND => Colors.orange,
          GraphEdge_Kind.KIND_CAPABILITY => Colors.purple,
          _ => Colors.grey,
        };
      canvas.drawLine(a, b, paint);
    }

    // Boxes + labels.
    for (final id in ids) {
      final c = centers[id]!;
      final rect = Rect.fromCenter(center: c, width: boxW, height: boxH);
      final rrect = RRect.fromRectAndRadius(rect, const Radius.circular(6));
      canvas.drawRRect(rrect, Paint()..color = color.withValues(alpha: 0.12));
      canvas.drawRRect(
          rrect,
          Paint()
            ..style = PaintingStyle.stroke
            ..strokeWidth = 1
            ..color = color.withValues(alpha: 0.6));
      final tp = TextPainter(
        text: TextSpan(
          text: id,
          style: TextStyle(color: textColor, fontSize: 11),
        ),
        textAlign: TextAlign.center,
        textDirection: TextDirection.ltr,
        maxLines: 1,
        ellipsis: '…',
      )..layout(maxWidth: boxW - 10);
      tp.paint(canvas, Offset(c.dx - tp.width / 2, c.dy - tp.height / 2));
    }
  }

  @override
  bool shouldRepaint(covariant _GraphPainter old) => true;
}
