// The theme editor (docs/design/visual/03-user-themes.md#authoring): clone a
// theme, change its tokens with a live preview and the validator's contrast
// checks beside each pair, import and export textproto, and save it private or
// shared with the tenant. The server parses and prints textproto
// (`ThemeService` `Import`/`Export`) and decides who may share; this page only
// edits a draft and shows what the validator will say about it.
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:grpc/grpc.dart' show GrpcError, StatusCode;

import '../gen/agent/v1/theme.pb.dart' as pb;
import '../gen/agent/v1/theme.pbenum.dart' as pbe;
import 'seddon_colors.dart';
import 'theme_controller.dart';
import 'theme_mapper.dart';
import 'theme_validate.dart';

/// One editable colour token: its wire name and its field number in `Palette`
/// (or in `SemanticColors` when [semantic]).
class ColourField {
  const ColourField(this.name, this.tag, {this.semantic = false});

  final String name;
  final int tag;
  final bool semantic;
}

/// The colour tokens the editor offers, in the order of `theme.proto`. The
/// `nav_tint` and `graph_edge` maps are kept from the cloned theme as they are.
const editorColourFields = [
  ColourField('seed', 1),
  ColourField('primary', 2),
  ColourField('on_primary', 3),
  ColourField('secondary', 4),
  ColourField('on_secondary', 5),
  ColourField('surface', 6),
  ColourField('on_surface', 7),
  ColourField('background', 8),
  ColourField('surface_variant', 9),
  ColourField('on_surface_variant', 10),
  ColourField('outline', 11),
  ColourField('error', 12),
  ColourField('on_error', 13),
  ColourField('focus', 14),
  ColourField('success', 1, semantic: true),
  ColourField('warning', 2, semantic: true),
  ColourField('info', 3, semantic: true),
  ColourField('muted', 4, semantic: true),
  ColourField('code_background', 5, semantic: true),
  ColourField('code_foreground', 6, semantic: true),
];

String colourOf(pb.Palette p, ColourField f) =>
    (f.semantic ? p.semantic : p).getField(f.tag) as String;

/// Set [f] in [p]; an empty [value] clears it.
void setColour(pb.Palette p, ColourField f, String value) {
  final m = f.semantic ? p.ensureSemantic() : p;
  if (value.isEmpty) {
    m.clearField(f.tag);
  } else {
    m.setField(f.tag, value);
  }
}

/// [base] if no theme in [taken] uses it, else `base-2`, `base-3`… Always a
/// valid id: cut to [maxIdLen] with room for the suffix.
String freeId(String base, Iterable<String> taken) {
  final used = taken.toSet();
  String cut(String s, int room) => s.length > room ? s.substring(0, room) : s;
  final first = cut(base, maxIdLen);
  if (!used.contains(first)) return first;
  for (var n = 2;; n++) {
    final suffix = '-$n';
    final id = '${cut(base, maxIdLen - suffix.length)}$suffix';
    if (!used.contains(id)) return id;
  }
}

/// A private copy of [source] to edit: a free id and a "(copy)" name, and none
/// of the server-set fields (`builtin`, `version`, `owner`).
pb.Theme cloneForEdit(pb.Theme source, Iterable<String> taken) {
  final t = source.deepCopy()
    ..clearBuiltin()
    ..clearVersion()
    ..clearOwner()
    ..visibility = pbe.Visibility.VISIBILITY_PRIVATE;
  t.id = freeId('${source.id.isEmpty ? 'theme' : source.id}-copy', taken);
  final name = '${source.name.isEmpty ? 'Theme' : source.name} (copy)';
  t.name = name.runes.length > maxNameChars
      ? String.fromCharCodes(name.runes.take(maxNameChars))
      : name;
  return t;
}

/// The draft to open the editor on: the active theme itself when it is a user
/// theme (edited in place, keeping its `version`), else a copy of it.
pb.Theme draftFor(ThemeController themes) => themes.active.builtin
    ? cloneForEdit(themes.active, themes.themes.map((t) => t.id))
    : themes.active.deepCopy();

/// The first rule [t] breaks, or null when `Put` would take it.
String? draftProblem(pb.Theme t) {
  try {
    validateTheme(t);
    return null;
  } on ThemeError catch (e) {
    return e.message;
  }
}

String _grpcMessage(Object e) => switch (e) {
      GrpcError(code: StatusCode.permissionDenied, :final message) =>
        'Not allowed: ${message ?? 'permission denied'}',
      GrpcError(code: StatusCode.aborted) =>
        'Someone else saved this theme first. Reopen it to see their version.',
      GrpcError(:final message, :final codeName) => message ?? codeName,
      _ => '$e',
    };

/// Opens the editor on [draftFor] the active theme.
class ThemeEditorButton extends StatelessWidget {
  const ThemeEditorButton({super.key});

  @override
  Widget build(BuildContext context) {
    final themes = ThemeScope.maybeOf(context);
    if (themes == null) return const SizedBox.shrink();
    return OutlinedButton.icon(
      key: const Key('theme_editor.open'),
      icon: const Icon(Icons.edit_outlined),
      label: Text(themes.active.builtin ? 'Edit a copy' : 'Edit'),
      onPressed: () => Navigator.of(context).push(MaterialPageRoute<void>(
        builder: (_) => ThemeScope(
          controller: themes,
          child: ThemeEditorPage(initial: draftFor(themes)),
        ),
      )),
    );
  }
}

class ThemeEditorPage extends StatefulWidget {
  const ThemeEditorPage({super.key, required this.initial});

  final pb.Theme initial;

  @override
  State<ThemeEditorPage> createState() => _ThemeEditorPageState();
}

class _ThemeEditorPageState extends State<ThemeEditorPage> {
  late pb.Theme _draft = widget.initial.deepCopy();

  /// Bumped when the draft is replaced (import, a new copy), so every field
  /// re-reads it.
  int _generation = 0;
  Brightness _side = Brightness.light;
  bool _busy = false;

  /// Why the server would refuse an imported draft, until it is edited.
  String? _serverProblem;

  /// Whether the draft is stored (so it has something to delete).
  late bool _stored = widget.initial.version > 0;

  @override
  void initState() {
    super.initState();
    if (!_draft.hasLight() && _draft.hasDark()) _side = Brightness.dark;
  }

  pb.Palette get _palette =>
      _side == Brightness.light ? _draft.ensureLight() : _draft.ensureDark();

  bool get _hasPalette => _side == Brightness.light ? _draft.hasLight() : _draft.hasDark();

  void _edit(void Function() change) => setState(() {
        change();
        _serverProblem = null;
      });

  void _replace(pb.Theme t, {String? problem}) => setState(() {
        _draft = t;
        _generation++;
        _serverProblem = (problem == null || problem.isEmpty) ? null : problem;
        _stored = t.version > 0;
        _side = !t.hasLight() && t.hasDark() ? Brightness.dark : Brightness.light;
      });

  void _say(String message) => ScaffoldMessenger.of(context)
      .showSnackBar(SnackBar(content: Text(message)));

  Future<void> _run(Future<void> Function(ThemeServer) call) async {
    final server = ThemeScope.maybeOf(context)?.server;
    if (server == null || _busy) return;
    setState(() => _busy = true);
    try {
      await call(server);
    } catch (e) {
      if (mounted) _say(_grpcMessage(e));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _save() => _run((server) async {
        final problem = draftProblem(_draft);
        if (problem != null) {
          _say('Not saved: $problem');
          return;
        }
        final stored = await server.put(_draft);
        if (!mounted) return;
        final themes = ThemeScope.maybeOf(context);
        if (themes != null && !themes.saved(stored)) {
          _say('Saved, but the server sent back a theme this portal cannot use');
          return;
        }
        _replace(stored);
        _say('Saved “${stored.name}” and switched to it');
      });

  Future<void> _delete() async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Delete “${_draft.name}”?'),
        content: const Text('Anyone using it goes back to the default theme.'),
        actions: [
          TextButton(
              key: const Key('theme_editor.delete.cancel'),
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('Cancel')),
          FilledButton(
              key: const Key('theme_editor.delete.confirm'),
              onPressed: () => Navigator.pop(ctx, true),
              child: const Text('Delete')),
        ],
      ),
    );
    if (ok != true || !mounted) return;
    await _run((server) async {
      await server.delete(_draft.id);
      if (!mounted) return;
      ThemeScope.maybeOf(context)?.removed(_draft.id);
      Navigator.of(context).pop();
    });
  }

  Future<void> _import() async {
    final text = await showDialog<String>(
      context: context,
      builder: (_) => _ImportDialog(monoFamily: _monoFamily(context)),
    );
    if (text == null || text.trim().isEmpty || !mounted) return;
    // The server refuses it too; this saves sending 64 KiB to be told so.
    if (text.length > maxThemeBytes) {
      _say('Not imported: the text is over ${maxThemeBytes ~/ 1024} KiB');
      return;
    }
    await _run((server) async {
      final (theme, problem) = await server.importText(text);
      if (!mounted) return;
      _replace(theme, problem: problem);
      _say(problem.isEmpty ? 'Imported “${theme.name}”' : 'Imported, with a problem to fix');
    });
  }

  Future<void> _export() => _run((server) async {
        final text = await server.exportText(_draft);
        if (!mounted) return;
        await showDialog<void>(
          context: context,
          builder: (ctx) => AlertDialog(
            title: Text('${_draft.id.isEmpty ? 'theme' : _draft.id}.textproto'),
            content: SizedBox(
              width: 560,
              height: 360,
              child: SingleChildScrollView(
                child: SelectableText(
                  text,
                  key: const Key('theme_editor.export.text'),
                  style: TextStyle(fontFamily: _monoFamily(context)),
                ),
              ),
            ),
            actions: [
              TextButton.icon(
                key: const Key('theme_editor.export.copy'),
                icon: const Icon(Icons.copy),
                label: const Text('Copy'),
                onPressed: () async {
                  await Clipboard.setData(ClipboardData(text: text));
                  if (ctx.mounted) Navigator.pop(ctx);
                  if (mounted) _say('Copied');
                },
              ),
              TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Close')),
            ],
          ),
        );
      });

  static String? _monoFamily(BuildContext context) {
    final f = ThemeScope.maybeOf(context)?.active.typography.monoFamily ?? '';
    return f.isEmpty ? 'monospace' : f;
  }

  @override
  Widget build(BuildContext context) {
    final server = ThemeScope.maybeOf(context)?.server;
    final online = server != null && !_busy;
    final problem = _serverProblem ?? draftProblem(_draft);
    return Scaffold(
      appBar: AppBar(
        title: Text('Theme editor — ${_draft.name.isEmpty ? 'untitled' : _draft.name}'),
        actions: [
          IconButton(
            key: const Key('theme_editor.import'),
            tooltip: 'Import textproto',
            icon: const Icon(Icons.file_open_outlined),
            onPressed: online ? _import : null,
          ),
          IconButton(
            key: const Key('theme_editor.export'),
            tooltip: 'Export textproto',
            icon: const Icon(Icons.ios_share),
            onPressed: online ? _export : null,
          ),
          if (_stored)
            IconButton(
              key: const Key('theme_editor.delete'),
              tooltip: 'Delete theme',
              icon: const Icon(Icons.delete_outline),
              onPressed: online ? _delete : null,
            ),
          const SizedBox(width: 8),
          Padding(
            padding: const EdgeInsets.only(right: 12),
            child: FilledButton.icon(
              key: const Key('theme_editor.save'),
              icon: const Icon(Icons.save_outlined),
              label: const Text('Save and use'),
              onPressed: online && problem == null ? _save : null,
            ),
          ),
        ],
      ),
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (server == null)
            const _Banner(
              key: Key('theme_editor.offline'),
              icon: Icons.cloud_off,
              text: 'Sign in to save, import or export themes. The preview still works.',
            ),
          _Banner(
            key: const Key('theme_editor.problem'),
            icon: problem == null ? Icons.check_circle_outline : Icons.error_outline,
            text: problem ?? 'Ready to save',
            error: problem != null,
          ),
          Expanded(
            child: LayoutBuilder(builder: (context, box) {
              final form = KeyedSubtree(
                key: ValueKey(_generation),
                child: _form(context),
              );
              final preview = _Preview(theme: _draft, side: _side);
              if (box.maxWidth >= 900) {
                return Row(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Expanded(flex: 3, child: SingleChildScrollView(padding: const EdgeInsets.all(16), child: form)),
                    const VerticalDivider(width: 1),
                    Expanded(flex: 2, child: SingleChildScrollView(padding: const EdgeInsets.all(16), child: preview)),
                  ],
                );
              }
              return SingleChildScrollView(
                padding: const EdgeInsets.all(16),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [form, const SizedBox(height: 24), preview],
                ),
              );
            }),
          ),
        ],
      ),
    );
  }

  Widget _form(BuildContext context) {
    final text = Theme.of(context).textTheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text('Details', style: text.titleMedium),
        const SizedBox(height: 8),
        Wrap(spacing: 12, runSpacing: 12, children: [
          _TextBox(
            key: const Key('theme_editor.id'),
            label: 'Id',
            value: _draft.id,
            width: 220,
            enabled: !_stored,
            helper: _stored ? 'Saved themes keep their id' : 'Letters, digits, . _ -',
            onChanged: (v) => _edit(() => _draft.id = v),
          ),
          _TextBox(
            key: const Key('theme_editor.name'),
            label: 'Name',
            value: _draft.name,
            width: 260,
            onChanged: (v) => _edit(() => _draft.name = v),
          ),
          _TextBox(
            label: 'Author',
            value: _draft.author,
            width: 220,
            onChanged: (v) => _edit(() => _draft.author = v),
          ),
        ]),
        const SizedBox(height: 12),
        _TextBox(
          label: 'Description',
          value: _draft.description,
          onChanged: (v) => _edit(() => _draft.description = v),
        ),
        const SizedBox(height: 12),
        SegmentedButton<pbe.Visibility>(
          key: const Key('theme_editor.visibility'),
          segments: const [
            ButtonSegment(
                value: pbe.Visibility.VISIBILITY_PRIVATE,
                icon: Icon(Icons.lock_outline),
                label: Text('Private')),
            ButtonSegment(
                value: pbe.Visibility.VISIBILITY_TENANT,
                icon: Icon(Icons.groups_outlined),
                label: Text('Shared with the tenant')),
          ],
          selected: {
            _draft.visibility == pbe.Visibility.VISIBILITY_TENANT
                ? pbe.Visibility.VISIBILITY_TENANT
                : pbe.Visibility.VISIBILITY_PRIVATE
          },
          onSelectionChanged: (s) => _edit(() => _draft.visibility = s.first),
        ),
        const SizedBox(height: 24),
        Row(children: [
          Text('Palette', style: text.titleMedium),
          const SizedBox(width: 16),
          SegmentedButton<Brightness>(
            key: const Key('theme_editor.side'),
            segments: const [
              ButtonSegment(
                  value: Brightness.light, icon: Icon(Icons.light_mode_outlined), label: Text('Light')),
              ButtonSegment(
                  value: Brightness.dark, icon: Icon(Icons.dark_mode_outlined), label: Text('Dark')),
            ],
            selected: {_side},
            onSelectionChanged: (s) => setState(() {
              _side = s.first;
              _generation++;
            }),
          ),
        ]),
        const SizedBox(height: 8),
        if (!_hasPalette) _addPalette(context) else ..._palettePanel(context),
        const SizedBox(height: 24),
        Text('Type, shape and density', style: text.titleMedium),
        const SizedBox(height: 8),
        _typeAndShape(context),
      ],
    );
  }

  Widget _addPalette(BuildContext context) {
    final other = _side == Brightness.light ? _draft.dark : _draft.light;
    final name = _side == Brightness.light ? 'light' : 'dark';
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Row(children: [
          Expanded(
              child: Text('This theme has no $name palette; in $name mode it uses the '
                  'other one.')),
          FilledButton.tonal(
            key: const Key('theme_editor.add_palette'),
            onPressed: () => setState(() {
              final copy = other.deepCopy();
              if (_side == Brightness.light) {
                _draft.light = copy;
              } else {
                _draft.dark = copy;
              }
              _generation++;
              _serverProblem = null;
            }),
            child: Text('Add a $name palette'),
          ),
        ]),
      ),
    );
  }

  List<Widget> _palettePanel(BuildContext context) {
    final p = _palette;
    final checks = paletteContrast(_draft, p);
    final canRemove = _draft.hasLight() && _draft.hasDark();
    return [
      Wrap(spacing: 12, runSpacing: 12, children: [
        for (final f in editorColourFields)
          _ColourBox(
            key: Key('theme_editor.color.${f.name}'),
            field: f,
            value: colourOf(p, f),
            onChanged: (v) => _edit(() => setColour(_palette, f, v.trim())),
          ),
      ]),
      const SizedBox(height: 16),
      Text('Contrast (${checks.isEmpty ? 'AA' : checks.first.level})',
          style: Theme.of(context).textTheme.titleSmall),
      const SizedBox(height: 8),
      Wrap(spacing: 8, runSpacing: 8, children: [
        for (final c in checks) _ContrastBadge(check: c),
      ]),
      if (canRemove) ...[
        const SizedBox(height: 12),
        Align(
          alignment: Alignment.centerLeft,
          child: TextButton.icon(
            key: const Key('theme_editor.remove_palette'),
            icon: const Icon(Icons.remove_circle_outline),
            label: Text('Remove the ${_side == Brightness.light ? 'light' : 'dark'} palette'),
            onPressed: () => setState(() {
              if (_side == Brightness.light) {
                _draft.clearLight();
                _side = Brightness.dark;
              } else {
                _draft.clearDark();
                _side = Brightness.light;
              }
              _generation++;
              _serverProblem = null;
            }),
          ),
        ),
      ],
    ];
  }

  Widget _typeAndShape(BuildContext context) {
    final typo = _draft.typography;
    Widget font(String which, String value, void Function(pb.Typography, String) set) {
      final known = value.isEmpty || fontFamilies.contains(value);
      return SizedBox(
        width: 220,
        child: DropdownButtonFormField<String>(
          key: Key('theme_editor.font.$which'),
          initialValue: known ? value : null,
          isExpanded: true,
          decoration: InputDecoration(labelText: '${which[0].toUpperCase()}${which.substring(1)} font'),
          items: [
            const DropdownMenuItem(value: '', child: Text('(default)')),
            for (final f in fontFamilies)
              DropdownMenuItem(
                  value: f,
                  child: Text(f, overflow: TextOverflow.ellipsis, style: TextStyle(fontFamily: f))),
          ],
          onChanged: (v) => _edit(() => set(_draft.ensureTypography(), v ?? '')),
        ),
      );
    }

    final scale = typo.scale == 0 ? 1.0 : typo.scale.clamp(0.75, 1.5).toDouble();
    final radius = _draft.shape.cornerRadius.clamp(0, 32);
    return Wrap(spacing: 16, runSpacing: 12, crossAxisAlignment: WrapCrossAlignment.center, children: [
      font('body', typo.bodyFamily, (t, v) => t.bodyFamily = v),
      font('heading', typo.headingFamily, (t, v) => t.headingFamily = v),
      font('mono', typo.monoFamily, (t, v) => t.monoFamily = v),
      _LabelledSlider(
        label: 'Text size ${(scale * 100).round()}%',
        value: scale,
        min: 0.75,
        max: 1.5,
        divisions: 15,
        onChanged: (v) => _edit(() => _draft.ensureTypography().scale = v),
      ),
      _LabelledSlider(
        label: 'Corner radius ${radius}px',
        value: radius.toDouble(),
        min: 0,
        max: 32,
        divisions: 32,
        onChanged: (v) => _edit(() => _draft.ensureShape().cornerRadius = v.round()),
      ),
      SizedBox(
        width: 220,
        child: DropdownButtonFormField<pbe.Density>(
          initialValue: _draft.density,
          isExpanded: true,
          decoration: const InputDecoration(labelText: 'Density'),
          items: const [
            DropdownMenuItem(value: pbe.Density.DENSITY_UNSPECIFIED, child: Text('Default')),
            DropdownMenuItem(value: pbe.Density.DENSITY_COMPACT, child: Text('Compact')),
            DropdownMenuItem(value: pbe.Density.DENSITY_STANDARD, child: Text('Standard')),
            DropdownMenuItem(value: pbe.Density.DENSITY_COMFORTABLE, child: Text('Comfortable')),
          ],
          onChanged: (v) => _edit(() => _draft.density = v ?? pbe.Density.DENSITY_UNSPECIFIED),
        ),
      ),
    ]);
  }
}

class _Banner extends StatelessWidget {
  const _Banner({super.key, required this.icon, required this.text, this.error = false});

  final IconData icon;
  final String text;
  final bool error;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final bg = error ? cs.errorContainer : cs.surfaceContainerHighest;
    final fg = error ? cs.onErrorContainer : cs.onSurface;
    return Material(
      color: bg,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
        child: Row(children: [
          Icon(icon, size: 18, color: fg),
          const SizedBox(width: 8),
          Expanded(child: Text(text, style: TextStyle(color: fg))),
        ]),
      ),
    );
  }
}

class _TextBox extends StatefulWidget {
  const _TextBox({
    super.key,
    required this.label,
    required this.value,
    required this.onChanged,
    this.width,
    this.enabled = true,
    this.helper,
  });

  final String label, value;
  final ValueChanged<String> onChanged;
  final double? width;
  final bool enabled;
  final String? helper;

  @override
  State<_TextBox> createState() => _TextBoxState();
}

class _TextBoxState extends State<_TextBox> {
  late final _c = TextEditingController(text: widget.value);

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => SizedBox(
        width: widget.width,
        child: TextField(
          controller: _c,
          enabled: widget.enabled,
          decoration: InputDecoration(labelText: widget.label, helperText: widget.helper),
          onChanged: widget.onChanged,
        ),
      );
}

/// A hex field with a swatch of what it parses to; a value that does not parse
/// shows the validator's message under the field.
class _ColourBox extends StatefulWidget {
  const _ColourBox({super.key, required this.field, required this.value, required this.onChanged});

  final ColourField field;
  final String value;
  final ValueChanged<String> onChanged;

  @override
  State<_ColourBox> createState() => _ColourBoxState();
}

class _ColourBoxState extends State<_ColourBox> {
  late final _c = TextEditingController(text: widget.value);

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final v = _c.text.trim();
    int? argb;
    String? error;
    if (v.isNotEmpty) {
      try {
        argb = parseColor(v);
      } on ThemeError catch (e) {
        error = e.message;
      }
    }
    final cs = Theme.of(context).colorScheme;
    return SizedBox(
      width: 190,
      child: TextField(
        controller: _c,
        maxLength: maxColorLen,
        decoration: InputDecoration(
          labelText: widget.field.name,
          hintText: '#RRGGBB',
          counterText: '',
          errorText: error,
          errorMaxLines: 2,
          prefixIcon: Padding(
            padding: const EdgeInsets.all(10),
            child: Container(
              width: 20,
              height: 20,
              decoration: BoxDecoration(
                color: argb == null ? null : Color(argb),
                border: Border.all(color: cs.outline),
              ),
              child: argb == null ? Icon(Icons.block, size: 14, color: cs.outline) : null,
            ),
          ),
        ),
        onChanged: (s) {
          setState(() {});
          widget.onChanged(s);
        },
      ),
    );
  }
}

/// A pair's ratio, with an icon and a word as well as a colour, so the result
/// never rests on colour alone (08-accessibility.md).
class _ContrastBadge extends StatelessWidget {
  const _ContrastBadge({required this.check});

  final ContrastCheck check;

  @override
  Widget build(BuildContext context) {
    final c = check;
    final cs = Theme.of(context).colorScheme;
    final (icon, word, colour) = c.ratio == null
        ? (Icons.remove_circle_outline, 'not set', cs.outline)
        : c.passes
            ? (Icons.check_circle, 'pass', cs.primary)
            : (Icons.cancel, 'fail', cs.error);
    final need = c.min == c.min.roundToDouble() ? c.min.toStringAsFixed(0) : '${c.min}';
    return Tooltip(
      message: '${c.fg} on ${c.bg}: needs $need:1',
      child: Chip(
        key: Key('theme_editor.contrast.${c.fg}.${c.bg}'),
        avatar: Icon(icon, size: 18, color: colour),
        label: Text('${c.fg} / ${c.bg}  ${c.shown}  $word'),
        side: BorderSide(color: colour),
      ),
    );
  }
}

class _LabelledSlider extends StatelessWidget {
  const _LabelledSlider({
    required this.label,
    required this.value,
    required this.min,
    required this.max,
    required this.divisions,
    required this.onChanged,
  });

  final String label;
  final double value, min, max;
  final int divisions;
  final ValueChanged<double> onChanged;

  @override
  Widget build(BuildContext context) => SizedBox(
        width: 260,
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text(label),
          Slider(
            value: value,
            min: min,
            max: max,
            divisions: divisions,
            label: label,
            semanticFormatterCallback: (_) => label,
            onChanged: onChanged,
          ),
        ]),
      );
}

/// The draft rendered: the widgets the portal is made of, in the draft's own
/// `ThemeData`, so a change shows at once. A palette that does not map yet (a
/// colour still being typed) says so instead.
class _Preview extends StatelessWidget {
  const _Preview({required this.theme, required this.side});

  final pb.Theme theme;
  final Brightness side;

  @override
  Widget build(BuildContext context) {
    ThemeData? data;
    String? why;
    try {
      data = themeToThemeData(theme, side);
    } catch (e) {
      why = e is ThemeError ? e.message : '$e';
    }
    return Column(
      key: const Key('theme_editor.preview'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text('Preview', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 8),
        if (data == null)
          Text('No preview until the palette is complete: $why')
        else
          Theme(data: data, child: const _Sample()),
      ],
    );
  }
}

class _Sample extends StatelessWidget {
  const _Sample();

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context);
    final cs = t.colorScheme;
    final sc = SeddonColors.of(context);
    return Material(
      color: t.scaffoldBackgroundColor,
      child: DefaultTextStyle(
        style: t.textTheme.bodyMedium!,
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            Text('Review drafts', style: t.textTheme.headlineSmall),
            const SizedBox(height: 8),
            Card(
              child: ListTile(
                leading: Icon(Icons.rate_review_outlined, color: cs.primary),
                title: const Text('randomizedcoder/xtcp2 #42'),
                subtitle: const Text('drafted · 3 findings'),
                trailing: const Chip(label: Text('drafted')),
              ),
            ),
            const SizedBox(height: 8),
            const TextField(decoration: InputDecoration(labelText: 'Repo', hintText: 'owner/name')),
            const SizedBox(height: 12),
            Wrap(spacing: 8, runSpacing: 8, children: [
              FilledButton(onPressed: () {}, child: const Text('Approve & post')),
              OutlinedButton(onPressed: () {}, child: const Text('Skip')),
              TextButton(onPressed: () {}, child: const Text('Cancel')),
            ]),
            const SizedBox(height: 12),
            Text('Body text reads like this, at the theme\'s size and weight.',
                style: t.textTheme.bodyLarge),
            Text('Secondary text is quieter.',
                style: t.textTheme.bodyMedium?.copyWith(color: cs.onSurfaceVariant)),
            const SizedBox(height: 12),
            Container(
              padding: const EdgeInsets.all(8),
              color: sc.codeBackground,
              child: Text('fn main() { println!("hello"); }',
                  style: sc.mono.copyWith(color: sc.codeForeground)),
            ),
            const SizedBox(height: 12),
            Wrap(spacing: 16, runSpacing: 8, children: [
              for (final (icon, word, colour) in [
                (Icons.check_circle_outline, 'Posted', sc.success),
                (Icons.warning_amber, 'Stale', sc.warning),
                (Icons.info_outline, 'Queued', sc.info),
                (Icons.error_outline, 'Failed', cs.error),
              ])
                Row(mainAxisSize: MainAxisSize.min, children: [
                  Icon(icon, color: colour, size: 18),
                  const SizedBox(width: 4),
                  Text(word, style: TextStyle(color: colour)),
                ]),
            ]),
          ]),
        ),
      ),
    );
  }
}

/// The paste box for Import. It owns its controller, so the controller lives
/// until the dialog's route is gone, not just until it pops.
class _ImportDialog extends StatefulWidget {
  const _ImportDialog({this.monoFamily});

  final String? monoFamily;

  @override
  State<_ImportDialog> createState() => _ImportDialogState();
}

class _ImportDialogState extends State<_ImportDialog> {
  final _controller = TextEditingController();

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
        title: const Text('Import a theme'),
        content: SizedBox(
          width: 560,
          child: TextField(
            key: const Key('theme_editor.import.text'),
            controller: _controller,
            maxLines: 14,
            style: TextStyle(fontFamily: widget.monoFamily),
            decoration: const InputDecoration(
              hintText: 'Paste a .textproto theme',
              border: OutlineInputBorder(),
            ),
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
          FilledButton(
              key: const Key('theme_editor.import.ok'),
              onPressed: () => Navigator.pop(context, _controller.text),
              child: const Text('Import')),
        ],
      );
}
