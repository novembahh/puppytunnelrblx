use crate::net::relay::Token;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct Identity {
    key: SigningKey,
}

impl Identity {
    pub fn ephemeral() -> Self {
        Self { key: SigningKey::generate(&mut rand::rngs::OsRng) }
    }

    pub fn stable(secret_hex: &str, port: u16) -> Self {
        Self::derive(b"puppy-identity", secret_hex, &port.to_be_bytes())
    }

    pub fn profile(secret_hex: &str) -> Self {
        Self::derive(b"puppy-profile-identity", secret_hex, &[])
    }

    fn derive(label: &[u8], secret_hex: &str, extra: &[u8]) -> Self {
        let mut h = Sha256::new();
        h.update(label);
        h.update(secret_hex.as_bytes());
        h.update(extra);
        let seed: [u8; 32] = h.finalize().into();
        Self { key: SigningKey::from_bytes(&seed) }
    }

    pub fn public(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    pub fn token(&self) -> Token {
        token_of(&self.public())
    }

    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.key.sign(msg).to_bytes()
    }
}

pub fn token_of(pk: &[u8; 32]) -> Token {
    let d = Sha256::digest(pk);
    let mut t = [0u8; 16];
    t.copy_from_slice(&d[..16]);
    t
}

pub fn verify(pk: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> bool {
    VerifyingKey::from_bytes(pk)
        .map(|vk| vk.verify(msg, &Signature::from_bytes(sig)).is_ok())
        .unwrap_or(false)
}

pub fn new_secret() -> String {
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex(&b)
}

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

pub fn reg_message(ts: u64) -> Vec<u8> {
    let mut m = b"puppy-reg\0".to_vec();
    m.extend_from_slice(&ts.to_be_bytes());
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_identity_is_per_port_and_signs() {
        let a = Identity::stable("secret", 55555);
        assert_eq!(a.public(), Identity::stable("secret", 55555).public());
        assert_ne!(a.public(), Identity::stable("secret", 55556).public());
        assert_eq!(a.token(), token_of(&a.public()));
        let sig = a.sign(b"hello");
        assert!(verify(&a.public(), b"hello", &sig));
        assert!(!verify(&a.public(), b"hellO", &sig));
        assert_eq!(unhex::<4>("0a0b0c0d"), Some([10, 11, 12, 13]));
    }
}
