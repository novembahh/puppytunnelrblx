use crate::identity::{self, hex, unhex, unix_now, Identity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const COMMUNITY_SIGNING_PREFIX: &[u8] = b"puppy-community-action\0";

pub const DEFAULT_MAX_ACTION_AGE_SECS: u64 = 300;

pub const DEFAULT_MAX_CLOCK_SKEW_SECS: u64 = 60;

pub const MAX_MESSAGE_LEN: usize = 4000;
pub const MAX_POST_TITLE_LEN: usize = 200;
pub const MAX_POST_BODY_LEN: usize = 16000;
pub const MAX_CHANNEL_NAME_LEN: usize = 64;
pub const MAX_CHANNEL_TOPIC_LEN: usize = 1024;
pub const MAX_GUILD_NAME_LEN: usize = 64;
pub const MAX_GUILD_DESC_LEN: usize = 1024;
pub const MAX_REASON_LEN: usize = 512;
pub const MAX_ROLE_NAME_LEN: usize = 32;
pub const MAX_TAG_LEN: usize = 32;
pub const MAX_TAGS_PER_POST: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityError {
    InvalidPublicKey,
    InvalidSignature,
    TimestampExpired { age: u64, max_age: u64 },
    TimestampInFuture { future_secs: u64 },
    ReplayDetected { nonce: String },
    ContentTooLong { length: usize, max: usize },
    ContentEmpty,
    InvalidCharacters(String),
    MalformedPayload(String),
    Unauthorized(String),
}

impl std::fmt::Display for SecurityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecurityError::InvalidPublicKey => write!(f, "Invalid public key format or encoding"),
            SecurityError::InvalidSignature => write!(f, "Cryptographic signature verification failed"),
            SecurityError::TimestampExpired { age, max_age } => {
                write!(f, "Action expired: timestamp age {}s exceeds max {}s", age, max_age)
            }
            SecurityError::TimestampInFuture { future_secs } => {
                write!(f, "Timestamp is in the future by {}s", future_secs)
            }
            SecurityError::ReplayDetected { nonce } => {
                write!(f, "Replay attack detected: nonce {} was already used", nonce)
            }
            SecurityError::ContentTooLong { length, max } => {
                write!(f, "Content length {} exceeds maximum allowed length {}", length, max)
            }
            SecurityError::ContentEmpty => write!(f, "Content cannot be empty"),
            SecurityError::InvalidCharacters(desc) => write!(f, "Invalid characters detected: {}", desc),
            SecurityError::MalformedPayload(desc) => write!(f, "Malformed action payload: {}", desc),
            SecurityError::Unauthorized(desc) => write!(f, "Unauthorized: {}", desc),
        }
    }
}

impl std::error::Error for SecurityError {}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", content = "data")]
pub enum ActionPayload {
    CreateMessage {
        guild_id: Option<String>,
        channel_id: String,
        content: String,
        reply_to: Option<String>,
        attachment_ids: Vec<String>,
    },
    EditMessage {
        channel_id: String,
        message_id: String,
        new_content: String,
    },
    DeleteMessage {
        channel_id: String,
        message_id: String,
    },
    CreatePost {
        guild_id: Option<String>,
        channel_id: String,
        title: String,
        content: String,
        tags: Vec<String>,
        attachment_ids: Vec<String>,
    },
    CreateInvite {
        guild_id: String,
        max_uses: u32,
        expires_at: Option<u64>,
    },
    RevokeInvite {
        guild_id: String,
        invite_code: String,
    },
    KickMember {
        guild_id: String,
        target_pubkey: String,
        reason: String,
    },
    BanMember {
        guild_id: String,
        target_pubkey: String,
        reason: String,
    },
    UnbanMember {
        guild_id: String,
        target_pubkey: String,
    },
    MuteMember {
        guild_id: String,
        target_pubkey: String,
        duration_secs: u64,
        reason: String,
    },
    UnmuteMember {
        guild_id: String,
        target_pubkey: String,
    },
    AssignRole {
        guild_id: String,
        target_pubkey: String,
        role_id: String,
    },
    RemoveRole {
        guild_id: String,
        target_pubkey: String,
        role_id: String,
    },
    CreateChannel {
        guild_id: String,
        name: String,
        topic: String,
        is_voice: bool,
    },
    DeleteChannel {
        guild_id: String,
        channel_id: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SignedAction {
    pub action_id: String,
    pub pubkey: String,
    pub sig: String,
    pub ts: u64,
    pub nonce: String,
    pub payload: ActionPayload,
}

impl SignedAction {
    pub fn signing_bytes(ts: u64, nonce: &str, payload: &ActionPayload) -> Vec<u8> {
        let mut bytes = COMMUNITY_SIGNING_PREFIX.to_vec();
        bytes.extend_from_slice(&ts.to_be_bytes());
        bytes.extend_from_slice(nonce.as_bytes());
        bytes.push(0);
        let payload_json = serde_json::to_vec(payload).unwrap_or_default();
        bytes.extend_from_slice(&payload_json);
        bytes
    }

    pub fn sign(id: &Identity, payload: ActionPayload) -> Self {
        let ts = unix_now();
        let nonce = format!("{}-{}", uuid::Uuid::new_v4(), ts);
        let action_id = uuid::Uuid::new_v4().to_string();
        let bytes = Self::signing_bytes(ts, &nonce, &payload);
        let sig = id.sign(&bytes);

        Self {
            action_id,
            pubkey: hex(&id.public()),
            sig: hex(&sig),
            ts,
            nonce,
            payload,
        }
    }

    pub fn verify(&self, max_clock_skew: u64, max_age: u64) -> Result<[u8; 32], SecurityError> {
        let pk = unhex::<32>(&self.pubkey).ok_or(SecurityError::InvalidPublicKey)?;
        let sig = unhex::<64>(&self.sig).ok_or(SecurityError::InvalidSignature)?;

        let now = unix_now();
        if self.ts > now + max_clock_skew {
            return Err(SecurityError::TimestampInFuture {
                future_secs: self.ts - now,
            });
        }
        if now > self.ts + max_age {
            return Err(SecurityError::TimestampExpired {
                age: now - self.ts,
                max_age,
            });
        }

        let bytes = Self::signing_bytes(self.ts, &self.nonce, &self.payload);
        if !identity::verify(&pk, &bytes, &sig) {
            return Err(SecurityError::InvalidSignature);
        }

        Ok(pk)
    }
}

#[derive(Debug, Default)]
pub struct ReplayGuard {
    seen_nonces: HashMap<String, u64>,
    last_actor_ts: HashMap<String, u64>,
    max_age_secs: u64,
    max_skew_secs: u64,
}

impl ReplayGuard {
    pub fn new(max_age_secs: u64, max_skew_secs: u64) -> Self {
        Self {
            seen_nonces: HashMap::new(),
            last_actor_ts: HashMap::new(),
            max_age_secs,
            max_skew_secs,
        }
    }

    pub fn default_guard() -> Self {
        Self::new(DEFAULT_MAX_ACTION_AGE_SECS, DEFAULT_MAX_CLOCK_SKEW_SECS)
    }

    pub fn prune(&mut self, now: u64) {
        let cutoff = now.saturating_sub(self.max_age_secs);
        self.seen_nonces.retain(|_, ts| *ts >= cutoff);
    }

    pub fn check_and_record(&mut self, action: &SignedAction) -> Result<[u8; 32], SecurityError> {
        let now = unix_now();
        self.prune(now);

        let pk = action.verify(self.max_skew_secs, self.max_age_secs)?;

        if self.seen_nonces.contains_key(&action.nonce) {
            return Err(SecurityError::ReplayDetected {
                nonce: action.nonce.clone(),
            });
        }

        if let Some(&last_ts) = self.last_actor_ts.get(&action.pubkey) {
            if action.ts + self.max_skew_secs < last_ts {
                return Err(SecurityError::MalformedPayload(format!(
                    "Monotonic timestamp violation: current {} is significantly behind last seen {}",
                    action.ts, last_ts
                )));
            }
        }

        self.seen_nonces.insert(action.nonce.clone(), action.ts);
        self.last_actor_ts.insert(action.pubkey.clone(), action.ts);

        Ok(pk)
    }
}

pub struct Sanitizer;

impl Sanitizer {
    pub fn strip_control_chars(input: &str) -> String {
        input
            .chars()
            .filter(|&c| {
                if c == '\n' || c == '\r' || c == '\t' {
                    return true;
                }
                if (c as u32) < 0x20 || c == '\x7F' {
                    return false;
                }
                if (0x80..=0x9F).contains(&(c as u32)) {
                    return false;
                }
                if matches!(
                    c,
                    '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}'
                ) {
                    return false;
                }
                true
            })
            .collect()
    }

    pub fn strip_html_and_scripts(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut in_tag = false;

        let chars: Vec<char> = input.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '<' {
                in_tag = true;
                i += 1;
                continue;
            } else if c == '>' && in_tag {
                in_tag = false;
                i += 1;
                continue;
            }

            if !in_tag {
                out.push(c);
            }
            i += 1;
        }

        let re_js = regex::Regex::new(r"(?i)javascript:").unwrap();
        let cleaned = re_js.replace_all(&out, "[blocked-uri]:");
        let re_data = regex::Regex::new(r"(?i)data:text/html").unwrap();
        re_data.replace_all(&cleaned, "[blocked-data]:").into_owned()
    }

    pub fn sanitize_message(raw: &str) -> Result<String, SecurityError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(SecurityError::ContentEmpty);
        }
        let stripped = Self::strip_control_chars(trimmed);
        let safe = Self::strip_html_and_scripts(&stripped);
        if safe.trim().is_empty() {
            return Err(SecurityError::ContentEmpty);
        }
        if safe.chars().count() > MAX_MESSAGE_LEN {
            return Err(SecurityError::ContentTooLong {
                length: safe.chars().count(),
                max: MAX_MESSAGE_LEN,
            });
        }
        Ok(safe)
    }

    pub fn sanitize_channel_name(raw: &str) -> Result<String, SecurityError> {
        let trimmed = raw.trim().to_ascii_lowercase();
        if trimmed.is_empty() {
            return Err(SecurityError::ContentEmpty);
        }
        let mut normalized = String::with_capacity(trimmed.len());
        for c in trimmed.chars() {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                normalized.push(c);
            } else if c.is_whitespace() {
                normalized.push('-');
            }
        }
        let normalized = normalized.trim_matches('-').to_string();
        if normalized.is_empty() {
            return Err(SecurityError::InvalidCharacters(
                "Channel name must contain alphanumeric characters".into(),
            ));
        }
        if normalized.chars().count() > MAX_CHANNEL_NAME_LEN {
            return Err(SecurityError::ContentTooLong {
                length: normalized.chars().count(),
                max: MAX_CHANNEL_NAME_LEN,
            });
        }
        Ok(normalized)
    }

    pub fn sanitize_text(raw: &str, max_len: usize) -> Result<String, SecurityError> {
        let trimmed = raw.trim();
        let stripped = Self::strip_control_chars(trimmed);
        let safe = Self::strip_html_and_scripts(&stripped);
        if safe.chars().count() > max_len {
            return Err(SecurityError::ContentTooLong {
                length: safe.chars().count(),
                max: max_len,
            });
        }
        Ok(safe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signed_action_flow() {
        let id = Identity::ephemeral();
        let payload = ActionPayload::CreateMessage {
            guild_id: Some("guild-1".into()),
            channel_id: "general".into(),
            content: "Hello community!".into(),
            reply_to: None,
            attachment_ids: vec![],
        };

        let action = SignedAction::sign(&id, payload.clone());
        let verified_pk = action.verify(60, 300).expect("should verify successfully");
        assert_eq!(verified_pk, id.public());

        let mut tampered = action.clone();
        if let ActionPayload::CreateMessage { ref mut content, .. } = tampered.payload {
            *content = "Malicious edit".into();
        }
        assert!(tampered.verify(60, 300).is_err());
    }

    #[test]
    fn test_replay_guard_prevents_replays() {
        let mut guard = ReplayGuard::default_guard();
        let id = Identity::ephemeral();
        let payload = ActionPayload::KickMember {
            guild_id: "guild-1".into(),
            target_pubkey: "target-pubkey".into(),
            reason: "Rule violation".into(),
        };

        let action = SignedAction::sign(&id, payload);

        assert!(guard.check_and_record(&action).is_ok());

        let res = guard.check_and_record(&action);
        assert!(matches!(res, Err(SecurityError::ReplayDetected { .. })));
    }

    #[test]
    fn test_sanitizer_strips_controls_and_scripts() {
        let bad_input = "Hello\x00World!\u{202E}gnitsop<script>alert(1)</script>Safe <a href='javascript:evil()'>link</a>";
        let cleaned = Sanitizer::sanitize_message(bad_input).unwrap();
        assert!(!cleaned.contains('\x00'));
        assert!(!cleaned.contains('\u{202E}'));
        assert!(!cleaned.contains("<script>"));
        assert!(!cleaned.contains("javascript:"));
        assert!(cleaned.contains("Safe"));
    }

    #[test]
    fn test_channel_name_sanitization() {
        assert_eq!(Sanitizer::sanitize_channel_name("General Chat!").unwrap(), "general-chat");
        assert_eq!(Sanitizer::sanitize_channel_name("dev_logs-2026").unwrap(), "dev_logs-2026");
        assert!(Sanitizer::sanitize_channel_name("   ").is_err());
    }
}
