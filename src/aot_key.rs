//! CLI-layer signing keys for AoT artifacts (#199).
//!
//! Key handling lives in the CLI, not the runtime crate: a key is a
//! file whose bytes are the HMAC-SHA256 key for the #198 container's
//! embedder-pluggable signature slot. HMAC-SHA256 is a symmetric
//! scheme — the same file signs (`compile --aot --sign-key`) and
//! verifies (`--verify-key`) — which makes it a developer/CI scheme,
//! not a distribution scheme; the container doc names ed25519 as the
//! suggested default for embedders that need asymmetric trust.
//!
//! The key id stored in the artifact is derived from the key
//! material (`hmac-sha256:<first 8 bytes of SHA-256(key), hex>`), so
//! the same key file always names the same id and the id alone does
//! not disclose the key.

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

/// An HMAC-SHA256 key loaded from a file, usable as both the #198
/// [`telomere::AotSigner`] and [`telomere::AotVerifier`].
pub struct HmacSha256Key {
    key_id: String,
    key: Vec<u8>,
}

impl HmacSha256Key {
    /// Loads a key from `path`. The file's raw bytes are the key;
    /// trailing newlines are significant, so generate key files
    /// without them (for example `head -c 32 /dev/urandom`).
    pub fn load(path: &Path) -> Result<Self> {
        let key = std::fs::read(path)
            .with_context(|| format!("failed to read key file `{}`", path.display()))?;
        if key.is_empty() {
            return Err(anyhow!("key file `{}` is empty", path.display()));
        }
        let digest = Sha256::digest(&key);
        let mut key_id = String::from("hmac-sha256:");
        for byte in &digest[..8] {
            key_id.push_str(&format!("{byte:02x}"));
        }
        Ok(Self { key_id, key })
    }

    /// The key id stored in artifacts signed with this key.
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    fn hmac(&self, message: &[u8]) -> Vec<u8> {
        let mut key_block = [0u8; 64];
        if self.key.len() > 64 {
            key_block[..32].copy_from_slice(&Sha256::digest(&self.key));
        } else {
            key_block[..self.key.len()].copy_from_slice(&self.key);
        }
        let mut inner = Sha256::new();
        inner.update(key_block.map(|b| b ^ 0x36));
        inner.update(message);
        let inner_hash = inner.finalize();
        let mut outer = Sha256::new();
        outer.update(key_block.map(|b| b ^ 0x5c));
        outer.update(inner_hash);
        outer.finalize().to_vec()
    }
}

impl telomere::AotSigner for HmacSha256Key {
    fn key_id(&self) -> &str {
        &self.key_id
    }

    fn sign(&self, payload: &[u8]) -> Vec<u8> {
        self.hmac(payload)
    }
}

impl telomere::AotVerifier for HmacSha256Key {
    fn key_id(&self) -> &str {
        &self.key_id
    }

    fn verify(&self, payload: &[u8], signature: &[u8]) -> bool {
        self.hmac(payload) == signature
    }
}
