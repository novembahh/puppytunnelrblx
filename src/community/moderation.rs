use crate::identity::{self, hex, unhex, unix_now, Identity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const AUDIT_SIGNING_PREFIX: &[u8] = b"puppy-audit-entry\0";
pub const REPORT_SIGNING_PREFIX: &[u8] = b"puppy-mod-report\0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModerationError {
    InvalidSignature,
    InvalidPublicKey,
    InvalidTarget(String),
    ReportNotFound(String),
    AlreadyResolved(String),
    RateLimitExceeded,
    Unauthorized(String),
}

impl std::fmt::Display for ModerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModerationError::InvalidSignature => write!(f, "Invalid cryptographic signature"),
            ModerationError::InvalidPublicKey => write!(f, "Invalid public key"),
            ModerationError::InvalidTarget(desc) => write!(f, "Invalid target: {}", desc),
            ModerationError::ReportNotFound(id) => write!(f, "Report {} not found", id),
            ModerationError::AlreadyResolved(id) => write!(f, "Report {} is already resolved or dismissed", id),
            ModerationError::RateLimitExceeded => write!(f, "Moderation action rate limit exceeded"),
            ModerationError::Unauthorized(desc) => write!(f, "Unauthorized moderation action: {}", desc),
        }
    }
}

impl std::error::Error for ModerationError {}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "action", content = "data")]
pub enum AuditAction {
    MemberKicked { target_pubkey: String },
    MemberBanned { target_pubkey: String },
    MemberUnbanned { target_pubkey: String },
    MemberMuted { target_pubkey: String, duration_secs: u64 },
    MemberUnmuted { target_pubkey: String },
    RoleAssigned { target_pubkey: String, role_id: String },
    RoleRemoved { target_pubkey: String, role_id: String },
    ChannelCreated { channel_id: String, name: String },
    ChannelUpdated { channel_id: String, name: String },
    ChannelDeleted { channel_id: String, name: String },
    MessageDeleted { channel_id: String, message_id: String, author_pubkey: String },
    InviteCreated { code: String },
    InviteRevoked { code: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AuditLogEntry {
    pub id: String,
    pub guild_id: String,
    pub actor_pubkey: String,
    pub action: AuditAction,
    pub target_id: Option<String>,
    pub reason: String,
    pub timestamp: u64,
    pub signature: String,
}

impl AuditLogEntry {
    pub fn signing_bytes(
        guild_id: &str,
        actor_pubkey: &str,
        action: &AuditAction,
        target_id: Option<&str>,
        reason: &str,
        timestamp: u64,
    ) -> Vec<u8> {
        let mut v = AUDIT_SIGNING_PREFIX.to_vec();
        v.extend_from_slice(guild_id.as_bytes());
        v.push(0);
        v.extend_from_slice(actor_pubkey.as_bytes());
        v.push(0);
        let action_bytes = serde_json::to_vec(action).unwrap_or_default();
        v.extend_from_slice(&action_bytes);
        v.push(0);
        if let Some(t) = target_id {
            v.extend_from_slice(t.as_bytes());
        }
        v.push(0);
        v.extend_from_slice(reason.as_bytes());
        v.push(0);
        v.extend_from_slice(&timestamp.to_be_bytes());
        v
    }

    pub fn create_and_sign(
        id: &Identity,
        guild_id: impl Into<String>,
        action: AuditAction,
        target_id: Option<String>,
        reason: impl Into<String>,
    ) -> Self {
        let guild_id = guild_id.into();
        let reason = reason.into();
        let actor_pubkey = hex(&id.public());
        let timestamp = unix_now();
        let bytes = Self::signing_bytes(
            &guild_id,
            &actor_pubkey,
            &action,
            target_id.as_deref(),
            &reason,
            timestamp,
        );
        let sig = id.sign(&bytes);

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            guild_id,
            actor_pubkey,
            action,
            target_id,
            reason,
            timestamp,
            signature: hex(&sig),
        }
    }

    pub fn verify(&self) -> Result<[u8; 32], ModerationError> {
        let pk = unhex::<32>(&self.actor_pubkey).ok_or(ModerationError::InvalidPublicKey)?;
        let sig = unhex::<64>(&self.signature).ok_or(ModerationError::InvalidSignature)?;
        let bytes = Self::signing_bytes(
            &self.guild_id,
            &self.actor_pubkey,
            &self.action,
            self.target_id.as_deref(),
            &self.reason,
            self.timestamp,
        );

        if !identity::verify(&pk, &bytes, &sig) {
            return Err(ModerationError::InvalidSignature);
        }
        Ok(pk)
    }
}

#[derive(Debug, Default, Clone)]
pub struct AuditFilter {
    pub actor_pubkey: Option<String>,
    pub target_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Default, Clone)]
pub struct AuditLog {
    entries: Vec<AuditLogEntry>,
}

impl AuditLog {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn record(&mut self, entry: AuditLogEntry) -> Result<(), ModerationError> {
        entry.verify()?;
        self.entries.push(entry);
        Ok(())
    }

    pub fn query(&self, guild_id: &str, filter: &AuditFilter) -> Vec<AuditLogEntry> {
        let mut results: Vec<AuditLogEntry> = self
            .entries
            .iter()
            .rev()
            .filter(|e| {
                if e.guild_id != guild_id {
                    return false;
                }
                if let Some(ref actor) = filter.actor_pubkey {
                    if e.actor_pubkey != *actor {
                        return false;
                    }
                }
                if let Some(ref target) = filter.target_id {
                    if e.target_id.as_ref() != Some(target) {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();

        if let Some(limit) = filter.limit {
            results.truncate(limit);
        }

        results
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportCategory {
    Spam,
    Harassment,
    InappropriateContent,
    Scam,
    Other,
}

impl ReportCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReportCategory::Spam => "Spam",
            ReportCategory::Harassment => "Harassment",
            ReportCategory::InappropriateContent => "Inappropriate Content",
            ReportCategory::Scam => "Scam / Phishing",
            ReportCategory::Other => "Other",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "target_type", content = "target_info")]
pub enum ReportTarget {
    Message { channel_id: String, message_id: String, author_pubkey: String },
    Post { channel_id: String, post_id: String, author_pubkey: String },
    Profile { target_pubkey: String },
    Guild { guild_id: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "status", content = "details")]
pub enum ReportStatus {
    Pending,
    Resolved {
        resolved_by: String,
        action_taken: String,
        notes: String,
        resolved_at: u64,
    },
    Dismissed {
        dismissed_by: String,
        reason: String,
        dismissed_at: u64,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub id: String,
    pub reporter_pubkey: String,
    pub guild_id: Option<String>,
    pub target: ReportTarget,
    pub category: ReportCategory,
    pub explanation: String,
    pub timestamp: u64,
    pub signature: String,
    pub status: ReportStatus,
}

impl Report {
    pub fn signing_bytes(
        reporter_pubkey: &str,
        guild_id: Option<&str>,
        target: &ReportTarget,
        category: ReportCategory,
        explanation: &str,
        timestamp: u64,
    ) -> Vec<u8> {
        let mut v = REPORT_SIGNING_PREFIX.to_vec();
        v.extend_from_slice(reporter_pubkey.as_bytes());
        v.push(0);
        if let Some(g) = guild_id {
            v.extend_from_slice(g.as_bytes());
        }
        v.push(0);
        let target_bytes = serde_json::to_vec(target).unwrap_or_default();
        v.extend_from_slice(&target_bytes);
        v.push(0);
        let cat_bytes = serde_json::to_vec(&category).unwrap_or_default();
        v.extend_from_slice(&cat_bytes);
        v.push(0);
        v.extend_from_slice(explanation.as_bytes());
        v.push(0);
        v.extend_from_slice(&timestamp.to_be_bytes());
        v
    }

    pub fn create_and_sign(
        id: &Identity,
        guild_id: Option<String>,
        target: ReportTarget,
        category: ReportCategory,
        explanation: impl Into<String>,
    ) -> Self {
        let reporter_pubkey = hex(&id.public());
        let explanation = explanation.into();
        let timestamp = unix_now();
        let bytes = Self::signing_bytes(
            &reporter_pubkey,
            guild_id.as_deref(),
            &target,
            category,
            &explanation,
            timestamp,
        );
        let sig = id.sign(&bytes);

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            reporter_pubkey,
            guild_id,
            target,
            category,
            explanation,
            timestamp,
            signature: hex(&sig),
            status: ReportStatus::Pending,
        }
    }

    pub fn verify(&self) -> Result<[u8; 32], ModerationError> {
        let pk = unhex::<32>(&self.reporter_pubkey).ok_or(ModerationError::InvalidPublicKey)?;
        let sig = unhex::<64>(&self.signature).ok_or(ModerationError::InvalidSignature)?;
        let bytes = Self::signing_bytes(
            &self.reporter_pubkey,
            self.guild_id.as_deref(),
            &self.target,
            self.category,
            &self.explanation,
            self.timestamp,
        );

        if !identity::verify(&pk, &bytes, &sig) {
            return Err(ModerationError::InvalidSignature);
        }
        Ok(pk)
    }
}

#[derive(Debug, Default, Clone)]
pub struct ModQueue {
    reports: HashMap<String, Report>,
}

impl ModQueue {
    pub fn new() -> Self {
        Self {
            reports: HashMap::new(),
        }
    }

    pub fn submit_report(&mut self, report: Report) -> Result<(), ModerationError> {
        report.verify()?;
        self.reports.insert(report.id.clone(), report);
        Ok(())
    }

    pub fn list_pending(&self, guild_id: Option<&str>) -> Vec<Report> {
        self.reports
            .values()
            .filter(|r| matches!(r.status, ReportStatus::Pending))
            .filter(|r| match guild_id {
                Some(g) => r.guild_id.as_deref() == Some(g),
                None => true,
            })
            .cloned()
            .collect()
    }

    pub fn resolve_report(
        &mut self,
        report_id: &str,
        moderator_pubkey: impl Into<String>,
        action_taken: impl Into<String>,
        notes: impl Into<String>,
    ) -> Result<(), ModerationError> {
        let report = self
            .reports
            .get_mut(report_id)
            .ok_or_else(|| ModerationError::ReportNotFound(report_id.into()))?;

        if !matches!(report.status, ReportStatus::Pending) {
            return Err(ModerationError::AlreadyResolved(report_id.into()));
        }

        report.status = ReportStatus::Resolved {
            resolved_by: moderator_pubkey.into(),
            action_taken: action_taken.into(),
            notes: notes.into(),
            resolved_at: unix_now(),
        };

        Ok(())
    }

    pub fn dismiss_report(
        &mut self,
        report_id: &str,
        moderator_pubkey: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<(), ModerationError> {
        let report = self
            .reports
            .get_mut(report_id)
            .ok_or_else(|| ModerationError::ReportNotFound(report_id.into()))?;

        if !matches!(report.status, ReportStatus::Pending) {
            return Err(ModerationError::AlreadyResolved(report_id.into()));
        }

        report.status = ReportStatus::Dismissed {
            dismissed_by: moderator_pubkey.into(),
            reason: reason.into(),
            dismissed_at: unix_now(),
        };

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MuteRecord {
    pub target_pubkey: String,
    pub guild_id: String,
    pub muted_by: String,
    pub muted_at: u64,
    pub expires_at: u64,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct BanRecord {
    pub target_pubkey: String,
    pub guild_id: String,
    pub banned_by: String,
    pub banned_at: u64,
    pub reason: String,
}

#[derive(Debug, Default, Clone)]
pub struct SanctionManager {
    mutes: HashMap<(String, String), MuteRecord>,
    bans: HashMap<(String, String), BanRecord>,
}

impl SanctionManager {
    pub fn new() -> Self {
        Self {
            mutes: HashMap::new(),
            bans: HashMap::new(),
        }
    }

    pub fn mute_member(&mut self, record: MuteRecord) {
        let key = (record.guild_id.clone(), record.target_pubkey.clone());
        self.mutes.insert(key, record);
    }

    pub fn unmute_member(&mut self, guild_id: &str, target_pubkey: &str) {
        self.mutes.remove(&(guild_id.to_string(), target_pubkey.to_string()));
    }

    pub fn is_muted(&mut self, guild_id: &str, target_pubkey: &str) -> bool {
        let key = (guild_id.to_string(), target_pubkey.to_string());
        if let Some(record) = self.mutes.get(&key) {
            let now = unix_now();
            if now >= record.expires_at {
                self.mutes.remove(&key);
                false
            } else {
                true
            }
        } else {
            false
        }
    }

    pub fn ban_member(&mut self, record: BanRecord) {
        let key = (record.guild_id.clone(), record.target_pubkey.clone());
        self.bans.insert(key, record);
    }

    pub fn unban_member(&mut self, guild_id: &str, target_pubkey: &str) {
        self.bans.remove(&(guild_id.to_string(), target_pubkey.to_string()));
    }

    pub fn is_banned(&self, guild_id: &str, target_pubkey: &str) -> bool {
        self.bans.contains_key(&(guild_id.to_string(), target_pubkey.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_log_signing_and_verification() {
        let id = Identity::ephemeral();
        let entry = AuditLogEntry::create_and_sign(
            &id,
            "guild-100",
            AuditAction::MemberKicked {
                target_pubkey: "bad-actor".into(),
            },
            Some("bad-actor".into()),
            "Spamming channels",
        );

        assert_eq!(entry.verify().unwrap(), id.public());

        let mut log = AuditLog::new();
        assert!(log.record(entry).is_ok());

        let results = log.query("guild-100", &AuditFilter::default());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].reason, "Spamming channels");
    }

    #[test]
    fn test_mod_queue_reports() {
        let reporter = Identity::ephemeral();
        let report = Report::create_and_sign(
            &reporter,
            Some("guild-100".into()),
            ReportTarget::Message {
                channel_id: "general".into(),
                message_id: "msg-1".into(),
                author_pubkey: "spammer-pk".into(),
            },
            ReportCategory::Spam,
            "Posting unsolicited bot links",
        );

        let mut queue = ModQueue::new();
        assert!(queue.submit_report(report.clone()).is_ok());

        let pending = queue.list_pending(Some("guild-100"));
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, report.id);

        let mod_id = Identity::ephemeral();
        assert!(queue
            .resolve_report(&report.id, hex(&mod_id.public()), "Message deleted & user muted", "Confirmed spam")
            .is_ok());

        assert_eq!(queue.list_pending(Some("guild-100")).len(), 0);
    }

    #[test]
    fn test_sanctions_manager() {
        let mut sanctions = SanctionManager::new();
        let now = unix_now();
        sanctions.mute_member(MuteRecord {
            target_pubkey: "user-1".into(),
            guild_id: "guild-1".into(),
            muted_by: "mod-1".into(),
            muted_at: now,
            expires_at: now + 3600,
            reason: "Excessive noise".into(),
        });

        assert!(sanctions.is_muted("guild-1", "user-1"));
        assert!(!sanctions.is_muted("guild-1", "user-2"));

        sanctions.unmute_member("guild-1", "user-1");
        assert!(!sanctions.is_muted("guild-1", "user-1"));

        sanctions.ban_member(BanRecord {
            target_pubkey: "user-1".into(),
            guild_id: "guild-1".into(),
            banned_by: "admin-1".into(),
            banned_at: now,
            reason: "Malicious activity".into(),
        });
        assert!(sanctions.is_banned("guild-1", "user-1"));
    }
}
