import 'dart:io';
import 'dart:typed_data';

import 'package:agent_portal/src/config.dart';
import 'package:agent_portal/src/gen/agent/v1/role.pbgrpc.dart';
import 'package:agent_portal/src/transport/native_tls.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../testkit/fakes/role_service.dart';
import '../testkit/recording.dart';

/// Native desktop TLS (security-hardening S24): the settings check, the file
/// loading, and real handshakes against a TLS gRPC server with a PKI made by
/// `openssl` for the run (`PORTAL_TEST_OPENSSL`, else `openssl` on PATH).
void main() {
  group('checkNativeTls', () {
    final ok = <String, (Map<String, String>, bool?)>{
      // (settings, null ⇒ plaintext | hasClientCert)
      'positive_all_empty_is_plaintext': ({}, null),
      'positive_ca_only': ({'ca': '/ca.pem'}, false),
      'positive_mtls': ({'ca': '/ca.pem', 'cert': '/c.pem', 'key': '/k.pem'}, true),
      'corner_client_cert_with_system_roots': ({'cert': '/c.pem', 'key': '/k.pem'}, true),
      'corner_server_name_ipv4': ({'ca': '/ca', 'name': '127.0.0.1'}, false),
      'corner_server_name_ipv6': ({'ca': '/ca', 'name': '::1'}, false),
      'boundary_server_name_63_char_label': ({'ca': '/ca', 'name': '${'a' * 63}.example'}, false),
    };
    ok.forEach((name, c) {
      test(name, () {
        final got = checkNativeTls(
            ca: c.$1['ca'] ?? '',
            cert: c.$1['cert'] ?? '',
            key: c.$1['key'] ?? '',
            serverName: c.$1['name'] ?? '');
        expect(got?.hasClientCert, c.$2);
      });
    });

    final refused = <String, (Map<String, String>, String)>{
      'negative_cert_without_key': ({'ca': '/ca', 'cert': '/c.pem'}, 'go together'),
      'negative_key_without_cert': ({'key': '/k.pem'}, 'go together'),
      'negative_server_name_without_tls': ({'name': 'agent'}, 'TLS is off'),
      'boundary_server_name_64_char_label': ({'ca': '/ca', 'name': '${'a' * 64}.example'}, 'DNS name'),
      'adversarial_server_name_with_port': ({'ca': '/ca', 'name': 'agent:443'}, 'DNS name'),
      'adversarial_server_name_crlf': ({'ca': '/ca', 'name': 'agent\r\nx: y'}, 'DNS name'),
      'adversarial_server_name_space': ({'ca': '/ca', 'name': 'a gent'}, 'DNS name'),
      'adversarial_server_name_wildcard': ({'ca': '/ca', 'name': '*.example'}, 'DNS name'),
    };
    refused.forEach((name, c) {
      test(name, () {
        expect(
            () => checkNativeTls(
                ca: c.$1['ca'] ?? '',
                cert: c.$1['cert'] ?? '',
                key: c.$1['key'] ?? '',
                serverName: c.$1['name'] ?? ''),
            throwsA(isA<NativeTlsError>()
                .having((e) => e.message, 'message', contains(c.$2))));
      });
    });
  });

  group('nativeCredentials', () {
    PortalConfig cfg({String ca = '', String cert = '', String key = '', String name = ''}) =>
        PortalConfig(tlsCa: ca, tlsCert: cert, tlsKey: key, tlsServerName: name);

    test('positive_plaintext_when_unset', () {
      expect(nativeCredentials(cfg()).isSecure, isFalse);
    });

    final failed = <String, (PortalConfig, Uint8List Function(String), String)>{
      'negative_missing_file_names_the_setting': (
        cfg(ca: '/nope/ca.pem'),
        readPem,
        'PORTAL_TLS_CA: cannot read `/nope/ca.pem`'
      ),
      'negative_bad_pem_is_refused': (
        cfg(ca: '/ca.pem'),
        (_) => Uint8List.fromList('not a certificate'.codeUnits),
        'do not load'
      ),
      'negative_inconsistent_settings': (cfg(cert: '/c.pem'), readPem, 'go together'),
    };
    failed.forEach((name, c) {
      test(name, () {
        final creds = nativeCredentials(c.$1, read: c.$2);
        expect(creds, isA<FailedTlsCredentials>());
        expect(creds.isSecure, isTrue, reason: 'never plaintext once TLS is asked for');
        expect(() => creds.securityContext,
            throwsA(isA<NativeTlsError>().having((e) => e.message, 'message', contains(c.$3))));
      });
    });

    test('boundary_pem_over_cap_is_refused', () async {
      final dir = await Directory.systemTemp.createTemp('pem-cap');
      addTearDown(() => dir.delete(recursive: true));
      final atCap = File('${dir.path}/at')..writeAsBytesSync(List.filled(maxPemBytes, 0x41));
      final over = File('${dir.path}/over')..writeAsBytesSync(List.filled(maxPemBytes + 1, 0x41));
      expect(readPem(atCap.path).length, maxPemBytes);
      expect(() => readPem(over.path),
          throwsA(isA<NativeTlsError>().having((e) => e.message, 'message', contains('larger than'))));
    });
  });

  group('handshake', () {
    final openssl = _openssl();
    late Pki pki;
    setUpAll(() async {
      if (openssl != null) pki = await Pki.make(openssl);
    });
    tearDownAll(() async {
      if (openssl != null) await pki.dir.delete(recursive: true);
    });

    /// Serve a fake RoleService over TLS with [leaf]; [requireClientCert] asks
    /// for a client certificate signed by the PKI's CA.
    Future<Server> serve(String leaf, {bool requireClientCert = true, bool tls = true}) async {
      final server = Server.create(services: [FakeRoleService(RecordingLog())]);
      await server.serve(
        address: InternetAddress.loopbackIPv4,
        port: 0,
        security: tls
            ? _ServerCreds(
                cert: pki.bytes('$leaf.crt'), key: pki.bytes('$leaf.key'), clientCa: pki.bytes('ca.crt'))
            : null,
        requireClientCertificate: tls && requireClientCert,
      );
      addTearDown(server.shutdown);
      return server;
    }

    Future<void> call(Server server, PortalConfig cfg) async {
      final channel = ClientChannel('127.0.0.1',
          port: server.port!, options: ChannelOptions(credentials: nativeCredentials(cfg)));
      addTearDown(channel.terminate);
      await RoleServiceClient(channel)
          .list(RoleListRequest(), options: CallOptions(timeout: const Duration(seconds: 4)));
    }

    PortalConfig mtls({String? ca, String name = ''}) => PortalConfig(
        tlsCa: ca ?? pki.path('ca.crt'),
        tlsCert: pki.path('client.crt'),
        tlsKey: pki.path('client.key'),
        tlsServerName: name);

    final skip = openssl == null && Platform.environment['PORTAL_TEST_OPENSSL'] == null
        ? 'no openssl to make a test PKI'
        : null;

    test('positive_mtls_round_trip', () async {
      await call(await serve('server'), mtls());
    }, skip: skip);

    test('positive_server_name_names_the_certificate', () async {
      await call(await serve('named'), mtls(name: 'agent.test'));
    }, skip: skip);

    test('negative_no_client_cert_refused', () async {
      final server = await serve('server');
      await expectLater(call(server, PortalConfig(tlsCa: pki.path('ca.crt'))), throwsA(isA<GrpcError>()));
    }, skip: skip);

    test('negative_plaintext_client_refused_by_tls_server', () async {
      final server = await serve('server');
      await expectLater(call(server, const PortalConfig()), throwsA(isA<GrpcError>()));
    }, skip: skip);

    test('corner_server_name_mismatch_refused', () async {
      final server = await serve('named'); // SAN agent.test only, not 127.0.0.1
      await expectLater(call(server, mtls()), throwsA(isA<GrpcError>()));
    }, skip: skip);

    test('adversarial_server_from_another_ca_refused', () async {
      final server = await serve('server');
      await expectLater(call(server, mtls(ca: pki.path('other-ca.crt'))), throwsA(isA<GrpcError>()));
    }, skip: skip);

    test('adversarial_broken_setting_never_falls_back_to_plaintext', () async {
      // A plaintext server would answer a plaintext call; the failed TLS setting
      // must not become one.
      final server = await serve('server', tls: false);
      await expectLater(
          call(server, PortalConfig(tlsCa: '${pki.dir.path}/missing.pem')),
          throwsA(isA<GrpcError>().having((e) => e.message, 'message', contains('PORTAL_TLS_CA'))));
    }, skip: skip);
  });
}

String? _openssl() {
  final env = Platform.environment['PORTAL_TEST_OPENSSL'];
  if (env != null && env.isNotEmpty) return env;
  final r = Process.runSync('sh', ['-c', 'command -v openssl']);
  final path = (r.stdout as String).trim();
  return r.exitCode == 0 && path.isNotEmpty ? path : null;
}

class _ServerCreds extends ServerTlsCredentials {
  _ServerCreds({required List<int> cert, required List<int> key, required this.clientCa})
      : super(certificate: cert, privateKey: key);

  final List<int> clientCa;

  @override
  SecurityContext get securityContext => super.securityContext
    ..setTrustedCertificatesBytes(clientCa)
    ..setClientAuthoritiesBytes(clientCa);
}

/// A throwaway PKI: `ca` signs `server` (SAN 127.0.0.1), `named` (SAN
/// agent.test only) and `client`; `other-ca` signs nothing the tests trust.
class Pki {
  Pki._(this.dir);

  final Directory dir;

  String path(String name) => '${dir.path}/$name';
  List<int> bytes(String name) => File(path(name)).readAsBytesSync();

  static Future<Pki> make(String openssl) async {
    final pki = Pki._(await Directory.systemTemp.createTemp('portal-tls'));
    Future<void> run(List<String> args) async {
      final r = await Process.run(openssl, args, workingDirectory: pki.dir.path);
      if (r.exitCode != 0) throw StateError('openssl ${args.first}: ${r.stderr}');
    }

    const ec = ['-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256', '-nodes'];
    for (final ca in ['ca', 'other-ca']) {
      await run(['req', '-x509', ...ec, '-keyout', '$ca.key', '-out', '$ca.crt', '-days', '1', '-subj', '/CN=$ca']);
    }
    final leaves = {
      'server': 'subjectAltName=IP:127.0.0.1\nextendedKeyUsage=serverAuth\n',
      'named': 'subjectAltName=DNS:agent.test\nextendedKeyUsage=serverAuth\n',
      'client': 'extendedKeyUsage=clientAuth\n',
    };
    for (final e in leaves.entries) {
      File(pki.path('${e.key}.ext')).writeAsStringSync(e.value);
      await run(['req', ...ec, '-keyout', '${e.key}.key', '-out', '${e.key}.csr', '-subj', '/CN=${e.key}']);
      await run([
        'x509', '-req', '-in', '${e.key}.csr', '-CA', 'ca.crt', '-CAkey', 'ca.key', '-CAcreateserial',
        '-out', '${e.key}.crt', '-days', '1', '-extfile', '${e.key}.ext',
      ]);
    }
    return pki;
  }
}
