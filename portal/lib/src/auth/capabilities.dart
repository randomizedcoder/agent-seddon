import 'package:flutter/widgets.dart';

/// What the signed-in user may do, from `WhoAmI` / `Exchange` (security-hardening
/// S13b, docs/design/security-hardening/03-rbac.md). The portal hides controls
/// the user cannot use; the server still enforces every call, so this is
/// presentation only, never a security boundary.
class Capabilities {
  const Capabilities._(this._perms, this._all);

  /// Everything shown: sign-in is off, or the token's permission list was too
  /// large to embed (`perms_ref`), so only the server can tell.
  static const all = Capabilities._(<String>{}, true);

  /// Exactly the `action:resource` pairs listed.
  factory Capabilities.of(Iterable<String> permissions) =>
      Capabilities._(permissions.toSet(), false);

  final Set<String> _perms;
  final bool _all;

  bool can(String action, String resource) =>
      _all || _perms.contains('$action:$resource');
}

/// Hands [Capabilities] down the tree. A page reads
/// `CapabilityScope.of(context).can('approve', 'review')`; with no scope above it
/// (a page mounted alone, as most widget tests do) everything is allowed.
class CapabilityScope extends InheritedWidget {
  const CapabilityScope({
    super.key,
    required this.capabilities,
    required super.child,
  });

  final Capabilities capabilities;

  static Capabilities of(BuildContext context) =>
      context
          .dependOnInheritedWidgetOfExactType<CapabilityScope>()
          ?.capabilities ??
      Capabilities.all;

  @override
  bool updateShouldNotify(CapabilityScope oldWidget) =>
      !identical(oldWidget.capabilities, capabilities);
}
