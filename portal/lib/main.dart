import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show rootBundle;

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
import 'src/pages/forges_page.dart';
import 'src/pages/graph_page.dart';
import 'src/pages/launcher_page.dart';
import 'src/pages/prompts_page.dart';
import 'src/pages/router_page.dart';
import 'src/pages/settings_page.dart';
import 'src/theme/seddon_colors.dart';
import 'src/theme/theme_controller.dart';
import 'src/theme/theme_picker.dart';

Future<void> main() async {
  // Build the accessibility tree from the start, not after the user finds
  // Flutter's hidden "Enable accessibility" button (visual/08-accessibility.md).
  WidgetsFlutterBinding.ensureInitialized().ensureSemantics();
  final themes = await ThemeController.load(rootBundle);
  runApp(AgentPortalApp(themes: themes));
}

/// The Agent Portal — a gRPC-only client for agent-seddon (docs/design/portal).
/// Talks to the `--serve-all` gateway (`:50100`) for everything; opens the
/// observability UIs in the browser from the Launcher. When the agent offers
/// browser sign-in, [AuthGate] shows the sign-in page first and every call then
/// carries the user's agent token (security-hardening S13b).
class AgentPortalApp extends StatefulWidget {
  const AgentPortalApp({super.key, required this.themes});

  /// The active theme and mode (docs/design/visual).
  final ThemeController themes;

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
    cliLogin: createCliLogin(_config),
  );
  int _index = 0;

  /// A GitHub App install return is waiting: open on the Forges tab, which
  /// relays it. `start()` sets the return aside before its first await, so this
  /// is known right after it is called.
  bool _openForges = false;

  @override
  void initState() {
    super.initState();
    _auth.start();
    _openForges = _auth.hasInstallCallback;
  }

  /// Nav-rail items in display order (index-aligned with the pages in [build]).
  /// Each is tinted by the theme's `nav_tint[id]` (prototype keeps the original
  /// muted tints; a theme without one uses its primary colour).
  /// `needs` is the `action:resource` a destination requires; one the signed-in
  /// user lacks is hidden (presentation only: the server enforces every call).
  static const _navItems = <({
    String label,
    IconData icon,
    IconData selected,
    String id,
    String? needs,
  })>[
    (
      label: 'Launch',
      id: 'launch',
      icon: Icons.dashboard_outlined,
      selected: Icons.dashboard,
      needs: null,
    ),
    (
      label: 'Prompts',
      id: 'prompts',
      icon: Icons.edit_note_outlined,
      selected: Icons.edit_note,
      needs: null,
    ),
    (
      label: 'Graph',
      id: 'graph',
      icon: Icons.account_tree_outlined,
      selected: Icons.account_tree,
      needs: null,
    ),
    (
      label: 'Agent',
      id: 'agent',
      icon: Icons.terminal_outlined,
      selected: Icons.terminal,
      needs: null,
    ),
    (
      label: 'Router',
      id: 'router',
      icon: Icons.alt_route_outlined,
      selected: Icons.alt_route,
      needs: null,
    ),
    (
      label: 'Fleet',
      id: 'fleet',
      icon: Icons.rate_review_outlined,
      selected: Icons.rate_review,
      needs: null,
    ),
    (
      label: 'Forges',
      id: 'forges',
      icon: Icons.cloud_outlined,
      selected: Icons.cloud,
      needs: 'read:forge_registry',
    ),
    (
      label: 'Access',
      id: 'access',
      icon: Icons.admin_panel_settings_outlined,
      selected: Icons.admin_panel_settings,
      needs: 'read:binding',
    ),
    (
      label: 'Settings',
      id: 'settings',
      icon: Icons.settings_outlined,
      selected: Icons.settings,
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
      FleetPage(clients: _clients, tenant: () => _auth.tenant),
      ForgesPage(clients: _clients, platform: _auth.platform, now: _auth.nowSecs),
      AccessPage(clients: _clients),
      SettingsPage(clients: _clients),
    ];
    final themes = widget.themes;
    return ThemeScope(
      controller: themes,
      child: ListenableBuilder(
      listenable: themes,
      builder: (context, _) => MaterialApp(
      // Also the browser-tab title once Flutter boots (it overwrites index.html's
      // <title>), so keep it in sync with the tab branding.
      title: 'Agent Seddon',
      theme: themes.light,
      darkTheme: themes.dark,
      highContrastTheme: themes.highContrastLight,
      highContrastDarkTheme: themes.highContrastDark,
      themeMode: themes.mode,
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
        if (_openForges) {
          _openForges = false;
          final at = shown.indexWhere((i) => _navItems[i].label == 'Forges');
          if (at >= 0) _index = at;
        }
        final index = _index.clamp(0, shown.length - 1);
        final scheme = Theme.of(context).colorScheme;
        final seddon = SeddonColors.of(context);
        return Scaffold(
        body: Row(
          children: [
            // Its own traversal group, so Tab walks every tab before the page:
            // without it reading order interleaves rail items with page
            // buttons that share a row.
            FocusTraversalGroup(
             child: NavigationRail(
              selectedIndex: index,
              onDestinationSelected: (i) => setState(() => _index = i),
              labelType: NavigationRailLabelType.all,
              leading: Padding(
                padding: const EdgeInsets.symmetric(vertical: 12),
                child: Icon(Icons.hub,
                    color: seddon.nav('settings', scheme.onSurfaceVariant)),
              ),
              trailing: Expanded(
                child: Align(
                  alignment: Alignment.bottomCenter,
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      const ThemeMenuButton(),
                      ...[account].nonNulls,
                    ],
                  ),
                ),
              ),
              destinations: [
                for (final it in [for (final i in shown) _navItems[i]])
                  NavigationRailDestination(
                    icon: Icon(it.icon,
                        color: seddon
                            .nav(it.id, scheme.primary)
                            .withValues(alpha: 0.85)),
                    selectedIcon:
                        Icon(it.selected, color: seddon.nav(it.id, scheme.primary)),
                    label: Text(it.label),
                  ),
              ],
             ),
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
      ),
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
