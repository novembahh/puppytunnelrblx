

use crate::community::client::CommunityClient;
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

use crate::ui::theme::*;
use crate::ui::widgets::*;
use eframe::egui::{
    self, Align, Color32, Frame, Id, Layout, Margin, Pos2, Rect, RichText, Rounding,
    ScrollArea, Sense, Stroke, TextEdit, Ui, UiBuilder, Vec2,
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
    Guilds,
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
    pub local_avatar: Option<egui::TextureHandle>,
    pub avatar_cache: HashMap<u64, egui::TextureHandle>,
    pub avatar_url_cache: HashMap<String, egui::TextureHandle>,
    pub pending_avatar_fetches: Arc<Mutex<HashSet<u64>>>,
    pub pending_avatar_url_fetches: Arc<Mutex<HashSet<String>>>,
    pub avatar_rx: Option<std::sync::mpsc::Receiver<(u64, Vec<u8>)>>,
    pub avatar_tx: std::sync::mpsc::Sender<(u64, Vec<u8>)>,
    pub avatar_url_rx: Option<std::sync::mpsc::Receiver<(String, Vec<u8>)>>,
    pub avatar_url_tx: std::sync::mpsc::Sender<(String, Vec<u8>)>,
    pub request_fetch_roblox: Option<String>,
    pub is_roblox_loading: bool,
    pub roblox_error: String,
    pub profile_save_status: Option<(String, Instant)>,
    pub roblox_query_input: String,


    pub client: Option<CommunityClient>,
    client_base: String,
    pending_open: bool,
    pub last_sync: Instant,
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
            local_avatar: None,
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
            request_fetch_roblox: None,
            is_roblox_loading: false,
            roblox_error: String::new(),
            profile_save_status: None,
            roblox_query_input: String::new(),

            client: None,
            client_base: String::new(),
            pending_open: false,
            last_sync: Instant::now() - Duration::from_secs(60),
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
            self.publish_profile(config);
            self.feed_loaded = false;
            self.pending_open = true;
        }
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
        if let Some(ref client) = self.client {
            let _ = client.update_profile(prof);
        }
    }

    pub fn open(&mut self) {
        self.pending_open = true;
    }

    pub fn select_tab(&mut self, tab: CommunityTab) {
        self.active_tab = tab;
        match tab {
            CommunityTab::Feed if !self.feed_loaded => self.refresh_feed(),
            CommunityTab::Messages => {
                self.fetch_social();
                self.fetch_conversations();
            }
            CommunityTab::Guilds => {
                self.fetch_social();
                self.fetch_guilds();
            }
            CommunityTab::Search => self.fetch_social(),
            CommunityTab::Notifications => self.fetch_notifications(),
            CommunityTab::Moderation => {
                self.fetch_reports();
                self.fetch_audit_logs();
            }
            _ => {}
        }
    }

    pub fn fetch_conversations(&mut self) {
        if let Some(ref client) = self.client {
            if let Ok(convs) = client.get_conversations() {
                let my_pk = client.identity_pubkey().unwrap_or_default();
                let blocked = self.blocked.clone();
                self.cached_conversations = convs
                    .into_iter()
                    .filter(|c| {
                        c.is_group
                            || c.members
                                .iter()
                                .all(|m| m.pubkey == my_pk || !blocked.contains(&m.pubkey))
                    })
                    .collect();
                if self.active_conv_id.is_none() {
                    if let Some(first) = self.cached_conversations.first() {
                        let id = first.id.clone();
                        self.active_conv_id = Some(id.clone());
                        self.fetch_dms(&id);
                    }
                }
            }
        }
    }

    pub fn fetch_guilds(&mut self) {
        if let Some(ref client) = self.client {
            if let Ok(guilds) = client.list_guilds() {
                self.cached_guilds = guilds;
                if self.active_guild_id.is_none() {
                    if let Some(first) = self.cached_guilds.first() {
                        let id = first.id.clone();
                        self.active_guild_id = Some(id.clone());
                        self.fetch_guild_channels(&id);
                    }
                }
            }
        }
    }

    pub fn fetch_notifications(&mut self) {
        if let Some(ref client) = self.client {
            if let Ok(mut notifs) = client.get_notifications() {
                notifs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                self.notifications = notifs;
            }
        }
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
        self.ensure_client(config);


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


        if self.last_sync.elapsed() > Duration::from_secs(3) {
            self.poll_updates(config);
            self.last_sync = Instant::now();
        }

        ui.vertical(|ui| {
            match self.active_tab {
                CommunityTab::Feed => self.render_feed(ui, config),
                CommunityTab::Messages => self.render_messages(ui, config),

                CommunityTab::Guilds => {
                    card(ui, None, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(40.0);
                            ui.label(RichText::new("Guilds are disabled for now.").size(15.0).color(INK));
                            ui.label(muted("They will return in a future update."));
                            ui.add_space(40.0);
                        });
                    });
                }
                CommunityTab::Notifications => self.render_notifications(ui, config),
                CommunityTab::Search => self.render_search(ui, config),
                CommunityTab::Moderation => self.render_moderation(ui, config),
            }

            self.render_active_modals(ui, config, ctx);
        });
    }


    fn render_feed(&mut self, ui: &mut Ui, config: &AppConfig) {
        ScrollArea::vertical().id_salt("community_scroll_1")
            .auto_shrink([false, false])
            .show(ui, |ui| {


                card(ui, Some("New post"), |ui| {
                    ui.add_space(2.0);


                    let focused = ui.memory(|m| m.has_focus(Id::new("feed_composer")));
                    Frame::none()
                        .fill(if focused { FIELD_HI } else { FIELD })
                        .stroke(Stroke::new(1.0_f32, if focused { MINT_DIM } else { LINE }))
                        .rounding(RADIUS_SM)
                        .inner_margin(Margin::symmetric(13.0, 10.0))
                        .show(ui, |ui| {
                            ui.add(
                                TextEdit::multiline(&mut self.composer_text)
                                    .id(Id::new("feed_composer"))
                                    .frame(false)
                                    .desired_rows(3)
                                    .hint_text(RichText::new("Share an update with the community...").color(DIM))
                                    .desired_width(ui.available_width()),
                            );
                        });

                    ui.add_space(8.0);


                    ui.horizontal(|ui| {
                        if secondary(ui, "Attach") {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"])
                                .pick_file()
                            {
                                self.composer_attachment = Some(path);
                            }
                        }

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let len = self.composer_text.len();
                            let can_post = !self.composer_text.trim().is_empty() && len <= 500;

                            if primary(ui, "Post", can_post) {
                                self.submit_post(config);
                            }

                            let char_color = if len > 500 { ERR } else { DIM };
                            ui.label(RichText::new(format!("{}/500", len)).size(12.0).color(char_color));
                        });
                    });


                    let mut remove_attachment = false;
                    if let Some(ref path) = self.composer_attachment {
                        ui.add_space(8.0);
                        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        ui.horizontal(|ui| {
                            egui::Frame::none()
                                .fill(FIELD_HI)
                                .rounding(Rounding::same(PILL))
                                .inner_margin(Margin::symmetric(12.0, 6.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(muted("file:"));
                                        ui.label(RichText::new(name).size(12.5).color(INK));
                                        if secondary(ui, "remove") {
                                            remove_attachment = true;
                                        }
                                    });
                                });
                        });
                    }
                    if remove_attachment {
                        self.composer_attachment = None;
                    }
                });

                ui.add_space(14.0);


                ui.horizontal(|ui| {
                    let filters = [
                        ("All Posts", FeedFilter::All),
                        ("Following", FeedFilter::Following),
                        ("Trending", FeedFilter::Trending),
                    ];
                    for (label, filter) in filters {
                        let active = self.feed_filter == filter;
                        if pill(ui, label, active, false).clicked() {
                            self.feed_filter = filter;
                            self.refresh_feed();
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if secondary(ui, "Refresh") {
                            self.refresh_feed();
                        }
                    });
                });

                ui.add_space(10.0);


                if self.cached_posts.is_empty() {
                    card(ui, None, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(20.0);
                            ui.label(RichText::new("No posts in this feed yet.").size(15.0).color(INK));
                            ui.label(muted("Be the first to share an update, showcase a map, or chat with fellow builders!"));
                            ui.add_space(20.0);
                        });
                    });
                } else {
                    let posts = self.cached_posts.clone();
                    for post in posts {
                        self.render_post_card(ui, &post, config);
                        ui.add_space(10.0);
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

        card(ui, None, |ui| {
            ui.horizontal(|ui| {

                ui.vertical(|ui| {
                    ui.set_width(42.0);
                    if let Some(ref tex) = headshot_tex {
                        ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::splat(40.0)).rounding(20.0));
                    } else {
                        let initial = if !author_display.is_empty() {
                            author_display.chars().next().unwrap_or('?').to_uppercase().to_string()
                        } else {
                            post.author_pubkey.chars().next().unwrap_or('?').to_uppercase().to_string()
                        };
                        let (dot, _) = ui.allocate_exact_size(Vec2::new(40.0, 40.0), Sense::hover());
                        let accent_col = hex_color(post.author_accent.as_deref().unwrap_or("#5fe3ac"));
                        ui.painter().rect_filled(dot, Rounding::same(20.0), FIELD_HI);
                        ui.painter().text(
                            dot.center(),
                            egui::Align2::CENTER_CENTER,
                            initial,
                            egui::FontId::proportional(15.0),
                            accent_col,
                        );
                    }
                });

                ui.add_space(8.0);


                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());


                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&author_display).size(14.5).strong().color(INK));

                        if is_verified {
                            crate::ui::widgets::chip(ui, "verified", MINT);
                        }

                        ui.label(RichText::new(&author_handle).size(13.0).color(DIM));
                        ui.label(RichText::new("·").size(12.0).color(DIM));
                        ui.label(RichText::new(format_relative_time(post.created_at)).size(12.0).color(DIM));

                        if post.is_pinned {
                            crate::ui::widgets::chip(ui, "pinned", WARN);
                        }


                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let post_id = post.id.clone();
                            let author_pub = post.author_pubkey.clone();

                            if secondary(ui, "report") {
                                self.active_report_dialog = Some(ReportDraft {
                                    target_type: ReportTargetType::Post,
                                    target_id: post_id.clone(),
                                    target_label: format!("Post by {}", &author_pub[..8.min(author_pub.len())]),
                                    reason: "Inappropriate or abusive content".to_string(),
                                    details: String::new(),
                                });
                            }

                            if is_me {
                                if secondary(ui, "edit") {
                                    self.editing_msg_id = Some(post_id.clone());
                                    self.edit_buffer = post.content.clone();
                                }
                                if secondary(ui, "delete") {
                                    self.delete_post(&post_id);
                                }
                            } else {
                                let following = self.following.contains(&author_pub);
                                let blocked = self.blocked.contains(&author_pub);
                                if secondary(ui, if following { "unfollow" } else { "follow" }) {
                                    self.toggle_follow(&author_pub);
                                }
                                if secondary(ui, if blocked { "unblock" } else { "block" }) {
                                    self.toggle_block(&author_pub);
                                }
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
                        ui.label(RichText::new(&post.content).size(14.0).color(INK));
                        if post.edited_at.is_some() {
                            ui.label(RichText::new("(edited)").size(11.0).color(DIM));
                        }
                    }


                    if !post.tags.is_empty() {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            for tag in &post.tags {
                                ui.label(RichText::new(format!("#{}", tag)).size(12.0).color(MINT));
                            }
                        });
                    }


                    self.render_attachment_list(ui, &post.attachments);

                    ui.add_space(8.0);


                    ui.horizontal(|ui| {
                        let post_id = post.id.clone();

                        let like_label = if post.likes_count > 0 {
                            format!("like ({})", post.likes_count)
                        } else {
                            "like".to_string()
                        };
                        let like_active = post.liked_by_me;
                        if pill(ui, &like_label, like_active, false).clicked() {
                            self.interact_post(&post_id, InteractionType::Like);
                        }

                        ui.add_space(6.0);

                        let is_expanded = self.expanded_replies.contains(&post.id);
                        let reply_label = if post.replies_count > 0 {
                            format!("reply ({})", post.replies_count)
                        } else {
                            "reply".to_string()
                        };
                        if pill(ui, &reply_label, is_expanded, false).clicked() {
                            if is_expanded {
                                self.expanded_replies.remove(&post.id);
                            } else {
                                self.expanded_replies.insert(post.id.clone());
                            }
                        }
                    });


                    if self.expanded_replies.contains(&post.id) {
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(6.0);


                        if !self.cached_replies.contains_key(&post.id) {
                            let mut list = Vec::new();
                            for p in &self.cached_posts {
                                if p.reply_to_id.as_deref() == Some(&post.id) {
                                    list.push(p.clone());
                                }
                            }
                            if let Some(ref client) = self.client {
                                if let Ok(server_replies) = client.get_replies(&post.id) {
                                    for r in server_replies {
                                        if !list.iter().any(|item| item.id == r.id) {
                                            list.push(r);
                                        }
                                    }
                                }
                            }
                            self.cached_replies.insert(post.id.clone(), list);
                        }


                        let replies = self.cached_replies.get(&post.id).cloned().unwrap_or_default();
                        for reply in &replies {
                            self.render_reply_card(ui, reply, config);
                            ui.add_space(6.0);
                        }


                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            let draft = self.reply_drafts.entry(post.id.clone()).or_default();
                            let resp = text_input(ui, Id::new(("reply_input", &post.id)), draft, "Write a reply...", ui.available_width() - 80.0);

                            let post_id = post.id.clone();
                            let can_reply = !draft.trim().is_empty();
                            if (primary(ui, "Reply", can_reply) || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && can_reply {
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

        Frame::none()
            .fill(FIELD_LO)
            .rounding(Rounding::same(10.0))
            .inner_margin(Margin::same(12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(32.0);
                        if let Some(ref tex) = headshot_tex {
                            ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::splat(28.0)).rounding(14.0));
                        } else {
                            let initial = author_display.chars().next().unwrap_or('?').to_uppercase().to_string();
                            let (dot, _) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::hover());
                            let accent_col = hex_color(reply.author_accent.as_deref().unwrap_or("#5fe3ac"));
                            ui.painter().rect_filled(dot, Rounding::same(14.0), FIELD_HI);
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
                            ui.label(RichText::new(&author_display).size(12.5).strong().color(INK));
                            if is_verified {
                                crate::ui::widgets::chip(ui, "verified", MINT);
                            }
                            ui.label(RichText::new(&author_handle).size(11.5).color(DIM));
                            ui.label(RichText::new("·").size(11.0).color(DIM));
                            ui.label(RichText::new(format_relative_time(reply.created_at)).size(11.0).color(DIM));
                        });
                        ui.add_space(2.0);
                        if self.editing_msg_id.as_deref() == Some(&reply.id) {
                            let reply_id = reply.id.clone();
                            text_input(ui, Id::new(("edit_reply", &reply.id)), &mut self.edit_buffer, "edit reply...", ui.available_width());
                            ui.horizontal(|ui| {
                                if primary(ui, "save", !self.edit_buffer.trim().is_empty()) {
                                    self.save_post_edit(&reply_id);
                                }
                                if secondary(ui, "cancel") {
                                    self.editing_msg_id = None;
                                    self.edit_buffer.clear();
                                }
                            });
                        } else {
                            ui.label(RichText::new(&reply.content).size(12.5).color(INK));
                            if reply.edited_at.is_some() {
                                ui.label(RichText::new("(edited)").size(10.5).color(DIM));
                            }
                        }
                        self.render_attachment_list(ui, &reply.attachments);
                        if is_me {
                            ui.horizontal(|ui| {
                                let reply_id = reply.id.clone();
                                if secondary(ui, "edit") {
                                    self.editing_msg_id = Some(reply_id.clone());
                                    self.edit_buffer = reply.content.clone();
                                }
                                if secondary(ui, "delete") {
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


            page_header(ui, "public profile", "how you appear in the community and on servers you host.");

            card(ui, Some("customise"), |ui| {
                field(ui, "display name", &mut config.display_name, "how people see you");
                field(ui, "bio", &mut config.profile_bio, "a line about you (200 characters)");
                config.profile_bio.truncate(config.profile_bio.char_indices().nth(200).map_or(config.profile_bio.len(), |(i, _)| i));

                ui.label(muted("accent colour"));
                ui.horizontal(|ui| {
                    let current = crate::profile::clean_accent(&config.profile_accent);
                    for (name, hex) in crate::profile::ACCENTS {
                        let selected = current == *hex;
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(32.0, 32.0), Sense::click());
                        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(*name);
                        let r = if selected { 13.0 } else if resp.hovered() { 12.0 } else { 10.0 };
                        ui.painter().circle_filled(rect.center() + Vec2::new(0.0, 1.5), r, Color32::from_black_alpha(60));
                        ui.painter().circle_filled(rect.center(), r, hex_color(hex));
                        if selected {
                            ui.painter().circle_filled(rect.center(), 4.0, MINT_INK);
                        }
                        if resp.clicked() {
                            crate::ui::sound::play_click();
                            config.profile_accent = name.to_string();
                        }
                    }
                });

                ui.add_space(10.0);
                field(ui, "community server", &mut config.community_instance, "empty uses your first master");
                toggle(ui, &mut config.show_profile, "show my profile on servers i host");

                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if primary(ui, "save profile", true) {
                        crate::config::save_config(config);
                        self.save_public_profile(config);
                    }
                    if let Some((ref msg, time)) = self.profile_save_status {
                        if time.elapsed() < Duration::from_secs(5) {
                            ui.add_space(8.0);
                            ui.label(RichText::new(lc(msg)).size(13.0).color(MINT));
                        }
                    }
                });
            });

            ui.add_space(12.0);
            card(ui, Some("preview"), |ui| {
                ui.horizontal(|ui| {
                    if let Some(ref tex) = self.local_headshot {
                        ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::splat(48.0)).rounding(24.0));
                    }
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                            ui.painter().circle_filled(dot.center(), 5.0, hex_color(&crate::profile::clean_accent(&config.profile_accent)));
                            ui.label(RichText::new(lc(config.effective_display_name())).size(16.0).color(INK));
                        });
                        let bio = if config.profile_bio.trim().is_empty() { "no bio yet".to_string() } else { config.profile_bio.clone() };
                        ui.label(muted(bio));
                    });
                });
            });
            ui.add_space(16.0);
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
        self.profile_save_status = Some(match client.update_profile(prof) {
            Ok(_) => ("profile saved and published".to_string(), Instant::now()),
            Err(e) => (format!("saved locally, community said: {}", e), Instant::now()),
        });
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
                            ui.label(RichText::new("GIF").size(11.0).color(MINT).strong());
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
                egui::Frame::none()
                    .fill(FIELD)
                    .rounding(8.0)
                    .inner_margin(Margin::symmetric(10.0, 6.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let icon = if is_video { "video" } else { "audio" };
                            ui.label(RichText::new(icon).size(11.0).color(MINT).strong());
                            ui.add_space(4.0);
                            ui.label(RichText::new(&att.filename).size(12.0).color(INK));
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
        egui::Frame::none()
            .fill(FIELD)
            .rounding(8.0)
            .inner_margin(Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&att.filename).size(12.0).color(INK));
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
        ui.horizontal(|ui| {

            ui.vertical(|ui| {
                ui.set_width((ui.available_width() * 0.3).clamp(200.0, 320.0));
                ui.set_min_height(full_h);


                card(ui, Some("Direct messages"), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("encrypted").size(12.0).color(MINT));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if primary(ui, "+ New chat", true) {
                                self.show_new_dm_modal = true;
                            }
                        });
                    });
                });

                ui.add_space(8.0);


                ScrollArea::vertical().id_salt("community_scroll_2")
                .auto_shrink([false, false])
                .max_height((full_h - 120.0).max(120.0))
                .show(ui, |ui| {
                    if self.cached_conversations.is_empty() {
                        card(ui, None, |ui| {
                            ui.label(muted("No active conversations. Click '+ New chat' to start!"));
                        });
                    } else {
                        let convs = self.cached_conversations.clone();
                        for conv in convs {
                            let is_active = self.active_conv_id.as_deref() == Some(&conv.id);
                            let title = conv.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| {
                                if conv.is_group {
                                    "Group Chat".to_string()
                                } else {
                                    conv.members
                                        .iter()
                                        .find(|m| Some(m.pubkey.clone()) != self.client.as_ref().and_then(|c| c.identity_pubkey()))
                                        .map(|m| short_handle(&m.handle))
                                        .unwrap_or_else(|| format!("DM {}", &conv.id[..conv.id.len().min(8)]))
                                }
                            });

                            let last_msg = conv.last_message.as_ref().map(|m| {
                                if is_encrypted_payload(&m.content) {
                                    "encrypted message".to_string()
                                } else {
                                    m.content.clone()
                                }
                            }).unwrap_or_else(|| "No messages yet".to_string());

                            Frame::none()
                                .fill(if is_active { FIELD_HI } else { CARD })
                                .rounding(14.0)
                                .inner_margin(Margin::same(12.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    let resp = ui.interact(ui.min_rect(), Id::new(("conv_pill", &conv.id)), Sense::click());
                                    if resp.clicked() {
                                        crate::ui::sound::play_click();
                                        self.active_conv_id = Some(conv.id.clone());
                                        self.fetch_dms(&conv.id);
                                    }

                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(if conv.is_group { "[group]" } else { "[dm]" }).size(11.0).color(DIM));
                                        ui.vertical(|ui| {
                                            ui.label(RichText::new(&title).size(13.5).strong().color(INK));
                                            ui.label(muted(last_msg));
                                        });
                                    });
                                });
                            ui.add_space(6.0);
                        }
                    }
                });
            });

            ui.add_space(10.0);


            ui.vertical(|ui| {
                if let Some(ref conv_id) = self.active_conv_id {
                    let conv_id = conv_id.clone();
                    ui.horizontal(|ui| {
                        let title = self.cached_conversations.iter()
                            .find(|c| c.id == conv_id)
                            .and_then(|c| c.title.clone())
                            .unwrap_or_else(|| format!("Chat with pup_{}", &conv_id[..conv_id.len().min(8)]));
                        ui.label(RichText::new(&title).size(15.0).strong().color(INK));
                        ui.label(muted("• encrypted"));
                    });

                    ui.add_space(6.0);


                    let messages = self.cached_dms.get(&conv_id).cloned().unwrap_or_default();
                    ScrollArea::vertical().id_salt("community_scroll_3")
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .max_height(ui.available_height() - 80.0)
                        .show(ui, |ui| {
                            if messages.is_empty() {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(30.0);
                                    ui.label(RichText::new("Encrypted conversation").size(14.0).color(MINT));
                                    ui.label(muted("Messages are encrypted on-device with X25519 and ChaCha20-Poly1305."));
                                    ui.add_space(30.0);
                                });
                            } else {
                                let my_pubkey = derive_ed25519_pubkey_hex(&config.server_secret);
                                for msg in messages {
                                    let is_mine = msg.sender_pubkey == my_pubkey;

                                    let display_text = if is_encrypted_payload(&msg.content) {
                                        decrypt_direct_message(&config.server_secret, &msg.sender_pubkey, &msg.content)
                                            .unwrap_or_else(|_| "[Encrypted Message - Key Mismatch]".to_string())
                                    } else {
                                        msg.content.clone()
                                    };

                                    let msg_roblox_id = if is_mine && config.roblox_id != 0 {
                                        Some(config.roblox_id)
                                    } else {
                                        None
                                    };

                                    let mut msg_avatar: Option<egui::TextureHandle> = if is_mine && self.local_headshot.is_some() {
                                        self.local_headshot.clone()
                                    } else if let Some(rid) = msg_roblox_id {
                                        self.avatar_cache.get(&rid).cloned()
                                    } else {
                                        None
                                    };

                                    if msg_avatar.is_none() && !is_mine {
                                        if let Some(ref avatar_url) = msg.author_avatar_url {
                                            if !avatar_url.is_empty() {
                                                msg_avatar = self.avatar_url_cache.get(avatar_url).cloned();
                                            }
                                        }
                                    }

                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        if is_mine {
                                            ui.allocate_space(Vec2::new(32.0, 32.0));
                                            ui.add_space(8.0);
                                        }

                                        ui.vertical(|ui| {
                                            if !is_mine {
                                                let sender_name = msg
                                                    .sender_display
                                                    .clone()
                                                    .filter(|d| !d.trim().is_empty())
                                                    .unwrap_or_else(|| msg.sender_handle.clone());
                                                ui.label(RichText::new(sender_name).size(10.5).color(DIM));
                                                ui.add_space(2.0);
                                            }
                                            Frame::none()
                                                .fill(if is_mine { FIELD_HI } else { FIELD })
                                                .rounding(Rounding::same(12.0))
                                                .inner_margin(Margin::symmetric(12.0, 7.0))
                                                .show(ui, |ui| {
                                                    if self.editing_msg_id.as_deref() == Some(&msg.id) {
                                                        let mid = msg.id.clone();
                                                        let cid = conv_id.clone();
                                                        text_input(ui, Id::new(("edit_dm", &msg.id)), &mut self.edit_buffer, "edit message...", ui.available_width() - 20.0);
                                                        ui.horizontal(|ui| {
                                                            if primary(ui, "save", !self.edit_buffer.trim().is_empty()) {
                                                                self.save_dm_edit(&cid, &mid, config);
                                                            }
                                                            if secondary(ui, "cancel") {
                                                                self.editing_msg_id = None;
                                                                self.edit_buffer.clear();
                                                            }
                                                        });
                                                    } else {
                                                        ui.label(RichText::new(&display_text).size(13.5).color(INK));
                                                    }
                                                });
                                            self.render_attachment_list(ui, &msg.attachments);
                                            if is_mine {
                                                ui.horizontal(|ui| {
                                                    let mid = msg.id.clone();
                                                    let cid = conv_id.clone();
                                                    if secondary(ui, "edit") {
                                                        self.editing_msg_id = Some(mid.clone());
                                                        self.edit_buffer = display_text.clone();
                                                    }
                                                    if secondary(ui, "delete") {
                                                        self.delete_dm(&cid, &mid);
                                                    }
                                                });
                                            }
                                        });

                                        if !is_mine {
                                            ui.add_space(8.0);
                                            ui.vertical(|ui| {
                                                if let Some(ref tex) = msg_avatar {
                                                    ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::splat(28.0)).rounding(14.0));
                                                } else {
                                                    let initial = msg.sender_pubkey.chars().next().unwrap_or('?').to_uppercase().to_string();
                                                    let (dot, _) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::hover());
                                                    ui.painter().rect_filled(dot, Rounding::same(14.0), FIELD_HI);
                                                    ui.painter().text(
                                                        dot.center(),
                                                        egui::Align2::CENTER_CENTER,
                                                        initial,
                                                        egui::FontId::proportional(12.0),
                                                        MINT,
                                                    );
                                                }
                                            });
                                        }
                                    });
                                    ui.add_space(4.0);
                                }
                            }
                        });

                    ui.add_space(6.0);


                    ui.horizontal(|ui| {
                        text_input(
                            ui,
                            Id::new("dm_composer_field"),
                            &mut self.dm_composer,
                            "Message...",
                            ui.available_width() - 80.0,
                        );

                        if primary(ui, "Send", !self.dm_composer.trim().is_empty()) {
                            let text = self.dm_composer.trim().to_string();
                            self.dm_composer.clear();
                            self.send_encrypted_dm(&conv_id, &text, config);
                        }
                    });
                } else {
                    card(ui, None, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(40.0);
                            ui.label(RichText::new("Select a conversation").size(15.0).color(INK));
                            ui.label(muted("Messages and group chats are end-to-end encrypted."));
                            ui.add_space(40.0);
                        });
                    });
                }
            });
        });
    }


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
                    if pill(ui, "+", false, false).on_hover_text("New guild").clicked() {
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
                    ui.label(RichText::new(&guild_name).size(16.0).strong().color(INK));
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
                                ui.label(RichText::new("channels").size(12.0).strong().color(DIM));
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
                            ui.label(RichText::new(format!("members ({})", members.len())).size(12.0).strong().color(DIM));
                            ui.add_space(4.0);
                            for m in members.iter() {
                                let name = m.nickname.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| short_handle(&m.handle));
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&name).size(13.5).strong().color(INK));
                                    ui.label(RichText::new(short_handle(&m.handle)).size(11.0).color(DIM));
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
                            ui.label(RichText::new(&ch_name).size(16.0).strong().color(INK));
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
                                    ui.add_space(20.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(muted("no messages yet — say hi!"));
                                    });
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
                                                ui.label(RichText::new(&name).size(14.0).strong().color(if mine { MINT } else { INK }));
                                                ui.label(RichText::new(format_relative_time(msg.created_at)).size(11.5).color(DIM));
                                                if msg.edited_at.is_some() {
                                                    ui.label(RichText::new("(edited)").size(11.5).color(DIM));
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
                                                ui.label(RichText::new(&msg.content).size(15.0).color(INK));
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
                            ui.vertical_centered(|ui| {
                                ui.add_space(40.0);
                                ui.label(RichText::new("Select a community to view channels").size(15.0).color(INK));
                                ui.label(muted("Join public developer guilds, share Roblox assets, and coordinate builds."));
                                ui.add_space(40.0);
                            });
                        });
                    }
                }
            });
        });
    }


    fn render_notifications(&mut self, ui: &mut Ui, _config: &AppConfig) {
        ScrollArea::vertical().id_salt("community_scroll_6").show(ui, |ui| {


            card(ui, Some("NOTIFICATIONS"), |ui| {
                ui.horizontal(|ui| {
                    ui.label(muted("Stay updated on replies, likes, mentions, and community invites."));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if secondary(ui, "Mark all read") {
                            self.mark_all_notifications_read();
                        }
                    });
                });
            });

            ui.add_space(10.0);

            if self.notifications.is_empty() {
                card(ui, None, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(30.0);
                        ui.label(RichText::new("All caught up!").size(15.0).color(INK));
                        ui.label(muted("No unread notifications at the moment."));
                        ui.add_space(30.0);
                    });
                });
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

                    card(ui, None, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("[{}]", tag)).size(11.5).color(MINT));
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&n.preview).size(13.5).color(INK));
                                ui.label(muted(format_relative_time(n.created_at)));
                            });
                        });
                    });
                    ui.add_space(6.0);
                }
            }
        });
    }


    fn render_search(&mut self, ui: &mut Ui, config: &AppConfig) {
        ScrollArea::vertical().id_salt("community_scroll_7").show(ui, |ui| {


            card(ui, Some("SEARCH COMMUNITY"), |ui| {
                ui.horizontal(|ui| {
                    text_input(
                        ui,
                        Id::new("search_bar_field"),
                        &mut self.search_query,
                        "Search creators, posts, guilds, or hashtags...",
                        ui.available_width() - 90.0,
                    );

                    if primary(ui, "Search", !self.search_query.trim().is_empty()) {
                        self.execute_search();
                    }
                });
            });

            ui.add_space(14.0);


            let users = self.search_results.users.clone();
            if !users.is_empty() {
                card(ui, Some("CREATORS FOUND"), |ui| {
                    for u in &users {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&u.display_name).size(14.0).strong().color(INK));
                            ui.label(muted(format!("@{}", u.handle)));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let pk = u.pubkey.clone();
                                if secondary(ui, if self.blocked.contains(&pk) { "unblock" } else { "block" }) {
                                    self.toggle_block(&pk);
                                }
                                if secondary(ui, if self.following.contains(&pk) { "unfollow" } else { "follow" }) {
                                    self.toggle_follow(&pk);
                                }
                            });
                        });
                        ui.add_space(4.0);
                    }
                });
                ui.add_space(10.0);
            }


            let posts = self.search_results.posts.clone();
            if !posts.is_empty() {
                card(ui, Some("POSTS FOUND"), |ui| {
                    for p in &posts {
                        self.render_post_card(ui, p, config);
                        ui.add_space(6.0);
                    }
                });
            }
        });
    }


    pub fn render_keys_page(&mut self, ui: &mut Ui, config: &mut AppConfig) {
        ScrollArea::vertical().id_salt("community_scroll_8").show(ui, |ui| {


            page_header(
                ui,
                "Key Vault & Migration",
                "Move your identity, profile, and E2EE encryption keys between PCs safely.",
            );


            card(ui, Some("Public identity keys"), |ui| {
                ui.vertical(|ui| {
                    let secret = config.server_secret.trim();
                    let ed_pub = derive_ed25519_pubkey_hex(secret);
                    let x_pub = get_x25519_public_hex(secret);

                    ui.horizontal(|ui| {
                        ui.label(muted("Public signing key (ed25519):"));
                        ui.label(RichText::new(&ed_pub).monospace().size(12.0).color(INK));
                        if secondary(ui, "Copy") {
                            copy_text(&ed_pub);
                            self.key_status_msg = Some(("Public key copied to clipboard!".into(), true, Instant::now()));
                        }
                    });

                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        ui.label(muted("Encryption key (x25519):"));
                        ui.label(RichText::new(&x_pub).monospace().size(12.0).color(INK));
                        if secondary(ui, "Copy") {
                            copy_text(&x_pub);
                            self.key_status_msg = Some(("Encryption key copied to clipboard!".into(), true, Instant::now()));
                        }
                    });
                });
            });

            ui.add_space(10.0);


            card(ui, Some("Export & backup"), |ui| {
                ui.vertical(|ui| {
                    ui.label(muted("Save your private key backup so you can log into PuppyTunnel Community on another computer."));
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        if secondary(ui, "Export backup (.json)") {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_file_name("puppytunnel-identity-backup.json")
                                .save_file()
                            {
                                if let Ok(json) = export_backup_json(config) {
                                    if std::fs::write(&path, json).is_ok() {
                                        self.key_status_msg = Some((format!("Backup exported to {}", path.display()), true, Instant::now()));
                                    }
                                }
                            }
                        }

                        if secondary(ui, if self.reveal_secret { "Hide secret" } else { "Reveal secret key" }) {
                            self.reveal_secret = !self.reveal_secret;
                        }

                        if secondary(ui, if self.show_mnemonic { "Hide words" } else { "Recovery phrase (32 words)" }) {
                            self.show_mnemonic = !self.show_mnemonic;
                        }
                    });

                    if self.reveal_secret {
                        ui.add_space(10.0);
                        Frame::none()
                            .fill(FIELD_HI)
                            .rounding(14.0)
                            .inner_margin(Margin::same(12.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&config.server_secret).monospace().size(12.5).color(MINT));
                                    if secondary(ui, "Copy Secret") {
                                        copy_text(&config.server_secret);
                                        self.key_status_msg = Some(("Secret key copied! Keep it safe.".to_string(), true, Instant::now()));
                                    }
                                });
                            });
                    }

                    if self.show_mnemonic {
                        ui.add_space(10.0);
                        let mnemonic = generate_mnemonic(&config.server_secret);
                        Frame::none()
                            .fill(FIELD)
                            .rounding(14.0)
                            .inner_margin(Margin::same(14.0))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new("Account Recovery Phrase:").size(13.0).color(WARN));
                                    ui.add_space(4.0);
                                    ui.label(RichText::new(&mnemonic).monospace().size(12.5).color(INK));
                                    ui.add_space(6.0);
                                    if secondary(ui, "Copy 32 Words") {
                                        copy_text(&mnemonic);
                                        self.key_status_msg = Some(("Recovery phrase copied!".into(), true, Instant::now()));
                                    }
                                });
                            });
                    }
                });
            });

            ui.add_space(10.0);


            card(ui, Some("Import & restore"), |ui| {
                ui.vertical(|ui| {
                    ui.label(muted("Paste a backup JSON string, raw 64-character secret hex, or 32 recovery words:"));
                    ui.add_space(8.0);

                    text_input(
                        ui,
                        Id::new("import_key_buffer"),
                        &mut self.import_buffer,
                        "Paste key backup here...",
                        ui.available_width(),
                    );

                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        if primary(ui, "Restore Account", !self.import_buffer.trim().is_empty()) {
                            match import_backup_str(&self.import_buffer, config) {
                                Ok(backup) => {
                                    self.import_buffer.clear();
                                    self.client = None;
                                    self.key_status_msg = Some((
                                        format!("Identity restored! Logged in as {}", backup.username),
                                        true,
                                        Instant::now(),
                                    ));
                                }
                                Err(e) => {
                                    self.key_status_msg = Some((format!("Import failed: {}", e), false, Instant::now()));
                                }
                            }
                        }

                        if secondary(ui, "Open backup file...") {
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
                });
            });


            if let Some((ref msg, success, t)) = self.key_status_msg {
                if t.elapsed() < Duration::from_secs(4) {
                    ui.add_space(10.0);
                    let color = if success { MINT } else { ERR };
                    ui.label(RichText::new(msg).size(13.5).color(color));
                }
            }
        });
    }


    fn render_moderation(&mut self, ui: &mut Ui, _config: &AppConfig) {
        ScrollArea::vertical().id_salt("community_scroll_9").show(ui, |ui| {


            page_header(
                ui,
                "Moderation Center",
                "System-wide tools for the Instance Owner and Guild Administrators.",
            );


            ui.horizontal(|ui| {
                let tabs = [
                    ("Mod Queue (Reports)", ModSubTab::ModQueue),
                    ("Global Ban Manager", ModSubTab::GlobalBans),
                    ("Audit Log", ModSubTab::AuditLog),
                ];
                for (label, tab) in tabs {
                    let active = self.mod_tab == tab;
                    if pill(ui, label, active, false).clicked() {
                        self.mod_tab = tab;
                    }
                }
            });

            ui.add_space(14.0);

            match self.mod_tab {
                ModSubTab::ModQueue => self.render_mod_queue(ui),
                ModSubTab::GlobalBans => self.render_global_bans(ui),
                ModSubTab::AuditLog => self.render_audit_log(ui),
            }
        });
    }

    fn render_mod_queue(&mut self, ui: &mut Ui) {
        card(ui, Some("Reports queue"), |ui| {
            ui.horizontal(|ui| {
                ui.label(muted("Review reports submitted by players regarding spam, harassment, or malicious activity."));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if secondary(ui, "Refresh") {
                        self.fetch_reports();
                    }
                });
            });
        });

        ui.add_space(8.0);

        if self.reports.is_empty() {
            card(ui, None, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.label(RichText::new("Mod queue is clear").size(14.0).color(MINT));
                    ui.label(muted("No pending abuse or harassment reports at this time."));
                    ui.add_space(24.0);
                });
            });
        } else {
            let reports = self.reports.clone();
            for r in reports {
                card(ui, None, |ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Report").size(13.0).color(ERR).strong());
                            ui.label(RichText::new(format!("{:?}", r.target_type)).size(13.0).color(INK));
                            ui.label(muted(format!("Target: {}", &r.target_id[..r.target_id.len().min(12)])));

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let rep_id = r.id.clone();
                                let target_id = r.target_id.clone();

                                if secondary(ui, "Dismiss") {
                                    self.resolve_report(&rep_id, "dismissed");
                                }
                                if secondary(ui, "Ban user") {
                                    self.execute_global_ban(&target_id, "Banned via user report investigation");
                                    self.resolve_report(&rep_id, "resolved");
                                }
                            });
                        });

                        ui.add_space(4.0);
                        ui.label(RichText::new(format!("Reason: {}", r.reason)).size(13.5).color(INK));
                        if let Some(ref d) = r.details {
                            ui.label(muted(format!("Details: {}", d)));
                        }
                    });
                });
                ui.add_space(6.0);
            }
        }
    }

    fn render_global_bans(&mut self, ui: &mut Ui) {
        card(ui, Some("Global ban manager"), |ui| {
            ui.vertical(|ui| {
                ui.label(muted("Banning a user prevents them from publishing posts, sending DMs, or joining guilds on this instance."));
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    text_input(
                        ui,
                        Id::new("ban_target_field"),
                        &mut self.ban_target_input,
                        "Target public key or handle...",
                        ui.available_width() * 0.45,
                    );

                    text_input(
                        ui,
                        Id::new("ban_reason_field"),
                        &mut self.ban_reason_input,
                        "Reason...",
                        ui.available_width() - 90.0,
                    );

                    if primary(ui, "Ban", !self.ban_target_input.trim().is_empty()) {
                        let target = self.ban_target_input.trim().to_string();
                        let reason = if !self.ban_reason_input.trim().is_empty() {
                            self.ban_reason_input.trim().to_string()
                        } else {
                            "Violation of community guidelines".to_string()
                        };
                        self.execute_global_ban(&target, &reason);
                        self.ban_target_input.clear();
                        self.ban_reason_input.clear();
                    }
                });
            });
        });


        if let Some((ref msg, t)) = self.mod_status_msg {
            if t.elapsed() < Duration::from_secs(4) {
                ui.add_space(8.0);
                ui.label(RichText::new(msg).size(13.5).color(MINT));
            }
        }
    }

    fn render_audit_log(&mut self, ui: &mut Ui) {
        card(ui, Some("Audit trail"), |ui| {
            ui.horizontal(|ui| {
                ui.label(muted("Chronological log of all moderation actions taken across the instance."));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if secondary(ui, "Refresh") {
                        self.fetch_audit_logs();
                    }
                });
            });
        });

        ui.add_space(8.0);

        if self.audit_logs.is_empty() {
            card(ui, None, |ui| {
                ui.label(muted("No audit entries recorded yet."));
            });
        } else {
            let logs = self.audit_logs.clone();
            for entry in logs {
                card(ui, None, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&entry.action).size(13.5).color(MINT).strong());
                        ui.label(muted(format!("by pup_{}", &entry.actor_pubkey[..entry.actor_pubkey.len().min(8)])));
                        ui.label(RichText::new(format!("target: {}", entry.target_id)).size(13.0).color(INK));

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(muted(format_relative_time(entry.created_at)));
                        });
                    });
                });
                ui.add_space(4.0);
            }
        }
    }


    fn render_active_modals(&mut self, _ui: &mut Ui, config: &AppConfig, ctx: &egui::Context) {
        let screen_rect = ctx.screen_rect();
        let esc_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));


        if let Some(mut draft) = self.active_report_dialog.take() {
            let mut close = esc_pressed;
            let mut submit = false;

            egui::Area::new(Id::new("report_modal_scrim"))
                .order(egui::Order::Foreground)
                .fixed_pos(screen_rect.min)
                .show(ctx, |ui| {
                    let (bg_rect, bg_resp) = ui.allocate_exact_size(screen_rect.size(), Sense::click());
                    ui.painter().rect_filled(bg_rect, 0.0, Color32::from_black_alpha(180));
                    if bg_resp.clicked() {
                        close = true;
                    }

                    let card_width = 340.0_f32.min(screen_rect.width() - 32.0);
                    let card_x = screen_rect.center().x - card_width / 2.0;
                    let card_y = (screen_rect.center().y - 110.0).max(screen_rect.min.y + 24.0);

                    let card_rect = Rect::from_min_size(Pos2::new(card_x, card_y), Vec2::new(card_width, 10.0));
                    ui.allocate_new_ui(UiBuilder::new().max_rect(card_rect), |ui| {
                        Frame::none()
                            .fill(CARD)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(Margin::same(20.0))
                            .show(ui, |ui| {
                                ui.set_width(card_width - 40.0);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("report").size(15.0).color(INK).strong());
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            if ui.add(egui::Button::new(RichText::new("x").size(13.0).color(DIM)).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                                close = true;
                                            }
                                        });
                                    });

                                    ui.add_space(10.0);

                                    ui.horizontal_wrapped(|ui| {
                                        for cat in ["spam", "harassment", "phishing", "inappropriate"] {
                                            let selected = draft.reason == cat;
                                            if pill(ui, cat, selected, false).clicked() {
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
                                    ui.horizontal(|ui| {
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            if primary(ui, "submit", !draft.reason.is_empty()) {
                                                submit = true;
                                            }
                                            ui.add_space(6.0);
                                            if secondary(ui, "cancel") {
                                                close = true;
                                            }
                                        });
                                    });
                                });
                            });
                    });
                });

            if submit {
                self.submit_report(&draft);
            } else if !close {
                self.active_report_dialog = Some(draft);
            }
        }


        if self.show_new_dm_modal {
            let mut close = esc_pressed;
            let mut start = false;

            egui::Area::new(Id::new("new_dm_modal_scrim"))
                .order(egui::Order::Foreground)
                .fixed_pos(screen_rect.min)
                .show(ctx, |ui| {
                    let (bg_rect, bg_resp) = ui.allocate_exact_size(screen_rect.size(), Sense::click());
                    ui.painter().rect_filled(bg_rect, 0.0, Color32::from_black_alpha(180));
                    if bg_resp.clicked() {
                        close = true;
                    }

                    let card_width = 340.0_f32.min(screen_rect.width() - 32.0);
                    let card_x = screen_rect.center().x - card_width / 2.0;
                    let card_y = (screen_rect.center().y - 80.0).max(screen_rect.min.y + 24.0);

                    let card_rect = Rect::from_min_size(Pos2::new(card_x, card_y), Vec2::new(card_width, 10.0));
                    ui.allocate_new_ui(UiBuilder::new().max_rect(card_rect), |ui| {
                        Frame::none()
                            .fill(CARD)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(Margin::same(20.0))
                            .show(ui, |ui| {
                                ui.set_width(card_width - 40.0);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("new message").size(15.0).color(INK).strong());
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            if ui.add(egui::Button::new(RichText::new("x").size(13.0).color(DIM)).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                                close = true;
                                            }
                                        });
                                    });

                                    ui.add_space(12.0);

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
                                        ui.label(RichText::new(&validation_error).size(11.5).color(ERR));
                                    }
                                    if !self.new_dm_error.is_empty() {
                                        ui.add_space(6.0);
                                        ui.label(RichText::new(&self.new_dm_error).size(11.5).color(ERR));
                                    }

                                    ui.add_space(16.0);

                                    ui.horizontal(|ui| {
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            let can_submit = !self.new_dm_target.trim().is_empty() && validation_error.is_empty();
                                            if primary(ui, "start", can_submit) {
                                                start = true;
                                            }
                                            ui.add_space(6.0);
                                            if secondary(ui, "cancel") {
                                                close = true;
                                            }
                                        });
                                    });
                                });
                            });
                    });
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
                let resolved = self
                    .client
                    .as_ref()
                    .and_then(|c| c.get_profile(&lookup).ok())
                    .map(|prof| prof.pubkey);
                match resolved {
                    Some(pk) => {
                        self.new_dm_target.clear();
                        self.new_dm_error.clear();
                        self.show_new_dm_modal = false;
                        self.create_dm_conversation(&pk, config);
                    }
                    None => {
                        self.new_dm_error = format!("no such person: {}", target);
                    }
                }
            } else if close {
                self.show_new_dm_modal = false;
            }
        }


        if self.show_create_guild {
            let mut close = esc_pressed;
            let mut create = false;

            egui::Area::new(Id::new("new_guild_modal_scrim"))
                .order(egui::Order::Foreground)
                .fixed_pos(screen_rect.min)
                .show(ctx, |ui| {
                    let (bg_rect, bg_resp) = ui.allocate_exact_size(screen_rect.size(), Sense::click());
                    ui.painter().rect_filled(bg_rect, 0.0, Color32::from_black_alpha(180));
                    if bg_resp.clicked() {
                        close = true;
                    }

                    let card_width = 340.0_f32.min(screen_rect.width() - 32.0);
                    let card_x = screen_rect.center().x - card_width / 2.0;
                    let card_y = (screen_rect.center().y - 100.0).max(screen_rect.min.y + 24.0);

                    let card_rect = Rect::from_min_size(Pos2::new(card_x, card_y), Vec2::new(card_width, 10.0));
                    ui.allocate_new_ui(UiBuilder::new().max_rect(card_rect), |ui| {
                        Frame::none()
                            .fill(CARD)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(Margin::same(20.0))
                            .show(ui, |ui| {
                                ui.set_width(card_width - 40.0);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("new guild").size(15.0).color(INK).strong());
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            if ui.add(egui::Button::new(RichText::new("x").size(13.0).color(DIM)).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                                close = true;
                                            }
                                        });
                                    });

                                    ui.add_space(12.0);

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

                                    ui.horizontal(|ui| {
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            let can_submit = !self.new_guild_name.trim().is_empty();
                                            if primary(ui, "create", can_submit) {
                                                create = true;
                                            }
                                            ui.add_space(6.0);
                                            if secondary(ui, "cancel") {
                                                close = true;
                                            }
                                        });
                                    });
                                });
                            });
                    });
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
            let mut close = esc_pressed;
            let mut create = false;

            egui::Area::new(Id::new("new_channel_modal_scrim"))
                .order(egui::Order::Foreground)
                .fixed_pos(screen_rect.min)
                .show(ctx, |ui| {
                    let (bg_rect, bg_resp) = ui.allocate_exact_size(screen_rect.size(), Sense::click());
                    ui.painter().rect_filled(bg_rect, 0.0, Color32::from_black_alpha(180));
                    if bg_resp.clicked() {
                        close = true;
                    }

                    let card_width = 340.0_f32.min(screen_rect.width() - 32.0);
                    let card_x = screen_rect.center().x - card_width / 2.0;
                    let card_y = (screen_rect.center().y - 80.0).max(screen_rect.min.y + 24.0);

                    let card_rect = Rect::from_min_size(Pos2::new(card_x, card_y), Vec2::new(card_width, 10.0));
                    ui.allocate_new_ui(UiBuilder::new().max_rect(card_rect), |ui| {
                        Frame::none()
                            .fill(CARD)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(Margin::same(20.0))
                            .show(ui, |ui| {
                                ui.set_width(card_width - 40.0);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("new channel").size(15.0).color(INK).strong());
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            if ui.add(egui::Button::new(RichText::new("x").size(13.0).color(DIM)).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                                close = true;
                                            }
                                        });
                                    });

                                    ui.add_space(12.0);

                                    text_input(
                                        ui,
                                        Id::new("new_channel_name_field"),
                                        &mut self.new_channel_name,
                                        "channel name",
                                        ui.available_width(),
                                    );

                                    ui.add_space(16.0);

                                    ui.horizontal(|ui| {
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            let can_submit = !self.new_channel_name.trim().is_empty();
                                            if primary(ui, "create", can_submit) {
                                                create = true;
                                            }
                                            ui.add_space(6.0);
                                            if secondary(ui, "cancel") {
                                                close = true;
                                            }
                                        });
                                    });
                                });
                            });
                    });
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
        if !self.feed_loaded {
            self.refresh_feed();
        }
        let lists_due = self
            .lists_checked_at
            .map(|t| t.elapsed() > Duration::from_secs(30))
            .unwrap_or(true);
        if lists_due {
            self.lists_checked_at = Some(Instant::now());
            self.fetch_social();
            self.fetch_conversations();
            self.fetch_guilds();
            self.fetch_notifications();
        }

        let due = self
            .verified_check_at
            .map(|t| t.elapsed() > Duration::from_secs(60))
            .unwrap_or(true);
        if due {
            self.verified_check_at = Some(Instant::now());
            self.publish_profile(config);
            if let Some(ref client) = self.client {
                if let Some(pk) = client.identity_pubkey() {
                    if let Ok(prof) = client.get_profile(&pk) {
                        let rid = config.roblox_id;
                        self.my_roblox_verified =
                            prof.roblox_verified && (rid == 0 || prof.roblox_id == Some(rid));
                    }
                }
            }
        }
    }

    pub fn refresh_feed(&mut self) {
        if let Some(ref client) = self.client {
            if let Ok(posts) = client.get_feed(30, None) {
                self.cached_posts = posts;
                let blocked = self.blocked.clone();
                self.cached_posts.retain(|p| !blocked.contains(&p.author_pubkey));
                self.feed_loaded = true;
            }
        }
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

        if let Some(ref client) = self.client {
            if let Ok(post) = client.create_post(&content, attachments, None, None, tags, "public") {
                self.cached_posts.insert(0, post);
                self.composer_text.clear();
                self.composer_attachment = None;
            }
        }
    }

    pub fn delete_post(&mut self, post_id: &str) {
        if let Some(ref client) = self.client {
            if client.delete_post(post_id).is_ok() {
                self.cached_posts.retain(|p| p.id != post_id);
                for list in self.cached_replies.values_mut() {
                    if let Some(pos) = list.iter().position(|p| p.id == post_id) {
                        let removed = list.remove(pos);
                        if let Some(pid) = removed.reply_to_id {
                            if let Some(parent) = self.cached_posts.iter_mut().find(|p| p.id == pid) {
                                parent.replies_count = parent.replies_count.saturating_sub(1);
                            }
                        }
                    }
                    list.retain(|p| p.reply_to_id.as_deref() != Some(post_id));
                }
            }
        }
    }

    pub fn save_post_edit(&mut self, post_id: &str) {
        let text = self.edit_buffer.trim().to_string();
        if text.is_empty() {
            return;
        }
        if let Some(ref client) = self.client {
            if let Ok(updated) = client.edit_post(post_id, &text) {
                if let Some(p) = self.cached_posts.iter_mut().find(|p| p.id == post_id) {
                    *p = updated.clone();
                }
                for list in self.cached_replies.values_mut() {
                    if let Some(r) = list.iter_mut().find(|r| r.id == post_id) {
                        *r = updated.clone();
                    }
                }
                self.editing_msg_id = None;
                self.edit_buffer.clear();
            }
        }
    }

    pub fn delete_dm(&mut self, conv_id: &str, msg_id: &str) {
        if let Some(ref client) = self.client {
            if let Ok(updated) = client.delete_dm(conv_id, msg_id) {
                if let Some(list) = self.cached_dms.get_mut(conv_id) {
                    if let Some(m) = list.iter_mut().find(|m| m.id == msg_id) {
                        *m = updated;
                    }
                }
            }
        }
    }

    pub fn save_dm_edit(&mut self, conv_id: &str, msg_id: &str, config: &AppConfig) {
        let text = self.edit_buffer.trim().to_string();
        if text.is_empty() {
            return;
        }
        if let Some(ref client) = self.client {
            let recipient_x25519 = get_x25519_public_hex(&config.server_secret);
            let stored = crate::community::e2ee::encrypt_direct_message(
                &config.server_secret,
                &recipient_x25519,
                &text,
            )
            .unwrap_or(text);
            if let Ok(updated) = client.edit_dm(conv_id, msg_id, &stored) {
                if let Some(list) = self.cached_dms.get_mut(conv_id) {
                    if let Some(m) = list.iter_mut().find(|m| m.id == msg_id) {
                        *m = updated;
                    }
                }
                self.editing_msg_id = None;
                self.edit_buffer.clear();
            }
        }
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
        if let Some(ref client) = self.client {
            let result = client.interact_post(post_id, kind, None);
            if let Ok(interaction) = result {
                if let Some(p) = self.cached_posts.iter_mut().find(|p| p.id == post_id) {
                    match kind {
                        InteractionType::Like => {
                            if let Some(liked) = interaction.liked {
                                p.liked_by_me = liked;
                            }
                            if let Some(count) = interaction.likes_count {
                                p.likes_count = count.max(0) as u32;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub fn submit_reply(&mut self, parent_id: &str, text: &str) {
        if let Some(ref client) = self.client {
            if let Ok(reply_post) = client.create_post(text, vec![], Some(parent_id.to_string()), None, vec![], "public") {
                self.cached_replies.entry(parent_id.to_string()).or_default().push(reply_post);
                if let Some(p) = self.cached_posts.iter_mut().find(|p| p.id == parent_id) {
                    p.replies_count += 1;
                }
            }
        }
    }

    pub fn fetch_dms(&mut self, conv_id: &str) {
        if let Some(ref client) = self.client {
            if let Ok(msgs) = client.get_messages(conv_id, 50) {
                let blocked = self.blocked.clone();
                let msgs: Vec<DirectMessage> =
                    msgs.into_iter().filter(|m| !blocked.contains(&m.sender_pubkey)).collect();
                self.cached_dms.insert(conv_id.to_string(), msgs);
            }
        }
    }

    pub fn send_encrypted_dm(&mut self, conv_id: &str, plaintext: &str, config: &AppConfig) {
        if let Some(ref client) = self.client {
            let recipient_x25519 = get_x25519_public_hex(&config.server_secret);
            if let Ok(msg) = client.send_e2ee_direct_message(conv_id, &recipient_x25519, plaintext, &config.server_secret) {
                self.cached_dms.entry(conv_id.to_string()).or_default().push(msg);
            }
        }
    }

    pub fn create_dm_conversation(&mut self, target_pubkey: &str, _config: &AppConfig) {
        if let Some(ref client) = self.client {
            if let Ok(conv) = client.create_conversation(vec![target_pubkey.to_string()], None) {
                self.active_conv_id = Some(conv.id.clone());
                self.cached_conversations.push(conv.clone());
            }
        }
    }

    pub fn fetch_guild_channels(&mut self, guild_id: &str) {
        if let Some(ref client) = self.client {
            if let Ok(guild) = client.get_guild(guild_id) {
                self.cached_channels.insert(guild_id.to_string(), guild.channels);
            }
        }
    }

    pub fn fetch_guild_members(&mut self, guild_id: &str) {
        if let Some(ref client) = self.client {
            if let Ok(members) = client.get_guild_members(guild_id) {
                self.cached_members.insert(guild_id.to_string(), members);
            }
        }
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
        if let Some(ref client) = self.client {
            if let Ok(msgs) = client.get_guild_messages(channel_id, 50) {
                let blocked = self.blocked.clone();
                let msgs: Vec<GuildMessage> =
                    msgs.into_iter().filter(|m| !blocked.contains(&m.author_pubkey)).collect();
                self.cached_guild_msgs.insert(channel_id.to_string(), msgs);
            }
        }
    }

    pub fn submit_guild_message(&mut self, guild_id: &str, channel_id: &str, text: &str) {
        if let Some(ref client) = self.client {
            if let Ok(msg) = client.send_guild_message(guild_id, channel_id, text, vec![], None) {
                self.cached_guild_msgs.entry(channel_id.to_string()).or_default().push(msg);
            }
        }
    }

    pub fn mark_all_notifications_read(&mut self) {
        if let Some(ref client) = self.client {
            let _ = client.mark_notifications_read(&[], true);
        }
        for n in self.notifications.iter_mut() {
            n.read = true;
        }
    }

    pub fn execute_search(&mut self) {
        let q = self.search_query.trim().to_string();
        if let Some(ref client) = self.client {
            if let Ok(res) = client.search(&q) {
                self.search_results = res;
            }
        }
    }

    pub fn follow_user(&mut self, pubkey: &str) {
        if let Some(ref client) = self.client {
            if client.follow_user(pubkey).is_ok() {
                self.following.insert(pubkey.to_string());
            }
        }
    }

    pub fn fetch_social(&mut self) {
        if let Some(ref client) = self.client {
            if let Ok((following, _followers, blocked)) = client.get_social_lists() {
                self.following = following.into_iter().collect();
                self.blocked = blocked.into_iter().collect();
            }
        }
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
        if let Some(ref client) = self.client {
            if self.following.contains(pubkey) {
                if client.unfollow_user(pubkey).is_ok() {
                    self.following.remove(pubkey);
                }
            } else if client.follow_user(pubkey).is_ok() {
                self.following.insert(pubkey.to_string());
            }
        }
    }

    pub fn toggle_block(&mut self, pubkey: &str) {
        if let Some(ref client) = self.client {
            let now_blocked = !self.blocked.contains(pubkey);
            if client.block_user(pubkey, now_blocked).is_ok() {
                if now_blocked {
                    self.blocked.insert(pubkey.to_string());
                    self.following.remove(pubkey);
                    self.apply_block_filter();
                } else {
                    self.blocked.remove(pubkey);
                }
            }
        }
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
        if let Some(ref client) = self.client {
            if let Ok(reps) = client.get_reports() {
                self.reports = reps;
            }
        }
    }

    pub fn fetch_audit_logs(&mut self) {
        if let Some(ref client) = self.client {
            if let Ok(logs) = client.get_audit_log() {
                self.audit_logs = logs;
            }
        }
    }

    pub fn submit_report(&mut self, draft: &ReportDraft) {
        if let Some(ref client) = self.client {
            let _ = client.report(draft.target_type, &draft.target_id, &draft.reason, Some(&draft.details));
            self.mod_status_msg = Some(("Report submitted to moderation queue.".into(), Instant::now()));
        }
    }

    pub fn resolve_report(&mut self, report_id: &str, status: &str) {
        self.reports.retain(|r| r.id != report_id);
        self.mod_status_msg = Some((format!("Report marked as {}.", status), Instant::now()));
    }

    pub fn execute_global_ban(&mut self, target: &str, reason: &str) {
        if let Some(ref client) = self.client {
            let _ = client.moderate(target, crate::community::types::ModActionType::Ban, reason, None, None);
            self.mod_status_msg = Some((format!("User {} banned successfully.", target), Instant::now()));
        }
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
