//! Webhook signature verification — Epic 18 Slice A.
//!
//! Pure: given a key, the raw body bytes, and the signature header's value,
//! says whether it verifies. No I/O, no knowledge of endpoints or storage —
//! that belongs to the HTTP layer, which must call this *before* ever
//! parsing the body as JSON (decision 2): an unverified payload is never
//! deserialized, because parsing untrusted bytes is the attack surface.

/// Verifies an HMAC-SHA256 signature.
///
/// `header_value` is expected to be `prefix` followed by the signature,
/// hex-encoded — GitHub's `sha256=<hex>` shape, generalized so a different
/// sender's prefix is configuration, not a code change.
///
/// Constant-time: [`hmac::Mac::verify_slice`] is what makes this safe against
/// a timing attack that recovers the signature byte by byte from response
/// latency — comparing the computed and received bytes with `==` would
/// reopen exactly the hole this exists to close.
#[must_use]
pub fn verify_hmac_sha256(secret: &[u8], body: &[u8], header_value: &str, prefix: &str) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let Some(hex_signature) = header_value.strip_prefix(prefix) else {
        return false;
    };
    let Some(signature_bytes) = hex_decode(hex_signature) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&signature_bytes).is_ok()
}

/// Decodes a hex string, `None` (not a panic) on anything malformed — an
/// attacker-controlled header must never be able to crash the receiver.
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Verifies an Ed25519 signature.
///
/// `header_value` is the signature, standard base64-encoded — the shape a
/// sender using Ed25519 (rather than HMAC) is expected to send.
#[must_use]
pub fn verify_ed25519(public_key: &[u8; 32], body: &[u8], header_value: &str) -> bool {
    use base64::Engine;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let Ok(verifying_key) = VerifyingKey::from_bytes(public_key) else {
        return false;
    };
    let Ok(signature_bytes) = base64::engine::general_purpose::STANDARD.decode(header_value) else {
        return false;
    };
    let Ok(signature) = Signature::try_from(signature_bytes.as_slice()) else {
        return false;
    };
    verifying_key.verify(body, &signature).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hmac_sign(secret: &[u8], body: &[u8]) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        use std::fmt::Write;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac key");
        mac.update(body);
        let bytes = mac.finalize().into_bytes();
        bytes.iter().fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
    }

    #[test]
    fn a_correct_hmac_signature_verifies() {
        let secret = b"top-secret";
        let body = b"{\"event\":\"push\"}";
        let signature = format!("sha256={}", hmac_sign(secret, body));
        assert!(verify_hmac_sha256(secret, body, &signature, "sha256="));
    }

    #[test]
    fn a_wrong_secret_does_not_verify() {
        let body = b"{\"event\":\"push\"}";
        let signature = format!("sha256={}", hmac_sign(b"the-real-secret", body));
        assert!(!verify_hmac_sha256(
            b"a-guessed-secret",
            body,
            &signature,
            "sha256="
        ));
    }

    #[test]
    fn a_tampered_body_does_not_verify() {
        let secret = b"top-secret";
        let original = b"{\"amount\":10}";
        let signature = format!("sha256={}", hmac_sign(secret, original));
        let tampered = b"{\"amount\":99999}";
        assert!(!verify_hmac_sha256(secret, tampered, &signature, "sha256="));
    }

    #[test]
    fn a_header_missing_the_configured_prefix_does_not_verify() {
        let secret = b"top-secret";
        let body = b"{\"event\":\"push\"}";
        // Correctly signed, but under a *different* prefix than configured —
        // a sender-specific detail this function must not guess around.
        let signature = hmac_sign(secret, body);
        assert!(!verify_hmac_sha256(secret, body, &signature, "sha256="));
    }

    #[test]
    fn non_hex_after_the_prefix_does_not_verify_or_panic() {
        assert!(!verify_hmac_sha256(
            b"secret",
            b"body",
            "sha256=not-hex-at-all!!",
            "sha256="
        ));
    }

    #[test]
    fn an_empty_header_value_does_not_verify() {
        assert!(!verify_hmac_sha256(b"secret", b"body", "", "sha256="));
    }

    // ---- Ed25519 ----

    fn ed25519_keypair() -> (ed25519_dalek::SigningKey, [u8; 32]) {
        use ed25519_dalek::SigningKey;
        use rand::rngs::OsRng;
        let signing_key = SigningKey::generate(&mut OsRng);
        let public = signing_key.verifying_key().to_bytes();
        (signing_key, public)
    }

    fn ed25519_sign(signing_key: &ed25519_dalek::SigningKey, body: &[u8]) -> String {
        use base64::Engine;
        use ed25519_dalek::Signer;
        let signature = signing_key.sign(body);
        base64::engine::general_purpose::STANDARD.encode(signature.to_bytes())
    }

    #[test]
    fn a_correct_ed25519_signature_verifies() {
        let (signing_key, public_key) = ed25519_keypair();
        let body = b"{\"event\":\"push\"}";
        let signature = ed25519_sign(&signing_key, body);
        assert!(verify_ed25519(&public_key, body, &signature));
    }

    #[test]
    fn an_ed25519_signature_from_a_different_key_does_not_verify() {
        let (signing_key, _) = ed25519_keypair();
        let (_, other_public_key) = ed25519_keypair();
        let body = b"{\"event\":\"push\"}";
        let signature = ed25519_sign(&signing_key, body);
        assert!(!verify_ed25519(&other_public_key, body, &signature));
    }

    #[test]
    fn an_ed25519_tampered_body_does_not_verify() {
        let (signing_key, public_key) = ed25519_keypair();
        let signature = ed25519_sign(&signing_key, b"original");
        assert!(!verify_ed25519(&public_key, b"tampered!", &signature));
    }

    #[test]
    fn malformed_base64_does_not_verify_or_panic() {
        let (_, public_key) = ed25519_keypair();
        assert!(!verify_ed25519(&public_key, b"body", "not valid base64!!"));
    }
}
