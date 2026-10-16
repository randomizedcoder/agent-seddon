//! Webhook signature verification (forge-auth F3).
//!
//! GitHub signs each webhook delivery with an HMAC-SHA256 over the raw request body,
//! keyed by the App's webhook secret, and sends it in the `X-Hub-Signature-256` header
//! as `sha256=<64 lowercase hex>`. The delivery endpoint is **attacker-reachable** —
//! anyone can POST it — so the payload is untrusted input: the receiver (forge-auth F6)
//! must verify this signature and **drop** anything that fails *before* it acts on the
//! body (per `docs/design/forge-auth/README.md`).
//!
//! This module is the pure, self-contained verification primitive that F6 will call.
//! It lives here, beside the installation-token minter, because both are the GitHub App
//! bootstrap secret's concern. The comparison is **constant-time** (`ring::hmac::verify`
//! recomputes the tag and compares without an early-exit), so a forged signature leaks
//! no timing signal about the real tag.

use ring::hmac;

/// Verify a GitHub `X-Hub-Signature-256` header against the raw request body.
///
/// `header` is the full header value, i.e. `sha256=<hex>`. Returns `true` iff
/// HMAC-SHA256(`secret`, `body`) equals the supplied tag, compared in constant time.
/// **Fails closed** (`false`) on any malformed header — missing/empty, a non-`sha256=`
/// algorithm prefix, non-hex characters, or a tag that is not exactly 32 bytes (64 hex
/// chars), and on an **empty `secret`** — an empty HMAC key authenticates anyone, so an
/// unconfigured secret is a bypass, never a valid signer. The prefix-strip and hex-decode
/// branch only on the attacker-supplied header, never on the secret, so they add no
/// secret-dependent timing.
///
/// `secret` is the raw webhook-secret bytes (read from an `env:`/`file:` reference, never
/// inline); an empty secret is rejected here as well as treated as "verification off" by
/// the config layer.
pub fn verify_webhook_signature(secret: &[u8], body: &[u8], header: &str) -> bool {
    // Fail closed on an empty key: HMAC with an empty secret is forgeable by anyone
    // (there is no secret to not know), so an unconfigured/misconfigured secret must
    // NEVER authenticate a delivery — it is a bypass, not a valid signer. The config
    // layer already treats an empty `webhook_secret_ref` as "verification off"; this is
    // the matching guard in the primitive so caller discipline can't reopen the hole.
    if secret.is_empty() {
        return false;
    }
    let Some(hex) = header.strip_prefix("sha256=") else {
        return false;
    };
    let Some(expected) = decode_hex_32(hex) else {
        return false;
    };
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    hmac::verify(&key, body, &expected).is_ok()
}

/// Decode exactly 32 bytes (64 hex chars) or return `None`. A wrong length, odd length,
/// or any non-hex byte is rejected — the full tag must be present and well-formed.
fn decode_hex_32(s: &str) -> Option<[u8; 32]> {
    let b = s.as_bytes();
    if b.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        let hi = hex_nibble(b[2 * i])?;
        let lo = hex_nibble(b[2 * i + 1])?;
        *slot = (hi << 4) | lo;
    }
    Some(out)
}

/// One hex digit → its 4-bit value; `None` for any non-hex byte. Upper-case is accepted
/// (GitHub sends lower-case, but a leniently-cased valid tag should still verify).
fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"It's a Secret to Everybody";
    const BODY: &[u8] = b"Hello, World!";

    /// Compute the header a correct signer would send: `sha256=` + lower-case hex of
    /// HMAC-SHA256(secret, body).
    fn sign(secret: &[u8], body: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
        let tag = hmac::sign(&key, body);
        let mut out = String::from("sha256=");
        for byte in tag.as_ref() {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
        out
    }

    /// `verify_webhook_signature` is the attacker-reachable webhook gate, so it is
    /// table-tested across the four classes plus the mandatory `adversarial_` set: every
    /// malformed header must fail closed, and only the exact HMAC over the exact body
    /// under the exact secret may pass.
    #[rstest::rstest]
    #[case::positive_valid_signature(SECRET, BODY, sign(SECRET, BODY), true)]
    #[case::positive_uppercase_hex_accepted(SECRET, BODY, sign(SECRET, BODY).to_uppercase().replace("SHA256=", "sha256="), true)]
    #[case::boundary_empty_body_valid(SECRET, b"", sign(SECRET, b""), true)]
    #[case::adversarial_empty_secret_fails_closed(b"", BODY, sign(b"", BODY), false)]
    #[case::negative_wrong_secret(SECRET, BODY, sign(b"wrong-secret", BODY), false)]
    #[case::negative_tampered_body(SECRET, b"tampered", sign(SECRET, BODY), false)]
    #[case::adversarial_missing_prefix(SECRET, BODY, sign(SECRET, BODY).trim_start_matches("sha256=").to_string(), false)]
    #[case::adversarial_wrong_algo_prefix(SECRET, BODY, format!("sha1={}", "a".repeat(40)), false)]
    #[case::adversarial_non_hex_chars(SECRET, BODY, format!("sha256={}", "z".repeat(64)), false)]
    #[case::adversarial_odd_length_hex(SECRET, BODY, format!("sha256={}", "a".repeat(63)), false)]
    #[case::adversarial_overlong_hex(SECRET, BODY, format!("sha256={}", "a".repeat(128)), false)]
    #[case::adversarial_empty_header(SECRET, BODY, String::new(), false)]
    #[case::adversarial_crlf_in_header(SECRET, BODY, format!("{}\r\nX-Injected: 1", sign(SECRET, BODY)), false)]
    fn verify_cases(
        #[case] secret: &[u8],
        #[case] body: &[u8],
        #[case] header: String,
        #[case] want: bool,
    ) {
        assert_eq!(verify_webhook_signature(secret, body, &header), want);
    }
}
