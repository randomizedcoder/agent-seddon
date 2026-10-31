// Choosing a theme (docs/design/visual/05-portal-integration.md#choosing-a-theme):
// the Appearance section at the top of Settings and a palette button on the nav
// rail. Both are keyboard- and screen-reader-reachable in every theme, or the
// people who need the accessible theme most could not get to it.
import 'package:flutter/material.dart';

import '../gen/agent/v1/theme.pb.dart' as pb;
import 'theme_controller.dart';
import 'theme_validate.dart';

/// A palette button that opens the theme list as a menu.
class ThemeMenuButton extends StatelessWidget {
  const ThemeMenuButton({super.key});

  @override
  Widget build(BuildContext context) {
    final themes = ThemeScope.maybeOf(context);
    if (themes == null) return const SizedBox.shrink();
    return PopupMenuButton<String>(
      key: const Key('theme.menu'),
      tooltip: 'Theme',
      icon: const Icon(Icons.palette_outlined),
      initialValue: themes.active.id,
      onSelected: themes.select,
      itemBuilder: (context) => [
        for (final t in themes.themes)
          CheckedPopupMenuItem<String>(
            key: Key('theme.menu.${t.id}'),
            value: t.id,
            checked: t.id == themes.active.id,
            child: Text(t.name),
          ),
      ],
    );
  }
}

/// The Appearance section: one card per theme and a System/Light/Dark switch.
class AppearanceSection extends StatelessWidget {
  const AppearanceSection({super.key});

  @override
  Widget build(BuildContext context) {
    final themes = ThemeScope.maybeOf(context);
    if (themes == null) return const SizedBox.shrink();
    final text = Theme.of(context).textTheme;
    return Column(
      key: const Key('settings.appearance'),
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Appearance', style: text.titleLarge),
        const SizedBox(height: 8),
        Wrap(
          spacing: 12,
          runSpacing: 12,
          children: [
            for (final t in themes.themes)
              _ThemeCard(
                theme: t,
                selected: t.id == themes.active.id,
                onTap: () => themes.select(t.id),
              ),
          ],
        ),
        const SizedBox(height: 12),
        SegmentedButton<ThemeMode>(
          key: const Key('settings.theme_mode'),
          segments: const [
            ButtonSegment(
                value: ThemeMode.system,
                icon: Icon(Icons.brightness_auto_outlined),
                label: Text('System')),
            ButtonSegment(
                value: ThemeMode.light,
                icon: Icon(Icons.light_mode_outlined),
                label: Text('Light')),
            ButtonSegment(
                value: ThemeMode.dark,
                icon: Icon(Icons.dark_mode_outlined),
                label: Text('Dark')),
          ],
          selected: {themes.mode},
          onSelectionChanged: (s) => themes.setMode(s.first),
        ),
      ],
    );
  }
}

class _ThemeCard extends StatelessWidget {
  const _ThemeCard(
      {required this.theme, required this.selected, required this.onTap});

  final pb.Theme theme;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    // The card previews its own palette: the light one if it has it.
    final p = theme.hasLight() ? theme.light : theme.dark;
    final swatch = [
      for (final hex in [p.background, p.surface, p.primary, p.secondary, p.seed])
        if (hex.isNotEmpty) Color(parseColor(hex)),
    ];
    final family = theme.typography.bodyFamily;
    final whose = theme.builtin
        ? null
        : theme.visibility == pb.Visibility.VISIBILITY_TENANT
            ? 'Shared'
            : 'Private';
    return Semantics(
      button: true,
      selected: selected,
      label: whose == null ? '${theme.name} theme' : '${theme.name} theme, ${whose.toLowerCase()}',
      onTap: onTap,
      excludeSemantics: true,
      child: SizedBox(
        width: 200,
        child: Card(
          key: Key('theme.card.${theme.id}'),
          clipBehavior: Clip.antiAlias,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(8),
            side: BorderSide(
              color: selected ? scheme.primary : scheme.outlineVariant,
              width: selected ? 3 : 1,
            ),
          ),
          child: InkWell(
            onTap: onTap,
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(children: [
                    Expanded(
                        child: Text(theme.name,
                            style: Theme.of(context).textTheme.titleMedium)),
                    if (selected) Icon(Icons.check_circle, color: scheme.primary),
                  ]),
                  const SizedBox(height: 8),
                  Row(children: [
                    for (final c in swatch)
                      Expanded(
                        child: Container(
                          height: 20,
                          decoration: BoxDecoration(
                            color: c,
                            border: Border.all(color: scheme.outlineVariant),
                          ),
                        ),
                      ),
                  ]),
                  const SizedBox(height: 8),
                  Text(
                    'The quick brown fox',
                    style: TextStyle(
                        fontFamily: family.isEmpty ? null : family),
                  ),
                  // Whose theme this is: everything but a built-in is
                  // someone's in this tenant.
                  if (whose != null)
                    Text(whose, style: Theme.of(context).textTheme.labelSmall),
                  if (theme.description.isNotEmpty)
                    Text(theme.description,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                        style: Theme.of(context).textTheme.bodySmall),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
