import 'dart:convert';

import 'package:flutter/material.dart';

/// A form driven by a JSON-Schema (draft-07, as emitted by the backend
/// `ConfigService.GetSchema`). Generalises the graph page's `_ParamsEditor` to
/// the whole-config shape: it resolves `$ref`/`allOf` into `definitions`, renders
/// scalars (string / enum-dropdown / number / bool), masks secrets
/// (`x-secret`), recurses into nested objects, and falls back to a raw-JSON
/// editor for arrays / maps / anything it can't render as a field.
///
/// It edits a working copy of [values] and emits the whole updated map via
/// [onChanged]; the caller diffs against the original to compute the minimal
/// set of changed paths.
class SchemaForm extends StatefulWidget {
  /// The schema node for this level (an object schema with `properties`, or a
  /// `$ref`/`allOf` wrapper that resolves to one).
  final Map<String, dynamic> schema;

  /// The full `definitions` map from the root schema, for `$ref` resolution.
  final Map<String, dynamic> definitions;

  /// Current values for this level (a JSON object).
  final Map<String, dynamic> values;

  /// Emits the full updated values map for this level on every edit.
  final ValueChanged<Map<String, dynamic>> onChanged;

  const SchemaForm({
    super.key,
    required this.schema,
    required this.definitions,
    required this.values,
    required this.onChanged,
  });

  @override
  State<SchemaForm> createState() => _SchemaFormState();
}

class _SchemaFormState extends State<SchemaForm> {
  late Map<String, dynamic> _v;

  @override
  void initState() {
    super.initState();
    _v = _deepCopy(widget.values);
  }

  @override
  void didUpdateWidget(SchemaForm old) {
    super.didUpdateWidget(old);
    // Re-seed when the caller swaps in a different section/values.
    if (!identical(old.values, widget.values)) {
      _v = _deepCopy(widget.values);
    }
  }

  void _set(String key, dynamic value) {
    setState(() => _v[key] = value);
    widget.onChanged(_v);
  }

  /// Resolve `$ref` and single-`allOf` wrappers to the concrete schema node,
  /// carrying the outer `description`/`default` down onto the result.
  Map<String, dynamic> _resolve(Map<String, dynamic> node) {
    var n = node;
    for (var i = 0; i < 8; i++) {
      if (n['\$ref'] is String) {
        final name = (n['\$ref'] as String).split('/').last;
        final def = widget.definitions[name];
        if (def is Map<String, dynamic>) {
          n = {...def, ..._carry(node)};
          continue;
        }
      }
      if (n['allOf'] is List && (n['allOf'] as List).length == 1) {
        final inner = (n['allOf'] as List).first;
        if (inner is Map<String, dynamic>) {
          n = {..._resolve(inner), ..._carry(node)};
          continue;
        }
      }
      break;
    }
    return n;
  }

  // Preserve outer annotations when following a ref.
  Map<String, dynamic> _carry(Map<String, dynamic> outer) => {
        if (outer['description'] != null) 'description': outer['description'],
        if (outer['default'] != null) 'default': outer['default'],
        if (outer['x-secret'] != null) 'x-secret': outer['x-secret'],
      };

  /// The effective scalar type of a node, tolerating `["string","null"]` unions.
  String? _typeOf(Map<String, dynamic> node) {
    final t = node['type'];
    if (t is String) return t;
    if (t is List) {
      return t.cast<dynamic>().firstWhere(
            (e) => e != 'null',
            orElse: () => null,
          ) as String?;
    }
    return null;
  }

  @override
  Widget build(BuildContext context) {
    final resolved = _resolve(widget.schema);
    final props = resolved['properties'];
    if (props is! Map<String, dynamic>) {
      // Not an object schema — render the whole thing raw.
      return _RawJsonField(
        label: 'value',
        value: _v,
        onChanged: (parsed) {
          if (parsed is Map<String, dynamic>) {
            setState(() => _v = parsed);
            widget.onChanged(_v);
          }
        },
      );
    }
    final keys = props.keys.toList()..sort();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final key in keys) _field(key, _resolve(props[key] as Map<String, dynamic>)),
      ],
    );
  }

  Widget _field(String key, Map<String, dynamic> node) {
    final desc = node['description'] as String?;
    final def = node['default'];
    final current = _v[key];
    final isSecret = node['x-secret'] == true;

    // Enum → dropdown.
    if (node['enum'] is List) {
      final options = (node['enum'] as List).map((e) => '$e').toList();
      final cur = current?.toString();
      final items = [...options];
      if (cur != null && !items.contains(cur)) items.insert(0, cur);
      return _wrap(
        DropdownButtonFormField<String>(
          initialValue: items.contains(cur) ? cur : null,
          isExpanded: true,
          decoration: _dec(key, desc, def),
          items: [
            for (final o in items)
              DropdownMenuItem(value: o, child: Text(o.isEmpty ? '(empty)' : o)),
          ],
          onChanged: (v) => _set(key, v),
        ),
      );
    }

    final type = _typeOf(node);

    if (type == 'boolean') {
      return SwitchListTile(
        title: Text(key),
        subtitle: desc == null ? null : Text(desc),
        value: current == true,
        onChanged: (v) => _set(key, v),
      );
    }

    if (type == 'string') {
      return _wrap(
        TextFormField(
          initialValue: isSecret ? '' : (current?.toString() ?? ''),
          obscureText: isSecret,
          decoration: _dec(
            key,
            isSecret ? '${desc ?? ''}  (leave blank to keep unchanged)'.trim() : desc,
            isSecret ? '••••••' : def,
          ),
          onChanged: (v) {
            // Empty secret = no change: keep the original masked value.
            if (isSecret && v.isEmpty) {
              _set(key, widget.values[key] ?? '');
            } else {
              _set(key, v);
            }
          },
        ),
      );
    }

    if (type == 'integer' || type == 'number') {
      final isInt = type == 'integer';
      return _wrap(
        TextFormField(
          initialValue: current?.toString() ?? '',
          keyboardType: TextInputType.numberWithOptions(decimal: !isInt),
          decoration: _dec(key, desc, def),
          onChanged: (v) {
            final t = v.trim();
            if (t.isEmpty) {
              _set(key, null);
            } else if (isInt) {
              final n = int.tryParse(t);
              if (n != null) _set(key, n);
            } else {
              final n = double.tryParse(t);
              if (n != null) _set(key, n);
            }
          },
        ),
      );
    }

    if (type == 'object' && node['properties'] is Map<String, dynamic>) {
      // Nested table — recurse under an expander.
      final childValues =
          (current is Map<String, dynamic>) ? current : <String, dynamic>{};
      return ExpansionTile(
        title: Text(key),
        subtitle: desc == null ? null : Text(desc, maxLines: 2, overflow: TextOverflow.ellipsis),
        childrenPadding: const EdgeInsets.only(left: 12, bottom: 8),
        children: [
          SchemaForm(
            schema: node,
            definitions: widget.definitions,
            values: childValues,
            onChanged: (m) => _set(key, m),
          ),
        ],
      );
    }

    // Arrays, maps (object + additionalProperties), unions, unknowns → raw JSON.
    return _wrap(
      _RawJsonField(
        label: key,
        help: desc,
        value: current,
        onChanged: (parsed) => _set(key, parsed),
      ),
    );
  }

  Widget _wrap(Widget child) =>
      Padding(padding: const EdgeInsets.symmetric(vertical: 6), child: child);

  InputDecoration _dec(String label, String? help, dynamic def) {
    final helper = [
      if (help != null && help.isNotEmpty) help,
      if (def != null && '$def'.isNotEmpty) 'default: $def',
    ].join('  •  ');
    return InputDecoration(
      labelText: label,
      helperText: helper.isEmpty ? null : helper,
      helperMaxLines: 3,
      border: const OutlineInputBorder(),
      isDense: true,
    );
  }

  static Map<String, dynamic> _deepCopy(Map<String, dynamic> m) =>
      jsonDecode(jsonEncode(m)) as Map<String, dynamic>;
}

/// A raw-JSON escape hatch for one value: a monospace field that parses on edit
/// and reports parse errors inline. Used for arrays / maps / shapes the form
/// can't render as typed fields.
class _RawJsonField extends StatefulWidget {
  final String label;
  final String? help;
  final dynamic value;
  final ValueChanged<dynamic> onChanged;
  const _RawJsonField({
    required this.label,
    this.help,
    required this.value,
    required this.onChanged,
  });

  @override
  State<_RawJsonField> createState() => _RawJsonFieldState();
}

class _RawJsonFieldState extends State<_RawJsonField> {
  late final TextEditingController _c;
  String? _error;

  @override
  void initState() {
    super.initState();
    _c = TextEditingController(
      text: const JsonEncoder.withIndent('  ').convert(widget.value),
    );
  }

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        TextField(
          controller: _c,
          maxLines: null,
          style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
          decoration: InputDecoration(
            labelText: '${widget.label} (JSON)',
            helperText: widget.help,
            helperMaxLines: 3,
            errorText: _error,
            border: const OutlineInputBorder(),
            isDense: true,
          ),
          onChanged: (t) {
            try {
              final parsed = jsonDecode(t);
              setState(() => _error = null);
              widget.onChanged(parsed);
            } catch (_) {
              setState(() => _error = 'Invalid JSON');
            }
          },
        ),
      ],
    );
  }
}
