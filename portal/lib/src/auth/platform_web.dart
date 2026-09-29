import 'package:web/web.dart' as web;

import '../config.dart';
import 'auth_platform.dart';
import 'cli_login.dart';

/// Web build: the browser's location, history and per-tab `sessionStorage`
/// (cleared when the tab closes, never shared with other tabs).
AuthPlatform createAuthPlatform() => _WebAuthPlatform();

class _WebAuthPlatform implements AuthPlatform {
  @override
  Uri get currentUri => Uri.parse(web.window.location.href);

  @override
  bool get canRedirect => true;

  @override
  void navigate(String url) => web.window.location.assign(url);

  @override
  void replaceUri(Uri uri) =>
      web.window.history.replaceState(null, '', uri.toString());

  @override
  String? read(String key) => web.window.sessionStorage.getItem(key);

  @override
  void write(String key, String value) =>
      web.window.sessionStorage.setItem(key, value);

  @override
  void remove(String key) => web.window.sessionStorage.removeItem(key);
}

/// Only the native desktop signs in through the CLI.
CliLogin? createCliLogin(PortalConfig cfg) => null;
