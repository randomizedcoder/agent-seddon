/// Where a Launch card points. The stack UIs' URLs are compile-time defines whose
/// default is `localhost`, which is only right in a browser on the stack's own
/// host. A loopback URL therefore follows the host the portal was opened from
/// (`http://l2:8092` ⇒ `http://l2:3000`), so a LAN browser reaches the same box
/// under whatever name it used. A non-loopback URL is an operator's explicit
/// choice and is kept. Only http(s) URLs are launched; anything else, or an
/// empty define (that UI is not deployed), yields null and the card is hidden.
Uri? obsLink(String configured, Uri page) {
  final url = Uri.tryParse(configured.trim());
  if (url == null || !(url.isScheme('http') || url.isScheme('https'))) {
    return null;
  }
  if (url.host.isEmpty) return null;
  final pageHost = page.host;
  final pageIsWeb = page.isScheme('http') || page.isScheme('https');
  if (_loopback(url.host) && pageIsWeb && pageHost.isNotEmpty) {
    return url.replace(host: pageHost);
  }
  return url;
}

bool _loopback(String host) =>
    host == 'localhost' ||
    host == '::1' ||
    host == '[::1]' ||
    host.startsWith('127.');
