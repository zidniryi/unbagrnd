//! API key generation, hashing and verification.
//!
//! The plaintext key is only ever seen once, at generation time, and is
//! never written to the database - only its SHA-256 hash is. See the
//! module docs on [`crate::auth`] for the request-time verification flow.

use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Every generated key starts with this, both so it's recognizable at a
/// glance (in logs, in a `.env` file, ...) and so a stray token from some
/// other service is never mistaken for one of ours.
pub const KEY_PREFIX: &str = "unb_live_";

/// How many random bytes make up a key's secret portion. 32 bytes = 256
/// bits of entropy, hex-encoded to 64 characters - far beyond brute-force
/// range.
const SECRET_BYTES: usize = 32;

/// How many hex characters of the secret are kept as the safe-to-display
/// `key_prefix` (e.g. `unb_live_7f83b1c9`), letting a user recognize which
/// key is which in `GET /v1/keys` without ever exposing the full secret.
const DISPLAY_PREFIX_CHARS: usize = 8;

pub struct GeneratedKey {
    /// Shown to the caller exactly once, at creation time. Never stored.
    pub plaintext: String,
    pub key_prefix: String,
    pub key_hash: String,
}

/// Generates a new, cryptographically random API key.
///
/// Uses `rand`'s thread-local CSPRNG (ChaCha-based, periodically reseeded
/// from the OS) rather than hashing a UUID/timestamp/incrementing ID, which
/// would be predictable and unsuitable as a secret.
pub fn generate_key() -> GeneratedKey {
    let mut bytes = [0u8; SECRET_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    let secret = hex::encode(bytes);

    let plaintext = format!("{KEY_PREFIX}{secret}");
    let key_prefix = format!("{KEY_PREFIX}{}", &secret[..DISPLAY_PREFIX_CHARS]);
    let key_hash = hash_key(&plaintext);

    GeneratedKey {
        plaintext,
        key_prefix,
        key_hash,
    }
}

/// Hashes a plaintext API key (whether just-generated or presented by a
/// caller on an incoming request) into its storable/comparable form.
pub fn hash_key(plaintext: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(plaintext.as_bytes());
    hex::encode(hasher.finalize())
}

/// Constant-time comparison of two key hashes, so a timing side-channel
/// can't leak how many leading hex characters of a guess matched the
/// stored hash.
pub fn hashes_match(a: &str, b: &str) -> bool {
    a.len() == b.len() && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_are_unique_and_well_formed() {
        let a = generate_key();
        let b = generate_key();

        assert!(a.plaintext.starts_with(KEY_PREFIX));
        assert_eq!(a.plaintext.len(), KEY_PREFIX.len() + SECRET_BYTES * 2);
        assert_ne!(a.plaintext, b.plaintext);
        assert_ne!(a.key_hash, b.key_hash);
        assert_eq!(a.key_hash, hash_key(&a.plaintext));
        assert!(a.plaintext.starts_with(&a.key_prefix));
    }

    #[test]
    fn hash_comparison_is_exact() {
        let key = generate_key();
        assert!(hashes_match(&key.key_hash, &hash_key(&key.plaintext)));
        assert!(!hashes_match(&key.key_hash, &hash_key("unb_live_wrong")));
    }
}
