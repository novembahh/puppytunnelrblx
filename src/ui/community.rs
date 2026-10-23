

use crate::community::client::CommunityClient;
use crate::community::worker::{Job, Worker};
use crate::community::e2ee::{
    decrypt_direct_message, get_x25519_public_hex, is_encrypted_payload,
};
use crate::community::keystore::{
    export_backup_json, generate_mnemonic, import_backup_str,
};
use crate::community::types::{
    Attachment, AuditLogEntry, Conversation, DirectMessage, Guild,
    GuildChannel, GuildMember, GuildMessage, InteractionType, Notification, NotificationKind, Post,
    UserProfile,
    Report, ReportStatus, ReportTargetType, SearchResults,
};
use crate::community::derive_ed25519_pubkey_hex;
use crate::config::{copy_text, AppConfig};
use crate::identity::Identity;
use crate::ui::icons::Icon;
use crate::ui::theme::*;
use crate::ui::widgets::*;
use eframe::egui::{
    self, pos2, Align, FontId, Id, Layout, Margin, RichText, Rounding, ScrollArea,
    Sense, TextEdit, Ui, Vec2,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};


pub struct GifAnim {
    frames: Vec<(egui::ColorImage, u64)>,
    total_ms: u64,
    started: Instant,
    tex: Option<egui::TextureHandle>,
}


pub fn short_handle(raw: &str) -> String {
    let bare = raw.trim().strip_prefix('@').unwrap_or(raw.trim());
    match bare.find('@') {
        Some(i) => format!("@{}", &bare[..i]),
        None if bare.is_empty() => "@?".to_string(),
        None => format!("@{}", bare),
    }
}

fn decode_gif_frames(data: &[u8]) -> Option<Vec<(egui::ColorImage, u64)>> {
    use image::codecs::gif::GifDecoder;
    use image::AnimationDecoder;
    let dec = GifDecoder::new(std::io::Cursor::new(data)).ok()?;
    let frames = dec.into_frames().collect_frames().ok()?;
    let mut out = Vec::new();
    let mut acc: u64 = 0;
    for f in frames.into_iter().take(60) {
        let (n, d) = f.delay().numer_denom_ms();
        let ms = if d == 0 { n as u64 } else { n as u64 / d.max(1) as u64 }.clamp(20, 1000);
        acc += ms;
        let mut buf = f.into_buffer();
        if buf.width() > 480 {
            let h = (480u32 * buf.height() / buf.width().max(1)).max(1);
            buf = image::imageops::thumbnail(&buf, 480, h);
        }
        let (w, h) = (buf.width() as usize, buf.height() as usize);
        out.push((egui::ColorImage::from_rgba_unmultiplied([w, h], buf.as_raw()), acc));
    }
    if out.is_empty() { None } else { Some(out) }
}


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommunityTab {
    Feed,
    Messages,
    Notifications,
    Search,
    Moderation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedFilter {
    All,
    Following,
    Trending,
}

#[derive(Clone, Debug)]
pub struct ReportDraft {
    pub target_type: ReportTargetType,
    pub target_id: String,
    pub target_label: String,
    pub reason: String,
    pub details: String,
}


pub struct CommunityUiState {
    pub active_tab: CommunityTab,


    pub feed_filter: FeedFilter,
    pub composer_text: String,
    pub composer_tag: String,
    pub composer_attachment: Option<PathBuf>,
    pub expanded_replies: HashSet<String>,
    pub reply_drafts: HashMap<String, String>,
    pub cached_replies: HashMap<String, Vec<Post>>,
    pub cached_posts: Vec<Post>,
    pub image_textures: HashMap<String, egui::TextureHandle>,
    pub gif_anims: HashMap<String, GifAnim>,
    pub feed_loaded: bool,


    pub active_conv_id: Option<String>,
    pub dm_composer: String,
    pub cached_conversations: Vec<Conversation>,
    pub cached_dms: HashMap<String, Vec<DirectMessage>>,
    pub new_dm_target: String,
    pub show_new_dm_modal: bool,
    pending_dm_lookup: Option<String>,
    pub new_group_name: String,
    pub group_keys: HashMap<String, [u8; 32]>,


    pub active_guild_id: Option<String>,
    pub active_channel_id: Option<String>,
    pub guild_composer: String,
    pub cached_guilds: Vec<Guild>,
    pub cached_channels: HashMap<String, Vec<GuildChannel>>,
    pub cached_guild_msgs: HashMap<String, Vec<GuildMessage>>,
    pub cached_members: HashMap<String, Vec<GuildMember>>,
    pub show_create_guild: bool,
    pub new_guild_name: String,
    pub new_guild_desc: String,
    pub show_create_channel: bool,
    pub new_channel_name: String,


    pub notifications: Vec<Notification>,
    pub notif_filter_unread: bool,


    pub search_query: String,
    pub search_results: SearchResults,
    pub is_searching: bool,


    pub reveal_secret: bool,
    pub show_mnemonic: bool,
    pub import_buffer: String,
    pub key_status_msg: Option<(String, bool, Instant)>,


    pub mod_tab: ModSubTab,
    pub reports: Vec<Report>,
    pub audit_logs: Vec<AuditLogEntry>,
    pub active_report_dialog: Option<ReportDraft>,
    pub ban_target_input: String,
    pub ban_reason_input: String,
    pub mod_status_msg: Option<(String, Instant)>,


    pub my_roblox_verified: bool,
    pub verified_check_at: Option<Instant>,
    pub lists_checked_at: Option<Instant>,
    pub new_dm_error: String,
    pub following: HashSet<String>,
    pub peer_profiles: HashMap<String, UserProfile>,
    pub pending_peer_profiles: Arc<Mutex<HashSet<String>>>,
    pub peer_profile_tx: std::sync::mpsc::Sender<(String, UserProfile)>,
    pub peer_profile_rx: Option<std::sync::mpsc::Receiver<(String, UserProfile)>>,
    pub blocked: HashSet<String>,
    pub editing_msg_id: Option<String>,
    pub edit_buffer: String,
    pub local_headshot: Option<egui::TextureHandle>,
    pub avatar_cache: HashMap<u64, egui::TextureHandle>,
    pub avatar_url_cache: HashMap<String, egui::TextureHandle>,
    pub pending_avatar_fetches: Arc<Mutex<HashSet<u64>>>,
    pub pending_avatar_url_fetches: Arc<Mutex<HashSet<String>>>,
    pub avatar_rx: Option<std::sync::mpsc::Receiver<(u64, Vec<u8>)>>,
    pub avatar_tx: std::sync::mpsc::Sender<(u64, Vec<u8>)>,
    pub avatar_url_rx: Option<std::sync::mpsc::Receiver<(String, Vec<u8>)>>,
    pub avatar_url_tx: std::sync::mpsc::Sender<(String, Vec<u8>)>,
    pub profile_save_status: Option<(String, Instant)>,
    pub roblox_query_input: String,


    pub client: Option<CommunityClient>,
    client_base: String,
    pending_open: bool,
    pub last_sync: Instant,
    worker: Option<Worker>,
    in_flight: HashSet<Job>,
    pub sync_error: String,
    write_tx: std::sync::mpsc::Sender<WriteDone>,
    write_rx: std::sync::mpsc::Receiver<WriteDone>,
    writes_in_flight: usize,
    pub action_error: String,
    action_error_at: Option<Instant>,
}

enum WriteDone {
    Post(Result<Post, String>),
    Reply { parent: String, result: Result<Post, String> },
    Like { post_id: String, result: Result<(Option<bool>, Option<i32>), String> },
    Dm { conv: String, result: Result<DirectMessage, String> },
    Conversation(Result<Conversation, String>),
    DeletedPost { id: String, result: Result<(), String> },
    EditedPost { result: Result<Post, String> },
    DeletedDm { conv: String, id: String, result: Result<DirectMessage, String> },
    EditedDm { conv: String, id: String, result: Result<DirectMessage, String> },
    Follow { pk: String, on: bool, result: Result<(), String> },
    Block { pk: String, on: bool, result: Result<(), String> },
    Search(Result<SearchResults, String>),
    Report(Result<(), String>),
    Read(Result<usize, String>),
    Profile(Result<UserProfile, String>),
    Moderation(Result<(), String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModSubTab {
    ModQueue,
    GlobalBans,
    AuditLog,
}

impl Default for CommunityUiState {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let (url_tx, url_rx) = std::sync::mpsc::channel();
        let (peer_tx, peer_rx) = std::sync::mpsc::channel();
        let (write_tx, write_rx) = std::sync::mpsc::channel();
        Self {
            active_tab: CommunityTab::Feed,
            feed_filter: FeedFilter::All,
            composer_text: String::new(),
            composer_tag: String::new(),
            composer_attachment: None,
            expanded_replies: HashSet::new(),
            reply_drafts: HashMap::new(),
            cached_replies: HashMap::new(),
            cached_posts: Vec::new(),
            image_textures: HashMap::new(),
            gif_anims: HashMap::new(),
            feed_loaded: false,

            active_conv_id: None,
            dm_composer: String::new(),
            cached_conversations: Vec::new(),
            cached_dms: HashMap::new(),
            new_dm_target: String::new(),
            show_new_dm_modal: false,
            pending_dm_lookup: None,
            new_group_name: String::new(),
            group_keys: HashMap::new(),

            active_guild_id: None,
            active_channel_id: None,
            guild_composer: String::new(),
            cached_guilds: Vec::new(),
            cached_channels: HashMap::new(),
            cached_guild_msgs: HashMap::new(),
            cached_members: HashMap::new(),
            show_create_guild: false,
            new_guild_name: String::new(),
            new_guild_desc: String::new(),
            show_create_channel: false,
            new_channel_name: String::new(),

            notifications: Vec::new(),
            notif_filter_unread: false,

            search_query: String::new(),
            search_results: SearchResults::default(),
            is_searching: false,

            reveal_secret: false,
            show_mnemonic: false,
            import_buffer: String::new(),
            key_status_msg: None,

            mod_tab: ModSubTab::ModQueue,
            reports: Vec::new(),
            audit_logs: Vec::new(),
            active_report_dialog: None,
            ban_target_input: String::new(),
            ban_reason_input: String::new(),
            mod_status_msg: None,

            my_roblox_verified: false,
            verified_check_at: None,
            lists_checked_at: None,
            new_dm_error: String::new(),
            following: HashSet::new(),
            peer_profiles: HashMap::new(),
            pending_peer_profiles: Arc::new(Mutex::new(HashSet::new())),
            blocked: HashSet::new(),
            editing_msg_id: None,
            edit_buffer: String::new(),
            local_headshot: None,
            avatar_cache: HashMap::new(),
            avatar_url_cache: HashMap::new(),
            pending_avatar_fetches: Arc::new(Mutex::new(HashSet::new())),
            pending_avatar_url_fetches: Arc::new(Mutex::new(HashSet::new())),
            avatar_rx: Some(rx),
            avatar_tx: tx,
            avatar_url_rx: Some(url_rx),
            avatar_url_tx: url_tx,
            peer_profile_tx: peer_tx,
            peer_profile_rx: Some(peer_rx),
            profile_save_status: None,
            roblox_query_input: String::new(),

            client: None,
            client_base: String::new(),
            pending_open: false,
            last_sync: Instant::now() - Duration::from_secs(60),
            worker: None,
            in_flight: HashSet::new(),
            sync_error: String::new(),
            write_tx,
            write_rx,
            writes_in_flight: 0,
            action_error: String::new(),
            action_error_at: None,
        }
    }
}

fn b64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(triple >> 18) & 0x3F] as char);
        out.push(TABLE[(triple >> 12) & 0x3F] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(triple >> 6) & 0x3F] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[triple & 0x3F] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn b64_decode(input: &str) -> Option<Vec<u8>> {
    let input = input.trim();
    if input.is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for &b in input.as_bytes() {
        if b == b'=' {
            break;
        }
        let val = match b {
            b'A'..=b'Z' => (b - b'A') as u32,
            b'a'..=b'z' => (b - b'a' + 26) as u32,
            b'0'..=b'9' => (b - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        };
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

impl CommunityUiState {

    pub fn ensure_client(&mut self, config: &AppConfig) {
        let base = config.community_base();
        if self.client.is_none() || self.client_base != base {
            let id = if !config.server_secret.is_empty() {
                Some(Identity::profile(&config.server_secret))
            } else {
                None
            };
            self.client = Some(CommunityClient::new(&base, id));
            self.client_base = base;
            self.worker = self.client.as_ref().map(|c| Worker::spawn(c.clone()));
            self.in_flight.clear();
            self.writes_in_flight = 0;
            self.last_sync = Instant::now();
            self.feed_loaded = false;
            self.pending_open = true;
        }
    }

    fn request(&mut self, job: Job) {
        if !self.in_flight.insert(job.clone()) {
            return;
        }
        match self.worker.as_ref() {
            Some(w) => w.request(job),
            None => {
                self.in_flight.remove(&job);
            }
        }
    }

    fn spawn_write(
        &mut self,
        run: impl FnOnce(&CommunityClient) -> WriteDone + Send + 'static,
    ) {
        let Some(client) = self.client.clone() else { return };
        self.writes_in_flight += 1;
        let tx = self.write_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(run(&client));
        });
    }

    fn failed(&mut self, message: String) {
        self.action_error = message;
        self.action_error_at = Some(Instant::now());
    }

    fn apply_results(&mut self) {
        use crate::community::worker::Done;
        let results = match self.worker.as_ref() {
            Some(w) => w.drain(),
            None => return,
        };
        for done in results {
            match done {
                Done::Feed(r) => {
                    self.in_flight.remove(&Job::Feed(30));
                    match r {
                        Ok(posts) => {
                            let blocked = self.blocked.clone();
                            let mut posts = posts;
                            posts.retain(|p| !blocked.contains(&p.author_pubkey));
                            self.cached_posts = posts;
                            self.feed_loaded = true;
                            self.sync_error.clear();
                        }
                        Err(e) => {
                            self.feed_loaded = true;
                            self.sync_error = e;
                        }
                    }
                }
                Done::Replies(id, r) => {
                    self.in_flight.remove(&Job::Replies(id.clone()));
                    if let Ok(mut replies) = r {
                        let blocked = self.blocked.clone();
                        replies.retain(|p| !blocked.contains(&p.author_pubkey));
                        if let Some(list) = self.cached_replies.get_mut(&id) {
                            for reply in replies {
                                if !list.iter().any(|r| r.id == reply.id) {
                                    list.push(reply);
                                }
                            }
                        } else {
                            self.cached_replies.insert(id, replies);
                        }
                    }
                }
                Done::Conversations(r) => {
                    self.in_flight.remove(&Job::Conversations);
                    match r {
                        Ok(list) => self.set_conversations(list),
                        Err(e) => self.sync_error = e,
                    }
                }
                Done::Messages(id, r) => {
                    self.in_flight.remove(&Job::Messages(id.clone()));
                    match r {
                        Ok(msgs) => self.set_messages(&id, msgs),
                        Err(e) => self.sync_error = e,
                    }
                }
                Done::Guilds(r) => {
                    self.in_flight.remove(&Job::Guilds);
                    match r {
                        Ok(list) => self.set_guilds(list),
                        Err(e) => self.sync_error = e,
                    }
                }
                Done::GuildChannels(id, r) => {
                    self.in_flight.remove(&Job::GuildChannels(id.clone()));
                    if let Ok(ch) = r {
                        self.cached_channels.insert(id, ch);
                    }
                }
                Done::GuildMembers(id, r) => {
                    self.in_flight.remove(&Job::GuildMembers(id.clone()));
                    if let Ok(m) = r {
                        self.cached_members.insert(id, m);
                    }
                }
                Done::GuildMessages(id, r) => {
                    self.in_flight.remove(&Job::GuildMessages(id.clone()));
                    if let Ok(m) = r {
                        self.set_guild_messages(&id, m);
                    }
                }
                Done::Notifications(r) => {
                    self.in_flight.remove(&Job::Notifications);
                    match r {
                        Ok(list) => self.set_notifications(list),
                        Err(e) => self.sync_error = e,
                    }
                }
                Done::Social(r) => {
                    self.in_flight.remove(&Job::Social);
                    match r {
                        Ok((following, _followers, blocked)) => {
                            self.following = following.into_iter().collect();
                            self.blocked = blocked.into_iter().collect();
                        }
                        Err(e) => self.sync_error = e,
                    }
                }
                Done::SelfProfile(r) => {
                    self.in_flight.remove(&Job::SelfProfile);
                    if let Ok(prof) = r {
                        self.my_roblox_verified = prof.roblox_verified;
                    }
                }
                Done::User(key, r) => {
                    self.in_flight.remove(&Job::User(key.clone()));
                    if let Ok(prof) = r {
                        let pubkey = prof.pubkey.clone();
                        self.peer_profiles.insert(pubkey.clone(), prof);
                        if self.pending_dm_lookup.as_deref() == Some(&key) {
                            self.pending_dm_lookup = None;
                            self.new_dm_target.clear();
                            self.new_dm_error.clear();
                            self.show_new_dm_modal = false;
                            self.spawn_write(move |client| {
                                WriteDone::Conversation(client.create_conversation(vec![pubkey], None))
                            });
                        }
                    } else if self.pending_dm_lookup.as_deref() == Some(&key) {
                        self.pending_dm_lookup = None;
                        self.new_dm_error = format!("no such person: {}", key);
                    }
                }
                Done::Reports(r) => {
                    self.in_flight.remove(&Job::Reports);
                    match r {
                        Ok(list) => self.reports = list,
                        Err(e) => self.sync_error = e,
                    }
                }
                Done::Audit(r) => {
                    self.in_flight.remove(&Job::Audit);
                    match r {
                        Ok(list) => self.audit_logs = list,
                        Err(e) => self.sync_error = e,
                    }
                }
            }
        }
        while let Ok(done) = self.write_rx.try_recv() {
            self.writes_in_flight = self.writes_in_flight.saturating_sub(1);
            match done {
                WriteDone::Post(result) => match result {
                    Ok(post) => {
                        self.cached_posts.insert(0, post);
                        self.feed_loaded = true;
                        self.action_error.clear();
                    }
                    Err(e) => self.failed(e),
                },
                WriteDone::Reply { parent, result } => match result {
                    Ok(reply) => {
                        self.cached_replies.entry(parent.clone()).or_default().push(reply);
                        if let Some(p) = self.cached_posts.iter_mut().find(|p| p.id == parent) {
                            p.replies_count += 1;
                        }
                        self.action_error.clear();
                    }
                    Err(e) => self.failed(e),
                },
                WriteDone::Like { post_id, result } => match result {
                    Ok((liked, count)) => {
                        if let Some(p) = self.cached_posts.iter_mut().find(|p| p.id == post_id) {
                            if let Some(liked) = liked {
                                p.liked_by_me = liked;
                            }
                            if let Some(count) = count {
                                p.likes_count = count.max(0) as u32;
                            }
                        }
                    }
                    Err(e) => self.failed(e),
                },
                WriteDone::Dm { conv, result } => match result {
                    Ok(msg) => {
                        self.cached_dms.entry(conv).or_default().push(msg);
                        self.action_error.clear();
                    }
                    Err(e) => self.failed(e),
                },
                WriteDone::Conversation(result) => match result {
                    Ok(conv) => {
                        self.active_conv_id = Some(conv.id.clone());
                        self.cached_conversations.push(conv.clone());
                        self.request(Job::Messages(conv.id));
                        self.action_error.clear();
                    }
                    Err(e) => self.failed(e),
                },
                WriteDone::DeletedPost { id, result } => match result {
                    Ok(()) => {
                        self.cached_posts.retain(|p| p.id != id);
                        for list in self.cached_replies.values_mut() {
                            list.retain(|p| p.id != id);
                        }
                    }
                    Err(e) => self.failed(e),
                },
                WriteDone::EditedPost { result } => match result {
                    Ok(updated) => {
                        if let Some(p) = self.cached_posts.iter_mut().find(|p| p.id == updated.id) {
                            *p = updated.clone();
                        }
                        for list in self.cached_replies.values_mut() {
                            if let Some(r) = list.iter_mut().find(|r| r.id == updated.id) {
                                *r = updated.clone();
                            }
                        }
                        self.editing_msg_id = None;
                        self.edit_buffer.clear();
                        self.action_error.clear();
                    }
                    Err(e) => self.failed(e),
                },
                WriteDone::DeletedDm { conv, id, result } => match result {
                    Ok(updated) => {
                        if let Some(list) = self.cached_dms.get_mut(&conv) {
                            if let Some(m) = list.iter_mut().find(|m| m.id == id) {
                                *m = updated;
                            }
                        }
                    }
                    Err(e) => self.failed(e),
                },
                WriteDone::EditedDm { conv, id, result } => match result {
                    Ok(updated) => {
                        if let Some(list) = self.cached_dms.get_mut(&conv) {
                            if let Some(m) = list.iter_mut().find(|m| m.id == id) {
                                *m = updated;
                            }
                        }
                        self.editing_msg_id = None;
                        self.edit_buffer.clear();
                        self.action_error.clear();
                    }
                    Err(e) => self.failed(e),
                },
                WriteDone::Follow { pk, on, result } => match result {
                    Ok(()) => {
                        if on {
                            self.following.insert(pk);
                        } else {
                            self.following.remove(&pk);
                        }
                    }
                    Err(e) => self.failed(e),
                },
                WriteDone::Block { pk, on, result } => match result {
                    Ok(()) => {
                        if on {
                            self.blocked.insert(pk);
                        } else {
                            self.blocked.remove(&pk);
                        }
                        self.apply_block_filter();
                    }
                    Err(e) => self.failed(e),
                },
                WriteDone::Search(result) => {
                    self.is_searching = false;
                    match result {
                        Ok(res) => self.search_results = res,
                        Err(e) => self.failed(e),
                    }
                }
                WriteDone::Report(result) => match result {
                    Ok(()) => {
                        self.mod_status_msg = Some(("report sent".to_string(), Instant::now()));
                        self.active_report_dialog = None;
                    }
                    Err(e) => self.failed(e),
                },
                WriteDone::Read(result) => {
                    if let Err(e) = result {
                        self.failed(e);
                    }
                }
                WriteDone::Profile(result) => match result {
                    Ok(_) => {
                        self.profile_save_status = Some(("saved".to_string(), Instant::now()));
                        self.action_error.clear();
                    }
                    Err(e) => {
                        self.profile_save_status = Some((format!("not saved: {}", e), Instant::now()));
                        self.failed(e);
                    }
                },
                WriteDone::Moderation(result) => match result {
                    Ok(()) => {
                        self.mod_status_msg = Some(("done".to_string(), Instant::now()));
                        self.request(Job::Reports);
                        self.request(Job::Audit);
                    }
                    Err(e) => self.failed(e),
                },
            }
        }
    }

    fn set_conversations(&mut self, convs: Vec<Conversation>) {
        let my_pk = self.client.as_ref().and_then(|c| c.identity_pubkey()).unwrap_or_default();
        let blocked = self.blocked.clone();
        self.cached_conversations = convs
            .into_iter()
            .filter(|c| {
                c.is_group || c.members.iter().all(|m| m.pubkey == my_pk || !blocked.contains(&m.pubkey))
            })
            .collect();
        if self.active_conv_id.is_none() {
            if let Some(first) = self.cached_conversations.first().map(|c| c.id.clone()) {
                self.active_conv_id = Some(first.clone());
                self.request(Job::Messages(first));
            }
        }
    }

    fn set_messages(&mut self, conv_id: &str, msgs: Vec<DirectMessage>) {
        let blocked = self.blocked.clone();
        let msgs = msgs.into_iter().filter(|m| !blocked.contains(&m.sender_pubkey)).collect();
        self.cached_dms.insert(conv_id.to_string(), msgs);
    }

    fn set_guild_messages(&mut self, channel_id: &str, msgs: Vec<GuildMessage>) {
        let blocked = self.blocked.clone();
        let msgs = msgs.into_iter().filter(|m| !blocked.contains(&m.author_pubkey)).collect();
        self.cached_guild_msgs.insert(channel_id.to_string(), msgs);
    }

    fn set_guilds(&mut self, guilds: Vec<Guild>) {
        self.cached_guilds = guilds;
        if self.active_guild_id.is_none() {
            if let Some(first) = self.cached_guilds.first().map(|g| g.id.clone()) {
                self.active_guild_id = Some(first.clone());
                self.request(Job::GuildChannels(first));
            }
        }
    }

    fn set_notifications(&mut self, mut list: Vec<Notification>) {
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        self.notifications = list;
    }


    pub fn publish_profile(&mut self, config: &AppConfig) {
        if config.server_secret.trim().is_empty() {
            return;
        }
        let pk = derive_ed25519_pubkey_hex(&config.server_secret);
        let roblox_name = config.roblox_name.trim().trim_start_matches('@');
        let handle = if !roblox_name.is_empty() {
            format!("@{}", roblox_name)
        } else {
            format!("@{}", &pk[..pk.len().min(8)])
        };
        let display = {
            let eff = config.effective_display_name();
            if !eff.trim().is_empty() {
                eff.to_string()
            } else if !roblox_name.is_empty() {
                roblox_name.to_string()
            } else {
                String::new()
            }
        };
        let prof = UserProfile {
            pubkey: pk,
            handle,
            display_name: display,
            roblox_id: if config.roblox_id != 0 { Some(config.roblox_id) } else { None },
            ..Default::default()
        };
        if self.client.is_some() {
            self.spawn_write(move |client| WriteDone::Profile(client.update_profile(prof)));
        }
    }

    pub fn open(&mut self) {
        self.pending_open = true;
    }

    pub fn select_tab(&mut self, tab: CommunityTab) {
        self.active_tab = tab;
        match tab {
            CommunityTab::Feed => {
                if !self.feed_loaded {
                    self.request(Job::Feed(30));
                }
                self.request(Job::Notifications);
            }
            CommunityTab::Messages => {
                self.request(Job::Social);
                self.request(Job::Conversations);
            }
            CommunityTab::Search => self.request(Job::Social),
            CommunityTab::Notifications => self.request(Job::Notifications),
            CommunityTab::Moderation => {
                self.request(Job::Reports);
                self.request(Job::Audit);
            }
        }
    }

    pub fn fetch_conversations(&mut self) {
        self.request(Job::Conversations);
    }

    pub fn fetch_guilds(&mut self) {
        self.request(Job::Guilds);
    }

    pub fn fetch_notifications(&mut self) {
        self.request(Job::Notifications);
    }

    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read).count()
    }

    pub fn pending_reports(&self) -> usize {
        self.reports.iter().filter(|r| r.status == ReportStatus::Pending).count()
    }

    pub fn instance_label(&self, config: &AppConfig) -> String {
        let url = config.community_base();
        url.split("://").last().unwrap_or(&url).to_string()
    }


    pub fn render(&mut self, ui: &mut Ui, config: &mut AppConfig, ctx: &egui::Context) {
        let t0 = Instant::now();
        self.ensure_client(config);
        self.apply_results();
        let t1 = Instant::now();


        if let Some(ref rx) = self.avatar_rx {
            while let Ok((id, bytes)) = rx.try_recv() {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    let rgba = img.to_rgba8();
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                    let tex = ctx.load_texture(format!("roblox_headshot_{}", id), color, egui::TextureOptions::LINEAR);
                    self.avatar_cache.insert(id, tex);
                }
            }
        }

        if let Some(ref rx) = self.peer_profile_rx {
            while let Ok((pk, prof)) = rx.try_recv() {
                let handle_ok = !prof.handle.trim().is_empty()
                    || !prof.display_name.trim().is_empty();
                if handle_ok || prof.avatar_url.is_some() || prof.roblox_id.is_some() {
                    self.peer_profiles.insert(pk, prof);
                }
            }
        }

        if let Some(ref rx) = self.avatar_url_rx {
            while let Ok((url, bytes)) = rx.try_recv() {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    let rgba = img.to_rgba8();
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                    let tex = ctx.load_texture(format!("avatar_url_{}", url), color, egui::TextureOptions::LINEAR);
                    self.avatar_url_cache.insert(url, tex);
                }
            }
        }

        if self.pending_open {
            self.pending_open = false;
            self.select_tab(self.active_tab);
        }
        let t2 = Instant::now();


        if self.last_sync.elapsed() > Duration::from_secs(3) {
            self.poll_updates(config);
            self.last_sync = Instant::now();
        }

        if !self.in_flight.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
        let t3 = Instant::now();

        ui.vertical(|ui| {
            if !self.sync_error.is_empty() {
                notice(ui, RED_LIT, &self.sync_error);
                ui.add_space(8.0);
            }
            if !self.action_error.is_empty()
                && self.action_error_at.map_or(false, |t| t.elapsed() < Duration::from_secs(5))
            {
                notice(ui, RED_LIT, &self.action_error);
                ui.add_space(8.0);
            }
            match self.active_tab {
                CommunityTab::Feed => self.render_feed(ui, config),
                CommunityTab::Messages => self.render_messages(ui, config),
                CommunityTab::Notifications => self.render_notifications(ui, config),
                CommunityTab::Search => self.render_search(ui, config),
                CommunityTab::Moderation => self.render_moderation(ui, config),
            }
            self.render_active_modals(ui, config, ctx);
        });
        let t4 = Instant::now();
        if let Ok(path) = std::env::var("PT_TRACE2") {
            let total = t0.elapsed().as_secs_f64() * 1000.0;
            if total > 30.0 {
                use std::io::Write as _;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                    let _ = writeln!(
                        f,
                        "total {:.0} client {:.0} results {:.0} skin {:.0} ui {:.0}",
                        total,
                        (t1 - t0).as_secs_f64() * 1000.0,
                        (t2 - t1).as_secs_f64() * 1000.0,
                        (t3 - t2).as_secs_f64() * 1000.0,
                        (t4 - t3).as_secs_f64() * 1000.0
                    );
                }
            }
        }
    }

    fn render_feed(&mut self, ui: &mut Ui, config: &AppConfig) {
        ScrollArea::vertical().id_salt("community_scroll_1")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                panel(ui, |ui| {
                ui.add(
                    TextEdit::multiline(&mut self.composer_text)
                        .id(Id::new("feed_composer"))
                        .frame(false)
                        .desired_rows(2)
                        .hint_text(RichText::new("say something").color(TX_FAINT))
                        .desired_width(ui.available_width()),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if quiet(ui, "attach") {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"])
                            .pick_file()
                        {
                            self.composer_attachment = Some(path);
                        }
                    }
                    if let Some(ref path) = self.composer_attachment.clone() {
                        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        ui.label(RichText::new(name).size(T_META).color(TX_FAINT));
                        if quiet(ui, "remove") {
                            self.composer_attachment = None;
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let len = self.composer_text.len();
                        let can_post = !self.composer_text.trim().is_empty() && len <= 500;
                        if primary(ui, "post", can_post) {
                            self.submit_post(config);
                        }
                        if len > 400 {
                            let c = if len > 500 { RED_LIT } else { TX_FAINT };
                            ui.label(RichText::new(format!("{}", 500 - len as i32)).size(T_META).color(c));
                        }
                    });
                });
                });

                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    for (label, filter) in [
                        ("all", FeedFilter::All),
                        ("following", FeedFilter::Following),
                        ("trending", FeedFilter::Trending),
                    ] {
                        if inline_action(ui, label, self.feed_filter == filter) {
                            self.feed_filter = filter;
                            self.refresh_feed();
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if quiet(ui, "refresh") {
                            self.refresh_feed();
                        }
                    });
                });

                ui.add_space(14.0);

                if self.cached_posts.is_empty() {
                    if self.in_flight.contains(&Job::Feed(30)) {
                        ui.label(RichText::new("loading…").size(T_BODY).color(TX_FAINT));
                        ui.add_space(20.0);
                    } else {
                        empty_state(ui, Icon::Feed, "nothing here yet", "");
                    }
                } else {
                    let posts = self.cached_posts.clone();
                    for post in posts {
                        self.render_post_card(ui, &post, config);
                        ui.add_space(22.0);
                    }
                }

                ui.add_space(20.0);
            });
    }

    fn render_post_card(&mut self, ui: &mut Ui, post: &Post, config: &AppConfig) {
        let my_pubkey = derive_ed25519_pubkey_hex(&config.server_secret);
        let is_me = my_pubkey == post.author_pubkey;

        let (peer_display, peer_handle, res_avatar, res_rid) = if is_me {
            (
                config.effective_display_name().to_string(),
                format!(
                    "@{}",
                    if !config.roblox_name.is_empty() {
                        &config.roblox_name
                    } else {
                        "me"
                    }
                ),
                None,
                None,
            )
        } else {
            self.resolve_peer(
                &post.author_pubkey,
                &post.author_display,
                &post.author_handle,
                &post.author_avatar_url,
                post.author_roblox_id,
            )
        };
        let author_display = peer_display;
        let author_handle = peer_handle;

        let is_verified = (is_me && self.my_roblox_verified) || post.author_roblox_verified;

        let roblox_id = if is_me && config.roblox_id != 0 {
            Some(config.roblox_id)
        } else {
            res_rid
        };
        let post_avatar_url = if is_me {
            post.author_avatar_url.clone()
        } else {
            res_avatar
        };

        let mut headshot_tex: Option<egui::TextureHandle> = if is_me && self.local_headshot.is_some() {
            self.local_headshot.clone()
        } else if let Some(rid) = roblox_id {
            if let Some(tex) = self.avatar_cache.get(&rid) {
                Some(tex.clone())
            } else {
                if let Ok(mut pending) = self.pending_avatar_fetches.lock() {
                    if !pending.contains(&rid) {
                        pending.insert(rid);
                        let tx = self.avatar_tx.clone();
                        let pending_arc = Arc::clone(&self.pending_avatar_fetches);
                        std::thread::spawn(move || {
                            if let Ok(bytes) = crate::rbxapi::headshot_png(rid) {
                                let _ = tx.send((rid, bytes));
                            }
                            if let Ok(mut p) = pending_arc.lock() {
                                p.remove(&rid);
                            }
                        });
                    }
                }
                None
            }
        } else {
            None
        };

        if headshot_tex.is_none() {
            if let Some(ref avatar_url) = post_avatar_url {
                if !avatar_url.is_empty() {
                    if let Some(tex) = self.avatar_url_cache.get(avatar_url) {
                        headshot_tex = Some(tex.clone());
                    } else if let Ok(mut pending) = self.pending_avatar_url_fetches.lock() {
                        if !pending.contains(avatar_url) {
                            pending.insert(avatar_url.clone());
                            let url = avatar_url.clone();
                            let tx = self.avatar_url_tx.clone();
                            let pending_arc = Arc::clone(&self.pending_avatar_url_fetches);
                            std::thread::spawn(move || {
                                if let Ok(resp) = ureq::get(&url).call() {
                                    let mut bytes = Vec::new();
                                    if let Ok(_) = resp.into_reader().read_to_end(&mut bytes) {
                                        let _ = tx.send((url.clone(), bytes));
                                    }
                                }
                                if let Ok(mut p) = pending_arc.lock() {
                                    p.remove(&url);
                                }
                            });
                        }
                    }
                }
            }
        }

        block(ui, R_BLOCK, Margin::symmetric(16.0, 14.0), mix(INK, MILK, 0.012), 0.0, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {

                ui.vertical(|ui| {
                    ui.set_width(34.0);
                    if let Some(ref tex) = headshot_tex {
                        ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::splat(32.0)).rounding(16.0));
                    } else {
                        let initial = if !author_display.is_empty() {
                            author_display.chars().next().unwrap_or('?').to_uppercase().to_string()
                        } else {
                            post.author_pubkey.chars().next().unwrap_or('?').to_uppercase().to_string()
                        };
                        let (dot, _) = ui.allocate_exact_size(Vec2::new(32.0, 32.0), Sense::hover());
                        let accent_col = post.author_accent.as_deref().map(hex_color).unwrap_or(BRUISE_LIT);
                        ui.painter().rect_filled(dot, Rounding::same(16.0), SURF_2);
                        ui.painter().text(
                            dot.center(),
                            egui::Align2::CENTER_CENTER,
                            initial,
                            egui::FontId::proportional(T_META + 1.0),
                            accent_col,
                        );
                    }
                });

                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());


                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&author_display).size(T_HEAD).color(BONE));
                        if is_verified {
                            ui.label(RichText::new("verified").size(T_META - 1.0).color(BRUISE_LIT));
                        }
                        ui.label(RichText::new(&author_handle).size(T_META).color(TX_FAINT));
                        ui.label(RichText::new(format_relative_time(post.created_at)).size(T_META).color(TX_FAINT));
                        if post.is_pinned {
                            ui.label(RichText::new("pinned").size(T_META - 1.0).color(AMBER));
                        }


                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let post_id = post.id.clone();
                            let author_pub = post.author_pubkey.clone();

                            let mut menu_action: Option<&'static str> = None;
                            ui.menu_button("⋯", |ui| {
                                if is_me {
                                    if quiet(ui, "edit") {
                                        menu_action = Some("edit");
                                        ui.close_menu();
                                    }
                                    if quiet(ui, "delete") {
                                        menu_action = Some("delete");
                                        ui.close_menu();
                                    }
                                } else {
                                    let following = self.following.contains(&author_pub);
                                    if quiet(ui, if following { "unfollow" } else { "follow" }) {
                                        menu_action = Some("follow");
                                        ui.close_menu();
                                    }
                                    let blocked = self.blocked.contains(&author_pub);
                                    if quiet(ui, if blocked { "unblock" } else { "block" }) {
                                        menu_action = Some("block");
                                        ui.close_menu();
                                    }
                                    if quiet(ui, "report") {
                                        menu_action = Some("report");
                                        ui.close_menu();
                                    }
                                }
                            });
                            match menu_action {
                                Some("edit") => {
                                    self.editing_msg_id = Some(post_id.clone());
                                    self.edit_buffer = post.content.clone();
                                }
                                Some("delete") => self.delete_post(&post_id),
                                Some("follow") => self.toggle_follow(&author_pub),
                                Some("block") => self.toggle_block(&author_pub),
                                Some("report") => {
                                    self.active_report_dialog = Some(ReportDraft {
                                        target_type: ReportTargetType::Post,
                                        target_id: post_id.clone(),
                                        target_label: format!("Post by {}", &author_pub[..8.min(author_pub.len())]),
                                        reason: "Inappropriate or abusive content".to_string(),
                                        details: String::new(),
                                    });
                                }
                                _ => {}
                            }
                        });
                    });

                    ui.add_space(4.0);


                    if self.editing_msg_id.as_deref() == Some(&post.id) {
                        let post_id = post.id.clone();
                        text_input(ui, Id::new(("edit_post", &post.id)), &mut self.edit_buffer, "edit post...", ui.available_width());
                        ui.horizontal(|ui| {
                            if primary(ui, "save", !self.edit_buffer.trim().is_empty()) {
                                self.save_post_edit(&post_id);
                            }
                            if secondary(ui, "cancel") {
                                self.editing_msg_id = None;
                                self.edit_buffer.clear();
                            }
                        });
                    } else {
                        ui.add(
                            egui::Label::new(RichText::new(&post.content).size(T_BODY).color(TX))
                                .wrap_mode(egui::TextWrapMode::Wrap),
                        );
                        if post.edited_at.is_some() {
                            ui.label(RichText::new("edited").size(T_META - 1.0).color(TX_FAINT));
                        }
                    }


                    self.render_attachment_list(ui, &post.attachments);

                    ui.add_space(4.0);


                    ui.horizontal(|ui| {
                        let post_id = post.id.clone();

                        let like_label = if post.likes_count > 0 {
                            format!("{}", post.likes_count)
                        } else {
                            "0".to_string()
                        };
                        let like_active = post.liked_by_me;
                        if inline_action(ui, &format!("like {}", like_label), like_active) {
                            self.interact_post(&post_id, InteractionType::Like);
                        }

                        let is_expanded = self.expanded_replies.contains(&post.id);
                        let reply_label = if post.replies_count > 0 {
                            format!("{}", post.replies_count)
                        } else {
                            "0".to_string()
                        };
                        if inline_action(ui, &format!("reply {}", reply_label), is_expanded) {
                            if is_expanded {
                                self.expanded_replies.remove(&post.id);
                            } else {
                                self.expanded_replies.insert(post.id.clone());
                            }
                        }
                    });


                    if self.expanded_replies.contains(&post.id) {
                        ui.add_space(10.0);


                        if !self.cached_replies.contains_key(&post.id) {
                            let mut list = Vec::new();
                            for p in &self.cached_posts {
                                if p.reply_to_id.as_deref() == Some(&post.id) {
                                    list.push(p.clone());
                                }
                            }
                            self.cached_replies.insert(post.id.clone(), list);
                            self.request(Job::Replies(post.id.clone()));
                        }


                        let replies = self.cached_replies.get(&post.id).cloned().unwrap_or_default();
                        for reply in &replies {
                            ui.horizontal_top(|ui| {
                                ui.add_space(30.0);
                                self.render_reply_card(ui, reply, config);
                            });
                            ui.add_space(8.0);
                        }


                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            let draft = self.reply_drafts.entry(post.id.clone()).or_default();
                            let resp = text_input(ui, Id::new(("reply_input", &post.id)), draft, "Write a reply...", ui.available_width() - 80.0);

                            let post_id = post.id.clone();
                            let can_reply = !draft.trim().is_empty();
                            if (inline_action(ui, "send", can_reply) || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && can_reply {
                                let text = draft.trim().to_string();
                                draft.clear();
                                self.submit_reply(&post_id, &text);
                            }
                        });
                    }
                });
            });
        });
    }

    fn render_reply_card(&mut self, ui: &mut Ui, reply: &Post, config: &AppConfig) {
        let my_pubkey = derive_ed25519_pubkey_hex(&config.server_secret);
        let is_me = my_pubkey == reply.author_pubkey;
        let is_verified = (is_me && self.my_roblox_verified) || reply.author_roblox_verified;

        let (author_display, author_handle, res_avatar, res_rid) = if is_me {
            (
                config.effective_display_name().to_string(),
                format!(
                    "@{}",
                    if !config.roblox_name.is_empty() {
                        &config.roblox_name
                    } else {
                        "me"
                    }
                ),
                None,
                None,
            )
        } else {
            self.resolve_peer(
                &reply.author_pubkey,
                &reply.author_display,
                &reply.author_handle,
                &reply.author_avatar_url,
                reply.author_roblox_id,
            )
        };
        let roblox_id = if is_me && config.roblox_id != 0 {
            Some(config.roblox_id)
        } else {
            res_rid
        };
        let reply_avatar_url = if is_me {
            reply.author_avatar_url.clone()
        } else {
            res_avatar
        };

        let mut headshot_tex: Option<egui::TextureHandle> = if is_me && self.local_headshot.is_some() {
            self.local_headshot.clone()
        } else if let Some(rid) = roblox_id {
            if let Some(tex) = self.avatar_cache.get(&rid) {
                Some(tex.clone())
            } else {
                if let Ok(mut pending) = self.pending_avatar_fetches.lock() {
                    if !pending.contains(&rid) {
                        pending.insert(rid);
                        let tx = self.avatar_tx.clone();
                        let pending_arc = Arc::clone(&self.pending_avatar_fetches);
                        std::thread::spawn(move || {
                            if let Ok(bytes) = crate::rbxapi::headshot_png(rid) {
                                let _ = tx.send((rid, bytes));
                            }
                            if let Ok(mut p) = pending_arc.lock() {
                                p.remove(&rid);
                            }
                        });
                    }
                }
                None
            }
        } else {
            None
        };

        if headshot_tex.is_none() {
            if let Some(ref avatar_url) = reply_avatar_url {
                if !avatar_url.is_empty() {
                    if let Some(tex) = self.avatar_url_cache.get(avatar_url) {
                        headshot_tex = Some(tex.clone());
                    } else if let Ok(mut pending) = self.pending_avatar_url_fetches.lock() {
                        if !pending.contains(avatar_url) {
                            pending.insert(avatar_url.clone());
                            let url = avatar_url.clone();
                            let tx = self.avatar_url_tx.clone();
                            let pending_arc = Arc::clone(&self.pending_avatar_url_fetches);
                            std::thread::spawn(move || {
                                if let Ok(resp) = ureq::get(&url).call() {
                                    let mut bytes = Vec::new();
                                    if let Ok(_) = resp.into_reader().read_to_end(&mut bytes) {
                                        let _ = tx.send((url.clone(), bytes));
                                    }
                                }
                                if let Ok(mut p) = pending_arc.lock() {
                                    p.remove(&url);
                                }
                            });
                        }
                    }
                }
            }
        }

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(32.0);
                    if let Some(ref tex) = headshot_tex {
                        ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::splat(28.0)).rounding(14.0));
                    } else {
                        let initial = author_display.chars().next().unwrap_or('?').to_uppercase().to_string();
                        let (dot, _) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::hover());
                        let accent_col = reply.author_accent.as_deref().map(hex_color).unwrap_or(BRUISE_LIT);
                        ui.painter().rect_filled(dot, Rounding::same(14.0), SURF_3);
                        ui.painter().text(
                            dot.center(),
                            egui::Align2::CENTER_CENTER,
                            initial,
                            egui::FontId::proportional(12.0),
                            accent_col,
                        );
                    }
                });

                ui.add_space(6.0);

                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&author_display).size(12.5).strong().color(TX));
                        if is_verified {
                            ui.label(RichText::new("verified").size(T_TINY).color(BRUISE_LIT));
                        }
                        ui.label(RichText::new(&author_handle).size(11.5).color(TX_DIM));
                        ui.label(RichText::new("·").size(11.0).color(TX_DIM));
                        ui.label(RichText::new(format_relative_time(reply.created_at)).size(11.0).color(TX_DIM));
                    });
                    ui.add_space(2.0);
                    if self.editing_msg_id.as_deref() == Some(&reply.id) {
                        let reply_id = reply.id.clone();
                        text_input(ui, Id::new(("edit_reply", &reply.id)), &mut self.edit_buffer, "edit reply...", ui.available_width());
                        ui.horizontal(|ui| {
                            if inline_action(ui, "save", !self.edit_buffer.trim().is_empty()) {
                                self.save_post_edit(&reply_id);
                            }
                            if inline_action(ui, "cancel", false) {
                                self.editing_msg_id = None;
                                self.edit_buffer.clear();
                            }
                        });
                    } else {
                        ui.label(RichText::new(&reply.content).size(12.5).color(TX));
                        if reply.edited_at.is_some() {
                            ui.label(RichText::new("(edited)").size(10.5).color(TX_DIM));
                        }
                    }
                    self.render_attachment_list(ui, &reply.attachments);
                    if is_me {
                        ui.horizontal(|ui| {
                            let reply_id = reply.id.clone();
                            if inline_action(ui, "edit", false) {
                                self.editing_msg_id = Some(reply_id.clone());
                                self.edit_buffer = reply.content.clone();
                            }
                            if inline_action(ui, "delete", false) {
                                self.delete_post(&reply_id);
                            }
                        });
                    }
                });
            });
        });
    }


    pub fn render_public_profile(&mut self, ui: &mut Ui, config: &mut AppConfig, _ctx: &egui::Context) {
        ScrollArea::vertical().id_salt("public_profile_scroll").auto_shrink([false, false]).show(ui, |ui| {
            panel(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(ref tex) = self.local_headshot {
                    ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::splat(38.0)).rounding(19.0));
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new(lc(config.effective_display_name())).size(T_HEAD).color(TX));
                    let bio = if config.profile_bio.trim().is_empty() { "no bio yet".to_string() } else { config.profile_bio.clone() };
                    ui.label(RichText::new(lc(&bio)).size(T_META).color(TX_FAINT));
                });
            });
            group(ui, "identity");
            field(ui, "display name", &mut config.display_name, "how people see you");
            field(ui, "bio", &mut config.profile_bio, "a line about you (200 characters)");
            config.profile_bio.truncate(config.profile_bio.char_indices().nth(200).map_or(config.profile_bio.len(), |(i, _)| i));
            field(ui, "community", &mut config.community_instance, "empty uses your first master");

            group(ui, "accent");
            ui.horizontal(|ui| {
                let current = crate::profile::clean_accent(&config.profile_accent);
                for (name, hex) in crate::profile::ACCENTS {
                    let selected = current == *hex;
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::click());
                    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(*name);
                    let r = if selected { 10.0 } else if resp.hovered() { 9.0 } else { 8.0 };
                    ui.painter().circle_filled(rect.center(), r, hex_color(hex));
                    if resp.clicked() {
                        crate::ui::sound::play_click();
                        config.profile_accent = name.to_string();
                    }
                }
            });

            group(ui, "visibility");
            toggle(ui, &mut config.show_profile, "show my profile on servers i host");
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if primary(ui, "save", true) {
                    crate::config::save_config(config);
                    self.save_public_profile(config);
                }
                if let Some((ref msg, time)) = self.profile_save_status {
                    if time.elapsed() < Duration::from_secs(5) {
                        ui.label(RichText::new(lc(msg)).size(T_META).color(BRUISE_LIT));
                    }
                }
            });
            ui.add_space(24.0);
            });
        });
    }

    fn save_public_profile(&mut self, config: &AppConfig) {
        let Some(ref client) = self.client else { return };
        let prof = crate::community::types::UserProfile {
            pubkey: client.identity_pubkey().unwrap_or_default(),
            handle: config.roblox_name.clone(),
            display_name: config.effective_display_name().to_string(),
            bio: config.profile_bio.clone(),
            accent_color: crate::profile::clean_accent(&config.profile_accent),
            roblox_id: if config.roblox_id != 0 { Some(config.roblox_id) } else { None },
            ..Default::default()
        };
        self.profile_save_status = Some(("saving".to_string(), Instant::now()));
        self.spawn_write(move |client| WriteDone::Profile(client.update_profile(prof)));
    }

    fn render_attachment_list(&mut self, ui: &mut Ui, attachments: &[Attachment]) {
        if attachments.is_empty() {
            return;
        }
        ui.add_space(4.0);
        for att in attachments {
            let is_image = att.media_type.starts_with("image/")
                || att.filename.ends_with(".png")
                || att.filename.ends_with(".jpg")
                || att.filename.ends_with(".jpeg")
                || att.filename.ends_with(".webp")
                || att.filename.ends_with(".gif");
            let is_gif = att.media_type == "image/gif" || att.filename.ends_with(".gif");
            let is_video = att.media_type.starts_with("video/")
                || att.filename.ends_with(".mp4")
                || att.filename.ends_with(".webm")
                || att.filename.ends_with(".mov");
            let is_audio = att.media_type.starts_with("audio/")
                || att.filename.ends_with(".mp3")
                || att.filename.ends_with(".ogg")
                || att.filename.ends_with(".wav");

            if is_image && !is_gif {
                if !self.image_textures.contains_key(&att.id) {
                    if att.url.starts_with("data:") {
                        if let Some(pos) = att.url.find(',') {
                            if let Some(bytes) = b64_decode(&att.url[pos + 1..]) {
                                if let Ok(img) = image::load_from_memory(&bytes) {
                                    let rgba = img.to_rgba8();
                                    let size = [rgba.width() as usize, rgba.height() as usize];
                                    let ci = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                                    let tex = ui.ctx().load_texture(&format!("att_{}", att.id), ci, egui::TextureOptions::LINEAR);
                                    self.image_textures.insert(att.id.clone(), tex);
                                }
                            }
                        }
                    } else if std::path::Path::new(&att.url).is_file() {
                        if let Ok(bytes) = std::fs::read(&att.url) {
                            if let Ok(img) = image::load_from_memory(&bytes) {
                                let rgba = img.to_rgba8();
                                let size = [rgba.width() as usize, rgba.height() as usize];
                                let ci = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                                let tex = ui.ctx().load_texture(&format!("att_{}", att.id), ci, egui::TextureOptions::LINEAR);
                                self.image_textures.insert(att.id.clone(), tex);
                            }
                        }
                    }
                }

                if let Some(tex) = self.image_textures.get(&att.id) {
                    let max_w = (ui.available_width() - 16.0).min(380.0);
                    let sz = tex.size_vec2();
                    let scale = (max_w / sz.x.max(1.0)).min(1.0);
                    let disp_sz = egui::vec2(sz.x * scale, (sz.y * scale).min(260.0));
                    ui.add(egui::Image::new(tex).fit_to_exact_size(disp_sz).rounding(8.0));
                } else {
                    self.render_attachment_file(ui, att);
                }
            } else if is_gif {
                if !self.gif_anims.contains_key(&att.id) {
                    let bytes: Option<Vec<u8>> = if att.url.starts_with("data:") {
                        att.url.find(',').and_then(|pos| b64_decode(&att.url[pos + 1..]))
                    } else {
                        std::fs::read(&att.url).ok()
                    };
                    if let Some(b) = bytes {
                        if let Some(frames) = decode_gif_frames(&b) {
                            let total = frames.last().map(|(_, end)| *end).unwrap_or(100).max(1);
                            self.gif_anims.insert(att.id.clone(), GifAnim {
                                frames,
                                total_ms: total,
                                started: Instant::now(),
                                tex: None,
                            });
                        }
                    }
                }


                let cur: Option<(egui::ColorImage, f32, f32)> = self
                    .gif_anims
                    .get(&att.id)
                    .and_then(|anim| {
                        let t = anim.started.elapsed().as_millis() as u64 % anim.total_ms;
                        let idx = anim.frames.iter().position(|(_, end)| *end > t).unwrap_or(0);
                        anim.frames.get(idx).map(|(img, _)| {
                            let max_w = 380.0f32;
                            let scale = (max_w / img.width().max(1) as f32).min(1.0);
                            (img.clone(), img.width() as f32 * scale, (img.height() as f32 * scale).min(260.0))
                        })
                    });
                if let Some((img, w, h)) = cur {
                    let disp_sz = egui::vec2(w.max(1.0), h.max(1.0));
                    ui.horizontal(|ui| {
                        let handle = self.gif_anims.get_mut(&att.id).unwrap();
                        if handle.tex.is_some() {
                            let mut tex = handle.tex.as_ref().unwrap().clone();
                            tex.set(img, egui::TextureOptions::LINEAR);
                            ui.add(egui::Image::new(&tex).fit_to_exact_size(disp_sz).rounding(8.0));
                        } else {
                            let tex = ui.ctx().load_texture(
                                &format!("att_{}", att.id),
                                img,
                                egui::TextureOptions::LINEAR,
                            );
                            ui.add(egui::Image::new(&tex).fit_to_exact_size(disp_sz).rounding(8.0));
                            handle.tex = Some(tex);
                        }
                        ui.vertical(|ui| {
                            ui.label(RichText::new("GIF").size(11.0).color(BRUISE_LIT).strong());
                            if secondary(ui, "download") {
                                self.download_attachment(att);
                            }
                        });
                    });

                    ui.ctx().request_repaint();
                } else {
                    self.render_attachment_file(ui, att);
                }
            } else if is_video || is_audio {
                block(ui, R_SM, Margin::symmetric(10.0, 6.0), SURF_2, 0.026, |ui| {
                    ui.horizontal(|ui| {
                        let icon = if is_video { "video" } else { "audio" };
                        ui.label(RichText::new(icon).size(11.0).color(BRUISE_LIT).strong());
                        ui.add_space(4.0);
                        ui.label(RichText::new(&att.filename).size(12.0).color(TX));
                        ui.label(muted(format!("{:.1} kb", att.size as f64 / 1024.0)));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if secondary(ui, "download") {
                                self.download_attachment(att);
                            }
                        });
                    });
                });
            } else {
                self.render_attachment_file(ui, att);
            }
        }
    }

    fn render_attachment_file(&mut self, ui: &mut Ui, att: &Attachment) {
        block(ui, R_SM, Margin::symmetric(10.0, 6.0), SURF_2, 0.026, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&att.filename).size(12.0).color(TX));
                ui.label(muted(format!("{:.1} kb", att.size as f64 / 1024.0)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if secondary(ui, "download") {
                        self.download_attachment(att);
                    }
                });
            });
        });
    }

    fn download_attachment(&self, att: &Attachment) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(&att.filename)
            .save_file()
        else {
            return;
        };
        if att.url.starts_with("data:") {
            if let Some(pos) = att.url.find(',') {
                if let Some(bytes) = b64_decode(&att.url[pos + 1..]) {
                    let _ = std::fs::write(&path, bytes);
                }
            }
        } else if std::path::Path::new(&att.url).is_file() {
            let _ = std::fs::copy(&att.url, &path);
        } else if att.url.starts_with("http") {
            if let Ok(resp) = ureq::get(&att.url).call() {
                let mut bytes = Vec::new();
                if let Ok(_) = resp.into_reader().read_to_end(&mut bytes) {
                    let _ = std::fs::write(&path, bytes);
                }
            }
        }
    }


    fn render_messages(&mut self, ui: &mut Ui, config: &AppConfig) {
        let full_h = ui.available_height();
        let my_pubkey = derive_ed25519_pubkey_hex(&config.server_secret);
        let mut new_chat = false;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(Vec2::new(196.0, full_h), Layout::top_down(Align::LEFT), |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("conversations").size(T_META).color(TX_FAINT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if quiet(ui, "new") {
                            new_chat = true;
                        }
                    });
                });
                ui.add_space(8.0);
                ScrollArea::vertical().id_salt("community_convs").auto_shrink([false, false]).show(ui, |ui| {
                    if self.cached_conversations.is_empty() {
                        ui.label(RichText::new("none yet").size(T_META).color(TX_FAINT));
                    }
                    let convs = self.cached_conversations.clone();
                    for conv in convs {
                        let is_active = self.active_conv_id.as_deref() == Some(&conv.id);
                        let title = conv.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| {
                            if conv.is_group {
                                "group".to_string()
                            } else {
                                conv.members
                                    .iter()
                                    .find(|m| Some(m.pubkey.clone()) != self.client.as_ref().and_then(|c| c.identity_pubkey()))
                                    .map(|m| short_handle(&m.handle))
                                    .unwrap_or_else(|| "conversation".to_string())
                            }
                        });
                        let last = conv.last_message.as_ref().map(|m| {
                            if is_encrypted_payload(&m.content) {
                                "encrypted".to_string()
                            } else {
                                m.content.chars().take(42).collect()
                            }
                        }).unwrap_or_default();
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 48.0), Sense::click());
                        if ui.is_rect_visible(rect) {
                            let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
                            if is_active {
                                ui.painter().rect_filled(rect, Rounding::same(R_SM), SURF_2);
                            } else if t > 0.01 {
                                ui.painter().rect_filled(rect, Rounding::same(R_SM), tint(MILK, 0.025 * t));
                            }
                            ui.painter().text(
                                pos2(rect.left() + 10.0, rect.top() + 17.0),
                                egui::Align2::LEFT_CENTER,
                                title,
                                FontId::proportional(T_BODY),
                                if is_active { BONE } else { TX },
                            );
                            if !last.is_empty() {
                                ui.painter().text(
                                    pos2(rect.left() + 10.0, rect.top() + 33.0),
                                    egui::Align2::LEFT_CENTER,
                                    last,
                                    FontId::proportional(T_TINY),
                                    TX_FAINT,
                                );
                            }
                        }
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            crate::ui::sound::play_click();
                            self.active_conv_id = Some(conv.id.clone());
                            self.fetch_dms(&conv.id);
                        }
                    }
                });
            });
            ui.add_space(24.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(full_h);
                if let Some(conv_id) = self.active_conv_id.clone() {
                    let title = self.cached_conversations.iter()
                        .find(|c| c.id == conv_id)
                        .and_then(|c| {
                            c.title.clone().filter(|t| !t.trim().is_empty()).or_else(|| {
                                if c.is_group {
                                    Some("group".to_string())
                                } else {
                                    c.members.iter()
                                        .find(|m| Some(m.pubkey.clone()) != self.client.as_ref().and_then(|x| x.identity_pubkey()))
                                        .map(|m| short_handle(&m.handle))
                                }
                            })
                        })
                        .unwrap_or_else(|| "conversation".to_string());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).size(T_HEAD).color(BONE));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if quiet(ui, "close") {
                                self.active_conv_id = None;
                            }
                        });
                    });
                    ui.add_space(8.0);
                    let messages = self.cached_dms.get(&conv_id).cloned().unwrap_or_default();
                    ScrollArea::vertical().id_salt(("community_dms", &conv_id))
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .max_height((ui.available_height() - 48.0).max(120.0))
                        .show(ui, |ui| {
                            if messages.is_empty() {
                                empty_state(ui, Icon::Lock, "no messages", "");
                            } else {
                                for msg in messages {
                                    let is_mine = msg.sender_pubkey == my_pubkey;
                                    let display_text = if is_encrypted_payload(&msg.content) {
                                        decrypt_direct_message(&config.server_secret, &msg.sender_pubkey, &msg.content)
                                            .unwrap_or_else(|_| "unreadable message".to_string())
                                    } else {
                                        msg.content.clone()
                                    };
                                    ui.horizontal_top(|ui| {
                                        if is_mine {
                                            ui.add_space(24.0);
                                        } else {
                                            let initial = msg.sender_handle.trim_start_matches('@').chars().next().unwrap_or('?').to_uppercase().to_string();
                                            let (dot, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                                            ui.painter().rect_filled(dot, Rounding::same(12.0), SURF_2);
                                            ui.painter().text(dot.center(), egui::Align2::CENTER_CENTER, initial, egui::FontId::proportional(T_TINY), TX_DIM);
                                            ui.add_space(8.0);
                                        }
                                        ui.vertical(|ui| {
                                            ui.set_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                let sender = msg.sender_display.clone().filter(|d| !d.trim().is_empty()).unwrap_or_else(|| short_handle(&msg.sender_handle));
                                                ui.label(RichText::new(sender).size(T_TINY).color(if is_mine { BRUISE_LIT } else { TX_DIM }));
                                                ui.label(RichText::new(format_relative_time(msg.created_at)).size(T_TINY).color(TX_FAINT));
                                                if is_mine {
                                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                        let mut edit = false;
                                                        let mut del = false;
                                                        ui.menu_button("⋯", |ui| {
                                                            if quiet(ui, "edit") {
                                                                edit = true;
                                                                ui.close_menu();
                                                            }
                                                            if quiet(ui, "delete") {
                                                                del = true;
                                                                ui.close_menu();
                                                            }
                                                        });
                                                        if edit {
                                                            self.editing_msg_id = Some(msg.id.clone());
                                                            self.edit_buffer = display_text.clone();
                                                        }
                                                        if del {
                                                            self.delete_dm(&conv_id, &msg.id);
                                                        }
                                                    });
                                                }
                                            });
                                            if self.editing_msg_id.as_deref() == Some(&msg.id) {
                                                text_input(ui, Id::new(("edit_dm", &msg.id)), &mut self.edit_buffer, "edit message", ui.available_width());
                                                ui.horizontal(|ui| {
                                                    if inline_action(ui, "save", !self.edit_buffer.trim().is_empty()) {
                                                        self.save_dm_edit(&conv_id, &msg.id, config);
                                                    }
                                                    if inline_action(ui, "cancel", false) {
                                                        self.editing_msg_id = None;
                                                        self.edit_buffer.clear();
                                                    }
                                                });
                                            } else {
                                                ui.label(RichText::new(&display_text).size(T_BODY).color(TX));
                                            }
                                            self.render_attachment_list(ui, &msg.attachments);
                                        });
                                    });
                                    ui.add_space(14.0);
                                }
                            }
                        });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        text_input(
                            ui,
                            Id::new("dm_composer_field"),
                            &mut self.dm_composer,
                            "message",
                            ui.available_width() - 58.0,
                        );
                        let can = !self.dm_composer.trim().is_empty();
                        if inline_action(ui, "send", can) && can {
                            let text = self.dm_composer.trim().to_string();
                            self.dm_composer.clear();
                            self.send_encrypted_dm(&conv_id, &text, config);
                        }
                    });
                } else {
                    empty_state(ui, Icon::Messages, "select a conversation", "");
                }
            });
        });
        if new_chat {
            self.show_new_dm_modal = true;
        }
    }

    #[allow(dead_code)]
    fn render_guilds(&mut self, ui: &mut Ui, config: &AppConfig) {


        let full_h = ui.available_height();

        if self.active_channel_id.is_none() {
            if let Some(ref gid) = self.active_guild_id {
                if let Some(ch) = self.cached_channels.get(gid).and_then(|v| v.first()).cloned() {
                    self.active_channel_id = Some(ch.id.clone());
                    self.fetch_channel_messages(&ch.id);
                }
            }
        }
        ui.horizontal(|ui| {

            ui.vertical(|ui| {
                ui.set_width(72.0);
                ui.set_min_height(full_h);
                ui.vertical_centered(|ui| {
                    if pill(ui, "+", false, false).on_hover_text("new guild").clicked() {
                        self.show_create_guild = true;
                    }
                });
                ui.add_space(6.0);
                ScrollArea::vertical()
                    .id_salt("community_scroll_4")
                    .auto_shrink([false, false])
                    .max_height((full_h - 70.0).max(120.0))
                    .show(ui, |ui| {
                        let guilds = self.cached_guilds.clone();
                        if guilds.is_empty() {
                            ui.label(muted("none yet"));
                        }
                        for guild in guilds {
                            let active = self.active_guild_id.as_deref() == Some(&guild.id);
                            let initial = guild.name.chars().next().unwrap_or('?').to_uppercase().to_string();
                            ui.vertical_centered(|ui| {
                                if pill(ui, &initial, active, true)
                                    .on_hover_text(&guild.name)
                                    .clicked()
                                {
                                    self.active_guild_id = Some(guild.id.clone());
                                    self.active_channel_id = None;
                                    self.fetch_guild_channels(&guild.id);
                                    self.fetch_guild_members(&guild.id);
                                }
                            });
                            ui.add_space(4.0);
                        }
                    });
            });

            ui.add_space(8.0);


            ui.vertical(|ui| {
                ui.set_width(236.0);
                ui.set_min_height(full_h);
                if let Some(ref guild_id) = self.active_guild_id.clone() {
                    let guild_name = self
                        .cached_guilds
                        .iter()
                        .find(|g| &g.id == guild_id)
                        .map(|g| g.name.clone())
                        .unwrap_or_default();
                    let guild_desc = self
                        .cached_guilds
                        .iter()
                        .find(|g| &g.id == guild_id)
                        .map(|g| g.description.clone())
                        .unwrap_or_default();
                    ui.label(RichText::new(&guild_name).size(16.0).strong().color(TX));
                    if !guild_desc.is_empty() {
                        ui.label(muted(&guild_desc));
                    }
                    ui.add_space(8.0);
                    ScrollArea::vertical()
                        .id_salt("community_scroll_4b")
                        .auto_shrink([false, false])
                        .max_height((full_h - 130.0).max(120.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("channels").size(12.0).strong().color(TX_DIM));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if secondary(ui, "+ new") {
                                        self.show_create_channel = true;
                                    }
                                });
                            });
                            ui.add_space(4.0);
                            let channels = self.cached_channels.get(guild_id).cloned().unwrap_or_default();
                            if channels.is_empty() {
                                ui.label(muted("no channels yet"));
                            }
                            for ch in channels {
                                let active = self.active_channel_id.as_deref() == Some(&ch.id);
                                if pill(ui, &format!("#{}", ch.name), active, false).clicked() {
                                    self.active_channel_id = Some(ch.id.clone());
                                    self.fetch_channel_messages(&ch.id);
                                }
                                ui.add_space(3.0);
                            }
                            ui.add_space(10.0);
                            let members = self.cached_members.get(guild_id).cloned().unwrap_or_default();
                            ui.label(RichText::new(format!("members ({})", members.len())).size(12.0).strong().color(TX_DIM));
                            ui.add_space(4.0);
                            for m in members.iter() {
                                let name = m.nickname.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| short_handle(&m.handle));
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&name).size(13.5).strong().color(TX));
                                    ui.label(RichText::new(short_handle(&m.handle)).size(11.0).color(TX_DIM));
                                });
                                ui.add_space(5.0);
                            }
                        });
                } else {
                    card(ui, None, |ui| {
                        ui.label(muted("pick a guild"));
                    });
                }
            });

            ui.add_space(8.0);


            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(full_h);
                let guild_id = self.active_guild_id.clone();
                let ch_id = self.active_channel_id.clone();
                match (guild_id, ch_id) {
                    (Some(guild_id), Some(ch_id)) => {
                        let my_pubkey = derive_ed25519_pubkey_hex(&config.server_secret);
                        let members = self.cached_members.get(&guild_id).cloned().unwrap_or_default();
                        let is_member = members.iter().any(|m| m.pubkey == my_pubkey);
                        let ch_name = self
                            .cached_channels
                            .get(&guild_id)
                            .and_then(|v| v.iter().find(|c| c.id == ch_id))
                            .map(|c| format!("#{}", c.name))
                            .unwrap_or_else(|| "#channel".to_string());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&ch_name).size(16.0).strong().color(TX));
                            ui.label(muted(format!("{} member{}", members.len(), if members.len() == 1 { "" } else { "s" })));
                        });
                        ui.add_space(6.0);
                        if !is_member {
                            card(ui, None, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(muted("you are previewing this community."));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let gid = guild_id.clone();
                                        if primary(ui, "join", true) {
                                            self.join_selected_guild(&gid);
                                        }
                                    });
                                });
                            });
                            ui.add_space(6.0);
                        }
                        let msgs = self.cached_guild_msgs.get(&ch_id).cloned().unwrap_or_default();
                        ScrollArea::vertical()
                            .id_salt("community_scroll_5")
                            .auto_shrink([false, false])
                            .stick_to_bottom(true)
                            .max_height(ui.available_height() - 90.0)
                            .show(ui, |ui| {
                                if msgs.is_empty() {
                                    empty_state(
                                        ui,
                                        Icon::Messages,
                                        "no messages yet",
                                        "say hi to get the channel going.",
                                    );
                                }
                                for msg in msgs {
                                    let mine = msg.author_pubkey == my_pubkey;
                                    let name = short_handle(&msg.author_handle);
                                    ui.horizontal(|ui| {
                                        if let Some(ref url) = msg.author_avatar_url {
                                            if let Some(tex) = self.avatar_url_cache.get(url) {
                                                ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::splat(34.0)).rounding(17.0));
                                            }
                                        }
                                        ui.vertical(|ui| {
                                            ui.set_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new(&name).size(14.0).strong().color(if mine { BRUISE_LIT } else { TX }));
                                                ui.label(RichText::new(format_relative_time(msg.created_at)).size(11.5).color(TX_DIM));
                                                if msg.edited_at.is_some() {
                                                    ui.label(RichText::new("(edited)").size(11.5).color(TX_DIM));
                                                }
                                                if mine {
                                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                        let mid = msg.id.clone();
                                                        let cid = ch_id.clone();
                                                        if secondary(ui, "delete") {
                                                            self.delete_guild_message(&cid, &mid);
                                                        }
                                                        if secondary(ui, "edit") {
                                                            self.editing_msg_id = Some(mid.clone());
                                                            self.edit_buffer = msg.content.clone();
                                                        }
                                                    });
                                                }
                                            });
                                            if self.editing_msg_id.as_deref() == Some(&msg.id) {
                                                let mid = msg.id.clone();
                                                let cid = ch_id.clone();
                                                text_input(ui, Id::new(("edit_gmsg", &msg.id)), &mut self.edit_buffer, "edit message...", ui.available_width() - 20.0);
                                                ui.horizontal(|ui| {
                                                    if primary(ui, "save", !self.edit_buffer.trim().is_empty()) {
                                                        self.save_guild_message_edit(&cid, &mid);
                                                    }
                                                    if secondary(ui, "cancel") {
                                                        self.editing_msg_id = None;
                                                        self.edit_buffer.clear();
                                                    }
                                                });
                                            } else {
                                                ui.label(RichText::new(&msg.content).size(15.0).color(TX));
                                            }
                                        });
                                    });
                                    self.render_attachment_list(ui, &msg.attachments);
                                    ui.add_space(10.0);
                                }
                            });
                        if is_member {
                            ui.horizontal(|ui| {
                                text_input(
                                    ui,
                                    Id::new("guild_msg_composer"),
                                    &mut self.guild_composer,
                                    "Message channel...",
                                    ui.available_width() - 80.0,
                                );
                                if primary(ui, "Send", !self.guild_composer.trim().is_empty()) {
                                    let text = self.guild_composer.trim().to_string();
                                    self.guild_composer.clear();
                                    self.submit_guild_message(&guild_id, &ch_id, &text);
                                }
                            });
                        }
                    }
                    _ => {
                        card(ui, None, |ui| {
                            empty_state(
                                ui,
                                Icon::Guilds,
                                "select a community",
                                "pick a guild on the left to see its channels.",
                            );
                        });
                    }
                }
            });
        });
    }


    fn render_notifications(&mut self, ui: &mut Ui, _config: &AppConfig) {
        ScrollArea::vertical().id_salt("community_scroll_6").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} unread", self.unread_count())).size(T_META).color(TX_FAINT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if quiet(ui, "mark read") {
                        self.mark_all_notifications_read();
                    }
                });
            });

            ui.add_space(16.0);

            if self.notifications.is_empty() {
                empty_state(ui, Icon::Bell, "all caught up", "");
            } else {
                let notifs = self.notifications.clone();
                for n in notifs {
                    let tag = match n.kind {
                        NotificationKind::Like => "like",
                        NotificationKind::Reply => "reply",
                        NotificationKind::Repost => "repost",
                        NotificationKind::Follow => "follow",
                        NotificationKind::DirectMessage => "dm",
                        NotificationKind::GuildInvite => "invite",
                        NotificationKind::SystemAnnouncement => "system",
                        _ => "notif",
                    };
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 48.0), Sense::hover());
                    ui.painter().text(
                        pos2(rect.left(), rect.top() + 17.0),
                        egui::Align2::LEFT_CENTER,
                        tag,
                        FontId::proportional(T_TINY),
                        BRUISE_LIT,
                    );
                    ui.painter().text(
                        pos2(rect.left() + 58.0, rect.top() + 17.0),
                        egui::Align2::LEFT_CENTER,
                        &n.preview,
                        FontId::proportional(T_BODY),
                        if n.read { TX_DIM } else { TX },
                    );
                    ui.painter().text(
                        pos2(rect.left() + 58.0, rect.top() + 33.0),
                        egui::Align2::LEFT_CENTER,
                        format_relative_time(n.created_at),
                        FontId::proportional(T_TINY),
                        TX_FAINT,
                    );
                    ui.add_space(4.0);
                }
            }
        });
    }


    fn render_search(&mut self, ui: &mut Ui, config: &AppConfig) {
        ScrollArea::vertical().id_salt("community_scroll_7").show(ui, |ui| {
            ui.horizontal(|ui| {
                text_input(
                    ui,
                    Id::new("search_bar_field"),
                    &mut self.search_query,
                    "search people and posts",
                    ui.available_width() - 64.0,
                );
                if inline_action(ui, "search", !self.search_query.trim().is_empty()) {
                    self.execute_search();
                }
            });

            ui.add_space(22.0);
            let users = self.search_results.users.clone();
            if !users.is_empty() {
                ui.label(RichText::new("people").size(T_META).color(TX_FAINT));
                ui.add_space(4.0);
                for u in &users {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&u.display_name).size(T_BODY).color(TX));
                        ui.label(RichText::new(format!("@{}", u.handle)).size(T_META).color(TX_FAINT));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let pk = u.pubkey.clone();
                            if inline_action(ui, if self.blocked.contains(&pk) { "unblock" } else { "block" }, false) {
                                self.toggle_block(&pk);
                            }
                            if inline_action(ui, if self.following.contains(&pk) { "unfollow" } else { "follow" }, self.following.contains(&pk)) {
                                self.toggle_follow(&pk);
                            }
                        });
                    });
                    ui.add_space(8.0);
                }
                ui.add_space(14.0);
            }

            let posts = self.search_results.posts.clone();
            if !posts.is_empty() {
                ui.label(RichText::new("posts").size(T_META).color(TX_FAINT));
                ui.add_space(6.0);
                for p in &posts {
                    self.render_post_card(ui, p, config);
                    ui.add_space(22.0);
                }
            } else if !self.is_searching && users.is_empty() && !self.search_query.trim().is_empty() {
                empty_state(ui, Icon::Search, "nothing found", "");
            }
            if self.is_searching {
                ui.label(RichText::new("searching…").size(T_META).color(TX_FAINT));
            }
        });
    }


    pub fn render_keys_page(&mut self, ui: &mut Ui, config: &mut AppConfig) {
        ScrollArea::vertical().id_salt("community_scroll_8").show(ui, |ui| {
            panel(ui, |ui| {
            let secret = config.server_secret.trim();
            let ed_pub = derive_ed25519_pubkey_hex(secret);
            let x_pub = get_x25519_public_hex(secret);
            group(ui, "identity");
            if copy_row(ui, "signing", &ed_pub, false) {
                copy_text(&ed_pub);
                self.key_status_msg = Some(("signing key copied".into(), true, Instant::now()));
            }
            if copy_row(ui, "encryption", &x_pub, false) {
                copy_text(&x_pub);
                self.key_status_msg = Some(("encryption key copied".into(), true, Instant::now()));
            }

            group(ui, "backup");
            ui.label(RichText::new("keep a copy so you can sign in on another machine").size(T_META).color(TX_FAINT));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if quiet(ui, "export backup") {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name("puppytunnel-identity-backup.json")
                        .save_file()
                    {
                        if let Ok(json) = export_backup_json(config) {
                            if std::fs::write(&path, json).is_ok() {
                                self.key_status_msg = Some((format!("backup exported to {}", path.display()), true, Instant::now()));
                            }
                        }
                    }
                }
                if quiet(ui, if self.reveal_secret { "hide secret" } else { "reveal secret" }) {
                    self.reveal_secret = !self.reveal_secret;
                }
                if quiet(ui, if self.show_mnemonic { "hide words" } else { "recovery phrase" }) {
                    self.show_mnemonic = !self.show_mnemonic;
                }
            });
            if self.reveal_secret {
                ui.add_space(10.0);
                if copy_row(ui, "secret", secret, false) {
                    copy_text(secret);
                    self.key_status_msg = Some(("secret key copied".to_string(), true, Instant::now()));
                }
                ui.label(RichText::new("never share this").size(T_TINY).color(AMBER));
            }
            if self.show_mnemonic {
                ui.add_space(10.0);
                let mnemonic = generate_mnemonic(&config.server_secret);
                ui.label(RichText::new("recovery phrase").size(T_META).color(AMBER));
                ui.add_space(4.0);
                if copy_row(ui, "", &mnemonic, false) {
                    copy_text(&mnemonic);
                    self.key_status_msg = Some(("recovery phrase copied".into(), true, Instant::now()));
                }
            }

            group(ui, "restore");
            ui.label(RichText::new("paste a backup, a secret, or 32 recovery words").size(T_META).color(TX_FAINT));
            ui.add_space(6.0);
            text_input(
                ui,
                Id::new("import_key_buffer"),
                &mut self.import_buffer,
                "backup or secret",
                ui.available_width(),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if primary(ui, "restore", !self.import_buffer.trim().is_empty()) {
                    match import_backup_str(&self.import_buffer, config) {
                        Ok(backup) => {
                            self.import_buffer.clear();
                            self.client = None;
                            self.key_status_msg = Some((format!("restored as {}", backup.username), true, Instant::now()));
                        }
                        Err(e) => {
                            self.key_status_msg = Some((format!("import failed: {}", e), false, Instant::now()));
                        }
                    }
                }
                if quiet(ui, "open file") {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("PuppyTunnel Backup", &["json"])
                        .pick_file()
                    {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            self.import_buffer = content;
                        }
                    }
                }
            });
            if let Some((ref msg, success, t)) = self.key_status_msg {
                if t.elapsed() < Duration::from_secs(4) {
                    ui.add_space(10.0);
                    let color = if success { BRUISE_LIT } else { RED_LIT };
                    ui.label(RichText::new(msg).size(T_META).color(color));
                }
            }
            ui.add_space(24.0);
            });
        });
    }

    fn render_moderation(&mut self, ui: &mut Ui, _config: &AppConfig) {
        ScrollArea::vertical().id_salt("community_scroll_9").show(ui, |ui| {
            let tabs = [
                ("queue", ModSubTab::ModQueue),
                ("bans", ModSubTab::GlobalBans),
                ("audit", ModSubTab::AuditLog),
            ];
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                for (label, tab) in tabs {
                    if inline_action(ui, label, self.mod_tab == tab) {
                        self.mod_tab = tab;
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if quiet(ui, "refresh") {
                        match self.mod_tab {
                            ModSubTab::ModQueue => self.fetch_reports(),
                            ModSubTab::GlobalBans => {}
                            ModSubTab::AuditLog => self.fetch_audit_logs(),
                        }
                    }
                });
            });
            ui.add_space(18.0);
            match self.mod_tab {
                ModSubTab::ModQueue => self.render_mod_queue(ui),
                ModSubTab::GlobalBans => self.render_global_bans(ui),
                ModSubTab::AuditLog => self.render_audit_log(ui),
            }
        });
    }

    fn render_mod_queue(&mut self, ui: &mut Ui) {
        let reports: Vec<Report> = self.reports.iter().filter(|r| r.status == ReportStatus::Pending).cloned().collect();
        if reports.is_empty() {
            empty_state(ui, Icon::Gavel, "queue is clear", "");
            return;
        }
        for r in reports {
            ui.horizontal(|ui| {
                ui.label(RichText::new(lc(&format!("{:?}", r.target_type))).size(T_META).color(RED_LIT));
                ui.label(RichText::new(&r.reason).size(T_BODY).color(TX));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let rep_id = r.id.clone();
                    let target_id = r.target_id.clone();
                    if quiet(ui, "dismiss") {
                        self.resolve_report(&rep_id, "dismissed");
                    }
                    if quiet(ui, "ban") {
                        self.execute_global_ban(&target_id, "banned via report");
                        self.resolve_report(&rep_id, "resolved");
                    }
                });
            });
            ui.label(RichText::new(format!("target {}", &r.target_id[..r.target_id.len().min(16)])).size(T_TINY).color(TX_FAINT));
            if let Some(ref d) = r.details {
                ui.label(RichText::new(d).size(T_META).color(TX_DIM));
            }
            ui.add_space(16.0);
        }
    }

    fn render_global_bans(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("ban a public key from posting, messaging and joining").size(T_META).color(TX_FAINT));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            text_input(
                ui,
                Id::new("ban_target_field"),
                &mut self.ban_target_input,
                "public key or handle",
                (ui.available_width() * 0.45).max(160.0),
            );
            text_input(
                ui,
                Id::new("ban_reason_field"),
                &mut self.ban_reason_input,
                "reason",
                (ui.available_width() - 70.0).max(160.0),
            );
            if inline_action(ui, "ban", !self.ban_target_input.trim().is_empty()) {
                let target = self.ban_target_input.trim().to_string();
                let reason = if !self.ban_reason_input.trim().is_empty() {
                    self.ban_reason_input.trim().to_string()
                } else {
                    "violation of community guidelines".to_string()
                };
                self.execute_global_ban(&target, &reason);
                self.ban_target_input.clear();
                self.ban_reason_input.clear();
            }
        });
        if let Some((ref msg, t)) = self.mod_status_msg {
            if t.elapsed() < Duration::from_secs(4) {
                ui.add_space(10.0);
                ui.label(RichText::new(msg).size(T_META).color(BRUISE_LIT));
            }
        }
    }

    fn render_audit_log(&mut self, ui: &mut Ui) {
        if self.audit_logs.is_empty() {
            ui.label(RichText::new("no entries").size(T_META).color(TX_FAINT));
            return;
        }
        for entry in self.audit_logs.clone() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&entry.action).size(T_BODY).color(BRUISE_LIT));
                ui.label(RichText::new(format!("pup_{}", &entry.actor_pubkey[..entry.actor_pubkey.len().min(8)])).size(T_META).color(TX_FAINT));
                ui.label(RichText::new(&entry.target_id).size(T_META).color(TX_DIM));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(format_relative_time(entry.created_at)).size(T_TINY).color(TX_FAINT));
                });
            });
            ui.add_space(10.0);
        }
    }

    fn render_active_modals(&mut self, _ui: &mut Ui, _config: &AppConfig, ctx: &egui::Context) {
        if let Some(mut draft) = self.active_report_dialog.take() {
            let mut submit = false;

            let close = modal(ctx, Id::new("report_modal"), "report", 110.0, |ui| {
                let mut cancel = false;
                ui.horizontal_wrapped(|ui| {
                    for cat in ["spam", "harassment", "phishing", "inappropriate"] {
                        if inline_action(ui, cat, draft.reason == cat) {
                            draft.reason = cat.to_string();
                        }
                    }
                });
                ui.add_space(10.0);
                text_input(
                    ui,
                    Id::new("report_details_field"),
                    &mut draft.details,
                    "details (optional)",
                    ui.available_width(),
                );
                ui.add_space(16.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if primary(ui, "submit", !draft.reason.is_empty()) {
                        submit = true;
                    }
                    ui.add_space(6.0);
                    if secondary(ui, "cancel") {
                        cancel = true;
                    }
                });
                submit || cancel
            });

            if submit {
                self.submit_report(&draft);
            } else if !close {
                self.active_report_dialog = Some(draft);
            }
        }


        if self.show_new_dm_modal {
            let mut start = false;

            let close = modal(ctx, Id::new("new_dm_modal"), "new message", 80.0, |ui| {
                let mut cancel = false;
                let input_resp = text_input(
                    ui,
                    Id::new("new_dm_field"),
                    &mut self.new_dm_target,
                    "pubkey or @handle",
                    ui.available_width(),
                );

                if input_resp.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) && !self.new_dm_target.trim().is_empty() {
                    start = true;
                }

                let mut validation_error = String::new();
                let target_trimmed = self.new_dm_target.trim();
                if !target_trimmed.is_empty() {
                    let is_pubkey = target_trimmed.len() == 64 && target_trimmed.chars().all(|c| c.is_ascii_hexdigit());
                    let is_handle = target_trimmed.starts_with('@') && target_trimmed.len() > 1;
                    if !is_pubkey && !is_handle {
                        validation_error = "invalid pubkey (64 hex chars) or @handle".to_string();
                    }
                }

                if !validation_error.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(&validation_error).size(T_META).color(RED_LIT));
                }
                if !self.new_dm_error.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(lc(&self.new_dm_error)).size(T_META).color(RED_LIT));
                }

                ui.add_space(16.0);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let can_submit = !self.new_dm_target.trim().is_empty() && validation_error.is_empty();
                    if primary(ui, "start", can_submit) {
                        start = true;
                    }
                    ui.add_space(6.0);
                    if secondary(ui, "cancel") {
                        cancel = true;
                    }
                });
                start || cancel
            });

            if start {
                let target = self.new_dm_target.trim().to_string();
                let is_hex = target.len() == 64 && target.chars().all(|c| c.is_ascii_hexdigit());
                let lookup = if is_hex {
                    target.clone()
                } else {
                    let norm = target.trim_start_matches('@');
                    norm.split('@').next().unwrap_or(norm).to_lowercase()
                };
                self.new_dm_error.clear();
                self.pending_dm_lookup = Some(lookup.clone());
                self.request(Job::User(lookup));
            } else if close {
                self.show_new_dm_modal = false;
            }
        }


        if self.show_create_guild {
            let mut create = false;

            let close = modal(ctx, Id::new("new_guild_modal"), "new guild", 100.0, |ui| {
                let mut cancel = false;
                text_input(
                    ui,
                    Id::new("new_guild_name_field"),
                    &mut self.new_guild_name,
                    "guild name",
                    ui.available_width(),
                );

                ui.add_space(8.0);

                text_input(
                    ui,
                    Id::new("new_guild_desc_field"),
                    &mut self.new_guild_desc,
                    "description (optional)",
                    ui.available_width(),
                );

                ui.add_space(16.0);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let can_submit = !self.new_guild_name.trim().is_empty();
                    if primary(ui, "create", can_submit) {
                        create = true;
                    }
                    ui.add_space(6.0);
                    if secondary(ui, "cancel") {
                        cancel = true;
                    }
                });
                create || cancel
            });

            if create {
                let name = self.new_guild_name.trim().to_string();
                let desc = self.new_guild_desc.trim().to_string();
                self.new_guild_name.clear();
                self.new_guild_desc.clear();
                self.show_create_guild = false;
                if let Some(ref client) = self.client {
                    if let Ok(guild) = client.create_guild(&name, &desc, None) {
                        self.cached_guilds.push(guild.clone());
                        self.active_guild_id = Some(guild.id);
                    }
                }
            } else if close {
                self.show_create_guild = false;
            }
        }


        if self.show_create_channel {
            let mut create = false;

            let close = modal(ctx, Id::new("new_channel_modal"), "new channel", 80.0, |ui| {
                let mut cancel = false;
                text_input(
                    ui,
                    Id::new("new_channel_name_field"),
                    &mut self.new_channel_name,
                    "channel name",
                    ui.available_width(),
                );

                ui.add_space(16.0);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let can_submit = !self.new_channel_name.trim().is_empty();
                    if primary(ui, "create", can_submit) {
                        create = true;
                    }
                    ui.add_space(6.0);
                    if secondary(ui, "cancel") {
                        cancel = true;
                    }
                });
                create || cancel
            });

            if create {
                let name = self.new_channel_name.trim().to_string();
                self.new_channel_name.clear();
                self.show_create_channel = false;
                if let Some(ref guild_id) = self.active_guild_id {
                    let ch = GuildChannel {
                        id: uuid::Uuid::new_v4().to_string(),
                        guild_id: guild_id.clone(),
                        name,
                        topic: String::new(),
                        kind: crate::community::types::ChannelKind::Text,
                        position: 0,
                        permission_overwrites: HashMap::new(),
                    };
                    self.cached_channels.entry(guild_id.clone()).or_default().push(ch.clone());
                    self.active_channel_id = Some(ch.id);
                }
            } else if close {
                self.show_create_channel = false;
            }
        }
    }


    pub fn poll_updates(&mut self, config: &AppConfig) {
        if self.active_tab == CommunityTab::Feed && !self.feed_loaded {
            self.request(Job::Feed(30));
        }
        let lists_due = self
            .lists_checked_at
            .map(|t| t.elapsed() > Duration::from_secs(30))
            .unwrap_or(true);
        if lists_due {
            self.lists_checked_at = Some(Instant::now());
            self.request(Job::Social);
            self.request(Job::Notifications);
            match self.active_tab {
                CommunityTab::Messages => self.request(Job::Conversations),
                CommunityTab::Moderation => {
                    self.request(Job::Reports);
                    self.request(Job::Audit);
                }
                _ => {}
            }
        }

        let due = self
            .verified_check_at
            .map(|t| t.elapsed() > Duration::from_secs(60))
            .unwrap_or(true);
        if due {
            self.verified_check_at = Some(Instant::now());
            self.publish_profile(config);
            self.request(Job::SelfProfile);
        }
    }

    pub fn refresh_feed(&mut self) {
        self.request(Job::Feed(30));
    }

    pub fn submit_post(&mut self, _config: &AppConfig) {
        let content = self.composer_text.trim().to_string();
        let tags = if !self.composer_tag.is_empty() {
            vec![self.composer_tag.clone()]
        } else {
            vec![]
        };

        let mut attachments = Vec::new();
        if let Some(ref path) = self.composer_attachment {
            if let Ok(bytes) = std::fs::read(path) {
                let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                let ext = path.extension().unwrap_or_default().to_string_lossy().to_lowercase();
                let mime = match ext.as_str() {
                    "png" => "image/png",
                    "jpg" | "jpeg" => "image/jpeg",
                    "gif" => "image/gif",
                    "webp" => "image/webp",
                    "bmp" => "image/bmp",
                    "avif" => "image/avif",
                    "mp4" => "video/mp4",
                    "webm" => "video/webm",
                    "mov" => "video/quicktime",
                    "mkv" => "video/x-matroska",
                    "mp3" => "audio/mpeg",
                    "ogg" | "oga" => "audio/ogg",
                    "wav" => "audio/wav",
                    "m4a" => "audio/mp4",
                    "flac" => "audio/flac",
                    _ => "application/octet-stream",
                };
                let b64 = b64_encode(&bytes);
                let data_url = format!("data:{};base64,{}", mime, b64);
                let att_id = uuid::Uuid::new_v4().to_string();
                attachments.push(Attachment {
                    id: att_id,
                    filename,
                    media_type: mime.to_string(),
                    url: data_url,
                    hash: String::new(),
                    size: bytes.len() as u64,
                    width: None,
                    height: None,
                });
            }
        }

        if self.client.is_some() {
            self.composer_text.clear();
            self.composer_attachment = None;
            self.spawn_write(move |client| {
                WriteDone::Post(client.create_post(&content, attachments, None, None, tags, "public"))
            });
        }
    }

    pub fn delete_post(&mut self, post_id: &str) {
        let id = post_id.to_string();
        self.spawn_write(move |client| WriteDone::DeletedPost {
            id: id.clone(),
            result: client.delete_post(&id),
        });
    }

    pub fn save_post_edit(&mut self, post_id: &str) {
        let text = self.edit_buffer.trim().to_string();
        if text.is_empty() {
            return;
        }
        let id = post_id.to_string();
        self.spawn_write(move |client| WriteDone::EditedPost {
            result: client.edit_post(&id, &text),
        });
    }

    pub fn delete_dm(&mut self, conv_id: &str, msg_id: &str) {
        let conv = conv_id.to_string();
        let id = msg_id.to_string();
        self.spawn_write(move |client| WriteDone::DeletedDm {
            result: client.delete_dm(&conv, &id),
            conv,
            id,
        });
    }

    pub fn save_dm_edit(&mut self, conv_id: &str, msg_id: &str, config: &AppConfig) {
        let text = self.edit_buffer.trim().to_string();
        if text.is_empty() {
            return;
        }
        let recipient_x25519 = get_x25519_public_hex(&config.server_secret);
        let stored = crate::community::e2ee::encrypt_direct_message(
            &config.server_secret,
            &recipient_x25519,
            &text,
        )
        .unwrap_or(text);
        let conv = conv_id.to_string();
        let id = msg_id.to_string();
        self.spawn_write(move |client| WriteDone::EditedDm {
            result: client.edit_dm(&conv, &id, &stored),
            conv,
            id,
        });
    }

    pub fn delete_guild_message(&mut self, channel_id: &str, msg_id: &str) {
        if let Some(ref client) = self.client {
            if let Ok(updated) = client.delete_guild_message(channel_id, msg_id) {
                if let Some(list) = self.cached_guild_msgs.get_mut(channel_id) {
                    if let Some(m) = list.iter_mut().find(|m| m.id == msg_id) {
                        *m = updated;
                    }
                }
            }
        }
    }

    pub fn save_guild_message_edit(&mut self, channel_id: &str, msg_id: &str) {
        let text = self.edit_buffer.trim().to_string();
        if text.is_empty() {
            return;
        }
        if let Some(ref client) = self.client {
            if let Ok(updated) = client.edit_guild_message(channel_id, msg_id, &text) {
                if let Some(list) = self.cached_guild_msgs.get_mut(channel_id) {
                    if let Some(m) = list.iter_mut().find(|m| m.id == msg_id) {
                        *m = updated;
                    }
                }
                self.editing_msg_id = None;
                self.edit_buffer.clear();
            }
        }
    }

    pub fn interact_post(&mut self, post_id: &str, kind: InteractionType) {
        if kind == InteractionType::Like {
            if let Some(p) = self.cached_posts.iter_mut().find(|p| p.id == post_id) {
                p.liked_by_me = !p.liked_by_me;
                p.likes_count = if p.liked_by_me {
                    p.likes_count.saturating_add(1)
                } else {
                    p.likes_count.saturating_sub(1)
                };
            }
        }
        let id = post_id.to_string();
        self.spawn_write(move |client| WriteDone::Like {
            post_id: id.clone(),
            result: client.interact_post(&id, kind, None).map(|i| (i.liked, i.likes_count.map(|c| c as i32))),
        });
    }

    pub fn submit_reply(&mut self, parent_id: &str, text: &str) {
        let parent = parent_id.to_string();
        let content = text.to_string();
        self.spawn_write(move |client| WriteDone::Reply {
            parent: parent.clone(),
            result: client.create_post(&content, vec![], Some(parent), None, vec![], "public"),
        });
    }

    pub fn fetch_dms(&mut self, conv_id: &str) {
        self.request(Job::Messages(conv_id.to_string()));
    }

    pub fn send_encrypted_dm(&mut self, conv_id: &str, plaintext: &str, config: &AppConfig) {
        let recipient_x25519 = get_x25519_public_hex(&config.server_secret);
        let secret = config.server_secret.clone();
        let conv = conv_id.to_string();
        let text = plaintext.to_string();
        self.spawn_write(move |client| WriteDone::Dm {
            conv: conv.clone(),
            result: client.send_e2ee_direct_message(&conv, &recipient_x25519, &text, &secret),
        });
    }

    pub fn create_dm_conversation(&mut self, target_pubkey: &str, _config: &AppConfig) {
        let target = target_pubkey.to_string();
        self.spawn_write(move |client| WriteDone::Conversation(client.create_conversation(vec![target], None)));
    }

    pub fn fetch_guild_channels(&mut self, guild_id: &str) {
        self.request(Job::GuildChannels(guild_id.to_string()));
    }

    pub fn fetch_guild_members(&mut self, guild_id: &str) {
        self.request(Job::GuildMembers(guild_id.to_string()));
    }

    pub fn join_selected_guild(&mut self, guild_id: &str) {
        if let Some(ref client) = self.client {
            if client.join_guild(guild_id).is_ok() {
                let id = guild_id.to_string();
                self.fetch_guild_members(&id);
                self.fetch_guild_channels(&id);
            }
        }
    }

    pub fn fetch_channel_messages(&mut self, channel_id: &str) {
        self.request(Job::GuildMessages(channel_id.to_string()));
    }

    pub fn submit_guild_message(&mut self, guild_id: &str, channel_id: &str, text: &str) {
        if let Some(ref client) = self.client {
            if let Ok(msg) = client.send_guild_message(guild_id, channel_id, text, vec![], None) {
                self.cached_guild_msgs.entry(channel_id.to_string()).or_default().push(msg);
            }
        }
    }

    pub fn mark_all_notifications_read(&mut self) {
        for n in self.notifications.iter_mut() {
            n.read = true;
        }
        self.spawn_write(|client| WriteDone::Read(client.mark_notifications_read(&[], true).map(|_| 0)));
    }

    pub fn execute_search(&mut self) {
        let q = self.search_query.trim().to_string();
        if q.is_empty() {
            return;
        }
        self.is_searching = true;
        self.spawn_write(move |client| WriteDone::Search(client.search(&q)));
    }

    pub fn follow_user(&mut self, pubkey: &str) {
        let pk = pubkey.to_string();
        self.following.insert(pk.clone());
        self.spawn_write(move |client| WriteDone::Follow {
            result: client.follow_user(&pk).map(|_| ()),
            pk,
            on: true,
        });
    }

    pub fn fetch_social(&mut self) {
        self.request(Job::Social);
    }


    pub fn ensure_peer_profile(&mut self, pubkey: &str) {
        if pubkey.is_empty() || self.peer_profiles.contains_key(pubkey) {
            return;
        }
        let already = self
            .pending_peer_profiles
            .lock()
            .map(|mut p| {
                if p.contains(pubkey) {
                    true
                } else {
                    p.insert(pubkey.to_string());
                    false
                }
            })
            .unwrap_or(true);
        if already {
            return;
        }
        if let Some(ref client) = self.client {
            let c = client.clone();
            let pk = pubkey.to_string();
            let tx = self.peer_profile_tx.clone();
            let pending = Arc::clone(&self.pending_peer_profiles);
            std::thread::spawn(move || {
                if let Ok(prof) = c.get_profile(&pk) {
                    let _ = tx.send((pk.clone(), prof));
                }
                if let Ok(mut p) = pending.lock() {
                    p.remove(&pk);
                }
            });
        }
    }


    fn resolve_peer(
        &mut self,
        author_pubkey: &str,
        post_display: &Option<String>,
        post_handle: &str,
        post_avatar_url: &Option<String>,
        post_roblox_id: Option<u64>,
    ) -> (String, String, Option<String>, Option<u64>) {
        self.ensure_peer_profile(author_pubkey);
        let prof = self.peer_profiles.get(author_pubkey);
        let display = post_display
            .clone()
            .filter(|d| !d.trim().is_empty())
            .or_else(|| prof.map(|p| p.display_name.clone()).filter(|d| !d.trim().is_empty()))
            .unwrap_or_else(|| post_handle.to_string());
        let handle = short_handle(
            &prof
                .map(|p| p.handle.clone())
                .filter(|h| !h.trim().is_empty())
                .unwrap_or_else(|| post_handle.to_string()),
        );
        let avatar_url = post_avatar_url
            .clone()
            .or_else(|| prof.and_then(|p| p.avatar_url.clone()));
        let roblox_id = post_roblox_id.or_else(|| prof.and_then(|p| p.roblox_id));
        (display, handle, avatar_url, roblox_id)
    }

    pub fn toggle_follow(&mut self, pubkey: &str) {
        let pk = pubkey.to_string();
        let on = !self.following.contains(&pk);
        if on {
            self.following.insert(pk.clone());
        } else {
            self.following.remove(&pk);
        }
        self.spawn_write(move |client| WriteDone::Follow {
            result: if on { client.follow_user(&pk).map(|_| ()) } else { client.unfollow_user(&pk).map(|_| ()) },
            pk,
            on,
        });
    }

    pub fn toggle_block(&mut self, pubkey: &str) {
        let pk = pubkey.to_string();
        let now_blocked = !self.blocked.contains(&pk);
        if now_blocked {
            self.blocked.insert(pk.clone());
            self.following.remove(&pk);
        } else {
            self.blocked.remove(&pk);
        }
        self.apply_block_filter();
        self.spawn_write(move |client| WriteDone::Block {
            result: client.block_user(&pk, now_blocked).map(|_| ()),
            pk,
            on: now_blocked,
        });
    }

    fn apply_block_filter(&mut self) {
        let blocked = &self.blocked;
        self.cached_posts.retain(|p| !blocked.contains(&p.author_pubkey));
        for list in self.cached_replies.values_mut() {
            list.retain(|p| !blocked.contains(&p.author_pubkey));
        }
        self.search_results.users.retain(|u| !blocked.contains(&u.pubkey));
        self.search_results.posts.retain(|p| !blocked.contains(&p.author_pubkey));
    }

    pub fn fetch_reports(&mut self) {
        self.request(Job::Reports);
    }

    pub fn fetch_audit_logs(&mut self) {
        self.request(Job::Audit);
    }

    pub fn submit_report(&mut self, draft: &ReportDraft) {
        let target_type = draft.target_type;
        let target_id = draft.target_id.clone();
        let reason = draft.reason.clone();
        let details = draft.details.clone();
        self.spawn_write(move |client| WriteDone::Report(
            client.report(target_type, &target_id, &reason, Some(&details)).map(|_| ()),
        ));
    }

    pub fn resolve_report(&mut self, report_id: &str, status: &str) {
        self.reports.retain(|r| r.id != report_id);
        self.mod_status_msg = Some((format!("Report marked as {}.", status), Instant::now()));
    }

    pub fn execute_global_ban(&mut self, target: &str, reason: &str) {
        let target = target.to_string();
        let reason = reason.to_string();
        self.spawn_write(move |client| WriteDone::Moderation(
            client
                .moderate(&target, crate::community::types::ModActionType::Ban, &reason, None, None)
                .map(|_| ()),
        ));
    }
}


fn format_relative_time(ts: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let diff = now.saturating_sub(ts);
    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}


#[cfg(test)]
mod gif_tests {
    use super::decode_gif_frames;


    const TINY_GIF: &[u8] = &[
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00,
        0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 0x04, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x2C, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02,
        0x44, 0x01, 0x00, 0x3B,
    ];

    #[test]
    fn test_gif_decodes_to_frames() {
        let frames = decode_gif_frames(TINY_GIF).expect("gif must decode");
        assert!(!frames.is_empty());
        let (img, end_ms) = &frames[0];
        assert!(img.width() >= 1 && img.height() >= 1);
        assert!(*end_ms >= 20, "frame must carry a positive timestamp");
    }

    #[test]
    fn test_gif_rejects_garbage() {
        assert!(decode_gif_frames(b"definitely not an image").is_none());
        assert!(decode_gif_frames(&[]).is_none());
    }
}
