use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub const EMBEDDED_RELEASE_PUBKEY: &str =
    "5a58f58705906656e746cea8f791f0053358f9df0e4a0713bc7935429524d86d";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifierError {
    IoError(String),
    ChecksumMismatch { expected: String, actual: String },
    InvalidPublicKey(String),
    InvalidSignatureFormat(String),
    SignatureVerificationFailed,
}

impl fmt::Display for VerifierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifierError::IoError(e) => write!(f, "I/O error during verification: {}", e),
            VerifierError::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "SHA-256 checksum mismatch: expected {}, got {}",
                    expected, actual
                )
            }
            VerifierError::InvalidPublicKey(e) => write!(f, "Invalid Ed25519 public key: {}", e),
            VerifierError::InvalidSignatureFormat(e) => {
                write!(f, "Invalid Ed25519 signature format: {}", e)
            }
            VerifierError::SignatureVerificationFailed => {
                write!(f, "Ed25519 signature verification failed")
            }
        }
    }
}

impl std::error::Error for VerifierError {}

impl From<io::Error> for VerifierError {
    fn from(err: io::Error) -> Self {
        VerifierError::IoError(err.to_string())
    }
}

pub fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    result.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn compute_file_sha256<P: AsRef<Path>>(path: P) -> Result<String, VerifierError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 16384];
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    let result = hasher.finalize();
    Ok(result.iter().map(|b| format!("{:02x}", b)).collect())
}

pub fn verify_sha256(data: &[u8], expected_hex: &str) -> Result<(), VerifierError> {
    let actual = compute_sha256(data);
    let expected = expected_hex.trim().to_ascii_lowercase();
    if actual == expected {
        Ok(())
    } else {
        Err(VerifierError::ChecksumMismatch {
            expected,
            actual,
        })
    }
}

pub fn verify_file_sha256<P: AsRef<Path>>(path: P, expected_hex: &str) -> Result<(), VerifierError> {
    let actual = compute_file_sha256(path)?;
    let expected = expected_hex.trim().to_ascii_lowercase();
    if actual == expected {
        Ok(())
    } else {
        Err(VerifierError::ChecksumMismatch {
            expected,
            actual,
        })
    }
}

pub fn parse_public_key(pubkey_hex: &str) -> Result<VerifyingKey, VerifierError> {
    let raw = unhex_32(pubkey_hex.trim())
        .ok_or_else(|| VerifierError::InvalidPublicKey("Public key must be 64 hex characters (32 bytes)".into()))?;
    VerifyingKey::from_bytes(&raw)
        .map_err(|e| VerifierError::InvalidPublicKey(e.to_string()))
}

pub fn parse_signature(sig_str: &str) -> Result<Signature, VerifierError> {
    let trimmed = sig_str.trim();

    if trimmed.len() == 128 {
        if let Some(bytes) = unhex_64(trimmed) {
            return Ok(Signature::from_bytes(&bytes));
        }
    }

    if let Ok(bytes) = decode_base64(trimmed) {
        if bytes.len() == 64 {
            let mut arr = [0u8; 64];
            arr.copy_from_slice(&bytes);
            return Ok(Signature::from_bytes(&arr));
        }
    }

    Err(VerifierError::InvalidSignatureFormat(
        "Signature must be 128-hex chars or 64-byte base64 string".into(),
    ))
}

pub fn verify_signature(
    data: &[u8],
    signature_str: &str,
    custom_pubkey_hex: Option<&str>,
) -> Result<(), VerifierError> {
    let pubkey_str = custom_pubkey_hex.unwrap_or(EMBEDDED_RELEASE_PUBKEY);
    let verifying_key = parse_public_key(pubkey_str)?;
    let signature = parse_signature(signature_str)?;

    verifying_key
        .verify(data, &signature)
        .map_err(|_| VerifierError::SignatureVerificationFailed)
}

pub fn verify_payload_file<P: AsRef<Path>>(
    path: P,
    expected_sha256: &str,
    signature: Option<&str>,
    custom_pubkey: Option<&str>,
) -> Result<(), VerifierError> {
    verify_file_sha256(path.as_ref(), expected_sha256)?;

    if let Some(sig) = signature {
        if !sig.trim().is_empty() {
            let mut file = File::open(path)?;
            let mut data = Vec::new();
            file.read_to_end(&mut data)?;
            verify_signature(&data, sig, custom_pubkey)?;
        }
    }

    Ok(())
}

fn unhex_32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn unhex_64(s: &str) -> Option<[u8; 64]> {
    if s.len() != 128 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 64];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn decode_base64(input: &str) -> Result<Vec<u8>, ()> {
    let mut b64_table = [0xFFu8; 256];
    for (i, &b) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/".iter().enumerate() {
        b64_table[b as usize] = i as u8;
    }
    b64_table[b'-' as usize] = 62;
    b64_table[b'_' as usize] = 63;

    let clean: Vec<u8> = input.bytes().filter(|&b| b != b'=' && !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity((clean.len() * 3) / 4);

    let mut buf = 0u32;
    let mut bits = 0;

    for &byte in &clean {
        let val = b64_table[byte as usize];
        if val == 0xFF {
            return Err(());
        }
        buf = (buf << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn test_sha256_verification() {
        let data = b"hello puppytunnel update";
        let hash = compute_sha256(data);
        assert_eq!(hash.len(), 64);
        assert!(verify_sha256(data, &hash).is_ok());
        assert!(verify_sha256(data, "0000000000000000000000000000000000000000000000000000000000000000").is_err());
    }

    #[test]
    fn test_signature_verification_embedded_key() {
        let seed: [u8; 32] = [
            0x14, 0x06, 0x81, 0x5a, 0x38, 0xeb, 0x00, 0x86, 0xa6, 0x13, 0x1c, 0xd7, 0xb7, 0x6c, 0x12, 0x59,
            0xcc, 0xbe, 0xa7, 0xaa, 0x1e, 0x4b, 0xd7, 0x09, 0x44, 0x47, 0xfd, 0xb1, 0xc1, 0x15, 0x49, 0x36,
        ];
        let signing_key = SigningKey::from_bytes(&seed);
        let message = b"puppytunnel-v1.0.1-binary-payload";
        let sig = signing_key.sign(message);
        let sig_hex: String = sig.to_bytes().iter().map(|b| format!("{:02x}", b)).collect();

        assert!(verify_signature(message, &sig_hex, None).is_ok());

        assert!(verify_signature(b"tampered", &sig_hex, None).is_err());

        let custom_seed = [42u8; 32];
        let custom_signing = SigningKey::from_bytes(&custom_seed);
        let custom_pubkey_hex: String = custom_signing.verifying_key().to_bytes().iter().map(|b| format!("{:02x}", b)).collect();
        let custom_sig = custom_signing.sign(message);
        let custom_sig_hex: String = custom_sig.to_bytes().iter().map(|b| format!("{:02x}", b)).collect();

        assert!(verify_signature(message, &custom_sig_hex, Some(&custom_pubkey_hex)).is_ok());
        assert!(verify_signature(message, &custom_sig_hex, None).is_err());
    }

    #[test]
    fn test_file_verification_and_payload() {
        let temp_dir = std::env::temp_dir().join(format!("pt_ver_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("test_file.bin");

        let content = b"verified payload content for puppytunnel update";
        std::fs::write(&file_path, content).unwrap();

        let sha = compute_file_sha256(&file_path).unwrap();
        assert_eq!(sha, compute_sha256(content));
        assert!(verify_file_sha256(&file_path, &sha).is_ok());
        assert!(verify_file_sha256(&file_path, "1111111111111111111111111111111111111111111111111111111111111111").is_err());

        let seed = [7u8; 32];
        let signing = SigningKey::from_bytes(&seed);
        let pubkey_hex: String = signing.verifying_key().to_bytes().iter().map(|b| format!("{:02x}", b)).collect();
        let sig = signing.sign(content);
        let sig_hex: String = sig.to_bytes().iter().map(|b| format!("{:02x}", b)).collect();

        assert!(verify_payload_file(&file_path, &sha, Some(&sig_hex), Some(&pubkey_hex)).is_ok());
        assert!(verify_payload_file(&file_path, &sha, Some("bad_sig"), Some(&pubkey_hex)).is_err());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_invalid_keys_and_formats() {
        assert!(parse_public_key("too_short").is_err());
        assert!(parse_public_key("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());
        assert!(parse_signature("invalid").is_err());
    }
}
