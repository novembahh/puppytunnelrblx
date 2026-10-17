use crate::community::crypto;
use crate::community::federation::DEFAULT_INSTANCE;
use crate::community::types::*;
use crate::identity::{hex, unix_now, Identity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CreatePostPayload {
    pub content: String,
    pub attachments: Vec<Attachment>,
    pub reply_to_id: Option<String>,
    pub repost_of_id: Option<String>,
    pub tags: Vec<String>,
    pub visibility: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InteractPostPayload {
    pub post_id: String,
    pub kind: InteractionType,
    pub content: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SendDmPayload {
    pub conversation_id: String,
    pub content: String,
    pub attachments: Vec<Attachment>,
    pub reply_to_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CreateConversationPayload {
    pub recipient_pubkeys: Vec<String>,
    pub title: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CreateGuildPayload {
    pub name: String,
    pub description: String,
    pub icon_url: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SendGuildMessagePayload {
    pub guild_id: String,
    pub channel_id: String,
    pub content: String,
    pub attachments: Vec<Attachment>,
    pub reply_to_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CreateChannelPayload {
    pub guild_id: String,
    pub name: String,
    pub topic: String,
    pub kind: ChannelKind,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JoinGuildPayload {
    pub invite_code: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FollowPayload {
    pub target_pubkey: String,
    pub follow: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlockPayload {
    pub target_pubkey: String,
    pub block: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReportPayload {
    pub target_type: ReportTargetType,
    pub target_id: String,
    pub reason: String,
    pub details: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EditPostPayload {
    pub post_id: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeletePostPayload {
    pub post_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EditDmPayload {
    pub conversation_id: String,
    pub message_id: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeleteDmPayload {
    pub conversation_id: String,
    pub message_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EditGuildMessagePayload {
    pub channel_id: String,
    pub message_id: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeleteGuildMessagePayload {
    pub channel_id: String,
    pub message_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MarkReadPayload {
    #[serde(default)]
    pub ids: Vec<String>,
    #[serde(default)]
    pub all: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ModerationPayload {
    pub target_pubkey: String,
    pub action_type: ModActionType,
    pub reason: String,
    pub duration_secs: Option<u64>,
    pub guild_id: Option<String>,
}

#[derive(Default, Clone, Debug)]
pub struct CommunityCache {
    pub posts: HashMap<String, Post>,
    pub feed: Vec<Post>,
    pub conversations: HashMap<String, Conversation>,
    pub messages: HashMap<String, Vec<DirectMessage>>,
    pub guilds: HashMap<String, Guild>,
    pub guild_messages: HashMap<String, Vec<GuildMessage>>,
    pub profiles: HashMap<String, UserProfile>,
}

#[derive(Clone)]
pub struct CommunityClient {
    base_url: String,
    identity: Option<Identity>,
    agent: ureq::Agent,
    pub cache: Arc<Mutex<CommunityCache>>,
    pub offline_mode: bool,
    time_offset: Arc<Mutex<Option<i64>>>,
}

fn parse_http_date(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() != 6 {
        return None;
    }
    let day: i64 = parts[1].parse().ok()?;
    let mon = match parts[2] {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year: i64 = parts[3].parse().ok()?;
    let t: Vec<&str> = parts[4].split(':').collect();
    if t.len() != 3 {
        return None;
    }
    let hh: i64 = t[0].parse().ok()?;
    let mm: i64 = t[1].parse().ok()?;
    let ss: i64 = t[2].parse().ok()?;
    if day < 1 || day > 31 || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    let y = if mon <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (mon + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146097 + doe - 719468) * 86400 + hh * 3600 + mm * 60 + ss)
}

impl CommunityClient {
    pub fn new(base_url: &str, identity: Option<Identity>) -> Self {
        let trimmed = base_url.trim().trim_end_matches('/');
        let url = if trimmed.is_empty() {
            format!("https://{}", DEFAULT_INSTANCE)
        } else if trimmed.contains("://") {
            trimmed.to_string()
        } else {
            format!("https://{}", trimmed)
        };

        Self {
            base_url: url,
            identity,
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(8))
                .build(),
            cache: Arc::new(Mutex::new(CommunityCache::default())),
            offline_mode: false,
            time_offset: Arc::new(Mutex::new(None)),
        }
    }

    fn cached_offset(&self) -> i64 {
        self.time_offset.lock().map(|g| g.unwrap_or(0)).unwrap_or(0)
    }

    fn signed_ts(&self) -> u64 {
        let now = unix_now() as i64;
        (now + self.cached_offset()).max(0) as u64
    }

    fn measure_offset(&self) -> Option<i64> {
        let local = unix_now() as i64;
        let resp = self.agent.head(&self.base_url).call().ok().or_else(|| {
            self.agent
                .get(&format!("{}/api/community", self.base_url))
                .call()
                .ok()
        })?;
        let date = resp.header("date")?;
        Some(parse_http_date(date)? - local)
    }

    fn refresh_offset(&self) -> bool {
        let prev = self.cached_offset();
        match self.measure_offset() {
            Some(off) if (off - prev).abs() > 30 => {
                if let Ok(mut g) = self.time_offset.lock() {
                    *g = Some(off);
                }
                true
            }
            _ => false,
        }
    }

    fn post_value(&self, url: &str, val: serde_json::Value) -> Result<serde_json::Value, String> {
        let resp = self
            .agent
            .post(url)
            .send_json(val)
            .map_err(|e| match e {
                ureq::Error::Status(c, r) => {
                    let msg = r
                        .into_json::<serde_json::Value>()
                        .ok()
                        .and_then(|v| v["error"].as_str().map(String::from));
                    format!("server error {}{}", c, msg.map(|m| format!(": {}", m)).unwrap_or_default())
                }
                e => e.to_string(),
            })?;
        resp.into_json().map_err(|e| e.to_string())
    }

    pub fn set_offline(&mut self, offline: bool) {
        self.offline_mode = offline;
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn identity_pubkey(&self) -> Option<String> {
        self.identity.as_ref().map(|id| hex(&id.public()))
    }

    fn require_identity(&self) -> Result<&Identity, String> {
        self.identity
            .as_ref()
            .ok_or_else(|| "profile identity required to sign community mutation".to_string())
    }

    fn post_envelope<P: Serialize + Clone, R: for<'de> Deserialize<'de>>(
        &self,
        endpoint: &str,
        action: &str,
        payload: P,
    ) -> Result<R, String> {
        let id = self.require_identity()?;

        if self.offline_mode {
            return Err("cannot perform mutations in offline mode".to_string());
        }

        let url = format!("{}{}", self.base_url, endpoint);
        let mut retried = false;
        let val: serde_json::Value = loop {
            let env = crypto::sign_envelope_with_ts(id, action, payload.clone(), self.signed_ts())
                .map_err(|e| format!("signing failed: {}", e))?;
            let body = serde_json::to_value(&env).map_err(|e| e.to_string())?;
            match self.post_value(&url, body) {
                Err(e) if !retried && e.starts_with("server error 401") && self.refresh_offset() => {
                    retried = true;
                    continue;
                }
                Err(e) => break Err(e),
                Ok(v) => break Ok(v),
            }
        }?;
        if let Ok(direct) = serde_json::from_value::<R>(val.clone()) {
            return Ok(direct);
        }
        for key in &["post", "guild", "message", "member", "reply", "user", "action", "data"] {
            if let Some(inner) = val.get(*key) {
                if let Ok(unwrapped) = serde_json::from_value::<R>(inner.clone()) {
                    return Ok(unwrapped);
                }
            }
        }
        serde_json::from_value::<R>(val).map_err(|e| e.to_string())
    }

    pub fn create_post(
        &self,
        content: &str,
        attachments: Vec<Attachment>,
        reply_to_id: Option<String>,
        repost_of_id: Option<String>,
        tags: Vec<String>,
        visibility: &str,
    ) -> Result<Post, String> {
        let payload = CreatePostPayload {
            content: content.to_string(),
            attachments,
            reply_to_id,
            repost_of_id,
            tags,
            visibility: visibility.to_string(),
        };

        let post: Post = self.post_envelope("/api/community/posts", "post:create", payload)?;
        let mut cache = self.cache.lock().unwrap();
        cache.posts.insert(post.id.clone(), post.clone());
        cache.feed.insert(0, post.clone());
        Ok(post)
    }

    pub fn get_feed(&self, limit: usize, before_ts: Option<u64>) -> Result<Vec<Post>, String> {
        if self.offline_mode {
            let cache = self.cache.lock().unwrap();
            return Ok(cache.feed.clone());
        }

        let endpoints = ["/api/community/posts", "/api/community/feed"];
        let mut last_err = String::new();

        for ep in &endpoints {
            let mut req = self.agent.get(&format!("{}{}", self.base_url, ep));
            req = req.query("limit", &limit.to_string());
            if let Some(ref viewer) = self.identity_pubkey() {
                req = req.query("viewer", viewer);
            }
            if let Some(ts) = before_ts {
                req = req.query("before", &ts.to_string());
            }

            match req.call() {
                Ok(resp) => {
                    let val: serde_json::Value = match resp.into_json() {
                        Ok(v) => v,
                        Err(e) => {
                            last_err = e.to_string();
                            continue;
                        }
                    };

                    let posts: Vec<Post> = if let Ok(list) = serde_json::from_value::<Vec<Post>>(val.clone()) {
                        list
                    } else if let Some(arr) = val.get("posts") {
                        serde_json::from_value::<Vec<Post>>(arr.clone()).unwrap_or_default()
                    } else {
                        vec![]
                    };

                    let mut cache = self.cache.lock().unwrap();
                    for p in &posts {
                        cache.posts.insert(p.id.clone(), p.clone());
                    }
                    cache.feed = posts.clone();
                    return Ok(posts);
                }
                Err(ureq::Error::Status(404, _)) => {
                    continue;
                }
                Err(e) => {
                    last_err = format!("feed request failed: {}", e);
                }
            }
        }

        let cache = self.cache.lock().unwrap();
        if !cache.feed.is_empty() {
            Ok(cache.feed.clone())
        } else if !last_err.is_empty() {
            Err(last_err)
        } else {
            Err("feed endpoint not found".to_string())
        }
    }

    pub fn get_replies(&self, parent_id: &str) -> Result<Vec<Post>, String> {
        let url = format!("{}/api/community/posts", self.base_url);
        let mut req = self.agent.get(&url).query("reply_to", parent_id);
        if let Some(ref viewer) = self.identity_pubkey() {
            req = req.query("viewer", viewer);
        }
        match req.call() {
            Ok(resp) => {
                let posts: Vec<Post> = resp.into_json().map_err(|e| e.to_string())?;
                Ok(posts)
            }
            Err(e) => Err(format!("get_replies failed: {}", e)),
        }
    }

    pub fn get_post(&self, post_id: &str) -> Result<Post, String> {
        if self.offline_mode {
            let cache = self.cache.lock().unwrap();
            return cache
                .posts
                .get(post_id)
                .cloned()
                .ok_or_else(|| "post not found in offline cache".to_string());
        }

        let url = format!("{}/api/community/post", self.base_url);
        match self.agent.get(&url).query("id", post_id).call() {
            Ok(resp) => {
                let post: Post = resp.into_json().map_err(|e| e.to_string())?;
                self.cache.lock().unwrap().posts.insert(post.id.clone(), post.clone());
                Ok(post)
            }
            Err(e) => {
                let cache = self.cache.lock().unwrap();
                if let Some(post) = cache.posts.get(post_id) {
                    Ok(post.clone())
                } else {
                    Err(format!("get_post failed: {}", e))
                }
            }
        }
    }

    pub fn interact_post(
        &self,
        post_id: &str,
        kind: InteractionType,
        content: Option<String>,
    ) -> Result<PostInteraction, String> {
        let payload = InteractPostPayload {
            post_id: post_id.to_string(),
            kind,
            content,
        };

        let action = match kind {
            InteractionType::Like => "post:like",
            InteractionType::Repost => "post:repost",
            InteractionType::Reply => "post:reply",
        };

        let resp: serde_json::Value = self.post_envelope("/api/community/post/interact", action, payload)?;
        if kind == InteractionType::Like {
            if let Ok(concrete) = serde_json::from_value::<crate::community::types::LikeResult>(resp.clone()) {
                return Ok(crate::community::types::PostInteraction {
                    id: concrete.id.unwrap_or_default(),
                    post_id: post_id.to_string(),
                    kind: InteractionType::Like,
                    author_pubkey: self.identity_pubkey().unwrap_or_default(),
                    author_handle: String::new(),
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                    content: None,
                    liked: concrete.liked,
                    likes_count: concrete.likes_count,
                });
            }
        }
        let interaction: PostInteraction = serde_json::from_value(resp).map_err(|e| e.to_string())?;
        Ok(interaction)
    }

    pub fn send_dm(
        &self,
        conversation_id: &str,
        content: &str,
        attachments: Vec<Attachment>,
        reply_to_id: Option<String>,
    ) -> Result<DirectMessage, String> {
        let payload = SendDmPayload {
            conversation_id: conversation_id.to_string(),
            content: content.to_string(),
            attachments,
            reply_to_id,
        };

        let msg: DirectMessage = self.post_envelope("/api/community/messages", "dm:send", payload)?;
        let mut cache = self.cache.lock().unwrap();
        cache
            .messages
            .entry(conversation_id.to_string())
            .or_default()
            .push(msg.clone());
        Ok(msg)
    }

    pub fn get_conversations(&self) -> Result<Vec<Conversation>, String> {
        let pk = self.identity_pubkey().unwrap_or_default();
        if self.offline_mode {
            let cache = self.cache.lock().unwrap();
            return Ok(cache.conversations.values().cloned().collect());
        }

        let url = format!("{}/api/community/conversations", self.base_url);
        match self.agent.get(&url).query("pubkey", &pk).call() {
            Ok(resp) => {
                let list: Vec<Conversation> = resp.into_json().map_err(|e| e.to_string())?;
                let mut cache = self.cache.lock().unwrap();
                for c in &list {
                    cache.conversations.insert(c.id.clone(), c.clone());
                }
                Ok(list)
            }
            Err(e) => {
                let cache = self.cache.lock().unwrap();
                if !cache.conversations.is_empty() {
                    Ok(cache.conversations.values().cloned().collect())
                } else {
                    Err(format!("get_conversations failed: {}", e))
                }
            }
        }
    }

    pub fn get_messages(
        &self,
        conversation_id: &str,
        limit: usize,
    ) -> Result<Vec<DirectMessage>, String> {
        if self.offline_mode {
            let cache = self.cache.lock().unwrap();
            return Ok(cache
                .messages
                .get(conversation_id)
                .cloned()
                .unwrap_or_default());
        }

        let url = format!("{}/api/community/messages", self.base_url);
        match self
            .agent
            .get(&url)
            .query("conversation_id", conversation_id)
            .query("limit", &limit.to_string())
            .call()
        {
            Ok(resp) => {
                let msgs: Vec<DirectMessage> = resp.into_json().map_err(|e| e.to_string())?;
                let mut cache = self.cache.lock().unwrap();
                cache.messages.insert(conversation_id.to_string(), msgs.clone());
                Ok(msgs)
            }
            Err(e) => {
                let cache = self.cache.lock().unwrap();
                if let Some(msgs) = cache.messages.get(conversation_id) {
                    Ok(msgs.clone())
                } else {
                    Err(format!("get_messages failed: {}", e))
                }
            }
        }
    }

    pub fn create_conversation(
        &self,
        recipient_pubkeys: Vec<String>,
        title: Option<String>,
    ) -> Result<Conversation, String> {
        let payload = CreateConversationPayload {
            recipient_pubkeys,
            title,
        };
        let conv: Conversation =
            self.post_envelope("/api/community/conversations", "conversation:create", payload)?;
        self.cache
            .lock()
            .unwrap()
            .conversations
            .insert(conv.id.clone(), conv.clone());
        Ok(conv)
    }

    pub fn create_guild(
        &self,
        name: &str,
        description: &str,
        icon_url: Option<String>,
    ) -> Result<Guild, String> {
        let payload = CreateGuildPayload {
            name: name.to_string(),
            description: description.to_string(),
            icon_url,
        };
        let guild: Guild = self.post_envelope("/api/community/guilds", "guild:create", payload)?;
        self.cache
            .lock()
            .unwrap()
            .guilds
            .insert(guild.id.clone(), guild.clone());
        Ok(guild)
    }

    pub fn get_guild(&self, guild_id: &str) -> Result<Guild, String> {
        if self.offline_mode {
            let cache = self.cache.lock().unwrap();
            return cache
                .guilds
                .get(guild_id)
                .cloned()
                .ok_or_else(|| "guild not found in cache".to_string());
        }

        let url = format!("{}/api/community/guild", self.base_url);
        match self.agent.get(&url).query("id", guild_id).call() {
            Ok(resp) => {
                let g: Guild = resp.into_json().map_err(|e| e.to_string())?;
                self.cache.lock().unwrap().guilds.insert(g.id.clone(), g.clone());
                Ok(g)
            }
            Err(e) => {
                let cache = self.cache.lock().unwrap();
                if let Some(g) = cache.guilds.get(guild_id) {
                    Ok(g.clone())
                } else {
                    Err(format!("get_guild failed: {}", e))
                }
            }
        }
    }

    pub fn list_guilds(&self) -> Result<Vec<Guild>, String> {
        if self.offline_mode {
            let cache = self.cache.lock().unwrap();
            return Ok(cache.guilds.values().cloned().collect());
        }

        let url = format!("{}/api/community/guilds", self.base_url);
        match self.agent.get(&url).call() {
            Ok(resp) => {
                let list: Vec<Guild> = resp.into_json().map_err(|e| e.to_string())?;
                let mut cache = self.cache.lock().unwrap();
                for g in &list {
                    cache.guilds.insert(g.id.clone(), g.clone());
                }
                Ok(list)
            }
            Err(e) => {
                let cache = self.cache.lock().unwrap();
                if !cache.guilds.is_empty() {
                    Ok(cache.guilds.values().cloned().collect())
                } else {
                    Err(format!("list_guilds failed: {}", e))
                }
            }
        }
    }

    pub fn join_guild(&self, invite_code: &str) -> Result<GuildMember, String> {
        let payload = JoinGuildPayload {
            invite_code: invite_code.to_string(),
        };
        self.post_envelope("/api/community/guild/join", "guild:join", payload)
    }

    pub fn send_guild_message(
        &self,
        guild_id: &str,
        channel_id: &str,
        content: &str,
        attachments: Vec<Attachment>,
        reply_to_id: Option<String>,
    ) -> Result<GuildMessage, String> {
        let payload = SendGuildMessagePayload {
            guild_id: guild_id.to_string(),
            channel_id: channel_id.to_string(),
            content: content.to_string(),
            attachments,
            reply_to_id,
        };
        let msg: GuildMessage =
            self.post_envelope("/api/community/guild/messages", "guild:message", payload)?;
        let mut cache = self.cache.lock().unwrap();
        cache
            .guild_messages
            .entry(channel_id.to_string())
            .or_default()
            .push(msg.clone());
        Ok(msg)
    }

    pub fn get_guild_members(&self, guild_id: &str) -> Result<Vec<GuildMember>, String> {
        let url = format!("{}/api/community/guild/members", self.base_url);
        match self.agent.get(&url).query("guild_id", guild_id).call() {
            Ok(resp) => resp
                .into_json::<Vec<GuildMember>>()
                .map_err(|e| format!("failed to parse guild members: {}", e)),
            Err(e) => Err(format!("get_guild_members failed: {}", e)),
        }
    }

    pub fn get_guild_messages(
        &self,
        channel_id: &str,
        limit: usize,
    ) -> Result<Vec<GuildMessage>, String> {
        if self.offline_mode {
            let cache = self.cache.lock().unwrap();
            return Ok(cache
                .guild_messages
                .get(channel_id)
                .cloned()
                .unwrap_or_default());
        }

        let url = format!("{}/api/community/guild/messages", self.base_url);
        match self
            .agent
            .get(&url)
            .query("channel_id", channel_id)
            .query("limit", &limit.to_string())
            .call()
        {
            Ok(resp) => {
                let msgs: Vec<GuildMessage> = resp.into_json().map_err(|e| e.to_string())?;
                let mut cache = self.cache.lock().unwrap();
                cache.guild_messages.insert(channel_id.to_string(), msgs.clone());
                Ok(msgs)
            }
            Err(e) => {
                let cache = self.cache.lock().unwrap();
                if let Some(msgs) = cache.guild_messages.get(channel_id) {
                    Ok(msgs.clone())
                } else {
                    Err(format!("get_guild_messages failed: {}", e))
                }
            }
        }
    }

    pub fn get_profile(&self, handle_or_pubkey: &str) -> Result<UserProfile, String> {
        if self.offline_mode {
            let cache = self.cache.lock().unwrap();
            return cache
                .profiles
                .get(handle_or_pubkey)
                .cloned()
                .ok_or_else(|| "profile not found in offline cache".to_string());
        }

        let url = format!("{}/api/community/profile", self.base_url);
        match self.agent.get(&url).query("id", handle_or_pubkey).call() {
            Ok(resp) => {
                let prof: UserProfile = resp.into_json().map_err(|e| e.to_string())?;
                let mut cache = self.cache.lock().unwrap();
                cache.profiles.insert(prof.pubkey.clone(), prof.clone());
                cache.profiles.insert(prof.handle.clone(), prof.clone());
                Ok(prof)
            }
            Err(e) => {
                let cache = self.cache.lock().unwrap();
                if let Some(p) = cache.profiles.get(handle_or_pubkey) {
                    Ok(p.clone())
                } else {
                    Err(format!("get_profile failed: {}", e))
                }
            }
        }
    }

    pub fn update_profile(&self, profile: UserProfile) -> Result<UserProfile, String> {
        let updated: UserProfile =
            self.post_envelope("/api/community/profile", "profile:update", profile)?;
        let mut cache = self.cache.lock().unwrap();
        cache.profiles.insert(updated.pubkey.clone(), updated.clone());
        cache.profiles.insert(updated.handle.clone(), updated.clone());
        Ok(updated)
    }

    pub fn follow_user(&self, target_pubkey: &str) -> Result<FollowRelation, String> {
        let payload = FollowPayload {
            target_pubkey: target_pubkey.to_string(),
            follow: true,
        };
        self.post_envelope("/api/community/follow", "user:follow", payload)
    }

    pub fn unfollow_user(&self, target_pubkey: &str) -> Result<FollowRelation, String> {
        let payload = FollowPayload {
            target_pubkey: target_pubkey.to_string(),
            follow: false,
        };
        self.post_envelope("/api/community/follow", "user:unfollow", payload)
    }

    pub fn search(&self, query: &str) -> Result<SearchResults, String> {
        let url = format!("{}/api/community/search", self.base_url);
        self.agent
            .get(&url)
            .query("q", query)
            .call()
            .map_err(|e| format!("search request failed: {}", e))?
            .into_json::<SearchResults>()
            .map_err(|e| format!("failed to parse search results: {}", e))
    }

    pub fn report(
        &self,
        target_type: ReportTargetType,
        target_id: &str,
        reason: &str,
        details: Option<&str>,
    ) -> Result<Report, String> {
        let payload = ReportPayload {
            target_type,
            target_id: target_id.to_string(),
            reason: reason.to_string(),
            details: details.map(|s| s.to_string()),
        };
        self.post_envelope("/api/community/report", "report:create", payload)
    }

    pub fn moderate(
        &self,
        target_pubkey: &str,
        action_type: ModActionType,
        reason: &str,
        duration_secs: Option<u64>,
        guild_id: Option<&str>,
    ) -> Result<ModerationAction, String> {
        let payload = ModerationPayload {
            target_pubkey: target_pubkey.to_string(),
            action_type,
            reason: reason.to_string(),
            duration_secs,
            guild_id: guild_id.map(|s| s.to_string()),
        };
        self.post_envelope("/api/community/moderate", "mod:action", payload)
    }

    pub fn get_reports(&self) -> Result<Vec<Report>, String> {
        let url = format!("{}/api/community/reports", self.base_url);
        self.agent
            .get(&url)
            .call()
            .map_err(|e| format!("get_reports failed: {}", e))?
            .into_json::<Vec<Report>>()
            .map_err(|e| format!("failed to parse reports: {}", e))
    }

    pub fn get_notifications(&self) -> Result<Vec<Notification>, String> {
        let pk = self.identity_pubkey().unwrap_or_default();
        let url = format!("{}/api/community/notifications", self.base_url);
        match self.agent.get(&url).query("pubkey", &pk).call() {
            Ok(resp) => resp
                .into_json::<Vec<Notification>>()
                .map_err(|e| format!("failed to parse notifications: {}", e)),
            Err(e) => Err(format!("get_notifications failed: {}", e)),
        }
    }

    pub fn mark_notifications_read(&self, ids: &[String], all: bool) -> Result<usize, String> {
        let payload = MarkReadPayload {
            ids: ids.to_vec(),
            all,
        };
        let v: serde_json::Value =
            self.post_envelope("/api/community/notifications/read", "notifications:read", payload)?;
        Ok(v
            .get("marked")
            .and_then(|n| n.as_u64())
            .unwrap_or(0) as usize)
    }

    pub fn edit_post(&self, post_id: &str, content: &str) -> Result<Post, String> {
        let payload = EditPostPayload {
            post_id: post_id.to_string(),
            content: content.to_string(),
        };
        let post: Post = self.post_envelope("/api/community/post/edit", "post:edit", payload)?;
        let mut cache = self.cache.lock().unwrap();
        cache.posts.insert(post.id.clone(), post.clone());
        Ok(post)
    }

    pub fn delete_post(&self, post_id: &str) -> Result<(), String> {
        let payload = DeletePostPayload {
            post_id: post_id.to_string(),
        };
        let _: serde_json::Value =
            self.post_envelope("/api/community/post/delete", "post:delete", payload)?;
        let mut cache = self.cache.lock().unwrap();
        cache.posts.remove(post_id);
        Ok(())
    }

    pub fn edit_dm(
        &self,
        conversation_id: &str,
        message_id: &str,
        content: &str,
    ) -> Result<DirectMessage, String> {
        let payload = EditDmPayload {
            conversation_id: conversation_id.to_string(),
            message_id: message_id.to_string(),
            content: content.to_string(),
        };
        self.post_envelope("/api/community/messages/edit", "dm:edit", payload)
    }

    pub fn delete_dm(&self, conversation_id: &str, message_id: &str) -> Result<DirectMessage, String> {
        let payload = DeleteDmPayload {
            conversation_id: conversation_id.to_string(),
            message_id: message_id.to_string(),
        };
        self.post_envelope("/api/community/messages/delete", "dm:delete", payload)
    }

    pub fn edit_guild_message(
        &self,
        channel_id: &str,
        message_id: &str,
        content: &str,
    ) -> Result<GuildMessage, String> {
        let payload = EditGuildMessagePayload {
            channel_id: channel_id.to_string(),
            message_id: message_id.to_string(),
            content: content.to_string(),
        };
        self.post_envelope("/api/community/guild/messages/edit", "guild:message:edit", payload)
    }

    pub fn delete_guild_message(
        &self,
        channel_id: &str,
        message_id: &str,
    ) -> Result<GuildMessage, String> {
        let payload = DeleteGuildMessagePayload {
            channel_id: channel_id.to_string(),
            message_id: message_id.to_string(),
        };
        self.post_envelope(
            "/api/community/guild/messages/delete",
            "guild:message:delete",
            payload,
        )
    }

    pub fn block_user(&self, target_pubkey: &str, block: bool) -> Result<bool, String> {
        let payload = BlockPayload {
            target_pubkey: target_pubkey.to_string(),
            block,
        };
        let v: serde_json::Value =
            self.post_envelope("/api/community/block", "user:block", payload)?;
        Ok(v.get("blocked").and_then(|b| b.as_bool()).unwrap_or(block))
    }

    pub fn get_social_lists(&self) -> Result<(Vec<String>, Vec<String>, Vec<String>), String> {
        let pk = self.identity_pubkey().unwrap_or_default();
        let url = format!("{}/api/community/follows", self.base_url);
        match self.agent.get(&url).query("pubkey", &pk).call() {
            Ok(resp) => {
                let v: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
                let get = |k: &str| {
                    v.get(k)
                        .and_then(|a| a.as_array())
                        .map(|a| {
                            a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()
                        })
                        .unwrap_or_default()
                };
                Ok((get("following"), get("followers"), get("blocked")))
            }
            Err(e) => Err(format!("get_social_lists failed: {}", e)),
        }
    }

    pub fn get_audit_log(&self) -> Result<Vec<AuditLogEntry>, String> {
        let url = format!("{}/api/community/audit_log", self.base_url);
        self.agent
            .get(&url)
            .call()
            .map_err(|e| format!("get_audit_log failed: {}", e))?
            .into_json::<Vec<AuditLogEntry>>()
            .map_err(|e| format!("failed to parse audit log: {}", e))
    }

    pub fn send_e2ee_direct_message(
        &self,
        conversation_id: &str,
        recipient_x25519_hex: &str,
        plaintext: &str,
        secret_hex: &str,
    ) -> Result<DirectMessage, String> {
        let encrypted_content = crate::community::e2ee::encrypt_direct_message(
            secret_hex,
            recipient_x25519_hex,
            plaintext,
        )
        .map_err(|e| e.to_string())?;

        self.send_dm(conversation_id, &encrypted_content, vec![], None)
    }

    pub fn decrypt_direct_message(
        &self,
        encrypted_content: &str,
        sender_x25519_hex: &str,
        secret_hex: &str,
    ) -> String {
        crate::community::e2ee::decrypt_direct_message(secret_hex, sender_x25519_hex, encrypted_content)
            .unwrap_or_else(|_| "[Encrypted Message - Key Mismatch]".to_string())
    }

    pub fn send_e2ee_group_message(
        &self,
        conversation_id: &str,
        group_key: &[u8; 32],
        plaintext: &str,
    ) -> Result<DirectMessage, String> {
        let encrypted_content =
            crate::community::e2ee::encrypt_group_message(group_key, plaintext)
                .map_err(|e| e.to_string())?;

        self.send_dm(conversation_id, &encrypted_content, vec![], None)
    }

    pub fn decrypt_group_message(
        &self,
        encrypted_content: &str,
        group_key: &[u8; 32],
    ) -> String {
        crate::community::e2ee::decrypt_group_message(group_key, encrypted_content)
            .unwrap_or_else(|_| "[Encrypted Group Message - Key Mismatch]".to_string())
    }
}
