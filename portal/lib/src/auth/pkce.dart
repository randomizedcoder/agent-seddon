import 'dart:convert';
import 'dart:math';

import 'package:crypto/crypto.dart';

/// PKCE (RFC 7636) for browser sign-in (security-hardening S13b). The portal
/// keeps the verifier and sends only its S256 challenge to `AuthService.Begin`;
/// the verifier goes to `Exchange` with the code, so a stolen code alone is
/// useless.

/// A fresh verifier: 32 bytes from a secure source, base64url without padding
/// (43 characters, inside RFC 7636's 43–128 unreserved-character range).
String newVerifier([Random? random]) {
  final rng = random ?? Random.secure();
  final bytes = List<int>.generate(32, (_) => rng.nextInt(256));
  return base64UrlNoPad(bytes);
}

/// The S256 challenge for [verifier]: base64url (no padding) of its SHA-256.
String challengeOf(String verifier) =>
    base64UrlNoPad(sha256.convert(ascii.encode(verifier)).bytes);

String base64UrlNoPad(List<int> bytes) =>
    base64Url.encode(bytes).replaceAll('=', '');
