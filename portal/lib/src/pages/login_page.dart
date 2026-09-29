import 'package:flutter/material.dart';

import '../auth/auth_state.dart';

/// The sign-in page (security-hardening S13b): one button per login issuer the
/// agent offers for browser sign-in, the reason the last attempt failed, and a
/// retry when the agent could not be asked. Shown by [AuthGate] until signed in.
class LoginPage extends StatelessWidget {
  const LoginPage({super.key, required this.auth});

  final AuthState auth;

  @override
  Widget build(BuildContext context) {
    final busy = switch (auth.phase) {
      AuthPhase.checking || AuthPhase.redirecting || AuthPhase.exchanging => true,
      _ => false,
    };
    final theme = Theme.of(context);
    return Scaffold(
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 420),
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                const Icon(Icons.hub, size: 40, color: Color(0xFF8A9199)),
                const SizedBox(height: 12),
                Text('Sign in to Agent Seddon',
                    textAlign: TextAlign.center,
                    style: theme.textTheme.headlineSmall),
                const SizedBox(height: 24),
                if (auth.expired)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 12),
                    child: Text('Your session has ended.',
                        key: const Key('login.expired'),
                        textAlign: TextAlign.center,
                        style: TextStyle(color: theme.colorScheme.tertiary)),
                  ),
                if (auth.error != null)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 12),
                    child: Text(auth.error!,
                        key: const Key('login.error'),
                        textAlign: TextAlign.center,
                        style: TextStyle(color: theme.colorScheme.error)),
                  ),
                if (busy)
                  const Center(
                      child: CircularProgressIndicator(key: Key('login.progress')))
                else if (auth.usesCliLogin) ...[
                  // Native desktop: sign-in is the CLI's (S23).
                  const Padding(
                    padding: EdgeInsets.only(bottom: 12),
                    child: Text(
                      'This app uses the agent CLI\'s sign-in. Run `agent login` '
                      'in a terminal, then try again.',
                      key: Key('login.cli.hint'),
                      textAlign: TextAlign.center,
                    ),
                  ),
                  OutlinedButton.icon(
                    key: const Key('login.retry'),
                    onPressed: auth.start,
                    icon: const Icon(Icons.refresh),
                    label: const Text('Try again'),
                  ),
                ] else if (auth.offered.isEmpty)
                  OutlinedButton.icon(
                    key: const Key('login.retry'),
                    onPressed: auth.start,
                    icon: const Icon(Icons.refresh),
                    label: const Text('Try again'),
                  )
                else
                  for (final issuer in auth.offered)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 8),
                      child: FilledButton.icon(
                        key: Key('login.issuer.${issuer.name}'),
                        onPressed: () => auth.beginSignIn(issuer.name),
                        icon: const Icon(Icons.login),
                        label: Text('Continue with ${_label(issuer.name, issuer.profile)}'),
                      ),
                    ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  static String _label(String name, String profile) => switch (profile) {
        'google' => 'Google',
        'entra' => 'Microsoft',
        _ => name,
      };
}
