import 'dart:io';
import 'dart:typed_data';

import 'package:grpc/grpc.dart';

import '../config.dart';

/// TLS for the native desktop channels (security-hardening S24).
///
/// The native build dials the agent directly over raw gRPC, so it needs its own
/// TLS: the agent's CA, and a client certificate when the agent's listener asks
/// for one (`[grpc.tls] client_ca`). The files are PEM, named by `PORTAL_TLS_*`
/// defines (see [PortalConfig]), and read once when the channels are made.
///
/// A setting that is wrong never falls back to plaintext: the channel is made
/// with [FailedTlsCredentials], so every call fails and says why.

/// Most bytes read from one PEM file.
const maxPemBytes = 256 * 1024;

// grpc-dart's own ALPN list (`supportedAlpnProtocols`, not exported).
const _alpn = ['grpc-exp', 'h2'];

// A DNS name (RFC 1123 labels); IP literals are checked with InternetAddress.
final _dnsName = RegExp(
    r'^(?=.{1,253}$)[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?(\.[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*$');

class NativeTlsError implements Exception {
  const NativeTlsError(this.message);

  final String message;

  @override
  String toString() => message;
}

/// The TLS files and server name the settings ask for.
class NativeTlsFiles {
  const NativeTlsFiles(
      {this.ca = '', this.cert = '', this.key = '', this.serverName = ''});

  final String ca;
  final String cert;
  final String key;
  final String serverName;

  bool get hasClientCert => cert.isNotEmpty;
}

/// Check the settings without reading any file: `null` ⇒ plaintext (all empty).
/// Throws [NativeTlsError] when they are inconsistent.
NativeTlsFiles? checkNativeTls({
  String ca = '',
  String cert = '',
  String key = '',
  String serverName = '',
}) {
  if (ca.isEmpty && cert.isEmpty && key.isEmpty) {
    if (serverName.isNotEmpty) {
      throw const NativeTlsError(
          'PORTAL_TLS_SERVER_NAME is set but TLS is off: set PORTAL_TLS_CA '
          '(or PORTAL_TLS_CERT and PORTAL_TLS_KEY).');
    }
    return null;
  }
  if (cert.isEmpty != key.isEmpty) {
    throw const NativeTlsError(
        'PORTAL_TLS_CERT and PORTAL_TLS_KEY go together: set both or neither.');
  }
  if (serverName.isNotEmpty &&
      !_dnsName.hasMatch(serverName) &&
      InternetAddress.tryParse(serverName) == null) {
    throw const NativeTlsError(
        'PORTAL_TLS_SERVER_NAME must be a DNS name or an IP address, with no port.');
  }
  return NativeTlsFiles(ca: ca, cert: cert, key: key, serverName: serverName);
}

/// Read a PEM file, refusing one over [maxPemBytes].
Uint8List readPem(String path) {
  final f = File(path);
  final RandomAccessFile raf;
  try {
    raf = f.openSync();
  } on FileSystemException catch (e) {
    throw NativeTlsError('cannot read `$path`: ${e.osError?.message ?? e.message}');
  }
  try {
    final bytes = raf.readSync(maxPemBytes + 1);
    if (bytes.length > maxPemBytes) {
      throw NativeTlsError('`$path` is larger than $maxPemBytes bytes');
    }
    return bytes;
  } finally {
    raf.closeSync();
  }
}

/// TLS from PEM bytes: trust [ca] (the system roots when empty) and present
/// [cert]/[key] when given. A new context per connection, as grpc-dart expects.
class PemTlsCredentials extends ChannelCredentials {
  PemTlsCredentials({
    required this.ca,
    required this.cert,
    required this.key,
    String? serverName,
  }) : super.secure(authority: serverName);

  final Uint8List? ca;
  final Uint8List? cert;
  final Uint8List? key;

  @override
  SecurityContext get securityContext {
    final ctx = SecurityContext(withTrustedRoots: ca == null);
    if (ca != null) ctx.setTrustedCertificatesBytes(ca!);
    if (cert != null) ctx.useCertificateChainBytes(cert!);
    if (key != null) ctx.usePrivateKeyBytes(key!);
    ctx.setAlpnProtocols(_alpn, false);
    return ctx;
  }
}

/// Credentials that fail every connection with [error]: TLS was asked for but
/// cannot be set up, and plaintext would send the user's token in the clear.
class FailedTlsCredentials extends ChannelCredentials {
  const FailedTlsCredentials(this.error) : super.secure();

  final NativeTlsError error;

  @override
  SecurityContext get securityContext => throw error;
}

/// The credentials for the native channels from [cfg]'s `PORTAL_TLS_*`
/// settings: plaintext when none is set, [PemTlsCredentials] when the files
/// load, and [FailedTlsCredentials] otherwise.
ChannelCredentials nativeCredentials(PortalConfig cfg,
    {Uint8List Function(String path) read = readPem}) {
  try {
    final files = checkNativeTls(
      ca: cfg.tlsCa,
      cert: cfg.tlsCert,
      key: cfg.tlsKey,
      serverName: cfg.tlsServerName,
    );
    if (files == null) return const ChannelCredentials.insecure();
    Uint8List? load(String setting, String path) {
      if (path.isEmpty) return null;
      try {
        return read(path);
      } on NativeTlsError catch (e) {
        throw NativeTlsError('$setting: ${e.message}');
      }
    }

    final creds = PemTlsCredentials(
      ca: load('PORTAL_TLS_CA', files.ca),
      cert: load('PORTAL_TLS_CERT', files.cert),
      key: load('PORTAL_TLS_KEY', files.key),
      serverName: files.serverName.isEmpty ? null : files.serverName,
    );
    // Parse the PEM now, so a bad file is named here rather than as a
    // connection error on the first call.
    try {
      creds.securityContext;
    } on TlsException catch (e) {
      throw NativeTlsError('PORTAL_TLS_* files do not load: ${e.message}');
    }
    return creds;
  } on NativeTlsError catch (e) {
    return FailedTlsCredentials(e);
  }
}
