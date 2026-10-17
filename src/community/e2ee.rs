use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

pub const E2EE_VERSION: u8 = 1;
pub const E2EE_TAG: &str = "\"e2ee\":true";

#[derive(Debug, Clone)]
pub enum E2eeError {
    InvalidKey(String),
    EncryptionFailed(String),
    DecryptionFailed(String),
    MalformedPayload(String),
}

impl fmt::Display for E2eeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey(e) => write!(f, "Invalid cryptographic key: {}", e),
            Self::EncryptionFailed(e) => write!(f, "Encryption failed: {}", e),
            Self::DecryptionFailed(e) => write!(f, "Decryption failed: {}", e),
            Self::MalformedPayload(e) => write!(f, "Malformed encrypted payload: {}", e),
        }
    }
}

impl std::error::Error for E2eeError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedEnvelope {
    pub e2ee: bool,
    pub v: u8,
    pub sender_x25519: String,
    pub nonce: String,
    pub ciphertext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupKeyDistribution {
    pub group_id: String,
    pub key_version: u32,
    pub created_at: u64,
    pub sender_x25519: String,
    pub wrapped_keys: HashMap<String, String>,
}

pub fn derive_x25519_secret(secret_hex: &str) -> StaticSecret {
    let mut hasher = Sha256::new();
    hasher.update(b"puppytunnel-e2ee-x25519-v1");
    hasher.update(secret_hex.as_bytes());
    let seed: [u8; 32] = hasher.finalize().into();
    StaticSecret::from(seed)
}

pub fn get_x25519_public(secret_hex: &str) -> X25519PublicKey {
    let secret = derive_x25519_secret(secret_hex);
    X25519PublicKey::from(&secret)
}

pub fn get_x25519_public_hex(secret_hex: &str) -> String {
    let pk = get_x25519_public(secret_hex);
    hex_encode(pk.as_bytes())
}

pub fn parse_x25519_public(key_hex: &str) -> Result<X25519PublicKey, E2eeError> {
    let bytes = hex_decode(key_hex)
        .ok_or_else(|| E2eeError::InvalidKey("Invalid hex public key".into()))?;
    if bytes.len() != 32 {
        return Err(E2eeError::InvalidKey(format!(
            "Public key must be 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(X25519PublicKey::from(arr))
}

fn derive_shared_aead_key(
    my_secret: &StaticSecret,
    their_public: &X25519PublicKey,
    context: &[u8],
) -> [u8; 32] {
    let shared_secret = my_secret.diffie_hellman(their_public);
    let mut hasher = Sha256::new();
    hasher.update(b"puppytunnel-hkdf-chacha20-v1:");
    hasher.update(context);
    hasher.update(shared_secret.as_bytes());
    hasher.finalize().into()
}

pub fn encrypt_direct_message(
    my_secret_hex: &str,
    recipient_x25519_hex: &str,
    plaintext: &str,
) -> Result<String, E2eeError> {
    let my_secret = derive_x25519_secret(my_secret_hex);
    let my_pub = X25519PublicKey::from(&my_secret);
    let their_pub = parse_x25519_public(recipient_x25519_hex)?;

    let aead_key = derive_shared_aead_key(&my_secret, &their_pub, b"dm:1to1");
    let cipher = ChaCha20Poly1305::new_from_slice(&aead_key)
        .map_err(|e| E2eeError::EncryptionFailed(e.to_string()))?;

    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| E2eeError::EncryptionFailed(e.to_string()))?;

    let envelope = EncryptedEnvelope {
        e2ee: true,
        v: E2EE_VERSION,
        sender_x25519: hex_encode(my_pub.as_bytes()),
        nonce: hex_encode(&nonce_bytes),
        ciphertext: hex_encode(&ciphertext),
    };

    serde_json::to_string(&envelope)
        .map_err(|e| E2eeError::EncryptionFailed(format!("Failed to serialize envelope: {}", e)))
}

pub fn decrypt_direct_message(
    my_secret_hex: &str,
    expected_sender_x25519_hex: &str,
    payload_or_plaintext: &str,
) -> Result<String, E2eeError> {
    if !is_encrypted_payload(payload_or_plaintext) {
        return Ok(payload_or_plaintext.to_string());
    }

    let envelope: EncryptedEnvelope = serde_json::from_str(payload_or_plaintext)
        .map_err(|e| E2eeError::MalformedPayload(e.to_string()))?;

    let my_secret = derive_x25519_secret(my_secret_hex);
    let sender_hex = if !envelope.sender_x25519.is_empty() {
        &envelope.sender_x25519
    } else {
        expected_sender_x25519_hex
    };

    let their_pub = parse_x25519_public(sender_hex)?;
    let aead_key = derive_shared_aead_key(&my_secret, &their_pub, b"dm:1to1");
    let cipher = ChaCha20Poly1305::new_from_slice(&aead_key)
        .map_err(|e| E2eeError::DecryptionFailed(e.to_string()))?;

    let nonce_bytes = hex_decode(&envelope.nonce)
        .ok_or_else(|| E2eeError::MalformedPayload("Invalid nonce hex".into()))?;
    if nonce_bytes.len() != 12 {
        return Err(E2eeError::MalformedPayload("Nonce must be 12 bytes".into()));
    }
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = hex_decode(&envelope.ciphertext)
        .ok_or_else(|| E2eeError::MalformedPayload("Invalid ciphertext hex".into()))?;

    let decrypted = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| E2eeError::DecryptionFailed("Authentication tag mismatch or corrupted ciphertext".into()))?;

    String::from_utf8(decrypted)
        .map_err(|e| E2eeError::DecryptionFailed(format!("Invalid UTF-8 plaintext: {}", e)))
}

pub fn generate_group_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut key);
    key
}

pub fn encrypt_group_message(group_key: &[u8; 32], plaintext: &str) -> Result<String, E2eeError> {
    let cipher = ChaCha20Poly1305::new_from_slice(group_key)
        .map_err(|e| E2eeError::EncryptionFailed(e.to_string()))?;

    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| E2eeError::EncryptionFailed(e.to_string()))?;

    let envelope = EncryptedEnvelope {
        e2ee: true,
        v: E2EE_VERSION,
        sender_x25519: String::new(),
        nonce: hex_encode(&nonce_bytes),
        ciphertext: hex_encode(&ciphertext),
    };

    serde_json::to_string(&envelope)
        .map_err(|e| E2eeError::EncryptionFailed(format!("Failed to serialize group envelope: {}", e)))
}

pub fn decrypt_group_message(group_key: &[u8; 32], payload_or_plaintext: &str) -> Result<String, E2eeError> {
    if !is_encrypted_payload(payload_or_plaintext) {
        return Ok(payload_or_plaintext.to_string());
    }

    let envelope: EncryptedEnvelope = serde_json::from_str(payload_or_plaintext)
        .map_err(|e| E2eeError::MalformedPayload(e.to_string()))?;

    let cipher = ChaCha20Poly1305::new_from_slice(group_key)
        .map_err(|e| E2eeError::DecryptionFailed(e.to_string()))?;

    let nonce_bytes = hex_decode(&envelope.nonce)
        .ok_or_else(|| E2eeError::MalformedPayload("Invalid nonce hex".into()))?;
    if nonce_bytes.len() != 12 {
        return Err(E2eeError::MalformedPayload("Nonce must be 12 bytes".into()));
    }
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = hex_decode(&envelope.ciphertext)
        .ok_or_else(|| E2eeError::MalformedPayload("Invalid ciphertext hex".into()))?;

    let decrypted = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| E2eeError::DecryptionFailed("Group key mismatch or corrupted ciphertext".into()))?;

    String::from_utf8(decrypted)
        .map_err(|e| E2eeError::DecryptionFailed(format!("Invalid UTF-8 plaintext: {}", e)))
}

pub fn wrap_group_key(
    my_secret_hex: &str,
    member_x25519_hex: &str,
    group_key: &[u8; 32],
) -> Result<String, E2eeError> {
    let group_key_hex = hex_encode(group_key);
    encrypt_direct_message(my_secret_hex, member_x25519_hex, &group_key_hex)
}

pub fn unwrap_group_key(
    my_secret_hex: &str,
    sender_x25519_hex: &str,
    wrapped_key_payload: &str,
) -> Result<[u8; 32], E2eeError> {
    let decrypted_hex = decrypt_direct_message(my_secret_hex, sender_x25519_hex, wrapped_key_payload)?;
    let key_bytes = hex_decode(&decrypted_hex)
        .ok_or_else(|| E2eeError::MalformedPayload("Invalid group key hex".into()))?;
    if key_bytes.len() != 32 {
        return Err(E2eeError::MalformedPayload(format!(
            "Group key must be 32 bytes, got {}",
            key_bytes.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&key_bytes);
    Ok(out)
}

pub fn is_encrypted_payload(text: &str) -> bool {
    text.starts_with('{') && text.contains(E2EE_TAG)
}

fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    let s = hex.trim();
    if s.len() % 2 != 0 || !s.is_ascii() {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_e2ee_direct_message_roundtrip() {
        let alice_secret = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let bob_secret = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

        let alice_pub = get_x25519_public_hex(alice_secret);
        let bob_pub = get_x25519_public_hex(bob_secret);

        assert_ne!(alice_pub, bob_pub);

        let secret_message = "Hello Bob! This is our top-secret E2EE message over PuppyTunnel.";

        let encrypted = encrypt_direct_message(alice_secret, &bob_pub, secret_message)
            .expect("encryption failed");

        assert!(is_encrypted_payload(&encrypted));
        assert!(!encrypted.contains("top-secret"));

        let decrypted = decrypt_direct_message(bob_secret, &alice_pub, &encrypted)
            .expect("decryption failed");

        assert_eq!(decrypted, secret_message);

        let eve_secret = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let eve_result = decrypt_direct_message(eve_secret, &alice_pub, &encrypted);
        assert!(eve_result.is_err());
    }

    #[test]
    fn test_e2ee_group_message_and_key_distribution() {
        let owner_secret = "1111111111111111111111111111111111111111111111111111111111111111";
        let member_secret = "2222222222222222222222222222222222222222222222222222222222222222";

        let owner_pub = get_x25519_public_hex(owner_secret);
        let member_pub = get_x25519_public_hex(member_secret);

        let group_key = generate_group_key();

        let wrapped = wrap_group_key(owner_secret, &member_pub, &group_key)
            .expect("wrap failed");

        let unwrapped = unwrap_group_key(member_secret, &owner_pub, &wrapped)
            .expect("unwrap failed");

        assert_eq!(group_key, unwrapped);

        let text = "Private group chat message #general";
        let encrypted_group = encrypt_group_message(&unwrapped, text)
            .expect("group encrypt failed");

        let decrypted = decrypt_group_message(&group_key, &encrypted_group)
            .expect("group decrypt failed");

        assert_eq!(decrypted, text);
    }
}
