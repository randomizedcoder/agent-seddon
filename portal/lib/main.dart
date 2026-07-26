import 'package:flutter/material.dart';

import 'src/clients.dart';
import 'src/config.dart';
import 'src/pages/agent_view_page.dart';
import 'src/pages/launcher_page.dart';
import 'src/pages/prompts_page.dart';

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
      AgentViewPage(clients: _clients),
    ];
    return MaterialApp(
      title: 'Agent Portal',
      theme: ThemeData(colorSchemeSeed: Colors.indigo, useMaterial3: true),
      darkTheme: ThemeData(
        colorSchemeSeed: Colors.indigo,
        brightness: Brightness.dark,
        useMaterial3: true,
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
                child: Icon(Icons.hub),
              ),
              destinations: const [
                NavigationRailDestination(
                  icon: Icon(Icons.dashboard_outlined),
                  selectedIcon: Icon(Icons.dashboard),
                  label: Text('Launch'),
                ),
                NavigationRailDestination(
                  icon: Icon(Icons.edit_note_outlined),
                  selectedIcon: Icon(Icons.edit_note),
                  label: Text('Prompts'),
                ),
                NavigationRailDestination(
                  icon: Icon(Icons.terminal_outlined),
                  selectedIcon: Icon(Icons.terminal),
                  label: Text('Agent'),
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
