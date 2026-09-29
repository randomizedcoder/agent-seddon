import 'package:flutter/material.dart';

import 'src/auth/auth_gate.dart';
import 'src/auth/auth_interceptor.dart';
import 'src/auth/auth_state.dart';
import 'src/auth/capabilities.dart';
import 'src/auth/platform_factory.dart';
import 'src/clients.dart';
import 'src/config.dart';
import 'src/pages/access_page.dart';
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
/// observability UIs in the browser from the Launcher. When the agent offers
/// browser sign-in, [AuthGate] shows the sign-in page first and every call then
/// carries the user's agent token (security-hardening S13b).
class AgentPortalApp extends StatefulWidget {
  const AgentPortalApp({super.key});

  @override
  State<AgentPortalApp> createState() => _AgentPortalAppState();
}

class _AgentPortalAppState extends State<AgentPortalApp> {
  static const _config = PortalConfig();
  late final PortalClients _clients = PortalClients(
    _config,
    interceptors: [
      AuthInterceptor(() => _auth.token, identity: () => _auth.identityHeaders),
    ],
  );
  late final AuthState _auth = AuthState(
    client: _clients.auth,
    platform: createAuthPlatform(),
    mode: parseAuthMode(_config.authMode),
    preferredIssuer: _config.authIssuer,
    redirectUriOverride: _config.redirectUri,
  );
  int _index = 0;

  @override
  void initState() {
    super.initState();
    _auth.start();
  }

  /// Nav-rail items in display order (index-aligned with the pages in [build]).
  /// Each carries a muted, dull-primary tint so the rail reads at a glance —
  /// following conventions users know (Settings = grey), never bright/saturated.
  /// `needs` is the `action:resource` a destination requires; one the signed-in
  /// user lacks is hidden (presentation only: the server enforces every call).
  static const _navItems = <({
    String label,
    IconData icon,
    IconData selected,
    Color color,
    String? needs,
  })>[
    (
      label: 'Launch',
      icon: Icons.dashboard_outlined,
      selected: Icons.dashboard,
      color: Color(0xFF5B7BA6), // slate blue
      needs: null,
    ),
    (
      label: 'Prompts',
      icon: Icons.edit_note_outlined,
      selected: Icons.edit_note,
      color: Color(0xFFBF9B4F), // muted amber
      needs: null,
    ),
    (
      label: 'Graph',
      icon: Icons.account_tree_outlined,
      selected: Icons.account_tree,
      color: Color(0xFF8A72B5), // muted violet
      needs: null,
    ),
    (
      label: 'Agent',
      icon: Icons.terminal_outlined,
      selected: Icons.terminal,
      color: Color(0xFF5E9C6B), // muted green
      needs: null,
    ),
    (
      label: 'Router',
      icon: Icons.alt_route_outlined,
      selected: Icons.alt_route,
      color: Color(0xFF4E9AA0), // muted teal
      needs: null,
    ),
    (
      label: 'Fleet',
      icon: Icons.rate_review_outlined,
      selected: Icons.rate_review,
      color: Color(0xFFC07A85), // muted rose
      needs: null,
    ),
    (
      label: 'Access',
      icon: Icons.admin_panel_settings_outlined,
      selected: Icons.admin_panel_settings,
      color: Color(0xFF9C8566), // muted bronze
      needs: 'read:binding',
    ),
    (
      label: 'Settings',
      icon: Icons.settings_outlined,
      selected: Icons.settings,
      color: Color(0xFF8A9199), // neutral grey (the familiar Settings cue)
      needs: null,
    ),
  ];

  @override
  void dispose() {
    _auth.dispose();
    _clients.shutdown();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final pages = [
      const LauncherPage(config: _config),
      PromptsPage(clients: _clients),
      GraphPage(clients: _clients),
      AgentViewPage(clients: _clients, tenant: () => _auth.tenant),
      RouterPage(clients: _clients),
      FleetPage(clients: _clients),
      AccessPage(clients: _clients),
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
      home: AuthGate(
        auth: _auth,
        builder: (context, account) => Builder(builder: (context) {
        // The destinations this user may open, from one filtered list so the
        // rail and the pages stay index-aligned.
        final caps = CapabilityScope.of(context);
        final shown = [
          for (var i = 0; i < _navItems.length; i++)
            if (_allowed(caps, _navItems[i].needs)) i,
        ];
        final index = _index.clamp(0, shown.length - 1);
        return Scaffold(
        body: Row(
          children: [
            NavigationRail(
              selectedIndex: index,
              onDestinationSelected: (i) => setState(() => _index = i),
              labelType: NavigationRailLabelType.all,
              leading: const Padding(
                padding: EdgeInsets.symmetric(vertical: 12),
                child: Icon(Icons.hub, color: Color(0xFF8A9199)),
              ),
              trailing: account == null
                  ? null
                  : Expanded(
                      child: Align(
                          alignment: Alignment.bottomCenter, child: account)),
              destinations: [
                for (final it in [for (final i in shown) _navItems[i]])
                  NavigationRailDestination(
                    icon: Icon(it.icon, color: it.color.withValues(alpha: 0.85)),
                    selectedIcon: Icon(it.selected, color: it.color),
                    label: Text(it.label),
                  ),
              ],
            ),
            const VerticalDivider(width: 1),
            Expanded(
                child: IndexedStack(
                    index: index,
                    children: [for (final i in shown) pages[i]])),
          ],
        ),
      );
      }),
      ),
    );
  }

  /// Whether [caps] allow a destination that `needs` an `action:resource`.
  static bool _allowed(Capabilities caps, String? needs) {
    if (needs == null) return true;
    final (action, resource) = switch (needs.split(':')) {
      [final a, final r] => (a, r),
      _ => ('', ''),
    };
    return caps.can(action, resource);
  }
}
