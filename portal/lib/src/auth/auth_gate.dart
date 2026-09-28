import 'package:flutter/material.dart';

import '../pages/login_page.dart';
import 'auth_state.dart';
import 'capabilities.dart';

/// Shows [LoginPage] until [auth] is signed in (or sign-in is off), then the
/// app, with the user's [Capabilities] in scope and an account strip (who, and
/// sign out) for [builder] to place.
class AuthGate extends StatelessWidget {
  const AuthGate({super.key, required this.auth, required this.builder});

  final AuthState auth;

  /// Builds the signed-in app; `account` is null when sign-in is off.
  final Widget Function(BuildContext context, Widget? account) builder;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: auth,
      builder: (context, _) {
        switch (auth.phase) {
          case AuthPhase.off:
            return builder(context, null);
          case AuthPhase.signedIn:
            return CapabilityScope(
              capabilities: auth.capabilities,
              child: builder(context, AccountStrip(auth: auth)),
            );
          default:
            return LoginPage(auth: auth);
        }
      },
    );
  }
}

/// Who is signed in, and a sign-out button — for the navigation rail's foot.
class AccountStrip extends StatelessWidget {
  const AccountStrip({super.key, required this.auth});

  final AuthState auth;

  @override
  Widget build(BuildContext context) {
    final p = auth.session?.principal;
    final who = (p == null)
        ? ''
        : (p.email.isNotEmpty ? p.email : p.subject);
    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Tooltip(
            message: '$who\n${p?.tenant ?? ''}',
            child: const Icon(Icons.account_circle,
                key: Key('login.account'), color: Color(0xFF8A9199)),
          ),
          IconButton(
            key: const Key('login.signout'),
            tooltip: 'Sign out',
            icon: const Icon(Icons.logout),
            onPressed: auth.signOut,
          ),
        ],
      ),
    );
  }
}
