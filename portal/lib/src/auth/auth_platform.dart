/// What browser sign-in needs from its host (security-hardening S13b): the page
/// URL (the IdP's redirect lands on it), navigation, rewriting the address bar,
/// and per-tab storage. The web build uses the browser; the native build and the
/// tests use [MemoryAuthPlatform].
abstract class AuthPlatform {
  /// The page URL, including the `?code&state` an IdP redirect appends.
  Uri get currentUri;

  /// Whether [navigate] can leave the app for the IdP (web only).
  bool get canRedirect;

  /// Send the browser to [url] (the IdP's authorization URL).
  void navigate(String url);

  /// Replace the address bar without reloading (drops `?code&state`).
  void replaceUri(Uri uri);

  String? read(String key);
  void write(String key, String value);
  void remove(String key);
}

/// An in-memory [AuthPlatform]: the native desktop build (no redirect; storage
/// lasts the process) and hermetic tests (which read [navigated]).
class MemoryAuthPlatform implements AuthPlatform {
  MemoryAuthPlatform({Uri? uri, this.canRedirect = false})
      : currentUri = uri ?? Uri.parse('http://127.0.0.1:8092/');

  @override
  Uri currentUri;

  @override
  final bool canRedirect;

  /// Every URL [navigate] was asked for, in order.
  final List<String> navigated = [];

  final Map<String, String> storage = {};

  @override
  void navigate(String url) => navigated.add(url);

  @override
  void replaceUri(Uri uri) => currentUri = uri;

  @override
  String? read(String key) => storage[key];

  @override
  void write(String key, String value) => storage[key] = value;

  @override
  void remove(String key) => storage.remove(key);
}
