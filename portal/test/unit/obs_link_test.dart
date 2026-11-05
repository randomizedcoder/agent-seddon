import 'package:agent_portal/src/obs_link.dart';
import 'package:flutter_test/flutter_test.dart';

/// L0 table for the Launch cards' URL: a loopback define follows the page's
/// host, an explicit host is kept, and anything not http(s) is never launched.
void main() {
  final l2 = Uri.parse('http://l2:8092/#/launch');
  for (final (name, configured, page, want) in [
    ('positive_localhost_follows_page', 'http://localhost:3000', l2, 'http://l2:3000'),
    ('positive_ipv4_loopback_follows_page', 'http://127.0.0.1:9090', l2, 'http://l2:9090'),
    ('positive_explicit_host_kept', 'http://grafana.lab:3000', l2, 'http://grafana.lab:3000'),
    ('positive_path_and_port_kept', 'http://localhost:3000/d/agent-seddon?x=1', l2,
        'http://l2:3000/d/agent-seddon?x=1'),
    ('positive_page_ip_followed', 'http://localhost:8080', Uri.parse('http://172.16.50.46:8092/'),
        'http://172.16.50.46:8080'),
    ('negative_empty_hidden', '', l2, null),
    ('negative_whitespace_hidden', '   ', l2, null),
    ('corner_page_on_localhost_unchanged', 'http://localhost:3000',
        Uri.parse('http://localhost:8092/'), 'http://localhost:3000'),
    ('corner_non_web_page_unchanged', 'http://localhost:3000', Uri.parse('file:///tmp/x'),
        'http://localhost:3000'),
    ('boundary_https_kept', 'https://localhost:3000', l2, 'https://l2:3000'),
    ('boundary_loopback_range', 'http://127.1.2.3:3000', l2, 'http://l2:3000'),
    ('positive_any_address_follows_page', 'http://0.0.0.0:3000', l2, 'http://l2:3000'),
    ('positive_ipv6_loopback_follows_page', 'http://[::1]:3000', l2, 'http://l2:3000'),
    ('corner_ipv6_page_host_bracketed', 'http://localhost:3000',
        Uri.parse('http://[fd00::46]:8092/'), 'http://[fd00::46]:3000'),
    ('adversarial_javascript_scheme', 'javascript:alert(1)', l2, null),
    ('adversarial_file_scheme', 'file:///etc/passwd', l2, null),
    ('adversarial_no_host', 'http:///x', l2, null),
    ('adversarial_lookalike_not_loopback', 'http://localhost.evil.example:3000', l2,
        'http://localhost.evil.example:3000'),
    ('adversarial_127_name_lookalike', 'http://127.evil.example:3000', l2,
        'http://127.evil.example:3000'),
    ('adversarial_userinfo_lookalike', 'http://localhost@evil.example:3000', l2,
        'http://localhost@evil.example:3000'),
  ]) {
    test(name, () {
      expect(obsLink(configured, page)?.toString(), want);
    });
  }
}
