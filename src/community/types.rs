use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PermissionFlags(pub u64);

impl PermissionFlags {
    pub const VIEW_CHANNELS: u64 = 1 << 0;
    pub const SEND_MESSAGES: u64 = 1 << 1;
    pub const EMBED_LINKS: u64 = 1 << 2;
    pub const ATTACH_FILES: u64 = 1 << 3;
    pub const READ_HISTORY: u64 = 1 << 4;
    pub const MENTION_EVERYONE: u64 = 1 << 5;
    pub const MANAGE_MESSAGES: u64 = 1 << 6;
    pub const MANAGE_CHANNELS: u64 = 1 << 7;
    pub const MANAGE_ROLES: u64 = 1 << 8;
    pub const MANAGE_GUILD: u64 = 1 << 9;
    pub const KICK_MEMBERS: u64 = 1 << 10;
    pub const BAN_MEMBERS: u64 = 1 << 11;
    pub const ADMINISTRATOR: u64 = 1 << 12;

    pub fn new(bits: u64) -> Self {
        Self(bits)
    }

    pub fn all() -> Self {
        Self(0x1FFF)
    }

    pub fn default_member() -> Self {
        Self(
            Self::VIEW_CHANNELS
                | Self::SEND_MESSAGES
                | Self::EMBED_LINKS
                | Self::ATTACH_FILES
                | Self::READ_HISTORY,
        )
    }

    pub fn contains(&self, perm: u64) -> bool {
        if (self.0 & Self::ADMINISTRATOR) != 0 {
            return true;
        }
        (self.0 & perm) == perm
    }

    pub fn add(&mut self, perm: u64) {
        self.0 |= perm;
    }

    pub fn remove(&mut self, perm: u64) {
        self.0 &= !perm;
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct Attachment {
    pub id: String,
    pub filename: String,
    pub media_type: String,
    pub url: String,
    pub hash: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InteractionType {
    Like,
    Repost,
    Reply,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LikeResult {
    pub id: Option<String>,
    pub post_id: Option<String>,
    pub kind: Option<InteractionType>,
    pub author_pubkey: Option<String>,
    pub author_handle: Option<String>,
    pub created_at: Option<u64>,
    pub content: Option<String>,
    pub liked: Option<bool>,
    pub likes_count: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PostInteraction {
    pub id: String,
    pub post_id: String,
    pub kind: InteractionType,
    pub author_pubkey: String,
    pub author_handle: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub likes_count: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Post {
    pub id: String,
    pub author_pubkey: String,
    #[serde(default, alias = "username")]
    pub author_handle: String,
    #[serde(default, alias = "display_name")]
    pub author_display: Option<String>,
    #[serde(default, alias = "avatar_url")]
    pub author_avatar_url: Option<String>,
    #[serde(default, alias = "accent")]
    pub author_accent: Option<String>,
    #[serde(default, alias = "roblox_id")]
    pub author_roblox_id: Option<u64>,
    #[serde(default)]
    pub author_roblox_verified: bool,
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repost_of_id: Option<String>,
    #[serde(default)]
    pub likes_count: u32,
    #[serde(default)]
    pub reposts_count: u32,
    #[serde(default)]
    pub replies_count: u32,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<u64>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_post_visibility")]
    pub visibility: String,
    #[serde(default)]
    pub federated: bool,
    #[serde(default)]
    pub origin_instance: String,
    #[serde(default, deserialize_with = "deserialize_bool")]
    pub is_pinned: bool,
    #[serde(default, deserialize_with = "deserialize_bool")]
    pub liked_by_me: bool,
}

pub fn deserialize_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct BoolVisitor;
    impl<'de> serde::de::Visitor<'de> for BoolVisitor {
        type Value = bool;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a boolean or integer 0 or 1")
        }
        fn visit_bool<E>(self, v: bool) -> Result<bool, E> {
            Ok(v)
        }
        fn visit_i64<E>(self, v: i64) -> Result<bool, E> {
            Ok(v != 0)
        }
        fn visit_u64<E>(self, v: u64) -> Result<bool, E> {
            Ok(v != 0)
        }
    }
    deserializer.deserialize_any(BoolVisitor)
}

fn default_post_visibility() -> String {
    "public".to_string()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DirectMessage {
    pub id: String,
    pub conversation_id: String,
    pub sender_pubkey: String,
    #[serde(default, alias = "username")]
    pub sender_handle: String,
    #[serde(default, alias = "display_name")]
    pub sender_display: Option<String>,
    #[serde(default, alias = "avatar_url")]
    pub author_avatar_url: Option<String>,
    #[serde(default, alias = "accent")]
    pub author_accent: Option<String>,
    #[serde(default, alias = "roblox_id")]
    pub author_roblox_id: Option<u64>,
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub read_by: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ConversationMember {
    pub pubkey: String,
    pub handle: String,
    pub joined_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Conversation {
    pub id: String,
    #[serde(default, deserialize_with = "deserialize_bool")]
    pub is_group: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub members: Vec<ConversationMember>,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message: Option<DirectMessage>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ReadState {
    pub conversation_id: String,
    pub pubkey: String,
    pub last_read_message_id: String,
    pub last_read_timestamp: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TypingState {
    pub target_id: String,
    pub pubkey: String,
    pub handle: String,
    pub expires_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChannelKind {
    Text,
    Voice,
    Announcement,
    Forum,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GuildRole {
    pub id: String,
    pub guild_id: String,
    pub name: String,
    pub color: String,
    pub hoist: bool,
    pub position: u32,
    pub permissions: PermissionFlags,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GuildChannel {
    pub id: String,
    pub guild_id: String,
    pub name: String,
    #[serde(default)]
    pub topic: String,
    pub kind: ChannelKind,
    pub position: u32,
    #[serde(default)]
    pub permission_overwrites: HashMap<String, u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GuildMember {
    pub guild_id: String,
    pub pubkey: String,
    pub handle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(default)]
    pub role_ids: Vec<String>,
    pub joined_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GuildInvite {
    pub code: String,
    pub guild_id: String,
    pub inviter_pubkey: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<u32>,
    #[serde(default)]
    pub uses: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GuildBan {
    pub guild_id: String,
    pub pubkey: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub banner_pubkey: String,
    pub created_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GuildMessage {
    pub id: String,
    pub guild_id: String,
    pub channel_id: String,
    pub author_pubkey: String,
    #[serde(default, alias = "username")]
    pub author_handle: String,
    #[serde(default, alias = "avatar_url")]
    pub author_avatar_url: Option<String>,
    #[serde(default, alias = "roblox_id")]
    pub author_roblox_id: Option<u64>,
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_bool")]
    pub pinned: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Guild {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banner_url: Option<String>,
    pub owner_pubkey: String,
    pub created_at: u64,
    #[serde(default)]
    pub channels: Vec<GuildChannel>,
    #[serde(default)]
    pub roles: Vec<GuildRole>,
    #[serde(default)]
    pub member_count: u32,
    #[serde(default)]
    pub default_role_id: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    Mention,
    Reply,
    Like,
    Repost,
    Follow,
    DirectMessage,
    GuildInvite,
    SystemAnnouncement,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct BlockRelation {
    pub blocker_pubkey: String,
    pub blocked_pubkey: String,
    pub created_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub id: String,
    pub recipient_pubkey: String,
    pub actor_pubkey: String,
    pub actor_handle: String,
    pub kind: NotificationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub preview: String,
    pub created_at: u64,
    pub read: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct UserProfile {
    pub pubkey: String,
    #[serde(default, alias = "username")]
    pub handle: String,
    pub display_name: String,
    #[serde(default)]
    pub bio: String,
    #[serde(default, alias = "accent")]
    pub accent_color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banner_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roblox_id: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_bool")]
    pub roblox_verified: bool,
    #[serde(default)]
    pub followers_count: u32,
    #[serde(default)]
    pub following_count: u32,
    #[serde(default)]
    pub posts_count: u32,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub instance: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FollowRelation {
    pub follower_pubkey: String,
    pub followed_pubkey: String,
    pub created_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct SearchResults {
    pub query: String,
    #[serde(default)]
    pub posts: Vec<Post>,
    #[serde(default)]
    pub users: Vec<UserProfile>,
    #[serde(default)]
    pub guilds: Vec<Guild>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModActionType {
    Warn,
    Mute,
    Kick,
    Ban,
    DeleteContent,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ModerationAction {
    pub id: String,
    pub target_pubkey: String,
    pub action_type: ModActionType,
    pub moderator_pubkey: String,
    pub reason: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guild_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportTargetType {
    Post,
    User,
    GuildMessage,
    DirectMessage,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Pending,
    Resolved,
    Dismissed,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub id: String,
    pub reporter_pubkey: String,
    pub target_type: ReportTargetType,
    pub target_id: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    pub created_at: u64,
    pub status: ReportStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_by: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AuditLogEntry {
    pub id: String,
    pub actor_pubkey: String,
    pub action: String,
    pub target_type: String,
    pub target_id: String,
    #[serde(default)]
    pub changes: serde_json::Value,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guild_id: Option<String>,
}
