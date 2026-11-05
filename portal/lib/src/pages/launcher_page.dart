import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';

import '../config.dart';
import '../obs_link.dart';

/// The Launcher: cards that open the existing observability UIs in the system
/// browser. The portal never scrapes their HTML — it just points at them. Each
/// URL follows the host the portal was opened from ([obsLink]); a UI whose URL is
/// empty is not deployed and gets no card.
class LauncherPage extends StatelessWidget {
  const LauncherPage({super.key, required this.config, this.pageUri});

  final PortalConfig config;

  /// The portal's own URL; `Uri.base` unless a test pins it.
  final Uri? pageUri;

  @override
  Widget build(BuildContext context) {
    final page = pageUri ?? Uri.base;
    final links = <_Launch>[
      for (final (title, subtitle, configured, icon) in [
        ('Grafana', 'Metrics dashboards (agent-seddon)', config.grafanaUrl,
            Icons.insights),
        ('HyperDX', 'Distributed traces (ClickStack)', config.hyperdxUrl,
            Icons.account_tree),
        ('Prometheus', 'Raw metrics + queries', config.prometheusUrl,
            Icons.query_stats),
      ])
        if (obsLink(configured, page) case final url?)
          _Launch(title, subtitle, url.toString(), icon),
    ];
    return ListView(
      padding: const EdgeInsets.all(24),
      children: [
        Text('Observability', style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 4),
        Text('Open a stack UI in your browser.',
            style: Theme.of(context).textTheme.bodyMedium),
        const SizedBox(height: 16),
        for (final l in links)
          Card(
            child: ListTile(
              leading: Icon(l.icon),
              title: Text(l.title),
              subtitle: Text('${l.subtitle}\n${l.url}'),
              isThreeLine: true,
              trailing: FilledButton.tonalIcon(
                // Test key: `launch.card.<service>` (grafana|hyperdx|prometheus).
                key: Key('launch.card.${l.title.toLowerCase()}'),
                onPressed: () => _open(context, l.url),
                icon: const Icon(Icons.open_in_new),
                label: const Text('Open'),
              ),
            ),
          ),
      ],
    );
  }

  Future<void> _open(BuildContext context, String url) async {
    final ok = await launchUrl(Uri.parse(url),
        mode: LaunchMode.externalApplication);
    if (!ok && context.mounted) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text('Could not open $url')));
    }
  }
}

class _Launch {
  const _Launch(this.title, this.subtitle, this.url, this.icon);
  final String title;
  final String subtitle;
  final String url;
  final IconData icon;
}
