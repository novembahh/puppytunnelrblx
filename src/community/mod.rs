pub mod attachments;
pub mod client;
pub mod crypto;
pub mod e2ee;
pub mod events;
pub mod federation;
pub mod keystore;
pub mod moderation;
pub mod permissions;
pub mod realtime;
pub mod security;
pub mod types;
pub mod worker;

pub use e2ee::{
    decrypt_direct_message, decrypt_group_message, derive_x25519_secret, encrypt_direct_message,
    encrypt_group_message, generate_group_key, get_x25519_public, get_x25519_public_hex,
    is_encrypted_payload, parse_x25519_public, unwrap_group_key, wrap_group_key, E2eeError,
    EncryptedEnvelope, GroupKeyDistribution,
};
pub use keystore::{
    create_backup, derive_ed25519_pubkey_hex, export_backup_file, export_backup_json,
    generate_mnemonic, import_backup_file, import_backup_str, mnemonic_to_secret,
    validate_secret_hex, IdentityBackup, KeystoreError,
};

pub use attachments::{AttachmentError, AttachmentMeta, AttachmentValidator, MAX_ATTACHMENT_SIZE};
pub use events::{
    CommunityEvent, CommunityMessage, EventQueue, PresenceStatus, SequencedEvent, SharedEventQueue,
};
pub use moderation::{
    AuditAction, AuditFilter, AuditLog, AuditLogEntry, BanRecord, ModQueue, ModerationError,
    MuteRecord, Report, ReportCategory, ReportStatus, ReportTarget, SanctionManager,
};
pub use permissions::{
    ChannelPermissionOverride, HierarchyRank, PermissionContext, PermissionResolver, Permissions,
    Role, SystemAdminRegistry,
};
pub use realtime::{
    CommunitySyncWorker, PresenceTracker, ReadReceiptTracker, SyncConfig, TypingTracker,
    UserPresence,
};
pub use security::{
    ActionPayload, ReplayGuard, Sanitizer, SecurityError, SignedAction,
};
pub use client::{CommunityCache, CommunityClient};
pub use crypto::{hash_payload, sign_envelope, verify_envelope, CommunityEnvelope, CryptoError};
pub use federation::{
    build_nodeinfo, sync_posts_from_peer, verify_federated_post, FederatedAddress,
    FederationPeer, HandshakeRequest, HandshakeResponse, InstanceMetadata, NodeInfo,
    NodeInfoSoftware, NodeInfoUsage, DEFAULT_INSTANCE, FEDERATION_MAX_AGE_SECS,
};
pub use types::{
    Attachment, ChannelKind, Conversation, ConversationMember, DirectMessage, FollowRelation,
    Guild, GuildBan, GuildChannel, GuildInvite, GuildMember, GuildMessage, GuildRole,
    Notification, NotificationKind, PermissionFlags, Post, PostInteraction, UserProfile,
};

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommunityConfig {
    pub default_instance: String,
    pub instance_url: String,
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
    #[serde(default)]
    pub offline_mode: bool,
    #[serde(default = "default_true")]
    pub federation_enabled: bool,
    #[serde(default)]
    pub trusted_peers: Vec<String>,
}

fn default_true() -> bool {
    true
}

impl Default for CommunityConfig {
    fn default() -> Self {
        Self {
            default_instance: DEFAULT_INSTANCE.to_string(),
            instance_url: format!("https://{}", DEFAULT_INSTANCE),
            cache_dir: None,
            offline_mode: false,
            federation_enabled: true,
            trusted_peers: Vec::new(),
        }
    }
}
