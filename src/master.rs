use crate::boombox::urlencoding_decode;
use crate::community::client::{
    CreateConversationPayload, CreateGuildPayload, CreatePostPayload,
    BlockPayload, DeleteDmPayload, DeleteGuildMessagePayload, DeletePostPayload, EditDmPayload,
    EditGuildMessagePayload, EditPostPayload, FollowPayload, InteractPostPayload, JoinGuildPayload,
    MarkReadPayload, ModerationPayload, ReportPayload,
    SendDmPayload, SendGuildMessagePayload,
};
use crate::community::crypto::{self, CommunityEnvelope};
use crate::community::federation::{
    build_nodeinfo, sync_posts_from_peer, verify_federated_post, FederationPeer, HandshakeRequest,
    HandshakeResponse, DEFAULT_INSTANCE, FEDERATION_MAX_AGE_SECS,
};
use crate::community::types::{
    AuditLogEntry, BlockRelation, ChannelKind, Conversation, ConversationMember, DirectMessage,
    FollowRelation,
    Guild, GuildChannel, GuildInvite, GuildMember, GuildMessage, GuildRole, ModerationAction,
    Notification, NotificationKind,
    PermissionFlags, Post, PostInteraction, Report, ReportStatus, SearchResults, UserProfile,
};
use crate::identity::{self, hex, token_of, unhex, unix_now, Identity};
use crate::net::relay::{parse_invite, query_stats};
use crate::privacy::is_public_ip;
use crate::profile::{self, HostInfo, PublicProfile, SignedProfile};
use crate::rbxapi::{self, RobloxUser};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::net::ToSocketAddrs;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

pub const DEFAULT_MASTER: &str = "https://roblox.puppytunnel.vip";
pub const FALLBACK_MASTER: &str = "https://updates.puppytunnel.vip";
const PEER_SYNC: Duration = Duration::from_secs(45);
const MAX_IMPORTED: usize = 2000;
const TTL: Duration = Duration::from_secs(90);
const ANNOUNCE_EVERY: Duration = Duration::from_secs(30);
const MAX_ENTRIES: usize = 5000;
const MAX_PER_IP: usize = 50;
const MAX_ANNOUNCES_PER_MIN: u32 = 120;
const MAX_PROBES: usize = 32;
const COMMUNITY_MAX_AGE_SECS: u64 = 45 * 24 * 3600;
const CLEANUP_INTERVAL: Duration = Duration::from_secs(3600);
const COMMUNITY_SAVE: Duration = Duration::from_secs(30);
const FRESH_DIRECT_SECS: u64 = 120;
const FRESH_PEER_SECS: u64 = 300;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Announce {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub map: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub max_players: u32,
    pub invite: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub host_sig: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub console_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overhead_tags: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict_profiles: Option<bool>,
    #[serde(default)]
    pub ts: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SignedAnnounce {
    pub pubkey: String,
    pub sig: String,
    #[serde(flatten)]
    pub body: Announce,
}

fn signing_bytes(b: &Announce) -> Vec<u8> {
    let mut v = b"puppy-list\0".to_vec();
    v.extend_from_slice(&serde_json::to_vec(b).unwrap_or_default());
    v
}

fn remove_message(ts: u64) -> Vec<u8> {
    let mut v = b"puppy-remove\0".to_vec();
    v.extend_from_slice(&ts.to_be_bytes());
    v
}

impl SignedAnnounce {
    pub fn sign(id: &Identity, mut body: Announce) -> Self {
        body.ts = unix_now();
        let sig = id.sign(&signing_bytes(&body));
        Self { pubkey: hex(&id.public()), sig: hex(&sig), body }
    }

    fn verify(&self, max_age: u64) -> Result<[u8; 32], &'static str> {
        let pk = unhex::<32>(&self.pubkey).ok_or("bad public key")?;
        let sig = unhex::<64>(&self.sig).ok_or("bad signature")?;
        if unix_now().abs_diff(self.body.ts) > max_age {
            return Err("stale or future timestamp");
        }
        if !identity::verify(&pk, &signing_bytes(&self.body), &sig) {
            return Err("signature does not match");
        }
        Ok(pk)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Listing {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub map: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub players: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub console_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overhead_tags: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict_profiles: Option<bool>,
    #[serde(default)]
    pub max_players: u32,
    pub invite: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RemoteServer {
    pub id: String,
    pub age_secs: u64,
    #[serde(default)]
    pub host: Option<HostInfo>,
    #[serde(flatten)]
    pub listing: Listing,
}

pub fn strict_profile_ok(display_name: &str) -> bool {
    !display_name.trim().is_empty()
}
fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

pub fn parse_tags(raw: &str) -> Vec<String> {
    raw.split(',').map(|t| t.trim().to_ascii_lowercase()).filter(|t| !t.is_empty()).collect()
}

fn to_listing(a: &Announce) -> Result<Listing, &'static str> {
    let name = clean(&a.name, 48);
    if name.is_empty() {
        return Err("name is required");
    }
    if parse_invite(&a.invite).is_none() || a.invite.len() > 200 {
        return Err("invite must be a valid puppy:// invite");
    }
    let mut tags: Vec<String> = a
        .tags
        .iter()
        .map(|t| clean(t, 16).to_ascii_lowercase())
        .map(|t| t.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>())
        .filter(|t| !t.is_empty())
        .collect();
    tags.dedup();
    tags.truncate(8);
    Ok(Listing {
        name,
        description: clean(&a.description, 200),
        map: clean(&a.map, 48),
        tags,
        players: 0,
        max_players: a.max_players.min(1000),
        invite: a.invite.clone(),
        version: clean(&a.version, 16),
        console_policy: a.console_policy.clone(),
        overhead_tags: a.overhead_tags,
        strict_profiles: a.strict_profiles,
    })
}

struct Entry {
    signed: SignedAnnounce,
    listing: Listing,
    ip: String,
    seen: Instant,
    first: Instant,
    imported: bool,
    verified: bool,
    host_key: String,
}

struct StoredProfile {
    signed: SignedProfile,
    user: Option<RobloxUser>,
    verified: bool,
    last_verified_unix: u64,
    seen: Instant,
}

const PROFILE_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const MAX_PROFILES: usize = 20000;
const VERIFY_CACHE_TTL_SECS: u64 = 7 * 24 * 3600;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CachedProfileVerification {
    pub pubkey: String,
    pub roblox_id: u64,
    pub verified: bool,
    pub last_verified_unix: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<RobloxUser>,
}

fn profiles_cache_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("profiles_cache.json")
}

fn profiles_store_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("profiles_store.json")
}

fn load_profiles_store(dir: &std::path::Path) -> Vec<SignedProfile> {
    std::fs::read_to_string(profiles_store_path(dir))
        .ok()
        .and_then(|c| serde_json::from_str::<Vec<SignedProfile>>(&c).ok())
        .unwrap_or_default()
}

fn save_profiles_store(dir: &std::path::Path, list: &[SignedProfile]) {
    if let Ok(json) = serde_json::to_string_pretty(list) {
        let _ = std::fs::create_dir_all(dir);
        let tmp = profiles_store_path(dir).with_extension("tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, profiles_store_path(dir));
        }
    }
}

fn load_profiles_cache(dir: &std::path::Path) -> HashMap<String, CachedProfileVerification> {
    let path = profiles_cache_path(dir);
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(records) = serde_json::from_str::<Vec<CachedProfileVerification>>(&content) {
            let now = unix_now();
            let mut map = HashMap::new();
            for r in records {
                if r.verified && now.saturating_sub(r.last_verified_unix) < VERIFY_CACHE_TTL_SECS {
                    map.insert(r.pubkey.clone(), r);
                }
            }
            return map;
        }
    }
    HashMap::new()
}

fn save_profiles_cache(dir: &std::path::Path, cache: &HashMap<String, CachedProfileVerification>) {
    let path = profiles_cache_path(dir);
    let records: Vec<&CachedProfileVerification> = cache.values().collect();
    if let Ok(json) = serde_json::to_string_pretty(&records) {
        let _ = std::fs::write(path, json);
    }
}

fn community_store_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("community_store.json")
}

#[derive(Serialize, Deserialize, Default)]
struct CommunityStore {
    #[serde(default)]
    posts: HashMap<String, Post>,
    #[serde(default)]
    post_envelopes: HashMap<String, CommunityEnvelope<Post>>,
    #[serde(default)]
    post_order: Vec<String>,
    #[serde(default)]
    interactions: Vec<PostInteraction>,
    #[serde(default)]
    liked_by: HashMap<String, Vec<String>>,
    #[serde(default)]
    conversations: HashMap<String, Conversation>,
    #[serde(default)]
    dms: HashMap<String, Vec<DirectMessage>>,
    #[serde(default)]
    guilds: HashMap<String, Guild>,
    #[serde(default)]
    guild_messages: HashMap<String, Vec<GuildMessage>>,
    #[serde(default)]
    guild_invites: HashMap<String, GuildInvite>,
    #[serde(default)]
    guild_members: HashMap<String, Vec<GuildMember>>,
    #[serde(default)]
    profiles: HashMap<String, UserProfile>,
    #[serde(default)]
    follows: Vec<FollowRelation>,
    #[serde(default)]
    blocks: Vec<BlockRelation>,
    #[serde(default)]
    reports: Vec<Report>,
    #[serde(default)]
    audit_log: Vec<AuditLogEntry>,
    #[serde(default)]
    notifications: Vec<Notification>,
}

fn load_community_store(dir: &std::path::Path) -> CommunityStore {
    std::fs::read_to_string(community_store_path(dir))
        .ok()
        .and_then(|c| serde_json::from_str::<CommunityStore>(&c).ok())
        .unwrap_or_default()
}

fn save_community_store(dir: &std::path::Path, st: &State) {
    let snap = CommunityStore {
        posts: st.community_posts.clone(),
        post_envelopes: st.community_post_envelopes.clone(),
        post_order: st.community_post_order.clone(),
        interactions: st.community_interactions.clone(),
        liked_by: st
            .community_liked_by
            .iter()
            .map(|(k, v)| (k.clone(), v.iter().cloned().collect()))
            .collect(),
        conversations: st.community_conversations.clone(),
        dms: st.community_dms.clone(),
        guilds: st.community_guilds.clone(),
        guild_messages: st.community_guild_messages.clone(),
        guild_invites: st.community_guild_invites.clone(),
        guild_members: st.community_guild_members.clone(),
        profiles: st.community_profiles.clone(),
        follows: st.community_follows.clone(),
        blocks: st.community_blocks.clone(),
        reports: st.community_reports.clone(),
        audit_log: st.community_audit_log.clone(),
        notifications: st.community_notifications.clone(),
    };
    if let Ok(json) = serde_json::to_string(&snap) {
        let _ = std::fs::create_dir_all(dir);
        let tmp = community_store_path(dir).with_extension("tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, community_store_path(dir));
        }
    }
}

fn apply_community_store(st: &mut State, snap: CommunityStore) {
    st.community_posts = snap.posts;
    st.community_post_envelopes = snap.post_envelopes;
    st.community_post_order = snap.post_order;
    st.community_interactions = snap.interactions;
    st.community_liked_by = snap
        .liked_by
        .into_iter()
        .map(|(k, v)| (k, v.into_iter().collect()))
        .collect();
    st.community_conversations = snap.conversations;
    st.community_dms = snap.dms;
    st.community_guilds = snap.guilds;
    st.community_guild_messages = snap.guild_messages;
    st.community_guild_invites = snap.guild_invites;
    st.community_guild_members = snap.guild_members;
    st.community_profiles = snap.profiles;
    st.community_follows = snap.follows;
    st.community_blocks = snap.blocks;
    st.community_reports = snap.reports;
    st.community_audit_log = snap.audit_log;
    st.community_notifications = snap.notifications;
}

struct State {
    profiles: HashMap<String, StoredProfile>,
    verification_cache: HashMap<String, CachedProfileVerification>,
    peers: Vec<String>,
    entries: HashMap<String, Entry>,
    rate: HashMap<String, (u32, Instant)>,
    community_posts: HashMap<String, Post>,
    community_post_envelopes: HashMap<String, CommunityEnvelope<Post>>,
    community_post_order: Vec<String>,
    community_interactions: Vec<PostInteraction>,
    community_liked_by: std::collections::HashMap<String, std::collections::HashSet<String>>,
    community_conversations: HashMap<String, Conversation>,
    community_dms: HashMap<String, Vec<DirectMessage>>,
    community_guilds: HashMap<String, Guild>,
    community_guild_messages: HashMap<String, Vec<GuildMessage>>,
    community_guild_invites: HashMap<String, GuildInvite>,
    community_guild_members: HashMap<String, Vec<GuildMember>>,
    community_profiles: HashMap<String, UserProfile>,
    community_follows: Vec<FollowRelation>,
    community_blocks: Vec<BlockRelation>,
    community_reports: Vec<Report>,
    community_audit_log: Vec<AuditLogEntry>,
    community_notifications: Vec<Notification>,
    instance_id: Identity,
    instance_name: String,
    last_cleanup: Instant,
}

impl Default for State {
    fn default() -> Self {
        Self {
            profiles: HashMap::new(),
            verification_cache: HashMap::new(),
            peers: Vec::new(),
            entries: HashMap::new(),
            rate: HashMap::new(),
            community_posts: HashMap::new(),
            community_post_envelopes: HashMap::new(),
            community_post_order: Vec::new(),
            community_interactions: Vec::new(),
            community_liked_by: std::collections::HashMap::new(),
            community_conversations: HashMap::new(),
            community_dms: HashMap::new(),
            community_guilds: HashMap::new(),
            community_guild_messages: HashMap::new(),
            community_guild_invites: HashMap::new(),
            community_guild_members: HashMap::new(),
            community_profiles: HashMap::new(),
            community_follows: Vec::new(),
            community_blocks: Vec::new(),
            community_reports: Vec::new(),
            community_audit_log: Vec::new(),
            community_notifications: Vec::new(),
            instance_id: Identity::ephemeral(),
            instance_name: DEFAULT_INSTANCE.to_string(),
            last_cleanup: Instant::now(),
        }
    }
}

fn push_notification(
    st: &mut State,
    recipient: &str,
    actor: &str,
    kind: NotificationKind,
    target_id: Option<String>,
    preview: String,
    ts: u64,
) {
    if recipient.is_empty() || recipient == actor {
        return;
    }
    let actor_handle = st
        .community_profiles
        .get(actor)
        .map(|p| p.handle.clone())
        .unwrap_or_else(|| format!("{}@{}", id_of(actor), st.instance_name));
    st.community_notifications.push(Notification {
        id: uuid::Uuid::new_v4().to_string(),
        recipient_pubkey: recipient.to_string(),
        actor_pubkey: actor.to_string(),
        actor_handle,
        kind,
        target_id,
        preview: preview.chars().take(140).collect(),
        created_at: ts,
        read: false,
    });
    if st.community_notifications.len() > 500 {
        let drop_n = st.community_notifications.len() - 500;
        st.community_notifications.drain(..drop_n);
    }
}

fn is_owner(st: &State, pubkey: &str) -> bool {
    st.profiles
        .get(pubkey)
        .map(|p| p.verified && p.user.as_ref().map(|u| u.id == 2721761670).unwrap_or(false))
        .unwrap_or(false)
}

fn is_fallback_handle(handle: &str, instance: &str) -> bool {
    match handle.split_once('@') {
        Some((user, dom)) => {
            dom == instance && user.len() == 8 && user.chars().all(|c| c.is_ascii_hexdigit())
        }
        None => false,
    }
}

fn refresh_profile(
    roblox_base: &str,
    verified_user: Option<crate::rbxapi::RobloxUser>,
    instance: &str,
    pk: &str,
    prof: &mut UserProfile,
) {
    if let Some(u) = verified_user {
        if prof.roblox_id != Some(u.id) {
            prof.roblox_id = Some(u.id);
        }
        if prof.handle.is_empty() || is_fallback_handle(&prof.handle, instance) {
            prof.handle = format!("@{}", u.name);
        }
        if prof.display_name.trim().is_empty() {
            prof.display_name = u.display_name.clone();
        }
        prof.roblox_verified = true;
    } else {
        prof.roblox_verified = false;
    }
    let need_fetch = prof.roblox_id.is_some()
        && (prof.avatar_url.is_none()
            || prof.handle.is_empty()
            || is_fallback_handle(&prof.handle, instance)
            || prof.display_name.trim().is_empty());
    if need_fetch {
        if let Some(rid) = prof.roblox_id {
            if let Ok(u) = crate::rbxapi::user_by_id(roblox_base, rid) {
                if prof.handle.is_empty() || is_fallback_handle(&prof.handle, instance) {
                    prof.handle = format!("@{}", u.name);
                }
                if prof.display_name.trim().is_empty() {
                    prof.display_name = u.display_name.clone();
                }
                if prof.avatar_url.is_none() {
                    if let Ok(url) = crate::rbxapi::thumbnail_url("avatar-headshot", rid, "150x150") {
                        prof.avatar_url = Some(url);
                    }
                }
            }
        }
    }
    if prof.handle.is_empty() {
        prof.handle = format!("{}@{}", id_of(pk), instance);
    }
}

fn cached_handle(st: &State, pk: &str) -> String {
    st.community_profiles
        .get(pk)
        .map(|p| p.handle.clone())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| format!("{}@{}", id_of(pk), st.instance_name))
}

#[allow(clippy::too_many_arguments)]
fn author_identity(
    ctx: &Ctx,
    st: &mut State,
    pk: &str,
) -> (String, Option<String>, Option<String>, Option<String>, Option<u64>, bool) {
    let verified_user = st
        .profiles
        .get(pk)
        .filter(|p| p.verified)
        .and_then(|p| p.user.clone());
    let mut prof = st.community_profiles.get(pk).cloned().unwrap_or_else(|| UserProfile {
        pubkey: pk.to_string(),
        instance: st.instance_name.clone(),
        ..Default::default()
    });
    let instance = st.instance_name.clone();
    refresh_profile(&ctx.roblox, verified_user, &instance, pk, &mut prof);
    let out = (
        prof.handle.clone(),
        if prof.display_name.trim().is_empty() { None } else { Some(prof.display_name.clone()) },
        prof.avatar_url.clone(),
        if prof.accent_color.trim().is_empty() { None } else { Some(prof.accent_color.clone()) },
        prof.roblox_id,
        prof.roblox_verified,
    );
    if st.community_profiles.get(pk) != Some(&prof) {
        st.community_profiles.insert(pk.to_string(), prof);
    }
    out
}

fn refresh_post(st: &State, p: &mut Post) {
    if let Some(prof) = st.community_profiles.get(&p.author_pubkey) {
        if !prof.handle.is_empty() {
            p.author_handle = prof.handle.clone();
        }
        if !prof.display_name.trim().is_empty() {
            p.author_display = Some(prof.display_name.clone());
        }
        if prof.avatar_url.is_some() {
            p.author_avatar_url = prof.avatar_url.clone();
        }
        if prof.roblox_id.is_some() {
            p.author_roblox_id = prof.roblox_id;
        }
        p.author_roblox_verified = prof.roblox_verified;
    }
}

fn refresh_guild_msg(st: &State, m: &mut GuildMessage) {
    if let Some(prof) = st.community_profiles.get(&m.author_pubkey) {
        if !prof.handle.is_empty() {
            m.author_handle = prof.handle.clone();
        }
        if prof.avatar_url.is_some() {
            m.author_avatar_url = prof.avatar_url.clone();
        }
        if prof.roblox_id.is_some() {
            m.author_roblox_id = prof.roblox_id;
        }
    }
}

fn refresh_dm(st: &State, m: &mut DirectMessage) {
    if let Some(prof) = st.community_profiles.get(&m.sender_pubkey) {
        if !prof.handle.is_empty() {
            m.sender_handle = prof.handle.clone();
        }
        if !prof.display_name.trim().is_empty() {
            m.sender_display = Some(prof.display_name.clone());
        }
        if prof.avatar_url.is_some() {
            m.author_avatar_url = prof.avatar_url.clone();
        }
        if prof.roblox_id.is_some() {
            m.author_roblox_id = prof.roblox_id;
        }
    }
}

fn cleanup_community_data(st: &mut State) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cutoff = now.saturating_sub(COMMUNITY_MAX_AGE_SECS);

    let old_posts: Vec<String> = st
        .community_posts
        .iter()
        .filter(|(_, p)| p.created_at < cutoff)
        .map(|(id, _)| id.clone())
        .collect();
    for id in &old_posts {
        st.community_posts.remove(id);
        st.community_post_order.retain(|pid| pid != id);
    }

    for msgs in st.community_dms.values_mut() {
        msgs.retain(|m| m.created_at >= cutoff);
    }

    for msgs in st.community_guild_messages.values_mut() {
        msgs.retain(|m| m.created_at >= cutoff);
    }
}

fn id_of(pubkey: &str) -> String {
    pubkey.chars().take(8).collect()
}

#[derive(Clone, Default)]
pub struct MasterOptions {
    pub trust_proxy: bool,
    pub peers: Vec<String>,
    pub allow_private_relays: bool,
    pub roblox_api: Option<String>,
    pub instance_name: Option<String>,
    pub instance_identity: Option<Identity>,
    pub persist_profiles_cache: bool,
    pub data_dir: Option<std::path::PathBuf>,
    pub strict_profiles: bool,
}

#[derive(Clone)]
struct Ctx {
    roblox: String,
    state: Arc<Mutex<State>>,
    probes: Arc<AtomicUsize>,
    allow_private: bool,
    persist_profiles: bool,
    data_dir: std::path::PathBuf,
    strict_profiles: bool,
}

pub struct MasterServer {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MasterServer {
    pub fn start(listen: &str, opts: MasterOptions) -> Result<Self, String> {
        Self::start_with(listen, opts, PEER_SYNC)
    }

    fn start_with(listen: &str, opts: MasterOptions, sync_every: Duration) -> Result<Self, String> {
        let server = Server::http(listen).map_err(|e| format!("cannot bind master on {}: {}", listen, e))?;
        let running = Arc::new(AtomicBool::new(true));
        let run = Arc::clone(&running);
        let peers: Vec<String> = opts.peers.iter().map(|p| normalize_url(p)).collect();
        let instance_id = opts.instance_identity.clone().unwrap_or_else(Identity::ephemeral);
        let instance_name = opts.instance_name.clone().unwrap_or_else(|| DEFAULT_INSTANCE.to_string());
        let data_dir = opts.data_dir.clone().unwrap_or_else(crate::config::get_script_dir);
        let initial_cache = if opts.persist_profiles_cache {
            load_profiles_cache(&data_dir)
        } else {
            HashMap::new()
        };
        let stored_profiles = if opts.persist_profiles_cache { load_profiles_store(&data_dir) } else { Vec::new() };
        let mut initial_state = State {
            verification_cache: initial_cache,
            peers: peers.clone(),
            instance_id,
            instance_name,
            ..Default::default()
        };
        if opts.persist_profiles_cache {
            apply_community_store(&mut initial_state, load_community_store(&data_dir));
        }
        let ctx = Ctx {
            state: Arc::new(Mutex::new(initial_state)),
            probes: Arc::new(AtomicUsize::new(0)),
            allow_private: opts.allow_private_relays,
            roblox: opts.roblox_api.clone().unwrap_or_else(|| rbxapi::USERS_API.to_string()),
            persist_profiles: opts.persist_profiles_cache,
            strict_profiles: opts.strict_profiles,
            data_dir,
        };
        for signed in stored_profiles {
            let Ok(pk) = signed.verify(u64::MAX / 2) else { continue };
            let key = hex(&pk);
            let id = signed.body.roblox_id;
            let cached = ctx.state.lock().unwrap().verification_cache.get(&key).cloned();
            let (user, verified, last) = match cached {
                Some(c) => (c.user, c.verified, c.last_verified_unix),
                None => (None, false, 0),
            };
            ctx.state.lock().unwrap().profiles.insert(
                key.clone(),
                StoredProfile { signed, user, verified, last_verified_unix: last, seen: Instant::now() },
            );
            if !verified {
                spawn_profile_check(&ctx, key, id);
            }
        }
        let trust_proxy = opts.trust_proxy;

        let thread = thread::spawn(move || {
            let mut last_sync: Option<Instant> = None;
            let mut last_csave: Option<Instant> = None;
            while run.load(Ordering::SeqCst) {
                if !peers.is_empty() && last_sync.map_or(true, |t| t.elapsed() >= sync_every) {
                    last_sync = Some(Instant::now());
                    sync_peers(&peers, &ctx);
                }
                if ctx.persist_profiles && last_csave.map_or(true, |t| t.elapsed() >= COMMUNITY_SAVE) {
                    last_csave = Some(Instant::now());
                    if let Ok(st) = ctx.state.lock() {
                        save_community_store(&ctx.data_dir, &st);
                    }
                }
                match server.recv_timeout(Duration::from_millis(500)) {
                    Ok(Some(req)) => handle(req, &ctx, trust_proxy),
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        });
        Ok(Self { running, thread: Some(thread) })
    }

    pub fn stop(&mut self) {
        if self.running.swap(false, Ordering::SeqCst) {
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

impl Drop for MasterServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn respond(req: Request, code: u16, ctype: &str, body: String, cors: bool) {
    let mut r = Response::from_string(body)
        .with_status_code(StatusCode(code))
        .with_header(Header::from_bytes(&b"Content-Type"[..], ctype.as_bytes()).unwrap());
    if cors {
        r = r.with_header(Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
    }
    let _ = req.respond(r);
}

fn json(req: Request, code: u16, v: serde_json::Value, cors: bool) {
    respond(req, code, "application/json", v.to_string(), cors);
}

fn client_ip(req: &Request, trust_proxy: bool) -> String {
    if trust_proxy {
        for h in req.headers() {
            let n = h.field.as_str().as_str();
            if n.eq_ignore_ascii_case("cf-connecting-ip") || n.eq_ignore_ascii_case("x-forwarded-for") {
                if let Some(ip) = h.value.as_str().split(',').next() {
                    return ip.trim().to_string();
                }
            }
        }
    }
    req.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default()
}

fn ingest(ctx: &Ctx, signed: SignedAnnounce, ip: &str, imported: bool) -> Result<(), (u16, &'static str)> {
    let max_age = if imported { FRESH_PEER_SECS } else { FRESH_DIRECT_SECS };
    let pk = signed.verify(max_age).map_err(|e| (400, e))?;
    let listing = to_listing(&signed.body).map_err(|e| (400, e))?;
    let (_, _, token) = parse_invite(&listing.invite).ok_or((400, "bad invite"))?;
    if token != token_of(&pk) {
        return Err((400, "invite is not bound to this key"));
    }

    let mut host_key = String::new();
    if !signed.body.host.is_empty() {
        let hk = unhex::<32>(&signed.body.host).ok_or((400, "bad host key"))?;
        let hs = unhex::<64>(&signed.body.host_sig).ok_or((400, "bad host signature"))?;
        if !identity::verify(&hk, &profile::host_message(&pk), &hs) {
            return Err((400, "host binding invalid"));
        }
        host_key = signed.body.host.clone();
    }

    let key = signed.pubkey.clone();
    let mut st = ctx.state.lock().unwrap();
    st.entries.retain(|_, e| e.seen.elapsed() < TTL);

    if st.entries.get(&key).map_or(false, |e| !e.imported) && imported {
        return Ok(());
    }
    let lname = listing.name.to_lowercase();
    if st.entries.iter().any(|(k, e)| *k != key && e.listing.name.to_lowercase() == lname) {
        return Err((409, "name taken by another server"));
    }

    let is_new = !st.entries.contains_key(&key);
    if !imported {
        let slot = st.rate.entry(ip.to_string()).or_insert((0, Instant::now()));
        if slot.1.elapsed() > Duration::from_secs(60) {
            *slot = (0, Instant::now());
        }
        slot.0 += 1;
        if slot.0 > MAX_ANNOUNCES_PER_MIN {
            return Err((429, "rate limited"));
        }
        let from_ip = st.entries.values().filter(|e| e.ip == ip).count();
        if is_new && (st.entries.len() >= MAX_ENTRIES || from_ip >= MAX_PER_IP) {
            return Err((429, "list full"));
        }
    } else {
        let imported_count = st.entries.values().filter(|e| e.imported).count();
        if is_new && imported_count >= MAX_IMPORTED {
            return Err((429, "too many imported listings"));
        }
    }

    let now = Instant::now();
    let (first, verified, players) = match st.entries.get(&key) {
        Some(e) if e.listing.invite == listing.invite => (e.first, e.verified, e.listing.players),
        Some(e) => (e.first, false, 0),
        None => (now, false, 0),
    };
    let mut listing = listing;
    listing.players = players;
    st.entries.insert(
        key.clone(),
        Entry { signed, listing: listing.clone(), ip: ip.to_string(), seen: now, first, imported, verified, host_key },
    );
    drop(st);
    spawn_probe(ctx, key, listing.invite);
    Ok(())
}

fn spawn_probe(ctx: &Ctx, key: String, invite: String) {
    if ctx.probes.fetch_add(1, Ordering::SeqCst) >= MAX_PROBES {
        ctx.probes.fetch_sub(1, Ordering::SeqCst);
        return;
    }
    let ctx = ctx.clone();
    thread::spawn(move || {
        let result = parse_invite(&invite).and_then(|(host, port, token)| {
            let addr = (host.as_str(), port).to_socket_addrs().ok()?.find(|a| a.is_ipv4())?;
            if !ctx.allow_private && !is_public_ip(&addr.ip()) {
                return None;
            }
            query_stats(addr, &token)
        });
        if let Ok(mut st) = ctx.state.lock() {
            if let Some(e) = st.entries.get_mut(&key) {
                match result {
                    Some((true, players)) => {
                        e.verified = true;
                        e.listing.players = players.min(1000);
                    }
                    _ => e.verified = false,
                }
            }
        }
        ctx.probes.fetch_sub(1, Ordering::SeqCst);
    });
}

fn ingest_community_post(
    st: &mut State,
    env: CommunityEnvelope<Post>,
    imported: bool,
    peer_url: Option<&str>,
) -> Result<(), &'static str> {
    if imported {
        let peer = peer_url.map(|u| FederationPeer {
            instance: u.to_string(),
            public_key: String::new(),
            name: u.to_string(),
            last_synced: unix_now(),
            trusted: true,
        });
        verify_federated_post(&env, peer.as_ref(), FRESH_PEER_SECS)?;
    } else {
        crypto::verify_envelope(&env, FRESH_DIRECT_SECS)
            .map_err(|_| "bad post envelope signature or stale timestamp")?;
    }

    let post_id = env.payload.id.clone();
    if !st.community_posts.contains_key(&post_id) {
        st.community_post_order.insert(0, post_id.clone());
    }
    st.community_posts.insert(post_id.clone(), env.payload.clone());
    st.community_post_envelopes.insert(post_id, env);
    Ok(())
}

fn get_manifest_bundle() -> serde_json::Value {
    serde_json::from_str(include_str!("updates_manifest.json")).unwrap_or_default()
}

fn get_update_manifest(channel: &str) -> Option<serde_json::Value> {
    let bundle = get_manifest_bundle();
    match channel.to_lowercase().as_str() {
        "stable" => bundle.get("stable").cloned(),
        "beta" => bundle.get("beta").cloned(),
        "nightly" => bundle.get("nightly").cloned(),
        _ => None,
    }
}

fn get_update_history() -> serde_json::Value {
    let bundle = get_manifest_bundle();
    bundle.get("history").cloned().unwrap_or_else(|| serde_json::json!([]))
}

fn compare_semver(a: &str, b: &str) -> std::cmp::Ordering {
    if a == b {
        return std::cmp::Ordering::Equal;
    }
    if a.is_empty() {
        return std::cmp::Ordering::Less;
    }
    if b.is_empty() {
        return std::cmp::Ordering::Greater;
    }
    let (v_a, pre_a) = a.split_once('-').unwrap_or((a, ""));
    let (v_b, pre_b) = b.split_once('-').unwrap_or((b, ""));

    let parts_a: Vec<u64> = v_a.split('.').filter_map(|x| x.parse().ok()).collect();
    let parts_b: Vec<u64> = v_b.split('.').filter_map(|x| x.parse().ok()).collect();

    for i in 0..parts_a.len().max(parts_b.len()) {
        let na = parts_a.get(i).copied().unwrap_or(0);
        let nb = parts_b.get(i).copied().unwrap_or(0);
        if na != nb {
            return na.cmp(&nb);
        }
    }

    match (pre_a.is_empty(), pre_b.is_empty()) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => pre_a.cmp(pre_b),
        (true, true) => std::cmp::Ordering::Equal,
    }
}

fn handle(mut req: Request, ctx: &Ctx, trust_proxy: bool) {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let params: HashMap<String, String> = query
        .split('&')
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((urlencoding_decode(k), urlencoding_decode(v)))
        })
        .collect();
    let method = req.method().clone();
    let ip = client_ip(&req, trust_proxy);

    if method == Method::Options {
        let r = Response::empty(204)
            .with_header(Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap())
            .with_header(Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, OPTIONS"[..]).unwrap())
            .with_header(Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type, Authorization"[..]).unwrap());
        let _ = req.respond(r);
        return;
    }

    if path.starts_with("/api/community") {
        let mut st = ctx.state.lock().unwrap();
        if st.last_cleanup.elapsed() >= CLEANUP_INTERVAL {
            cleanup_community_data(&mut st);
            st.last_cleanup = Instant::now();
            if ctx.persist_profiles {
                save_community_store(&ctx.data_dir, &st);
            }
        }
    }

    match (method, path) {
        (Method::Get, "/") => respond(req, 200, "text/html; charset=utf-8", BROWSER_HTML.to_string(), false),
        (Method::Get, "/icon.png") | (Method::Get, "/favicon.ico") => {
            let resp = Response::from_data(include_bytes!("../assets/icon/puppytunnel-64.png").to_vec())
                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..]).unwrap());
            let _ = req.respond(resp);
        }
        (Method::Get, "/health") => json(req, 200, serde_json::json!({"ok": true}), false),
        (Method::Get, "/api/peers") => {
            let peers = ctx.state.lock().unwrap().peers.clone();
            json(req, 200, serde_json::json!(peers), true)
        }
        (Method::Get, "/api/signed") => {
            let mut st = ctx.state.lock().unwrap();
            st.entries.retain(|_, e| e.seen.elapsed() < TTL);
            let list: Vec<SignedAnnounce> =
                st.entries.values().filter(|e| e.verified && !e.imported).map(|e| e.signed.clone()).collect();
            drop(st);
            json(req, 200, serde_json::to_value(list).unwrap_or_default(), true);
        }
        (Method::Get, "/api/servers") => {
            let mut st = ctx.state.lock().unwrap();
            st.entries.retain(|_, e| e.seen.elapsed() < TTL);
            let q = params.get("q").map(|s| s.to_lowercase()).unwrap_or_default();
            let tag = params.get("tag").map(|s| s.to_lowercase()).unwrap_or_default();
            let local_only = params.get("local").map_or(false, |v| v == "1");
            let mut list: Vec<RemoteServer> = st
                .entries
                .iter()
                .filter(|(_, e)| {
                    let l = &e.listing;
                    e.verified
                        && (!local_only || !e.imported)
                        && (q.is_empty()
                            || l.name.to_lowercase().contains(&q)
                            || l.map.to_lowercase().contains(&q)
                            || l.tags.iter().any(|t| t.contains(&q)))
                        && (tag.is_empty() || l.tags.contains(&tag))
                })
                .map(|(k, e)| RemoteServer {
                    id: id_of(k),
                    age_secs: e.first.elapsed().as_secs(),
                    host: host_info(&st.profiles, &e.host_key),
                    listing: e.listing.clone(),
                })
                .collect();
            sort_servers(&mut list, params.get("sort").map(|s| s.as_str()).unwrap_or("players"));
            drop(st);
            json(req, 200, serde_json::to_value(list).unwrap_or_default(), true);
        }
        (Method::Get, "/api/profile") => {
            let id = params.get("id").cloned().unwrap_or_default();
            let st = ctx.state.lock().unwrap();
            let found = st.profiles.get(&id).filter(|p| p.seen.elapsed() < PROFILE_TTL);
            let body = found.map(|p| {
                let code = profile::verify_code(&id, p.signed.body.roblox_id);
                match (&p.user, p.verified) {
                    (Some(u), true) => PublicProfile {
                        id: id_of(&id),
                        verified: true,
                        profile_name: p.signed.body.display_name.clone(),
                        roblox_id: u.id,
                        name: u.name.clone(),
                        display_name: u.display_name.clone(),
                        bio: p.signed.body.bio.clone(),
                        accent: p.signed.body.accent.clone(),
                        code,
                    },
                    _ => PublicProfile {
                        id: id_of(&id),
                        verified: false,
                        profile_name: p.signed.body.display_name.clone(),
                        bio: p.signed.body.bio.clone(),
                        accent: p.signed.body.accent.clone(),
                        code,
                        ..Default::default()
                    },
                }
            });
            drop(st);
            match body {
                Some(b) => json(req, 200, serde_json::to_value(b).unwrap_or_default(), true),
                None => json(req, 404, serde_json::json!({"error": "no such profile"}), true),
            }
        }
        (Method::Post, "/api/profile") => {
            let mut body = String::new();
            let _ = req.as_reader().take(8 * 1024).read_to_string(&mut body);
            let Ok(signed) = serde_json::from_str::<SignedProfile>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid profile"}), false);
            };
            match ingest_profile(ctx, signed, &ip) {
                Ok(code) => json(req, 200, serde_json::json!({"ok": true, "code": code}), false),
                Err((c, m)) => json(req, c, serde_json::json!({"error": m}), false),
            }
        }
        (Method::Post, "/api/announce") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(signed) = serde_json::from_str::<SignedAnnounce>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid announcement"}), false);
            };
            match ingest(ctx, signed, &ip, false) {
                Ok(()) => json(req, 200, serde_json::json!({"ok": true}), false),
                Err((code, msg)) => json(req, code, serde_json::json!({"error": msg}), false),
            }
        }
        (Method::Post, "/api/remove") => {
            let mut body = String::new();
            let _ = req.as_reader().take(4 * 1024).read_to_string(&mut body);
            let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let pubkey = v["pubkey"].as_str().unwrap_or("").to_string();
            let ts = v["ts"].as_u64().unwrap_or(0);
            let ok = match (unhex::<32>(&pubkey), unhex::<64>(v["sig"].as_str().unwrap_or(""))) {
                (Some(pk), Some(sig)) => unix_now().abs_diff(ts) <= FRESH_DIRECT_SECS && identity::verify(&pk, &remove_message(ts), &sig),
                _ => false,
            };
            if !ok {
                return json(req, 400, serde_json::json!({"error": "bad signature"}), false);
            }
            let mut st = ctx.state.lock().unwrap();
            if st.entries.get(&pubkey).map_or(false, |e| !e.imported) {
                st.entries.remove(&pubkey);
            }
            drop(st);
            json(req, 200, serde_json::json!({"ok": true}), false);
        }
        (Method::Get, "/api/updates/check") => {
            let channel = params.get("channel").map(|s| s.as_str()).unwrap_or("stable").to_lowercase();
            let curr_version = params.get("version").map(|s| s.as_str()).unwrap_or("");
            let os = params.get("os").map(|s| s.as_str()).unwrap_or("windows").to_lowercase();
            let arch = params.get("arch").map(|s| s.as_str()).unwrap_or("x86_64").to_lowercase();

            let manifest = match get_update_manifest(&channel) {
                Some(m) => m,
                None => {
                    return json(
                        req,
                        400,
                        serde_json::json!({
                            "error": format!("Invalid channel '{}'. Supported: stable, beta, nightly", channel)
                        }),
                        true,
                    );
                }
            };

            let latest_version = manifest["version"].as_str().unwrap_or("");
            let is_newer = curr_version.is_empty()
                || compare_semver(latest_version, curr_version) == std::cmp::Ordering::Greater;

            let platform_key = format!("{}-{}", os, arch);
            let asset = manifest["assets"]
                .get(&platform_key)
                .or_else(|| manifest["assets"].get("windows-x86_64"))
                .or_else(|| manifest["assets_map"].get(&platform_key))
                .or_else(|| manifest["assets_map"].get("windows-x86_64"))
                .or_else(|| manifest["assets"].as_object().and_then(|m| m.values().next()))
                .or_else(|| manifest["assets_map"].as_object().and_then(|m| m.values().next()));

            if is_newer {
                json(
                    req,
                    200,
                    serde_json::json!({
                        "update_available": true,
                        "channel": manifest["channel"],
                        "current_version": if curr_version.is_empty() { "0.0.0" } else { curr_version },
                        "latest_version": latest_version,
                        "release_date": manifest["release_date"],
                        "changelog": manifest["changelog"],
                        "pubkey": manifest["pubkey"],
                        "signature": manifest["signature"],
                        "download_url": asset.and_then(|a| a.get("url")),
                        "filename": asset.and_then(|a| a.get("filename")),
                        "sha256": asset.and_then(|a| a.get("sha256")),
                        "ed25519_signature": asset.and_then(|a| a.get("signature")),
                        "size_bytes": asset.and_then(|a| a.get("size_bytes")),
                        "min_os_version": manifest["min_os_version"],
                        "mandatory": false
                    }),
                    true,
                );
            } else {
                json(
                    req,
                    200,
                    serde_json::json!({
                        "update_available": false,
                        "channel": manifest["channel"],
                        "current_version": curr_version,
                        "latest_version": latest_version,
                        "release_date": manifest["release_date"]
                    }),
                    true,
                );
            }
        }
        (Method::Get, "/api/updates/manifest") => {
            let channel_param = params.get("channel").map(|s| s.to_lowercase());
            if let Some(ch) = channel_param {
                match get_update_manifest(&ch) {
                    Some(m) => json(req, 200, m, true),
                    None => json(
                        req,
                        400,
                        serde_json::json!({
                            "error": format!("Invalid channel '{}'. Supported: stable, beta, nightly", ch)
                        }),
                        true,
                    ),
                }
            } else {
                let stable = get_update_manifest("stable");
                let beta = get_update_manifest("beta");
                let nightly = get_update_manifest("nightly");
                json(
                    req,
                    200,
                    serde_json::json!({
                        "channels": {
                            "stable": stable,
                            "beta": beta,
                            "nightly": nightly
                        }
                    }),
                    true,
                );
            }
        }
        (Method::Get, "/api/updates/history") => {
            json(req, 200, get_update_history(), true);
        }
        (Method::Get, "/.well-known/nodeinfo") => {
            let st = ctx.state.lock().unwrap();
            let host = format!("https://{}", st.instance_name);
            json(
                req,
                200,
                serde_json::json!({
                    "links": [
                        {
                            "rel": "http://nodeinfo.diaspora.software/ns/schema/2.1",
                            "href": format!("{}/nodeinfo/2.1", host)
                        }
                    ]
                }),
                true,
            );
        }
        (Method::Get, "/nodeinfo/2.1") => {
            let st = ctx.state.lock().unwrap();
            let pubkey = hex(&st.instance_id.public());
            let users = st.community_profiles.len() as u64;
            let posts = st.community_posts.len() as u64;
            let nodeinfo = build_nodeinfo(
                &st.instance_name,
                "PuppyTunnel Federated Community Node",
                &pubkey,
                users,
                posts,
                st.peers.clone(),
            );
            drop(st);
            json(req, 200, serde_json::to_value(nodeinfo).unwrap_or_default(), true);
        }
        (Method::Get, "/api/community/posts") => {
            let limit: usize = params.get("limit").and_then(|l| l.parse().ok()).unwrap_or(50).min(100);
            let before = params.get("before").and_then(|b| b.parse().ok());
            let tag = params.get("tag").map(|t| t.to_lowercase());
            let author = params.get("author").cloned();
            let reply_to = params.get("reply_to").cloned();
            let viewer = params.get("viewer").cloned();
            let include_replies = params.contains_key("include_replies") || reply_to.is_some();
            let st = ctx.state.lock().unwrap();
            let posts: Vec<Post> = st
                .community_post_order
                .iter()
                .filter_map(|id| {
                    let mut p = st.community_posts.get(id)?.clone();
                    refresh_post(&st, &mut p);
                    if let Some(ref r) = reply_to {
                        if p.reply_to_id.as_deref() != Some(r) {
                            return None;
                        }
                    } else if !include_replies && p.reply_to_id.is_some() {
                        return None;
                    }
                    if let Some(b) = before {
                        if p.created_at >= b {
                            return None;
                        }
                    }
                    if let Some(ref t) = tag {
                        if !p.tags.iter().any(|pt| pt.to_lowercase() == *t) {
                            return None;
                        }
                    }
                    if let Some(ref a) = author {
                        if p.author_pubkey != *a && p.author_handle != *a {
                            return None;
                        }
                    }
                    if let Some(ref v) = viewer {
                        p.liked_by_me = st.community_liked_by.get(&p.id).map_or(false, |s| s.contains(v));
                    }
                    Some(p)
                })
                .take(limit)
                .collect();
            drop(st);
            json(req, 200, serde_json::to_value(posts).unwrap_or_default(), true);
        }
        (Method::Post, "/api/community/posts") => {
            let mut body = String::new();
            let _ = req.as_reader().take(8 * 1024 * 1024).read_to_string(&mut body);
            if let Ok(env) = serde_json::from_str::<CommunityEnvelope<CreatePostPayload>>(&body) {
                let pk = match crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                    Ok(k) => hex(&k),
                    Err(e) => return json(req, 400, serde_json::json!({"error": e.to_string()}), true),
                };
                let mut st = ctx.state.lock().unwrap();
                let post_id = uuid::Uuid::new_v4().to_string();
                let (author_handle, author_display, author_avatar_url, author_accent, author_roblox_id, author_roblox_verified) =
                author_identity(ctx, &mut st, &pk);
                let post = Post {
                    id: post_id.clone(),
                    author_pubkey: pk,
                    author_handle,
                    author_display,
                    author_avatar_url,
                    author_accent,
                    author_roblox_id,
                    author_roblox_verified,
                    content: clean(&env.payload.content, 4000),
                    attachments: env.payload.attachments,
                    reply_to_id: env.payload.reply_to_id.clone(),
                    repost_of_id: env.payload.repost_of_id,
                    likes_count: 0,
                    reposts_count: 0,
                    replies_count: 0,
                    created_at: env.ts,
                    edited_at: None,
                    tags: env.payload.tags,
                    visibility: env.payload.visibility,
                    federated: true,
                    origin_instance: st.instance_name.clone(),
                    is_pinned: false,
                    liked_by_me: false,
                };
                if let Some(ref parent_id) = post.reply_to_id {
                    if let Some(parent) = st.community_posts.get_mut(parent_id) {
                        parent.replies_count += 1;
                    }
                }
                if let Some(ref parent_id) = post.reply_to_id {
                    let note = st.community_posts.get(parent_id).map(|parent| {
                        (
                            parent.author_pubkey.clone(),
                            format!(
                                "replied: {}",
                                post.content.chars().take(80).collect::<String>()
                            ),
                        )
                    });
                    if let Some((author, preview)) = note {
                        push_notification(
                            &mut st,
                            &author,
                            &post.author_pubkey,
                            crate::community::types::NotificationKind::Reply,
                            Some(post.id.clone()),
                            preview,
                            env.ts,
                        );
                    }
                }
                if let Ok(post_env) = crypto::sign_envelope(&st.instance_id, "post:create", post.clone()) {
                    st.community_post_envelopes.insert(post_id.clone(), post_env);
                }
                st.community_posts.insert(post_id.clone(), post.clone());
                st.community_post_order.insert(0, post_id);
                drop(st);
                return json(req, 200, serde_json::to_value(post).unwrap_or_default(), true);
            }
            if let Ok(env) = serde_json::from_str::<CommunityEnvelope<Post>>(&body) {
                let mut st = ctx.state.lock().unwrap();
                match ingest_community_post(&mut st, env.clone(), false, None) {
                    Ok(()) => {
                        let post = env.payload;
                        drop(st);
                        return json(req, 200, serde_json::to_value(post).unwrap_or_default(), true);
                    }
                    Err(e) => return json(req, 400, serde_json::json!({"error": e}), true),
                }
            }
            json(req, 400, serde_json::json!({"error": "invalid post envelope"}), true);
        }
        (Method::Get, "/api/community/post") => {
            let id = params.get("id").cloned().unwrap_or_default();
            let st = ctx.state.lock().unwrap();
            match st.community_posts.get(&id).map(|p| {
            let mut p = p.clone();
            refresh_post(&st, &mut p);
            p
            }) {
                Some(p) => json(req, 200, serde_json::to_value(p).unwrap_or_default(), true),
                None => json(req, 404, serde_json::json!({"error": "post not found"}), true),
            }
        }
        (Method::Post, "/api/community/post/edit") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<EditPostPayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid edit payload"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            match st.community_posts.get_mut(&env.payload.post_id) {
                Some(post) if post.author_pubkey == env.pubkey => {
                    post.content = clean(&env.payload.content, 4000);
                    post.edited_at = Some(env.ts);
                    let updated = post.clone();
                    drop(st);
                    json(req, 200, serde_json::to_value(updated).unwrap_or_default(), true)
                }
                Some(_) => json(req, 403, serde_json::json!({"error": "not your post"}), true),
                None => json(req, 404, serde_json::json!({"error": "post not found"}), true),
            }
        }
        (Method::Post, "/api/community/post/delete") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<DeletePostPayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid delete payload"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            let allowed = st
                .community_posts
                .get(&env.payload.post_id)
                .map(|p| p.author_pubkey == env.pubkey)
                .unwrap_or(false)
                || is_owner(&st, &env.pubkey);
            if !allowed {
                return json(req, 403, serde_json::json!({"error": "not your post"}), true);
            }
            let mut removed_ids = vec![env.payload.post_id.clone()];
            removed_ids.extend(
                st.community_posts
                    .values()
                    .filter(|p| p.reply_to_id.as_deref() == Some(env.payload.post_id.as_str()))
                    .map(|p| p.id.clone())
                    .collect::<Vec<_>>(),
            );
            let mut parent_id: Option<String> = None;
            for id in &removed_ids {
                if let Some(post) = st.community_posts.remove(id) {
                    if id == &env.payload.post_id {
                        parent_id = post.reply_to_id.clone();
                    }
                }
                st.community_post_order.retain(|pid| pid != id);
                st.community_liked_by.remove(id);
            }
            if let Some(pid) = parent_id {
                if let Some(parent) = st.community_posts.get_mut(&pid) {
                    parent.replies_count = parent.replies_count.saturating_sub(1);
                }
            }
            drop(st);
            json(req, 200, serde_json::json!({"deleted": env.payload.post_id}), true);
        }
        (Method::Post, "/api/community/post/interact") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<InteractPostPayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid interaction payload"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            let author_pubkey = env.pubkey.clone();
            let author_handle = format!("{}@{}", id_of(&author_pubkey), st.instance_name);
            let mut result_liked = false;
            let mut result_likes_count: u32 = 0;
            let pid = env.payload.post_id.clone();
            if env.payload.kind == crate::community::types::InteractionType::Like {
                let is_own = st
                    .community_posts
                    .get(&pid)
                    .map(|p| p.author_pubkey == author_pubkey)
                    .unwrap_or(false);
                if is_own {
                    return json(req, 400, serde_json::json!({"error": "cannot like your own post"}), true);
                }
                let likers = st.community_liked_by.entry(pid.clone()).or_default();
                if likers.contains(&author_pubkey) {
                    likers.remove(&author_pubkey);
                    result_liked = false;
                } else {
                    likers.insert(author_pubkey.clone());
                    result_liked = true;
                }
            }
            if let Some(post) = st.community_posts.get_mut(&pid) {
                match env.payload.kind {
                    crate::community::types::InteractionType::Like => {
                        if result_liked {
                            post.likes_count += 1;
                        } else {
                            post.likes_count = post.likes_count.saturating_sub(1);
                        }
                        result_likes_count = post.likes_count;
                    }
                    crate::community::types::InteractionType::Repost => post.reposts_count += 1,
                    crate::community::types::InteractionType::Reply => post.replies_count += 1,
                }
            }
            if env.payload.kind == crate::community::types::InteractionType::Like && result_liked {
                let note = st.community_posts.get(&pid).map(|post| {
                    (
                        post.author_pubkey.clone(),
                        format!(
                            "liked your post: {}",
                            post.content.chars().take(80).collect::<String>()
                        ),
                    )
                });
                if let Some((author, preview)) = note {
                    push_notification(
                        &mut st,
                        &author,
                        &author_pubkey,
                        crate::community::types::NotificationKind::Like,
                        Some(pid.clone()),
                        preview,
                        env.ts,
                    );
                }
            }
            let interaction = PostInteraction {
                id: uuid::Uuid::new_v4().to_string(),
                post_id: env.payload.post_id.clone(),
                kind: env.payload.kind,
                author_pubkey,
                author_handle,
                created_at: env.ts,
                content: env.payload.content,
                liked: Some(result_liked),
                likes_count: Some(result_likes_count as i64),
            };
            st.community_interactions.push(interaction.clone());
            drop(st);
            json(req, 200, serde_json::to_value(&interaction).unwrap_or_default(), true);
        }
        (Method::Get, "/api/community/conversations") => {
            let pk = params.get("pubkey").cloned().unwrap_or_default();
            let st = ctx.state.lock().unwrap();
            let list: Vec<Conversation> = st
                .community_conversations
                .values()
                .filter(|c| pk.is_empty() || c.members.iter().any(|m| m.pubkey == pk))
                .cloned()
                .map(|mut c| {
                    for m in &mut c.members {
                        if let Some(prof) = st.community_profiles.get(&m.pubkey) {
                            if !prof.handle.is_empty() {
                                m.handle = prof.handle.clone();
                            }
                        }
                    }
                    if let Some(ref mut last) = c.last_message {
                        refresh_dm(&st, last);
                    }
                    c
                })
                .collect();
            drop(st);
            json(req, 200, serde_json::to_value(list).unwrap_or_default(), true);
        }
        (Method::Post, "/api/community/conversations") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<CreateConversationPayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid conversation envelope"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            let conv_id = uuid::Uuid::new_v4().to_string();
            let mut members = vec![ConversationMember {
                pubkey: env.pubkey.clone(),
                handle: cached_handle(&st, &env.pubkey),
                joined_at: env.ts,
                role: Some("admin".to_string()),
            }];
            for rpk in &env.payload.recipient_pubkeys {
                if *rpk != env.pubkey {
                    members.push(ConversationMember {
                        pubkey: rpk.clone(),
                        handle: cached_handle(&st, rpk),
                        joined_at: env.ts,
                        role: Some("member".to_string()),
                    });
                }
            }
            let is_group = members.len() > 2;
            let conv = Conversation {
                id: conv_id.clone(),
                is_group,
                title: env.payload.title,
                members,
                created_at: env.ts,
                updated_at: env.ts,
                last_message: None,
            };
            st.community_conversations.insert(conv_id, conv.clone());
            drop(st);
            json(req, 200, serde_json::to_value(conv).unwrap_or_default(), true);
        }
        (Method::Get, "/api/community/messages") => {
            let conv_id = params.get("conversation_id").cloned().unwrap_or_default();
            let limit: usize = params.get("limit").and_then(|l| l.parse().ok()).unwrap_or(50).min(100);
            let st = ctx.state.lock().unwrap();
            let msgs = st
                .community_dms
                .get(&conv_id)
                .map(|list| {
                    let n = list.len();
                    let mut v = list[n.saturating_sub(limit)..].to_vec();
                    for m in &mut v {
                        refresh_dm(&st, m);
                    }
                    v
                })
                .unwrap_or_default();
            drop(st);
            json(req, 200, serde_json::to_value(msgs).unwrap_or_default(), true);
        }
        (Method::Post, "/api/community/messages") => {
            let mut body = String::new();
            let _ = req.as_reader().take(64 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<SendDmPayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid message envelope"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            let msg_id = uuid::Uuid::new_v4().to_string();
            let (sender_handle, sender_display, sender_avatar_url, sender_accent, sender_roblox_id, _sender_verified) =
            author_identity(ctx, &mut st, &env.pubkey);
            let msg = DirectMessage {
                id: msg_id,
                conversation_id: env.payload.conversation_id.clone(),
                sender_pubkey: env.pubkey.clone(),
                sender_handle,
                sender_display,
                author_avatar_url: sender_avatar_url,
                author_accent: sender_accent,
                author_roblox_id: sender_roblox_id,
                content: clean(&env.payload.content, 4000),
                attachments: env.payload.attachments,
                created_at: env.ts,
                read_by: vec![env.pubkey.clone()],
                reply_to_id: env.payload.reply_to_id,
            };
            st.community_dms
                .entry(env.payload.conversation_id.clone())
                .or_default()
                .push(msg.clone());
            if let Some(c) = st.community_conversations.get_mut(&env.payload.conversation_id) {
                c.updated_at = env.ts;
                c.last_message = Some(msg.clone());
            }
            let others: Vec<String> = st
                .community_conversations
                .get(&env.payload.conversation_id)
                .map(|c| {
                    c.members
                        .iter()
                        .map(|m| m.pubkey.clone())
                        .filter(|pk| pk != &env.pubkey)
                        .collect()
                })
                .unwrap_or_default();
            for pk in others {
                push_notification(
                    &mut st,
                    &pk,
                    &env.pubkey,
                    crate::community::types::NotificationKind::DirectMessage,
                    Some(env.payload.conversation_id.clone()),
                    msg.content.clone(),
                    env.ts,
                );
            }
            drop(st);
            json(req, 200, serde_json::to_value(msg).unwrap_or_default(), true);
        }
        (Method::Post, "/api/community/messages/edit") => {
            let mut body = String::new();
            let _ = req.as_reader().take(64 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<EditDmPayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid edit payload"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            let updated = st
                .community_dms
                .get_mut(&env.payload.conversation_id)
                .and_then(|list| list.iter_mut().find(|m| m.id == env.payload.message_id))
                .filter(|m| m.sender_pubkey == env.pubkey)
                .map(|m| {
                    m.content = clean(&env.payload.content, 4000);
                    m.clone()
                });
            drop(st);
            match updated {
                Some(m) => json(req, 200, serde_json::to_value(m).unwrap_or_default(), true),
                None => json(req, 403, serde_json::json!({"error": "cannot edit this message"}), true),
            }
        }
        (Method::Post, "/api/community/messages/delete") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<DeleteDmPayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid delete payload"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            let updated = st
                .community_dms
                .get_mut(&env.payload.conversation_id)
                .and_then(|list| list.iter_mut().find(|m| m.id == env.payload.message_id))
                .filter(|m| m.sender_pubkey == env.pubkey)
                .map(|m| {
                    m.content = "[deleted]".to_string();
                    m.attachments.clear();
                    m.clone()
                });
            drop(st);
            match updated {
                Some(m) => json(req, 200, serde_json::to_value(m).unwrap_or_default(), true),
                None => json(req, 403, serde_json::json!({"error": "cannot delete this message"}), true),
            }
        }
        (Method::Get, "/api/community/guilds") => {
            let st = ctx.state.lock().unwrap();
            let list: Vec<Guild> = st.community_guilds.values().cloned().collect();
            drop(st);
            json(req, 200, serde_json::to_value(list).unwrap_or_default(), true);
        }
        (Method::Post, "/api/community/guilds") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<CreateGuildPayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid guild envelope"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            let guild_id = uuid::Uuid::new_v4().to_string();
            let channel_id = uuid::Uuid::new_v4().to_string();
            let role_id = uuid::Uuid::new_v4().to_string();
            let default_channel = GuildChannel {
                id: channel_id,
                guild_id: guild_id.clone(),
                name: "general".to_string(),
                topic: "General discussion".to_string(),
                kind: ChannelKind::Text,
                position: 0,
                permission_overwrites: HashMap::new(),
            };
            let default_role = GuildRole {
                id: role_id.clone(),
                guild_id: guild_id.clone(),
                name: "everyone".to_string(),
                color: "#99aab5".to_string(),
                hoist: false,
                position: 0,
                permissions: PermissionFlags::default_member(),
            };
            let guild = Guild {
                id: guild_id.clone(),
                name: clean(&env.payload.name, 64),
                description: clean(&env.payload.description, 500),
                icon_url: env.payload.icon_url,
                banner_url: None,
                owner_pubkey: env.pubkey.clone(),
                created_at: env.ts,
                channels: vec![default_channel],
                roles: vec![default_role],
                member_count: 1,
                default_role_id: role_id,
            };
            let member_handle = format!("{}@{}", id_of(&env.pubkey), st.instance_name);
            st.community_guilds.insert(guild_id.clone(), guild.clone());
            st.community_guild_members
                .entry(guild_id.clone())
                .or_default()
                .push(GuildMember {
                    guild_id,
                    pubkey: env.pubkey.clone(),
                    handle: member_handle,
                    nickname: None,
                    role_ids: vec![],
                    joined_at: env.ts,
                });
            drop(st);
            json(req, 200, serde_json::to_value(guild).unwrap_or_default(), true);
        }
        (Method::Get, "/api/community/guild/members") => {
            let guild_id = params.get("guild_id").cloned().unwrap_or_default();
            let st = ctx.state.lock().unwrap();
            let members = st
                .community_guild_members
                .get(&guild_id)
                .cloned()
                .unwrap_or_default();
            drop(st);
            json(req, 200, serde_json::to_value(members).unwrap_or_default(), true);
        }
        (Method::Get, "/api/community/guild") => {
            let id = params.get("id").cloned().unwrap_or_default();
            let st = ctx.state.lock().unwrap();
            match st.community_guilds.get(&id) {
                Some(g) => json(req, 200, serde_json::to_value(g).unwrap_or_default(), true),
                None => json(req, 404, serde_json::json!({"error": "guild not found"}), true),
            }
        }
        (Method::Post, "/api/community/guild/join") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<JoinGuildPayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid join envelope"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            let guild_id = if let Some(inv) = st.community_guild_invites.get_mut(&env.payload.invite_code) {
                inv.uses += 1;
                inv.guild_id.clone()
            } else if st.community_guilds.contains_key(&env.payload.invite_code) {
                env.payload.invite_code.clone()
            } else {
                return json(req, 404, serde_json::json!({"error": "invite code not found"}), true);
            };
            if let Some(g) = st.community_guilds.get_mut(&guild_id) {
                g.member_count += 1;
            }
            let member = GuildMember {
                guild_id: guild_id.clone(),
                pubkey: env.pubkey.clone(),
                handle: format!("{}@{}", id_of(&env.pubkey), st.instance_name),
                nickname: None,
                role_ids: vec![],
                joined_at: env.ts,
            };
            st.community_guild_members.entry(guild_id).or_default().push(member.clone());
            drop(st);
            json(req, 200, serde_json::to_value(member).unwrap_or_default(), true);
        }
        (Method::Get, "/api/community/guild/messages") => {
            let channel_id = params.get("channel_id").cloned().unwrap_or_default();
            let limit: usize = params.get("limit").and_then(|l| l.parse().ok()).unwrap_or(50).min(100);
            let st = ctx.state.lock().unwrap();
            let msgs = st
                .community_guild_messages
                .get(&channel_id)
                .map(|list| {
                    let n = list.len();
                    let mut v = list[n.saturating_sub(limit)..].to_vec();
                    for m in &mut v {
                        refresh_guild_msg(&st, m);
                    }
                    v
                })
                .unwrap_or_default();
            drop(st);
            json(req, 200, serde_json::to_value(msgs).unwrap_or_default(), true);
        }
        (Method::Post, "/api/community/guild/messages") => {
            let mut body = String::new();
            let _ = req.as_reader().take(64 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<SendGuildMessagePayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid guild message envelope"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            let msg_id = uuid::Uuid::new_v4().to_string();
            let (author_handle, _author_display, author_avatar_url, _author_accent, author_roblox_id, _author_verified) =
            author_identity(ctx, &mut st, &env.pubkey);
            let msg = GuildMessage {
                id: msg_id,
                guild_id: env.payload.guild_id,
                channel_id: env.payload.channel_id.clone(),
                author_pubkey: env.pubkey.clone(),
                author_handle,
                author_avatar_url,
                author_roblox_id,
                content: clean(&env.payload.content, 4000),
                attachments: env.payload.attachments,
                created_at: env.ts,
                edited_at: None,
                reply_to_id: env.payload.reply_to_id,
                pinned: false,
            };
            st.community_guild_messages
                .entry(env.payload.channel_id)
                .or_default()
                .push(msg.clone());
            drop(st);
            json(req, 200, serde_json::to_value(msg).unwrap_or_default(), true);
        }
        (Method::Post, "/api/community/guild/messages/edit") => {
            let mut body = String::new();
            let _ = req.as_reader().take(64 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<EditGuildMessagePayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid edit payload"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            let updated = st
                .community_guild_messages
                .get_mut(&env.payload.channel_id)
                .and_then(|list| list.iter_mut().find(|m| m.id == env.payload.message_id))
                .filter(|m| m.author_pubkey == env.pubkey)
                .map(|m| {
                    m.content = clean(&env.payload.content, 4000);
                    m.edited_at = Some(env.ts);
                    m.clone()
                });
            drop(st);
            match updated {
                Some(m) => json(req, 200, serde_json::to_value(m).unwrap_or_default(), true),
                None => json(req, 403, serde_json::json!({"error": "cannot edit this message"}), true),
            }
        }
        (Method::Post, "/api/community/guild/messages/delete") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<DeleteGuildMessagePayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid delete payload"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            let guild_owner = st
                .community_guilds
                .values()
                .find(|g| g.channels.iter().any(|c| c.id == env.payload.channel_id))
                .map(|g| g.owner_pubkey.clone())
                .unwrap_or_default();
            let updated = st
                .community_guild_messages
                .get_mut(&env.payload.channel_id)
                .and_then(|list| list.iter_mut().find(|m| m.id == env.payload.message_id))
                .filter(|m| m.author_pubkey == env.pubkey || env.pubkey == guild_owner)
                .map(|m| {
                    m.content = "[deleted]".to_string();
                    m.attachments.clear();
                    m.clone()
                });
            drop(st);
            match updated {
                Some(m) => json(req, 200, serde_json::to_value(m).unwrap_or_default(), true),
                None => json(req, 403, serde_json::json!({"error": "cannot delete this message"}), true),
            }
        }
        (Method::Get, "/api/community/profile") => {
            let id = params.get("id").cloned().unwrap_or_default();
            let st = ctx.state.lock().unwrap();
            if let Some(prof) = st.community_profiles.get(&id) {
                return json(req, 200, serde_json::to_value(prof).unwrap_or_default(), true);
            }
            if let Some(p) = st.profiles.get(&id).filter(|p| p.seen.elapsed() < PROFILE_TTL) {
                let (name, display, roblox_id, verified) = match (&p.user, p.verified) {
                    (Some(u), true) => (u.name.clone(), u.display_name.clone(), Some(u.id), true),
                    _ => (id_of(&id), id_of(&id), None, false),
                };
                let prof = UserProfile {
                    pubkey: id.clone(),
                    handle: format!("{}@{}", name, st.instance_name),
                    display_name: display,
                    bio: p.signed.body.bio.clone(),
                    accent_color: p.signed.body.accent.clone(),
                    avatar_url: None,
                    banner_url: None,
                    roblox_id,
                    roblox_verified: verified,
                    followers_count: 0,
                    following_count: 0,
                    posts_count: 0,
                    created_at: p.signed.body.ts,
                    instance: st.instance_name.clone(),
                };
                return json(req, 200, serde_json::to_value(prof).unwrap_or_default(), true);
            }
            drop(st);
            json(req, 404, serde_json::json!({"error": "profile not found"}), true);
        }
        (Method::Post, "/api/community/profile") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<UserProfile>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid profile envelope"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            let mut prof = env.payload;
            prof.pubkey = env.pubkey.clone();
            {
            let verified_user = st
                .profiles
                .get(&env.pubkey)
                .filter(|p| p.verified)
                .and_then(|p| p.user.clone());
            let instance = st.instance_name.clone();
            refresh_profile(&ctx.roblox, verified_user, &instance, &env.pubkey, &mut prof);
            }
            st.community_profiles.insert(env.pubkey.clone(), prof.clone());
            st.community_profiles.insert(prof.handle.clone(), prof.clone());
            drop(st);
            json(req, 200, serde_json::to_value(prof).unwrap_or_default(), true);
        }
        (Method::Post, "/api/community/follow") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<FollowPayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid follow envelope"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            let rel = FollowRelation {
                follower_pubkey: env.pubkey.clone(),
                followed_pubkey: env.payload.target_pubkey.clone(),
                created_at: env.ts,
            };
            if env.payload.follow {
                if !st.community_follows.iter().any(|r| {
                    r.follower_pubkey == rel.follower_pubkey && r.followed_pubkey == rel.followed_pubkey
                }) {
                    st.community_follows.push(rel.clone());
                    if let Some(p) = st.community_profiles.get_mut(&rel.follower_pubkey) {
                        p.following_count += 1;
                    }
                    if let Some(p) = st.community_profiles.get_mut(&rel.followed_pubkey) {
                        p.followers_count += 1;
                    }
                    push_notification(
                        &mut st,
                        &rel.followed_pubkey,
                        &rel.follower_pubkey,
                        crate::community::types::NotificationKind::Follow,
                        None,
                        "started following you".to_string(),
                        env.ts,
                    );
                }
            } else {
                st.community_follows.retain(|r| {
                    !(r.follower_pubkey == rel.follower_pubkey && r.followed_pubkey == rel.followed_pubkey)
                });
                if let Some(p) = st.community_profiles.get_mut(&rel.follower_pubkey) {
                    p.following_count = p.following_count.saturating_sub(1);
                }
                if let Some(p) = st.community_profiles.get_mut(&rel.followed_pubkey) {
                    p.followers_count = p.followers_count.saturating_sub(1);
                }
            }
            drop(st);
            json(req, 200, serde_json::to_value(rel).unwrap_or_default(), true);
        }
        (Method::Post, "/api/community/block") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<BlockPayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid block envelope"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            if env.payload.block {
                if env.pubkey == env.payload.target_pubkey {
                    return json(req, 400, serde_json::json!({"error": "cannot block yourself"}), true);
                }
                if !st.community_blocks.iter().any(|r| {
                    r.blocker_pubkey == env.pubkey && r.blocked_pubkey == env.payload.target_pubkey
                }) {
                    st.community_blocks.push(BlockRelation {
                        blocker_pubkey: env.pubkey.clone(),
                        blocked_pubkey: env.payload.target_pubkey.clone(),
                        created_at: env.ts,
                    });
                    st.community_follows.retain(|r| {
                        !((r.follower_pubkey == env.pubkey && r.followed_pubkey == env.payload.target_pubkey)
                            || (r.follower_pubkey == env.payload.target_pubkey && r.followed_pubkey == env.pubkey))
                    });
                }
            } else {
                st.community_blocks.retain(|r| {
                    !(r.blocker_pubkey == env.pubkey && r.blocked_pubkey == env.payload.target_pubkey)
                });
            }
            drop(st);
            json(req, 200, serde_json::json!({"blocked": env.payload.block}), true);
        }
        (Method::Get, "/api/community/follows") => {
            let pk = params.get("pubkey").cloned().unwrap_or_default();
            let st = ctx.state.lock().unwrap();
            let following: Vec<String> = st
                .community_follows
                .iter()
                .filter(|r| r.follower_pubkey == pk)
                .map(|r| r.followed_pubkey.clone())
                .collect();
            let followers: Vec<String> = st
                .community_follows
                .iter()
                .filter(|r| r.followed_pubkey == pk)
                .map(|r| r.follower_pubkey.clone())
                .collect();
            let blocked: Vec<String> = st
                .community_blocks
                .iter()
                .filter(|r| r.blocker_pubkey == pk)
                .map(|r| r.blocked_pubkey.clone())
                .collect();
            drop(st);
            json(
                req,
                200,
                serde_json::json!({"following": following, "followers": followers, "blocked": blocked}),
                true,
            );
        }
        (Method::Get, "/api/community/search") => {
            let q = params.get("q").map(|s| s.to_lowercase()).unwrap_or_default();
            let st = ctx.state.lock().unwrap();
            let posts: Vec<Post> = if q.is_empty() {
                vec![]
            } else {
                st.community_posts
                    .values()
                    .filter(|p| {
                        p.content.to_lowercase().contains(&q)
                            || p.tags.iter().any(|t| t.to_lowercase().contains(&q))
                    })
                    .take(20)
                    .cloned()
                    .collect()
            };
            let users: Vec<UserProfile> = if q.is_empty() {
                vec![]
            } else {
                st.community_profiles
                    .values()
                    .filter(|u| {
                        u.handle.to_lowercase().contains(&q)
                            || u.display_name.to_lowercase().contains(&q)
                            || u.bio.to_lowercase().contains(&q)
                    })
                    .take(20)
                    .cloned()
                    .collect()
            };
            let guilds: Vec<Guild> = if q.is_empty() {
                vec![]
            } else {
                st.community_guilds
                    .values()
                    .filter(|g| {
                        g.name.to_lowercase().contains(&q)
                            || g.description.to_lowercase().contains(&q)
                    })
                    .take(20)
                    .cloned()
                    .collect()
            };
            let results = SearchResults {
                query: q,
                posts,
                users,
                guilds,
            };
            drop(st);
            json(req, 200, serde_json::to_value(results).unwrap_or_default(), true);
        }
        (Method::Post, "/api/community/report") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<ReportPayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid report envelope"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            let report = Report {
                id: uuid::Uuid::new_v4().to_string(),
                reporter_pubkey: env.pubkey,
                target_type: env.payload.target_type,
                target_id: env.payload.target_id,
                reason: env.payload.reason,
                details: env.payload.details,
                created_at: env.ts,
                status: ReportStatus::Pending,
                resolved_by: None,
            };
            st.community_reports.push(report.clone());
            drop(st);
            json(req, 200, serde_json::to_value(report).unwrap_or_default(), true);
        }
        (Method::Post, "/api/community/moderate") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<ModerationPayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid moderation envelope"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            if env.payload.guild_id.is_none() {
                let owner = st
                    .profiles
                    .get(&env.pubkey)
                    .map(|p| p.verified && p.user.as_ref().map(|u| u.id == 2721761670).unwrap_or(false))
                    .unwrap_or(false);
                if !owner {
                    return json(req, 403, serde_json::json!({"error": "owner only"}), true);
                }
            }
            let action = ModerationAction {
                id: uuid::Uuid::new_v4().to_string(),
                target_pubkey: env.payload.target_pubkey.clone(),
                action_type: env.payload.action_type,
                moderator_pubkey: env.pubkey.clone(),
                reason: env.payload.reason.clone(),
                created_at: env.ts,
                duration_secs: env.payload.duration_secs,
                guild_id: env.payload.guild_id.clone(),
            };
            st.community_audit_log.push(AuditLogEntry {
                id: uuid::Uuid::new_v4().to_string(),
                actor_pubkey: env.pubkey,
                action: format!("{:?}", env.payload.action_type),
                target_type: "user".to_string(),
                target_id: env.payload.target_pubkey,
                changes: serde_json::json!({"reason": env.payload.reason}),
                created_at: env.ts,
                guild_id: env.payload.guild_id,
            });
            drop(st);
            json(req, 200, serde_json::to_value(action).unwrap_or_default(), true);
        }
        (Method::Get, "/api/community/reports") => {
            let st = ctx.state.lock().unwrap();
            let reports = st.community_reports.clone();
            drop(st);
            json(req, 200, serde_json::to_value(reports).unwrap_or_default(), true);
        }
        (Method::Get, "/api/community/notifications") => {
            let pk = params.get("pubkey").cloned().unwrap_or_default();
            let st = ctx.state.lock().unwrap();
            let mut list: Vec<Notification> = st
                .community_notifications
                .iter()
                .filter(|n| n.recipient_pubkey == pk)
                .cloned()
                .collect();
            list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            list.truncate(50);
            drop(st);
            json(req, 200, serde_json::to_value(list).unwrap_or_default(), true);
        }
        (Method::Post, "/api/community/notifications/read") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(env) = serde_json::from_str::<CommunityEnvelope<MarkReadPayload>>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid read envelope"}), true);
            };
            if let Err(e) = crypto::verify_envelope(&env, FRESH_DIRECT_SECS) {
                return json(req, 400, serde_json::json!({"error": e.to_string()}), true);
            }
            let mut st = ctx.state.lock().unwrap();
            let mut marked = 0;
            for n in st.community_notifications.iter_mut() {
                if n.recipient_pubkey == env.pubkey && (env.payload.all || env.payload.ids.contains(&n.id)) {
                    if !n.read {
                        n.read = true;
                        marked += 1;
                    }
                }
            }
            drop(st);
            json(req, 200, serde_json::json!({"marked": marked}), true);
        }
        (Method::Get, "/api/community/audit_log") => {
            let st = ctx.state.lock().unwrap();
            let logs = st.community_audit_log.clone();
            drop(st);
            json(req, 200, serde_json::to_value(logs).unwrap_or_default(), true);
        }
        (Method::Get, "/api/community/federation/posts") => {
            let since = params.get("since").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            let st = ctx.state.lock().unwrap();
            let envelopes: Vec<CommunityEnvelope<Post>> = st
                .community_post_envelopes
                .values()
                .filter(|env| env.payload.created_at >= since && env.payload.visibility == "public")
                .cloned()
                .collect();
            drop(st);
            json(req, 200, serde_json::to_value(envelopes).unwrap_or_default(), true);
        }
        (Method::Post, "/api/community/federation/handshake") => {
            let mut body = String::new();
            let _ = req.as_reader().take(16 * 1024).read_to_string(&mut body);
            let Ok(handshake) = serde_json::from_str::<HandshakeRequest>(&body) else {
                return json(req, 400, serde_json::json!({"error": "invalid handshake request"}), true);
            };
            let st = ctx.state.lock().unwrap();
            let self_name = st.instance_name.clone();
            if let Err(e) = handshake.verify(&self_name, FEDERATION_MAX_AGE_SECS) {
                return json(req, 400, serde_json::json!({"error": e}), true);
            }
            let users = st.community_profiles.len() as u64;
            let posts = st.community_posts.len() as u64;
            let nodeinfo = build_nodeinfo(
                &self_name,
                "PuppyTunnel Peer Node",
                &hex(&st.instance_id.public()),
                users,
                posts,
                st.peers.clone(),
            );
            let resp = HandshakeResponse::sign(
                &st.instance_id,
                nodeinfo,
                &handshake.origin_instance,
                &self_name,
                "accepted",
            );
            drop(st);
            json(req, 200, serde_json::to_value(resp).unwrap_or_default(), true);
        }
        _ => json(req, 404, serde_json::json!({"error": "not found"}), false),
    }
}

fn host_info(profiles: &HashMap<String, StoredProfile>, key: &str) -> Option<HostInfo> {
    let p = profiles.get(key).filter(|p| p.verified && p.seen.elapsed() < PROFILE_TTL)?;
    let u = p.user.as_ref()?;
    Some(HostInfo {
        id: id_of(key),
        name: u.name.clone(),
        display_name: u.display_name.clone(),
        accent: p.signed.body.accent.clone(),
    })
}

fn ingest_profile(ctx: &Ctx, mut signed: SignedProfile, ip: &str) -> Result<String, (u16, &'static str)> {
    signed.verify(FRESH_DIRECT_SECS).map_err(|e| (400, e))?;
    let key = signed.pubkey.clone();
    let id = signed.body.roblox_id;
    let sanitized = profile::ProfileBody {
        roblox_id: id,
        display_name: clean(&signed.body.display_name, 40),
        bio: clean(&signed.body.bio, 200),
        accent: profile::clean_accent(&signed.body.accent),
        ts: signed.body.ts,
    };
    if ctx.strict_profiles && !strict_profile_ok(&sanitized.display_name) {
        return Err((400, "display name is required"));
    }
    signed.body = sanitized;

    let mut st = ctx.state.lock().unwrap();
    st.profiles.retain(|_, p| p.seen.elapsed() < PROFILE_TTL);
    let slot = st.rate.entry(ip.to_string()).or_insert((0, Instant::now()));
    if slot.1.elapsed() > Duration::from_secs(60) {
        *slot = (0, Instant::now());
    }
    slot.0 += 1;
    if slot.0 > MAX_ANNOUNCES_PER_MIN {
        return Err((429, "rate limited"));
    }
    if !st.profiles.contains_key(&key) && st.profiles.len() >= MAX_PROFILES {
        return Err((429, "profile store full"));
    }
    let keep = st.profiles.get(&key).filter(|p| p.signed.body.roblox_id == id);
    let cached_ver = st.verification_cache.get(&key).filter(|c| c.roblox_id == id && c.verified);
    let now_unix = unix_now();
    let (user, verified, last_verified_unix) = if let Some(p) = keep {
        (p.user.clone(), p.verified, p.last_verified_unix)
    } else if let Some(c) = cached_ver {
        (c.user.clone(), c.verified, c.last_verified_unix)
    } else {
        (None, false, 0)
    };

    let is_recently_verified = verified && last_verified_unix > 0 && now_unix.saturating_sub(last_verified_unix) < VERIFY_CACHE_TTL_SECS;

    st.profiles.insert(
        key.clone(),
        StoredProfile {
            signed,
            user,
            verified: is_recently_verified,
            last_verified_unix,
            seen: Instant::now(),
        },
    );
    let snapshot: Option<Vec<SignedProfile>> = if ctx.persist_profiles {
        Some(st.profiles.values().map(|p| p.signed.clone()).collect())
    } else {
        None
    };
    drop(st);
    if let Some(snap) = snapshot {
        save_profiles_store(&ctx.data_dir, &snap);
    }
    if !is_recently_verified {
        spawn_profile_check(ctx, key.clone(), id);
    }
    Ok(profile::verify_code(&key, id))
}

fn spawn_profile_check(ctx: &Ctx, key: String, id: u64) {
    if ctx.probes.fetch_add(1, Ordering::SeqCst) >= MAX_PROBES {
        ctx.probes.fetch_sub(1, Ordering::SeqCst);
        return;
    }
    let ctx = ctx.clone();
    thread::spawn(move || {
        let result = rbxapi::user_by_id(&ctx.roblox, id);
        let mut save_payload = None;
        if let Ok(mut st) = ctx.state.lock() {
            if let Some(p) = st.profiles.get_mut(&key) {
                if p.signed.body.roblox_id == id {
                    match result {
                        Ok(u) => {
                            let verified = u.description.contains(&profile::verify_code(&key, id));
                            p.verified = verified;
                            p.user = Some(u.clone());
                            if verified {
                                let now_ts = unix_now();
                                p.last_verified_unix = now_ts;
                                st.verification_cache.insert(
                                    key.clone(),
                                    CachedProfileVerification {
                                        pubkey: key.clone(),
                                        roblox_id: id,
                                        verified: true,
                                        last_verified_unix: now_ts,
                                        user: Some(u),
                                    },
                                );
                                if ctx.persist_profiles {
                                    save_payload = Some(st.verification_cache.clone());
                                }
                            }
                        }
                        Err(_) => {}
                    }
                }
            }
        }
        if let Some(cache) = save_payload {
            save_profiles_cache(&ctx.data_dir, &cache);
        }
        ctx.probes.fetch_sub(1, Ordering::SeqCst);
    });
}

fn sort_servers(list: &mut [RemoteServer], sort: &str) {
    match sort {
        "name" => list.sort_by(|a, b| a.listing.name.to_lowercase().cmp(&b.listing.name.to_lowercase())),
        "new" => list.sort_by_key(|s| s.age_secs),
        _ => list.sort_by(|a, b| b.listing.players.cmp(&a.listing.players).then(a.listing.name.cmp(&b.listing.name))),
    }
}

fn sync_peers(peers: &[String], ctx: &Ctx) {
    for peer in peers {
        if let Ok(list) = fetch_signed(peer) {
            for s in list {
                let _ = ingest(ctx, s, "", true);
            }
        }
        if let Ok(posts) = sync_posts_from_peer(peer, 0) {
            let mut st = ctx.state.lock().unwrap();
            for env in posts {
                let _ = ingest_community_post(&mut st, env, true, Some(peer));
            }
        }
    }
}


pub fn normalize_url(master: &str) -> String {
    let m = master.trim().trim_end_matches('/');
    if m.contains("://") { m.to_string() } else { format!("https://{}", m) }
}

pub fn master_list(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in raw.split(|c: char| c == ',' || c.is_whitespace()).filter(|p| !p.is_empty()) {
        let u = normalize_url(part);
        if !out.contains(&u) {
            out.push(u);
        }
    }
    if out.is_empty() {
        out.push(DEFAULT_MASTER.to_string());
    }
    out
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(8)).build()
}

fn post(master: &str, path: &str, body: serde_json::Value) -> Result<(), String> {
    agent()
        .post(&format!("{}{}", normalize_url(master), path))
        .send_json(body)
        .map(|_| ())
        .map_err(|e| match e {
            ureq::Error::Status(c, r) => {
                let msg = r.into_json::<serde_json::Value>().ok().and_then(|v| v["error"].as_str().map(String::from));
                format!("master said {}{}", c, msg.map(|m| format!(": {}", m)).unwrap_or_default())
            }
            e => e.to_string(),
        })
}

fn fetch_signed(master: &str) -> Result<Vec<SignedAnnounce>, String> {
    agent()
        .get(&format!("{}/api/signed", normalize_url(master)))
        .call()
        .map_err(|e| e.to_string())?
        .into_json::<Vec<SignedAnnounce>>()
        .map_err(|e| e.to_string())
}

fn fetch_one(master: &str, query: &str, tag: &str, sort: &str, local: bool) -> Result<Vec<RemoteServer>, String> {
    let mut req = agent()
        .get(&format!("{}/api/servers", normalize_url(master)))
        .query("q", query)
        .query("tag", tag)
        .query("sort", sort);
    if local {
        req = req.query("local", "1");
    }
    req.call().map_err(|e| e.to_string())?.into_json::<Vec<RemoteServer>>().map_err(|e| e.to_string())
}

pub fn fetch_all(raw: &str, query: &str, tag: &str, sort: &str) -> (Vec<RemoteServer>, Vec<String>) {
    let handles: Vec<_> = master_list(raw)
        .into_iter()
        .map(|m| {
            let (q, t, so) = (query.to_string(), tag.to_string(), sort.to_string());
            thread::spawn(move || (m.clone(), fetch_one(&m, &q, &t, &so, false)))
        })
        .collect();
    let mut merged: HashMap<String, RemoteServer> = HashMap::new();
    let mut errors = Vec::new();
    for h in handles {
        match h.join() {
            Ok((_, Ok(list))) => {
                for s in list {
                    match merged.get(&s.listing.invite) {
                        Some(old) if old.listing.players >= s.listing.players => {}
                        _ => {
                            merged.insert(s.listing.invite.clone(), s);
                        }
                    }
                }
            }
            Ok((m, Err(e))) => errors.push(format!("{}: {}", m, e)),
            Err(_) => {}
        }
    }
    let mut list: Vec<RemoteServer> = merged.into_values().collect();
    sort_servers(&mut list, sort);
    (list, errors)
}

pub fn spawn_announcer<L>(
    masters: Vec<String>,
    identity: Identity,
    base: Announce,
    stop: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
    log: L,
) -> JoinHandle<()>
where
    L: Fn(&str, &str) + Send + Sync + 'static,
{
    let log = Arc::new(log);
    thread::spawn(move || {
        let inner: Vec<_> = masters
            .into_iter()
            .map(|m| {
                let (id, b, st, rd, lg) = (identity.clone(), base.clone(), Arc::clone(&stop), Arc::clone(&ready), Arc::clone(&log));
                thread::spawn(move || announce_loop(m, id, b, st, rd, move |msg, tag| lg(msg, tag)))
            })
            .collect();
        for h in inner {
            let _ = h.join();
        }
    })
}

fn announce_loop<L: Fn(&str, &str)>(master: String, id: Identity, base: Announce, stop: Arc<AtomicBool>, ready: Arc<AtomicBool>, log: L) {
    let mut ok_before = false;
    let mut failed_reported = false;
    let mut waited = false;
    let mut last: Option<Instant> = None;
    while !stop.load(Ordering::SeqCst) {
        if !ready.load(Ordering::SeqCst) {
            if !waited {
                waited = true;
                log(&format!("Waiting for the game server port before listing \"{}\"...", base.name), "info");
            }
            thread::sleep(Duration::from_millis(500));
            continue;
        }
        if last.map_or(true, |t| t.elapsed() >= ANNOUNCE_EVERY) {
            last = Some(Instant::now());
            let signed = SignedAnnounce::sign(&id, base.clone());
            match post(&master, "/api/announce", serde_json::to_value(&signed).unwrap_or_default()) {
                Ok(()) if !ok_before => {
                    ok_before = true;
                    failed_reported = false;
                    log(&format!("Announced to {} as \"{}\".", master, base.name), "ok");
                }
                Ok(()) => {}
                Err(e) => {
                    if !failed_reported {
                        log(&format!("Master {} announce failed (will keep retrying): {}", master, e), "warn");
                        failed_reported = true;
                    }
                    ok_before = false;
                }
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
    let ts = unix_now();
    let sig = id.sign(&remove_message(ts));
    let _ = post(&master, "/api/remove", serde_json::json!({"pubkey": hex(&id.public()), "ts": ts, "sig": hex(&sig)}));
}

const BROWSER_HTML: &str = r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>PuppyTunnel servers</title><link rel="icon" type="image/png" href="/icon.png">
<style>
:root{--bg:#fff;--ink:#1a1817;--muted:#6f6a66;--line:#e9e4e0;--card:#faf8f7;--accent:#2fbf8b;--accent-ink:#0a1c14}
@media(prefers-color-scheme:dark){:root{--bg:#100d0c;--ink:#ece4dd;--muted:#9b8f87;--line:#2b2725;--card:#1b1817;--accent:#5fe3ac}}
*{margin:0;padding:0;box-sizing:border-box}
body{font:16px/1.6 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Inter,Helvetica,Arial,sans-serif;background:var(--bg);color:var(--ink);-webkit-font-smoothing:antialiased}
header{border-bottom:1px solid var(--line)}
.wrap{max-width:880px;margin:0 auto;padding:0 24px}
.bar{display:flex;align-items:center;gap:10px;height:60px}
.brand{font-weight:700;letter-spacing:-.02em;font-size:18px}.beta{color:var(--muted);font-size:13px}
h1{font-size:28px;letter-spacing:-.02em;margin:40px 0 4px}.sub{color:var(--muted);margin-bottom:24px}
.tools{display:flex;gap:10px;margin-bottom:8px}
input,select{font:inherit;font-size:15px;color:var(--ink);background:var(--card);border:1px solid var(--line);border-radius:8px;padding:9px 12px}
input{flex:1;min-width:0}input:focus,select:focus{outline:none;border-color:var(--accent)}
table{width:100%;border-collapse:collapse}
th{text-align:left;color:var(--muted);font-size:13px;font-weight:500;padding:10px 0;border-bottom:1px solid var(--line)}
td{padding:14px 0;border-bottom:1px solid var(--line);vertical-align:top}
td.n{font-weight:600}.d{display:block;color:var(--muted);font-weight:400;font-size:14px}
td.c{white-space:nowrap;padding-left:12px}.m{color:var(--muted)}
button{font:inherit;font-size:14px;font-weight:600;color:var(--accent-ink);background:var(--accent);border:0;border-radius:8px;padding:7px 16px;cursor:pointer}
button:hover{filter:brightness(.94)}
.empty{color:var(--muted);padding:28px 0}
footer{color:var(--muted);font-size:14px;padding:28px 0 48px}
</style></head><body>
<header><div class="wrap bar"><img src="/icon.png" alt="" width="26" height="26" style="border-radius:6px"><span class="brand">puppytunnel</span><span class="beta">beta 1.0</span></div></header>
<main class="wrap"><h1>Servers</h1><p class="sub" id="n">Loading</p>
<div class="tools"><input id="q" placeholder="Search name, map or tag"><select id="sort"><option value="players">Most players</option><option value="name">Name</option><option value="new">Newest</option></select></div>
<table><thead><tr><th>Name</th><th>Map</th><th>Players</th><th></th></tr></thead><tbody id="rows"></tbody></table>
<p class="empty" id="empty" hidden>No servers found.</p>
<footer>Copy an invite, then open PuppyTunnel, choose Join and paste it. Or run <code>puppytunnel join puppy://…</code></footer></main>
<script>
const q=document.getElementById('q'),sort=document.getElementById('sort'),rows=document.getElementById('rows'),n=document.getElementById('n'),empty=document.getElementById('empty');
function cell(tr,t,c){const td=document.createElement('td');if(c)td.className=c;if(t!==undefined)td.textContent=t;tr.appendChild(td);return td}
async function load(){try{const r=await fetch('/api/servers?q='+encodeURIComponent(q.value)+'&sort='+sort.value);const d=await r.json();
rows.replaceChildren();n.textContent=d.length+(d.length===1?' server online':' servers online');empty.hidden=d.length>0;
for(const s of d){const tr=document.createElement('tr');const nm=cell(tr,s.name,'n');
const det=[s.description,s.tags.join(', ')].filter(Boolean).join(' · ');
if(det){const m=document.createElement('span');m.className='d';m.textContent=det;nm.appendChild(m)}
cell(tr,s.map,'m');cell(tr,s.players+(s.max_players?' / '+s.max_players:''));
const b=document.createElement('button');b.textContent='Copy invite';b.onclick=()=>{navigator.clipboard.writeText(s.invite);b.textContent='Copied';setTimeout(()=>b.textContent='Copy invite',1500)};
cell(tr,undefined,'c').appendChild(b);rows.appendChild(tr)}}catch(e){n.textContent='Could not load servers'}}
q.oninput=load;sort.onchange=load;load();setInterval(load,15000);
</script></body></html>"#;


#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::relay::{frame, make_invite, run_host_agent, RelayServer, T_HELLO};
    use std::net::{SocketAddr, UdpSocket};

    fn free_udp() -> u16 {
        UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    struct Host {
        id: Identity,
        invite: String,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl Drop for Host {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    fn live_host(relay_port: u16) -> Host {
        let id = Identity::ephemeral();
        let relay: SocketAddr = ([127, 0, 0, 1], relay_port).into();
        let stop = Arc::new(AtomicBool::new(false));
        let (i2, s2) = (id.clone(), Arc::clone(&stop));
        let thread = Some(thread::spawn(move || {
            let _ = run_host_agent(relay, i2, 9, s2, Arc::new(AtomicUsize::new(0)), |_, _| {});
        }));
        for _ in 0..30 {
            thread::sleep(Duration::from_millis(100));
            if query_stats(relay, &id.token()).map_or(false, |(reg, _)| reg) {
                break;
            }
        }
        let invite = make_invite(&format!("127.0.0.1:{}", relay_port), &id.token());
        Host { id, invite, stop, thread }
    }

    fn body(name: &str, invite: &str) -> Announce {
        Announce { name: name.into(), map: "Obby".into(), tags: vec!["Fun".into(), "bad tag!".into()], max_players: 8, invite: invite.into(), ..Default::default() }
    }

    fn post_signed(master: &str, s: &SignedAnnounce) -> Result<(), String> {
        post(master, "/api/announce", serde_json::to_value(s).unwrap())
    }

    fn wait_for<F: Fn(&[RemoteServer]) -> bool>(master: &str, cond: F) -> bool {
        for _ in 0..40 {
            if let Ok(l) = fetch_one(master, "", "", "name", false) {
                if cond(&l) {
                    return true;
                }
            }
            thread::sleep(Duration::from_millis(150));
        }
        false
    }

    fn opts() -> MasterOptions {
        MasterOptions { allow_private_relays: true, ..Default::default() }
    }

    #[test]
    fn strict_profile_names_gated() {
        let id = Identity::profile("test-secret");
        let mk = |display_name: &str| SignedProfile::sign(&id, profile::ProfileBody { roblox_id: 7, display_name: display_name.into(), bio: "hi".into(), accent: "mint".into(), ts: 0 });
        let ctx = |strict: bool| Ctx { roblox: String::new(), state: Arc::new(Mutex::new(State::default())), probes: Arc::new(AtomicUsize::new(0)), allow_private: true, persist_profiles: false, data_dir: std::path::PathBuf::new(), strict_profiles: strict };
        assert!(ingest_profile(&ctx(false), mk("   "), "127.0.0.1").is_ok());
        assert_eq!(ingest_profile(&ctx(true), mk("   "), "127.0.0.1"), Err((400, "display name is required")));
        assert!(ingest_profile(&ctx(true), mk("Somebody"), "127.0.0.1").is_ok());
        assert!(strict_profile_ok("Somebody"));
        assert!(!strict_profile_ok("   "));
    }

    #[test]
    fn announce_option_fields_round_trip() {
        let relay_port = free_udp();
        let mut relay_srv = RelayServer::start(([127, 0, 0, 1], relay_port).into(), |_| {}).unwrap();
        let host = live_host(relay_port);
        let master = "http://127.0.0.1:48999";
        let _m = MasterServer::start("127.0.0.1:48999", opts()).unwrap();
        let mut gated = body("Gated", &host.invite);
        gated.strict_profiles = Some(true);
        gated.overhead_tags = Some(false);
        gated.console_policy = Some("123,456".into());
        post_signed(master, &SignedAnnounce::sign(&host.id, gated)).unwrap();
        assert!(wait_for(master, |l| l.len() == 1), "gated listing never appeared");
        let l = fetch_one(master, "", "", "name", false).unwrap();
        assert_eq!(l[0].listing.strict_profiles, Some(true));
        assert_eq!(l[0].listing.overhead_tags, Some(false));
        assert_eq!(l[0].listing.console_policy.as_deref(), Some("123,456"));
        relay_srv.stop();
    }

    #[test]
    fn default_master() {
        assert_eq!(master_list(""), vec![DEFAULT_MASTER.to_string()]);
        assert_eq!(master_list("a.example, https://b.example/"), vec!["https://a.example", "https://b.example"]);
    }

    #[test]
    fn listings_cannot_be_spoofed() {
        let relay_port = free_udp();
        let mut relay_srv = RelayServer::start(([127, 0, 0, 1], relay_port).into(), |_| {}).unwrap();

        let master = "http://127.0.0.1:48990";
        let mut m = MasterServer::start("127.0.0.1:48990", opts()).unwrap();
        let a = live_host(relay_port);

        let honest = SignedAnnounce::sign(&a.id, body("Real\u{7}Server", &a.invite));
        post_signed(master, &honest).unwrap();
        assert!(wait_for(master, |l| l.len() == 1), "verified listing never appeared");
        let l = fetch_one(master, "", "", "name", false).unwrap();
        assert_eq!(l[0].listing.name, "RealServer");
        assert_eq!(l[0].listing.tags, vec!["fun", "badtag"]);
        assert_eq!(l[0].listing.players, 0);

        let joiner = UdpSocket::bind("127.0.0.1:0").unwrap();
        let hello = frame(T_HELLO, &a.id.token());
        joiner.send_to(&hello, ("127.0.0.1", relay_port)).unwrap();
        thread::sleep(Duration::from_millis(200));
        post_signed(master, &SignedAnnounce::sign(&a.id, body("RealServer", &a.invite))).unwrap();
        assert!(wait_for(master, |l| l.len() == 1 && l[0].listing.players == 1), "relay player count not shown");

        let mut tampered = honest.clone();
        tampered.body.name = "Official".into();
        assert!(post_signed(master, &tampered).unwrap_err().contains("400"));

        let thief = Identity::ephemeral();
        let stolen = SignedAnnounce::sign(&thief, body("Free Robux", &a.invite));
        assert!(post_signed(master, &stolen).unwrap_err().contains("not bound"));

        let b = live_host(relay_port);
        let squat = SignedAnnounce::sign(&b.id, body("realserver", &b.invite));
        assert!(post_signed(master, &squat).unwrap_err().contains("409"));

        let mut old = body("Replay", &b.invite);
        old.ts = unix_now() - 1000;
        let sig = b.id.sign(&signing_bytes(&old));
        let replay = SignedAnnounce { pubkey: hex(&b.id.public()), sig: hex(&sig), body: old };
        assert!(post_signed(master, &replay).unwrap_err().contains("stale"));

        let ghost = Identity::ephemeral();
        let ghost_invite = make_invite(&format!("127.0.0.1:{}", relay_port), &ghost.token());
        post_signed(master, &SignedAnnounce::sign(&ghost, body("Ghost", &ghost_invite))).unwrap();
        thread::sleep(Duration::from_millis(800));
        assert!(fetch_one(master, "ghost", "", "name", false).unwrap().is_empty());

        let bad_sig = hex(&thief.sign(&remove_message(unix_now())));
        let forged = serde_json::json!({"pubkey": hex(&a.id.public()), "ts": unix_now(), "sig": bad_sig});
        assert!(post(master, "/api/remove", forged).is_err());
        assert_eq!(fetch_one(master, "realserver", "", "name", false).unwrap().len(), 1);
        let ts = unix_now();
        let good = serde_json::json!({"pubkey": hex(&a.id.public()), "ts": ts, "sig": hex(&a.id.sign(&remove_message(ts)))});
        post(master, "/api/remove", good).unwrap();
        assert!(fetch_one(master, "realserver", "", "name", false).unwrap().is_empty());

        m.stop();
        relay_srv.stop();
    }

    #[test]
    fn peers_cannot_forge_listings() {
        let relay_port = free_udp();
        let mut relay_srv = RelayServer::start(([127, 0, 0, 1], relay_port).into(), |_| {}).unwrap();
        let host = live_host(relay_port);

        let mut b = MasterServer::start("127.0.0.1:48991", opts()).unwrap();
        let mut a = MasterServer::start_with("127.0.0.1:48992", MasterOptions { peers: vec!["http://127.0.0.1:48991".into()], ..opts() }, Duration::from_millis(300)).unwrap();

        post_signed("http://127.0.0.1:48991", &SignedAnnounce::sign(&host.id, body("OnB", &host.invite))).unwrap();
        assert!(wait_for("http://127.0.0.1:48992", |l| l.iter().any(|s| s.listing.name == "OnB")), "peer listing never reached master A");
        assert!(fetch_one("http://127.0.0.1:48992", "", "", "name", true).unwrap().is_empty(), "imported listings must not be re-shared");

        let (all, errs) = fetch_all("http://127.0.0.1:48991, http://127.0.0.1:48992 http://127.0.0.1:1", "", "", "name");
        assert_eq!(all.len(), 1);
        assert_eq!(errs.len(), 1);

        let ctx = Ctx { roblox: rbxapi::USERS_API.to_string(), state: Arc::new(Mutex::new(State::default())), probes: Arc::new(AtomicUsize::new(0)), allow_private: true, persist_profiles: false, data_dir: std::env::temp_dir(), strict_profiles: false };
        let mut forged = SignedAnnounce::sign(&host.id, body("Legit", &host.invite));
        forged.body.description = "send me your password".into();
        assert!(ingest(&ctx, forged, "", true).is_err());

        a.stop();
        b.stop();
        relay_srv.stop();
    }

    #[test]
    fn profiles_need_roblox_proof_and_bind_to_listings() {
        use crate::profile::{host_message, ProfileBody, SignedProfile};

        let about = Arc::new(Mutex::new(String::from("just a builder")));
        let mock = Server::http("127.0.0.1:48995").unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let (about2, stop2) = (Arc::clone(&about), Arc::clone(&stop));
        let mock_thread = thread::spawn(move || {
            while !stop2.load(Ordering::SeqCst) {
                if let Ok(Some(req)) = mock.recv_timeout(Duration::from_millis(100)) {
                    let body = if req.url() == "/v1/users/42" {
                        serde_json::json!({"id": 42, "name": "RealUser", "displayName": "Real Display", "description": about2.lock().unwrap().clone()})
                    } else {
                        serde_json::json!({"errors": []})
                    };
                    let _ = req.respond(Response::from_string(body.to_string()));
                }
            }
        });

        let relay_port = free_udp();
        let mut relay_srv = RelayServer::start(([127, 0, 0, 1], relay_port).into(), |_| {}).unwrap();
        let master = "http://127.0.0.1:48993";
        let mut m = MasterServer::start("127.0.0.1:48993", MasterOptions { roblox_api: Some("http://127.0.0.1:48995".into()), ..opts() }).unwrap();

        let owner = Identity::profile("owner-secret");
        let owner_hex = hex(&owner.public());
        let body = ProfileBody { roblox_id: 42, display_name: String::new(), bio: "hello there".into(), accent: "sky".into(), ts: 0 };
        let publish = |id: &Identity| profile::publish(&[master.to_string()], &SignedProfile::sign(id, body.clone()));

        assert!(publish(&owner).is_empty());
        thread::sleep(Duration::from_millis(400));
        let st = profile::status(master, &owner_hex).unwrap();
        assert!(!st.verified);
        assert_eq!(st.code, profile::verify_code(&owner_hex, 42));
        assert!(st.name.is_empty(), "unverified profiles must not expose a claimed name");

        let thief = Identity::profile("thief-secret");
        *about.lock().unwrap() = format!("about {}", st.code);
        assert!(publish(&thief).is_empty());
        thread::sleep(Duration::from_millis(400));
        assert!(!profile::status(master, &hex(&thief.public())).unwrap().verified);

        assert!(publish(&owner).is_empty());
        let mut verified = None;
        for _ in 0..20 {
            thread::sleep(Duration::from_millis(150));
            let p = profile::status(master, &owner_hex).unwrap();
            if p.verified {
                verified = Some(p);
                break;
            }
        }
        let p = verified.expect("profile never verified");
        assert_eq!((p.name.as_str(), p.display_name.as_str()), ("RealUser", "Real Display"));
        assert_eq!((p.bio.as_str(), p.accent.as_str()), ("hello there", "#6cb8ff"));

        let a = live_host(relay_port);
        let mut b = body_for(&a, "Hosted");
        b.host = owner_hex.clone();
        b.host_sig = hex(&owner.sign(&host_message(&a.id.public())));
        post_signed(master, &SignedAnnounce::sign(&a.id, b)).unwrap();
        assert!(wait_for(master, |l| l.len() == 1));
        let l = fetch_one(master, "", "", "name", false).unwrap();
        let host = l[0].host.clone().expect("host info missing");
        assert_eq!(host.display_name, "Real Display");

        let c = live_host(relay_port);
        let mut forged = body_for(&c, "Impostor");
        forged.host = owner_hex.clone();
        forged.host_sig = hex(&thief.sign(&host_message(&c.id.public())));
        assert!(post_signed(master, &SignedAnnounce::sign(&c.id, forged)).unwrap_err().contains("400"));

        let d = live_host(relay_port);
        let mut plain = body_for(&d, "Anonymous");
        plain.host = hex(&thief.public());
        plain.host_sig = hex(&thief.sign(&host_message(&d.id.public())));
        post_signed(master, &SignedAnnounce::sign(&d.id, plain)).unwrap();
        assert!(wait_for(master, |l| l.len() == 2));
        let l = fetch_one(master, "anonymous", "", "name", false).unwrap();
        assert!(l[0].host.is_none());

        m.stop();
        relay_srv.stop();
        stop.store(true, Ordering::SeqCst);
        let _ = mock_thread.join();
    }

    fn body_for(h: &Host, name: &str) -> Announce {
        body(name, &h.invite)
    }

    #[test]
    fn test_community_backend_endpoints_and_client() {
        use crate::community::CommunityClient;
        use crate::community::types::InteractionType;

        let master_url = "http://127.0.0.1:48996";
        let mut m = MasterServer::start("127.0.0.1:48996", opts()).unwrap();

        let alice_id = Identity::profile("alice-test-secret");
        let mut client = CommunityClient::new(master_url, Some(alice_id.clone()));

        let post = client
            .create_post(
                "Welcome to PuppyTunnel Community!",
                vec![],
                None,
                None,
                vec!["roblox".into(), "studio".into()],
                "public",
            )
            .expect("create post failed");

        assert_eq!(post.content, "Welcome to PuppyTunnel Community!");
        assert_eq!(post.tags, vec!["roblox", "studio"]);

        let feed = client.get_feed(10, None).expect("get feed failed");
        assert!(!feed.is_empty());
        assert_eq!(feed[0].id, post.id);

        let own_like = client.interact_post(&post.id, InteractionType::Like, None);
        assert!(own_like.is_err(), "self-like must be rejected");
        let bob_client = CommunityClient::new(master_url, Some(Identity::profile("bob-test-secret")));
        let interaction = bob_client
            .interact_post(&post.id, InteractionType::Like, None)
            .expect("interact failed");
        assert_eq!(interaction.kind, InteractionType::Like);
        assert_eq!(interaction.liked, Some(true));

        let updated_post = client.get_post(&post.id).expect("get post failed");
        assert_eq!(updated_post.likes_count, 1);

        let unlike = bob_client
            .interact_post(&post.id, InteractionType::Like, None)
            .expect("unlike failed");
        assert_eq!(unlike.liked, Some(false));
        let updated_post = client.get_post(&post.id).expect("get post failed");
        assert_eq!(updated_post.likes_count, 0);

        let bob_id = Identity::profile("bob-test-secret");
        let conv = client
            .create_conversation(vec![hex(&bob_id.public())], Some("Project Collab".into()))
            .expect("create conv failed");
        assert_eq!(conv.title.as_deref(), Some("Project Collab"));

        let dm = client
            .send_dm(&conv.id, "Hey Bob, let's build a map!", vec![], None)
            .expect("send dm failed");
        assert_eq!(dm.content, "Hey Bob, let's build a map!");

        let dms = client.get_messages(&conv.id, 10).expect("get dms failed");
        assert_eq!(dms.len(), 1);
        assert_eq!(dms[0].id, dm.id);

        let guild = client
            .create_guild("Puppy Devs", "Game developers guild", None)
            .expect("create guild failed");
        assert_eq!(guild.name, "Puppy Devs");
        assert!(!guild.channels.is_empty());

        let general_ch_id = &guild.channels[0].id;
        let guild_msg = client
            .send_guild_message(&guild.id, general_ch_id, "Hello guild!", vec![], None)
            .expect("send guild msg failed");
        assert_eq!(guild_msg.content, "Hello guild!");

        let guild_msgs = client
            .get_guild_messages(general_ch_id, 10)
            .expect("get guild msgs failed");
        assert_eq!(guild_msgs.len(), 1);
        assert_eq!(guild_msgs[0].id, guild_msg.id);

        let follow = client
            .follow_user(&hex(&bob_id.public()))
            .expect("follow user failed");
        assert_eq!(follow.followed_pubkey, hex(&bob_id.public()));

        let alice_notifs = client.get_notifications().expect("get notifications failed");
        assert!(alice_notifs.iter().any(|n| matches!(
            n.kind,
            crate::community::types::NotificationKind::Like
        )));
        let marked = client
            .mark_notifications_read(&[], true)
            .expect("mark read failed");
        assert!(marked >= 1);
        let alice_notifs = client.get_notifications().expect("get notifications failed");
        assert!(alice_notifs.iter().all(|n| n.read));

        let post2 = client
            .create_post("temp post", vec![], None, None, vec![], "public")
            .expect("create post2 failed");
        let edited = client
            .edit_post(&post2.id, "edited content")
            .expect("edit post failed");
        assert_eq!(edited.content, "edited content");
        assert!(edited.edited_at.is_some());
        client.delete_post(&post2.id).expect("delete post failed");
        let feed_after = client.get_feed(10, None).expect("feed after delete failed");
        assert!(!feed_after.iter().any(|p| p.id == post2.id));

        assert!(client
            .block_user(&hex(&bob_id.public()), true)
            .expect("block failed"));
        let (_, _, blocked) = client.get_social_lists().expect("social lists failed");
        assert!(blocked.contains(&hex(&bob_id.public())));
        client
            .block_user(&hex(&bob_id.public()), false)
            .expect("unblock failed");
        let (_, _, blocked) = client.get_social_lists().expect("social lists failed");
        assert!(!blocked.contains(&hex(&bob_id.public())));

        client.set_offline(true);
        let cached_feed = client.get_feed(10, None).expect("offline feed failed");
        assert!(!cached_feed.is_empty());
        assert_eq!(cached_feed[0].id, post.id);

        let cached_post = client.get_post(&post.id).expect("offline post failed");
        assert_eq!(cached_post.id, post.id);

        m.stop();
    }

    #[test]
    fn test_community_federation_between_masters() {
        use crate::community::CommunityClient;

        let beta_opts = MasterOptions {
            instance_name: Some("beta.puppytunnel.ma".into()),
            ..opts()
        };
        let mut beta = MasterServer::start("127.0.0.1:48997", beta_opts).unwrap();

        let alpha_opts = MasterOptions {
            instance_name: Some("alpha.puppytunnel.ma".into()),
            peers: vec!["http://127.0.0.1:48997".into()],
            ..opts()
        };
        let mut alpha = MasterServer::start_with(
            "127.0.0.1:48998",
            alpha_opts,
            Duration::from_millis(250),
        )
        .unwrap();

        let author_id = Identity::profile("author-fed-secret");
        let beta_client = CommunityClient::new("http://127.0.0.1:48997", Some(author_id.clone()));
        let post = beta_client
            .create_post(
                "Federated announcement from Beta node!",
                vec![],
                None,
                None,
                vec!["federation".into()],
                "public",
            )
            .expect("post on beta failed");

        let alpha_client = CommunityClient::new("http://127.0.0.1:48998", None);
        let mut synced = false;
        for _ in 0..30 {
            thread::sleep(Duration::from_millis(150));
            if let Ok(feed) = alpha_client.get_feed(10, None) {
                if feed.iter().any(|p| p.content == post.content) {
                    synced = true;
                    break;
                }
            }
        }
        assert!(synced, "federated post from Beta was not synced to Alpha");

        alpha.stop();
        beta.stop();
    }

    #[test]
    fn test_community_store_survives_restart() {
        let dir = std::env::temp_dir()
            .join(format!("puppy-community-store-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut st = State::default();
        st.community_posts.insert(
            "p1".to_string(),
            Post {
                id: "p1".to_string(),
                author_pubkey: "abc".to_string(),
                author_handle: "@tester".to_string(),
                author_display: None,
                author_avatar_url: None,
                author_accent: None,
                author_roblox_id: None,
                author_roblox_verified: false,
                content: "hello persistent world".to_string(),
                attachments: Vec::new(),
                reply_to_id: None,
                repost_of_id: None,
                likes_count: 3,
                reposts_count: 0,
                replies_count: 1,
                created_at: 1_700_000_000,
                edited_at: None,
                tags: vec!["test".to_string()],
                visibility: "public".to_string(),
                federated: false,
                origin_instance: String::new(),
                is_pinned: false,
                liked_by_me: false,
            },
        );
        st.community_post_order.push("p1".to_string());
        st.community_liked_by
            .entry("p1".to_string())
            .or_default()
            .insert("liker1".to_string());
        st.community_follows.push(FollowRelation {
            follower_pubkey: "a".to_string(),
            followed_pubkey: "b".to_string(),
            created_at: 1_700_000_000,
        });
        st.community_notifications.push(Notification {
            id: "n1".to_string(),
            recipient_pubkey: "a".to_string(),
            actor_pubkey: "abc".to_string(),
            actor_handle: "@tester".to_string(),
            kind: NotificationKind::Like,
            target_id: Some("p1".to_string()),
            preview: "liked your post".to_string(),
            created_at: 1_700_000_000,
            read: false,
        });
        save_community_store(&dir, &st);

        let mut st2 = State::default();
        assert!(st2.community_posts.is_empty());
        apply_community_store(&mut st2, load_community_store(&dir));
        assert_eq!(
            st2.community_posts["p1"].content,
            "hello persistent world"
        );
        assert_eq!(st2.community_post_order, vec!["p1".to_string()]);
        assert!(st2.community_liked_by["p1"].contains("liker1"));
        assert_eq!(st2.community_follows.len(), 1);
        assert_eq!(st2.community_notifications.len(), 1);
        assert!(!st2.community_notifications[0].read);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_refresh_profile_backfills_roblox() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().expect("ip addr").port();
        let base = format!("http://127.0.0.1:{}", port);
        let _srv = std::thread::spawn(move || {
            for req in server.incoming_requests().take(3) {
                let body = r#"{"id":7,"name":"MockUser","displayName":"Mock Display","description":""}"#;
                let _ = req.respond(tiny_http::Response::from_string(body));
            }
        });
        let mut prof = UserProfile {
            pubkey: "abc".to_string(),
            roblox_id: Some(7),
            instance: "test.instance".to_string(),
            ..Default::default()
        };
        refresh_profile(&base, None, "test.instance", "abc", &mut prof);
        assert_eq!(prof.handle, "@MockUser");
        assert_eq!(prof.display_name, "Mock Display");
        assert!(!prof.roblox_verified, "client-claimed verify must not stick without signed proof");
        let verified = crate::rbxapi::user_by_id(&base, 7).expect("mock user");
        refresh_profile(&base, Some(verified), "test.instance", "abc", &mut prof);
        assert!(prof.roblox_verified);
    }

    #[test]
    fn test_update_endpoints() {
        assert_eq!(compare_semver("1.0.0", "0.9.0"), std::cmp::Ordering::Greater);
        assert_eq!(compare_semver("1.0.0", "1.0.0"), std::cmp::Ordering::Equal);
        assert_eq!(compare_semver("1.0.0", "1.0.1-beta.1"), std::cmp::Ordering::Less);
        assert_eq!(compare_semver("1.0.0", "1.0.0-beta.1"), std::cmp::Ordering::Greater);

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let master = format!("http://127.0.0.1:{}", port);
        let mut m = MasterServer::start(&format!("127.0.0.1:{}", port), opts()).unwrap();

        let check_res: serde_json::Value = ureq::get(&format!("{}/api/updates/check?channel=stable&version=0.9.0&os=windows&arch=x86_64", master))
            .call()
            .unwrap()
            .into_json()
            .unwrap();
        assert_eq!(check_res["update_available"], true);
        assert_eq!(check_res["channel"], "stable");
        assert_eq!(check_res["latest_version"], "1.0.0");

        let up_to_date: serde_json::Value = ureq::get(&format!("{}/api/updates/check?channel=stable&version=1.0.0&os=windows&arch=x86_64", master))
            .call()
            .unwrap()
            .into_json()
            .unwrap();
        assert_eq!(up_to_date["update_available"], false);

        let manifest_res: serde_json::Value = ureq::get(&format!("{}/api/updates/manifest?channel=beta", master))
            .call()
            .unwrap()
            .into_json()
            .unwrap();
        assert_eq!(manifest_res["channel"], "beta");
        assert_eq!(manifest_res["version"], "1.0.1-beta.3");

        let history_res: serde_json::Value = ureq::get(&format!("{}/api/updates/history", master))
            .call()
            .unwrap()
            .into_json()
            .unwrap();
        assert!(history_res.as_array().unwrap().len() >= 3);

        m.stop();
    }

    #[test]
    fn profiles_survive_restart() {
        use crate::profile::{ProfileBody, SignedProfile};
        let dir = std::env::temp_dir().join(format!("puppy-store-{}", uuid::Uuid::new_v4()));
        let id = Identity::profile("persist-secret");
        let signed = SignedProfile::sign(
            &id,
            ProfileBody { roblox_id: 42, display_name: "Melody".into(), bio: "hi".into(), accent: "#6cb8ff".into(), ts: 0 },
        );
        save_profiles_store(&dir, &[signed.clone()]);
        let loaded = load_profiles_store(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].pubkey, signed.pubkey);
        assert_eq!(loaded[0].body.display_name, "Melody");
        assert_eq!(loaded[0].body.bio, "hi");
        assert!(loaded[0].verify(60).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
