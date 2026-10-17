use crate::identity::{self, hex, unhex, unix_now, Identity};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CommunityEnvelope<T> {
    pub action: String,
    pub pubkey: String,
    pub sig: String,
    pub ts: u64,
    pub nonce: String,
    pub payload_hash: String,
    pub payload: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CryptoError {
    BadPublicKey,
    BadSignature,
    StaleTimestamp { diff: u64, max_age: u64 },
    PayloadHashMismatch,
    SignatureInvalid,
    Serialization(String),
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CryptoError::BadPublicKey => write!(f, "invalid ed25519 public key"),
            CryptoError::BadSignature => write!(f, "invalid ed25519 signature format"),
            CryptoError::StaleTimestamp { diff, max_age } => {
                write!(f, "stale or future timestamp (diff: {}s, max allowed: {}s)", diff, max_age)
            }
            CryptoError::PayloadHashMismatch => write!(f, "payload hash does not match content"),
            CryptoError::SignatureInvalid => write!(f, "cryptographic signature verification failed"),
            CryptoError::Serialization(e) => write!(f, "serialization error: {}", e),
        }
    }
}

impl std::error::Error for CryptoError {}

pub fn hash_payload<T: Serialize>(payload: &T) -> Result<String, CryptoError> {
    let bytes = serde_json::to_vec(payload).map_err(|e| CryptoError::Serialization(e.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex(&hasher.finalize()))
}

pub fn signing_message(action: &str, ts: u64, nonce: &str, payload_hash: &str) -> Vec<u8> {
    let mut msg = b"puppy-community\0".to_vec();
    msg.extend_from_slice(action.as_bytes());
    msg.push(0);
    msg.extend_from_slice(&ts.to_be_bytes());
    msg.extend_from_slice(nonce.as_bytes());
    msg.push(0);
    msg.extend_from_slice(payload_hash.as_bytes());
    msg
}

pub fn sign_envelope<T: Serialize>(
    id: &Identity,
    action: &str,
    payload: T,
) -> Result<CommunityEnvelope<T>, CryptoError> {
    sign_envelope_with_ts(id, action, payload, unix_now())
}

pub fn sign_envelope_with_ts<T: Serialize>(
    id: &Identity,
    action: &str,
    payload: T,
    ts: u64,
) -> Result<CommunityEnvelope<T>, CryptoError> {
    let payload_hash = hash_payload(&payload)?;
    let nonce = Uuid::new_v4().to_string();
    let msg = signing_message(action, ts, &nonce, &payload_hash);
    let sig = id.sign(&msg);

    Ok(CommunityEnvelope {
        action: action.to_string(),
        pubkey: hex(&id.public()),
        sig: hex(&sig),
        ts,
        nonce,
        payload_hash,
        payload,
    })
}

pub fn verify_envelope<T: Serialize>(
    envelope: &CommunityEnvelope<T>,
    max_age_secs: u64,
) -> Result<[u8; 32], CryptoError> {
    let pk = unhex::<32>(&envelope.pubkey).ok_or(CryptoError::BadPublicKey)?;
    let sig = unhex::<64>(&envelope.sig).ok_or(CryptoError::BadSignature)?;

    let now = unix_now();
    let diff = now.abs_diff(envelope.ts);
    if diff > max_age_secs {
        return Err(CryptoError::StaleTimestamp { diff, max_age: max_age_secs });
    }

    let expected_hash = hash_payload(&envelope.payload)?;
    if envelope.payload_hash != expected_hash {
        return Err(CryptoError::PayloadHashMismatch);
    }

    let msg = signing_message(
        &envelope.action,
        envelope.ts,
        &envelope.nonce,
        &envelope.payload_hash,
    );

    if !identity::verify(&pk, &msg, &sig) {
        return Err(CryptoError::SignatureInvalid);
    }

    Ok(pk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_signing_and_verification() {
        let id = Identity::profile("test-secret-community");
        let payload = serde_json::json!({
            "content": "Hello community!",
            "tags": ["welcome", "puppy"]
        });

        let envelope = sign_envelope(&id, "post:create", payload.clone()).unwrap();
        assert_eq!(envelope.action, "post:create");
        assert_eq!(envelope.pubkey, hex(&id.public()));

        let pk = verify_envelope(&envelope, 60).unwrap();
        assert_eq!(pk, id.public());

        let mut tampered = envelope.clone();
        tampered.payload = serde_json::json!({
            "content": "Tampered content!",
            "tags": ["welcome", "puppy"]
        });
        assert_eq!(
            verify_envelope(&tampered, 60).unwrap_err(),
            CryptoError::PayloadHashMismatch
        );

        let mut tampered_action = envelope.clone();
        tampered_action.action = "post:delete".to_string();
        assert_eq!(
            verify_envelope(&tampered_action, 60).unwrap_err(),
            CryptoError::SignatureInvalid
        );

        let future = unix_now() + 3600;
        let explicit = sign_envelope_with_ts(&id, "post:create", payload.clone(), future).unwrap();
        assert_eq!(explicit.ts, future);
        assert!(verify_envelope(&explicit, 7200).is_ok());
        assert!(matches!(
            verify_envelope(&explicit, 60),
            Err(CryptoError::StaleTimestamp { .. })
        ));

        let mut stale = envelope.clone();
        stale.ts = unix_now() - 3600;
        match verify_envelope(&stale, 60) {
            Err(CryptoError::StaleTimestamp { diff, max_age }) => {
                assert!(diff >= 3600);
                assert_eq!(max_age, 60);
            }
            other => panic!("expected StaleTimestamp, got {:?}", other),
        }
    }
}
