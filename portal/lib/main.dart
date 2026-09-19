import 'package:flutter/material.dart';

import 'src/clients.dart';
import 'src/config.dart';
import 'src/pages/agent_view_page.dart';
import 'src/pages/fleet_page.dart';
import 'src/pages/graph_page.dart';
import 'src/pages/launcher_page.dart';
import 'src/pages/prompts_page.dart';
import 'src/pages/router_page.dart';
import 'src/pages/settings_page.dart';

void main() {
  runApp(const AgentPortalApp());
}

/// The Agent Portal — a gRPC-only client for agent-seddon (docs/design/portal).
/// Talks to the `--serve-all` gateway (`:50100`) for everything; opens the
/// observability UIs in the browser from the Launcher.
class AgentPortalApp extends StatefulWidget {
  const AgentPortalApp({super.key});

  @override
  State<AgentPortalApp> createState() => _AgentPortalAppState();
}

class _AgentPortalAppState extends State<AgentPortalApp> {
  static const _config = PortalConfig();
  final _clients = PortalClients(_config);
  int _index = 0;

  /// Nav-rail items in display order (index-aligned with [_pages]). Each carries
  /// a muted, dull-primary tint so the rail reads at a glance — following
  /// conventions users know (Settings = grey), never bright/saturated.
  static const _navItems =
      <({String label, IconData icon, IconData selected, Color color})>[
    (
      label: 'Launch',
      icon: Icons.dashboard_outlined,
      selected: Icons.dashboard,
      color: Color(0xFF5B7BA6), // slate blue
    ),
    (
      label: 'Prompts',
      icon: Icons.edit_note_outlined,
      selected: Icons.edit_note,
      color: Color(0xFFBF9B4F), // muted amber
    ),
    (
      label: 'Graph',
      icon: Icons.account_tree_outlined,
      selected: Icons.account_tree,
      color: Color(0xFF8A72B5), // muted violet
    ),
    (
      label: 'Agent',
      icon: Icons.terminal_outlined,
      selected: Icons.terminal,
      color: Color(0xFF5E9C6B), // muted green
    ),
    (
      label: 'Router',
      icon: Icons.alt_route_outlined,
      selected: Icons.alt_route,
      color: Color(0xFF4E9AA0), // muted teal
    ),
    (
      label: 'Fleet',
      icon: Icons.rate_review_outlined,
      selected: Icons.rate_review,
      color: Color(0xFFC07A85), // muted rose
    ),
    (
      label: 'Settings',
      icon: Icons.settings_outlined,
      selected: Icons.settings,
      color: Color(0xFF8A9199), // neutral grey (the familiar Settings cue)
    ),
  ];

  @override
  void dispose() {
    _clients.shutdown();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final pages = [
      const LauncherPage(config: _config),
      PromptsPage(clients: _clients),
      GraphPage(clients: _clients),
      AgentViewPage(clients: _clients),
      RouterPage(clients: _clients),
      FleetPage(clients: _clients),
      SettingsPage(clients: _clients),
    ];
    return MaterialApp(
      // Also the browser-tab title once Flutter boots (it overwrites index.html's
      // <title>), so keep it in sync with the tab branding.
      title: 'Agent Seddon',
      theme: ThemeData(
        colorSchemeSeed: Colors.indigo,
        useMaterial3: true,
        fontFamily: 'SourceSerif4',
      ),
      darkTheme: ThemeData(
        colorSchemeSeed: Colors.indigo,
        brightness: Brightness.dark,
        useMaterial3: true,
        fontFamily: 'SourceSerif4',
      ),
      home: Scaffold(
        body: Row(
          children: [
            NavigationRail(
              selectedIndex: _index,
              onDestinationSelected: (i) => setState(() => _index = i),
              labelType: NavigationRailLabelType.all,
              leading: const Padding(
                padding: EdgeInsets.symmetric(vertical: 12),
                child: Icon(Icons.hub, color: Color(0xFF8A9199)),
              ),
              destinations: [
                for (final it in _navItems)
                  NavigationRailDestination(
                    icon: Icon(it.icon, color: it.color.withValues(alpha: 0.85)),
                    selectedIcon: Icon(it.selected, color: it.color),
                    label: Text(it.label),
                  ),
              ],
            ),
            const VerticalDivider(width: 1),
            Expanded(child: IndexedStack(index: _index, children: pages)),
          ],
        ),
      ),
    );
  }
}
