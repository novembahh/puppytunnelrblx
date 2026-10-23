use crate::boombox::{ensure_media_local, BoomboxServer};
use crate::bridge::BridgeServer;
use crate::config::{
    copy_text, gen_guid, load_config, save_config, split_host_port, ts, write_session_log,
    AppConfig, PROXY_PORT,
};
use crate::identity::Identity;
use crate::master::{self, Announce, RemoteServer};
use crate::net::relay::{make_invite, parse_invite};
use crate::net::{
    run_connectivity_test, run_echo_test, start_proxy, start_proxy_relay, udp_port_in_use, warm_tunnel, EchoServer,
    ProxyHandle,
};
use crate::privacy::tunnel_addr_warnings;
use crate::roblox::{
    get_studio_path, inject_map, install_studio_plugin, launch_client, launch_server,
    watch_media_requests, VINEGAR,
};
use crate::ui::icons::Icon;
use crate::ui::theme::*;
use crate::profile;
use crate::rbxapi::{self, RobloxUser};

use crate::ui::picker::{native_pick, PickResult, PickTarget, Picker};
use crate::ui::widgets::*;
use eframe::egui::{
    self, pos2, Align, Color32, FontId, LayerId, Layout, Margin, Rect, RichText, Rounding, ScrollArea, Sense, Vec2,
};
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};


pub const CONTENT: f32 = 1040.0;

pub const MAX_LOG_ENTRIES: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Screen {
    Host,
    HostRunning,
    Join,
    JoinRunning,
    Browse,
    Community,
    Profile,
    Tools,
    About,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Section {
    Play,
    Community,
    Profile,
    Tools,
    About,
}

impl Section {
    const ALL: [Section; 5] = [Section::Play, Section::Community, Section::Profile, Section::Tools, Section::About];

    fn label(self) -> &'static str {
        match self {
            Section::Play => "session",
            Section::Community => "community",
            Section::Profile => "profile",
            Section::Tools => "tools",
            Section::About => "about",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Section::Play => Icon::Play,
            Section::Community => Icon::Community,
            Section::Profile => Icon::Profile,
            Section::Tools => Icon::Tools,
            Section::About => Icon::About,
        }
    }
}

fn section_of(screen: Screen) -> Section {
    match screen {
        Screen::Host | Screen::HostRunning | Screen::Join | Screen::JoinRunning | Screen::Browse => Section::Play,
        Screen::Community => Section::Community,
        Screen::Profile => Section::Profile,
        Screen::Tools => Section::Tools,
        Screen::About => Section::About,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ProfileTab {
    Account,
    Public,
    Keys,
}

#[derive(Clone, Copy, PartialEq)]
enum ToolTab {
    Echo,
    Models,
}

#[derive(Default)]
struct ProfileState {

    server: Option<crate::profile::PublicProfile>,
    server_fresh: bool,
    loading: bool,
    error: String,
    fresh: bool,
    user: Option<RobloxUser>,

    render: Option<Vec<u8>>,
    headshot: Option<Vec<u8>>,

    verified: Option<bool>,
    publishing: bool,
    publish_msg: String,
    publish_ok: Option<bool>,
}

#[derive(Default)]
struct BrowserState {
    servers: Arc<Vec<RemoteServer>>,
    loading: bool,
    error: String,
}

struct LogEntry {
    time: String,
    message: String,
    tag: String,
}

struct CryptoIcons {
    ltc: egui::TextureHandle,
    xrp: egui::TextureHandle,
    doge: egui::TextureHandle,
    sol: egui::TextureHandle,
    eth: egui::TextureHandle,
    btc: egui::TextureHandle,
    usdc: egui::TextureHandle,
}

pub struct NepApp {
    config: AppConfig,
    screen: Screen,
    tool_tab: ToolTab,
    profile_tab: ProfileTab,
    status_text: String,

    bridge_server: BridgeServer,
    boombox_server: BoomboxServer,
    echo_server: EchoServer,
    proxy_handle: Option<ProxyHandle>,
    join_client: Option<std::sync::Arc<std::sync::Mutex<std::process::Child>>>,
    join_client_dead: std::sync::Arc<std::sync::atomic::AtomicBool>,

    logs: Arc<Mutex<Vec<LogEntry>>>,
    stop_flag: Arc<AtomicBool>,

    server_process: Option<Child>,
    server_start_time: Option<Instant>,
    server_live: bool,
    host_media: bool,
    join_media: bool,

    agent_stop: Arc<AtomicBool>,
    agent_thread: Option<thread::JoinHandle<()>>,
    invite: Option<String>,
    raw_join: Option<String>,
    game_port: Arc<AtomicUsize>,
    game_port_poll: Option<Instant>,
    game_token: crate::net::relay::Token,
    game_zone: Arc<Mutex<Option<String>>>,
    players: Arc<AtomicUsize>,
    announcer: Option<thread::JoinHandle<()>>,
    announce_ready: Arc<AtomicBool>,

    browser: Arc<Mutex<BrowserState>>,
    browser_query: String,
    browser_sort: String,
    browser_dirty: bool,
    browser_edit_masters: bool,

    join_echo_running: Arc<AtomicBool>,
    profile_state: Arc<Mutex<ProfileState>>,
    roblox_query: String,

    avatar_tex: Option<egui::TextureHandle>,
    headshot: Option<egui::TextureHandle>,
    profile_loaded: bool,
    profile_synced: bool,
    logo: Option<egui::TextureHandle>,
    picker: Option<Picker>,
    picker_request: Option<PickTarget>,
    model_status: Option<(bool, String)>,
    discord_icon: Option<egui::TextureHandle>,
    session_icon: Option<egui::TextureHandle>,
    signal_icon: Option<egui::TextureHandle>,
    copied_target: Option<(String, Instant)>,
    crypto_icons: Option<CryptoIcons>,
    throne_icon: Option<egui::TextureHandle>,
    update_manager: crate::update::UpdateManager,
    community_state: crate::ui::community::CommunityUiState,
    shot_at: Option<Instant>,
}

impl NepApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_theme(&cc.egui_ctx);

        let mut config = load_config();
        if config.studio.is_empty() {
            let detected = get_studio_path();
            if !detected.is_empty() {
                config.studio = detected;
                save_config(&config);
            }
        }

        let mut bridge = BridgeServer::new();
        let _ = bridge.start();
        let _ = install_studio_plugin();

        let status_text = if config.studio == VINEGAR {
            "roblox studio via vinegar (flatpak)".to_string()
        } else if !config.studio.is_empty() {
            format!("roblox studio: {}", config.studio)
        } else {
            "roblox studio not found. set its path on the host page.".to_string()
        };

        let update_manager = crate::update::UpdateManager::new();
        if config.auto_update {
            use std::str::FromStr;
            let ch = crate::update::Channel::from_str(&config.update_channel)
                .unwrap_or(crate::update::Channel::Beta);
            let source = crate::update::UpdateSource::from_config(
                &config.update_source_type,
                &config.update_source_url,
            );
            update_manager.check_for_updates(env!("CARGO_PKG_VERSION"), ch, source);
        }

        #[cfg(debug_assertions)]
        let (start_screen, start_profile_tab, start_tool_tab) = preview_target();
        #[cfg(not(debug_assertions))]
        let (start_screen, start_profile_tab, start_tool_tab) = (Screen::Host, ProfileTab::Account, ToolTab::Echo);

        let logs = Arc::new(Mutex::new(Vec::new()));
        Self {
            config,
            screen: start_screen,
            tool_tab: start_tool_tab,
            profile_tab: start_profile_tab,
            status_text,

            bridge_server: bridge,
            boombox_server: BoomboxServer::new(),
            echo_server: EchoServer::new(),
            proxy_handle: None,
            join_client: None,
            join_client_dead: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),

            logs,
            stop_flag: Arc::new(AtomicBool::new(false)),

            server_process: None,
            server_start_time: None,
            server_live: false,
            host_media: false,
            join_media: false,

            agent_stop: Arc::new(AtomicBool::new(false)),
            agent_thread: None,
            invite: None,
            raw_join: None,
            game_port: Arc::new(AtomicUsize::new(0)),
            game_port_poll: None,
            game_token: [0u8; 16],
            game_zone: Arc::new(Mutex::new(None)),
            players: Arc::new(AtomicUsize::new(0)),
            announcer: None,
            announce_ready: Arc::new(AtomicBool::new(cfg!(not(target_os = "linux")))),

            browser: Arc::new(Mutex::new(BrowserState::default())),
            browser_query: String::new(),
            browser_sort: "players".to_string(),
            browser_dirty: matches!(start_screen, Screen::Browse),
            browser_edit_masters: false,

            join_echo_running: Arc::new(AtomicBool::new(false)),
            profile_state: Arc::new(Mutex::new(ProfileState::default())),
            roblox_query: String::new(),

            avatar_tex: None,
            headshot: None,
            profile_loaded: false,
            profile_synced: false,
            logo: load_rgba_texture(&cc.egui_ctx, "logo", include_bytes!("../../assets/icon/puppytunnel-64.png")),
            picker: None,
            picker_request: None,
            model_status: None,
            discord_icon: None,
            session_icon: None,
            signal_icon: None,
            copied_target: None,
            crypto_icons: None,
            throne_icon: None,
            update_manager,
            community_state: crate::ui::community::CommunityUiState::default(),
            shot_at: None,
        }
    }

    fn add_log(&self, message: &str, tag: &str) {
        self.log_sink()(message, tag);
    }

    fn log_sink(&self) -> impl Fn(&str, &str) + Send + Sync + 'static {
        let logs = Arc::clone(&self.logs);
        move |msg: &str, tag: &str| {
            if let Ok(mut l) = logs.lock() {
                l.push(LogEntry { time: ts(), message: msg.to_string(), tag: tag.to_string() });
                if l.len() > MAX_LOG_ENTRIES {
                    let drop = l.len() - MAX_LOG_ENTRIES;
                    l.drain(..drop);
                }
            }
        }
    }

    fn clear_logs(&self) {
        if let Ok(mut l) = self.logs.lock() {
            l.clear();
        }
    }

    fn stop_host_services(&mut self) {
        if let Some(mut child) = self.server_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.agent_stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.agent_thread.take() {
            let _ = t.join();
        }
        if let Some(t) = self.announcer.take() {
            let _ = t.join();
        }
        self.invite = None;
        self.raw_join = None;
        self.server_live = false;
        self.boombox_server.stop();
    }

    fn stop_join_services(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(mut p) = self.proxy_handle.take() {
            p.stop();
        }
        if let Some(client) = self.join_client.take() {
            if let Ok(mut c) = client.lock() {
                let _ = c.kill();
            }
        }
        self.join_client_dead.store(false, Ordering::SeqCst);
        self.boombox_server.stop();
    }

    fn hosting(&self) -> bool {
        self.server_process.is_some()
    }

    fn poll_server_process(&mut self) {
        let exited = match self.server_process.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => Some(format!("Studio server exited ({}). Relay stopped.", status)),
                Err(e) => Some(format!("Studio server error ({}). Relay stopped.", e)),
                Ok(None) => None,
            },
            None => None,
        };
        if let Some(msg) = exited {
            self.add_log(&msg, "err");
            self.stop_host_services();
            self.screen = Screen::Host;
        }
    }

    fn joined(&self) -> bool {
        self.proxy_handle.is_some()
    }

    fn needs_frame(&self) -> Option<Duration> {
        let mut next: Option<Duration> = None;
        let mut want = |d: Duration| {
            next = Some(match next {
                Some(n) if n < d => n,
                _ => d,
            });
        };
        if let Some(start) = self.server_start_time {
            if !self.server_live && self.hosting() {
                want(Duration::from_secs(5).saturating_sub(start.elapsed()));
            }
        }
        if let Some(t) = self.game_port_poll {
            let zone_pending = self.game_zone.lock().map(|z| z.is_none()).unwrap_or(false);
            if self.game_port.load(Ordering::Relaxed) != 0 && zone_pending && t.elapsed() < Duration::from_secs(12) {
                want(Duration::from_millis(150));
            } else {
                want(Duration::from_secs(12).saturating_sub(t.elapsed()));
            }
        }
        if self.join_echo_running.load(Ordering::SeqCst) {
            want(Duration::from_millis(500));
        }
        if let Some((_, at)) = &self.copied_target {
            if at.elapsed() < Duration::from_secs(2) {
                want(Duration::from_secs(2).saturating_sub(at.elapsed()));
            }
        }
        if self.update_manager.is_busy() {
            want(Duration::from_millis(250));
        }
        if self.browser.lock().map_or(false, |s| s.loading) {
            want(Duration::from_millis(500));
        }
        if self.profile_state.lock().map_or(false, |s| s.loading || s.publishing) {
            want(Duration::from_millis(500));
        }
        if self.screen == Screen::Community {
            want(Duration::from_secs(3).saturating_sub(self.community_state.last_sync.elapsed()));
        }
        next.map(|d| d.max(Duration::from_millis(16)))
    }
}

impl Drop for NepApp {
    fn drop(&mut self) {
        self.stop_host_services();
        self.stop_join_services();
    }
}

impl eframe::App for NepApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let trace_start = Instant::now();
        if let Some(every) = self.needs_frame() {
            ctx.request_repaint_after(every);
        }
        handle_resize(ctx);
        let screen = ctx.screen_rect();
        let radius = if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) { 0.0 } else { R_WINDOW };
        ctx.layer_painter(LayerId::background())
            .rect_filled(screen.shrink(0.5), Rounding::same(radius), INK);

        if !self.profile_loaded {
            self.profile_loaded = true;
            if self.config.roblox_id != 0 {
                self.roblox_query = self.config.roblox_id.to_string();
                self.fetch_roblox(self.roblox_query.clone());
            }
        }
        if !self.profile_synced && self.config.roblox_id != 0 {
            self.profile_synced = true;
            let config = self.config.clone();
            let state = Arc::clone(&self.profile_state);
            thread::spawn(move || {
                let pk = crate::identity::hex(&profile::profile_identity(&config).public());
                let found = profile::publish_targets(&config).iter().find_map(|t| profile::status(t, &pk).ok());
                if let (Some(p), Ok(mut st)) = (found, state.lock()) {
                    st.server = Some(p);
                    st.server_fresh = true;
                }
            });
        }
        self.apply_profile_results(ctx);

        if self.join_client_dead.load(Ordering::SeqCst) && self.proxy_handle.is_some() {
            self.add_log("Studio closed, disconnected.", "warn");
            self.stop_join_services();
            self.screen = Screen::Join;
        }
        self.poll_server_process();
        self.poll_game_port();

        self.community_state.local_headshot = self.headshot.clone();

        let w = ctx.screen_rect().width();
        let narrow = w < 720.0;
        let rail = if narrow { 48.0 } else { 60.0 };
        egui::TopBottomPanel::top("titlebar")
            .exact_height(40.0)
            .frame(
                egui::Frame::none()
                    .fill(Color32::TRANSPARENT)
                    .inner_margin(Margin { left: 18.0, right: 10.0, top: 0.0, bottom: 0.0 }),
            )
            .show(ctx, |ui| self.render_titlebar(ui, ctx));

        egui::SidePanel::left("rail")
            .exact_width(rail)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(Color32::TRANSPARENT)
                    .inner_margin(Margin {
                        left: 4.0,
                        right: 4.0,
                        top: 8.0,
                        bottom: 16.0,
                    }),
            )
            .show(ctx, |ui| self.render_rail(ui, narrow));

        egui::TopBottomPanel::bottom("status")
            .exact_height(22.0)
            .frame(
                egui::Frame::none()
                    .fill(Color32::TRANSPARENT)
                    .inner_margin(Margin { left: rail + 16.0, right: 18.0, top: 0.0, bottom: 0.0 }),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let (color, text) = self.status_line();
                    ui.label(RichText::new(text).size(T_META).color(color));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("v{}", crate::update::CURRENT_VERSION))
                                .size(T_META)
                                .color(tint(TX_FAINT, 0.7)),
                        );
                    });
                });
            });

        let content_w = (ctx.screen_rect().width() - rail - 48.0).clamp(320.0, CONTENT);
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::TRANSPARENT).inner_margin(Margin {
                left: 8.0,
                right: ((w - rail - content_w) / 2.0).max(16.0),
                top: 4.0,
                bottom: 10.0,
            }))
            .show(ctx, |ui| {
                ui.set_max_width(content_w);
                self.render_page(ui, ctx, narrow);
            });

        if let Some(picker) = &mut self.picker {
            match picker.show(ctx) {
                PickResult::Open => {}
                PickResult::Cancel => self.picker = None,
                PickResult::Native(target) => {
                    self.picker = None;
                    self.pick_native(target);
                }
                PickResult::Done(target, files) => {
                    self.picker = None;
                    self.apply_pick(target, files);
                }
            }
        }
        if let Ok(path) = std::env::var("PT_TRACE") {
            let ms = trace_start.elapsed().as_secs_f64() * 1000.0;
            if ms > 16.0 {
                use std::io::Write as _;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                    let _ = writeln!(f, "{:?} frame {:.1} ms", self.screen, ms);
                }
            }
        }
        if let Ok(spec) = std::env::var("PT_AUTO") {
            if self.shot_at.is_none() {
                self.shot_at = Some(Instant::now());
            }
            if self.shot_at.map_or(false, |t| t.elapsed() > Duration::from_secs(2)) {
                self.shot_at = Some(Instant::now() + Duration::from_secs(600));
                match spec.as_str() {
                    "community" => self.go_section(Section::Community),
                    "profile" => self.go_section(Section::Profile),
                    _ => {}
                }
                ctx.request_repaint();
            }
        }
        if std::env::var("PT_SHOT").is_ok() {
            if self.shot_at.is_none() {
                self.shot_at = Some(Instant::now());
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
                ctx.request_repaint();
            }
            if let Some(img) = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            }) {
                if let Ok(path) = std::env::var("PT_SHOT") {
                    let (w, h) = (img.size[0], img.size[1]);
                    let mut rgba = Vec::with_capacity(w * h * 4);
                    for px in img.pixels.iter() {
                        rgba.extend_from_slice(&[px.r(), px.g(), px.b(), px.a()]);
                    }
                    let marker = format!("{}.log", path);
                    let mut png = Vec::new();
                    let note = match image::RgbaImage::from_raw(w as u32, h as u32, rgba) {
                        Some(buf) => {
                            use image::ImageEncoder;
                            match image::codecs::png::PngEncoder::new(&mut png)
                                .write_image(buf.as_raw(), w as u32, h as u32, image::ExtendedColorType::Rgba8)
                            {
                                Ok(()) => match std::fs::write(&path, &png) {
                                    Ok(()) => format!("saved {}x{} {} bytes", w, h, png.len()),
                                    Err(e) => format!("write error: {}", e),
                                },
                                Err(e) => format!("encode error: {}", e),
                            }
                        }
                        None => format!("bad buffer {}x{}", w, h),
                    };
                    let _ = std::fs::write(marker, note);
                }
                self.shot_at = None;
            }
        }
    }
}


#[cfg(debug_assertions)]
fn preview_target() -> (Screen, ProfileTab, ToolTab) {
    let which = std::env::var("PT_PREVIEW").unwrap_or_default();
    match which.as_str() {
        "host" => (Screen::Host, ProfileTab::Account, ToolTab::Echo),
        "join" => (Screen::Join, ProfileTab::Account, ToolTab::Echo),
        "servers" => (Screen::Browse, ProfileTab::Account, ToolTab::Echo),
        "community" => (Screen::Community, ProfileTab::Account, ToolTab::Echo),
        "profile" => (Screen::Profile, ProfileTab::Account, ToolTab::Echo),
        "public" => (Screen::Profile, ProfileTab::Public, ToolTab::Echo),
        "keys" => (Screen::Profile, ProfileTab::Keys, ToolTab::Echo),
        "tools" => (Screen::Tools, ProfileTab::Account, ToolTab::Echo),
        "models" => (Screen::Tools, ProfileTab::Account, ToolTab::Models),
        "about" => (Screen::About, ProfileTab::Account, ToolTab::Echo),
        "updates" => (Screen::About, ProfileTab::Account, ToolTab::Echo),
        _ => (Screen::Host, ProfileTab::Account, ToolTab::Echo),
    }
}

fn load_rgba_texture(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Option<egui::TextureHandle> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Some(ctx.load_texture(name, color, egui::TextureOptions::LINEAR))
}

fn handle_resize(ctx: &egui::Context) {
    use egui::{CursorIcon, ResizeDirection as D, ViewportCommand};
    if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
        return;
    }
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    let r = ctx.screen_rect();
    let m = 6.0;
    let (l, rt, t, b) = (pos.x - r.left() < m, r.right() - pos.x < m, pos.y - r.top() < m, r.bottom() - pos.y < m);
    let (dir, icon) = match (l, rt, t, b) {
        (true, _, true, _) => (D::NorthWest, CursorIcon::ResizeNwSe),
        (_, true, true, _) => (D::NorthEast, CursorIcon::ResizeNeSw),
        (true, _, _, true) => (D::SouthWest, CursorIcon::ResizeNeSw),
        (_, true, _, true) => (D::SouthEast, CursorIcon::ResizeNwSe),
        (true, ..) => (D::West, CursorIcon::ResizeHorizontal),
        (_, true, ..) => (D::East, CursorIcon::ResizeHorizontal),
        (_, _, true, _) => (D::North, CursorIcon::ResizeVertical),
        (_, _, _, true) => (D::South, CursorIcon::ResizeVertical),
        _ => return,
    };
    ctx.set_cursor_icon(icon);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
    }
}

fn window_button(ui: &mut egui::Ui, kind: u8) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.hovered() {
        let fill = if kind == 2 { tint(RED, 0.7) } else { SURF_2 };
        ui.painter().rect_filled(rect.shrink(2.0), Rounding::same(R_SM), fill);
    }
    let c = rect.center();
    let col = if resp.hovered() && kind == 2 { egui::Color32::WHITE } else { TX };
    let st = egui::Stroke::new(1.25_f32, col);
    let p = ui.painter();
    match kind {
        0 => {
            p.line_segment([c + egui::vec2(-4.5, 3.0), c + egui::vec2(4.5, 3.0)], st);
        }
        1 => {
            p.rect_stroke(egui::Rect::from_center_size(c, egui::vec2(9.0, 9.0)), 1.5, st);
        }
        _ => {
            p.line_segment([c + egui::vec2(-4.0, -4.0), c + egui::vec2(4.0, 4.0)], st);
            p.line_segment([c + egui::vec2(-4.0, 4.0), c + egui::vec2(4.0, -4.0)], st);
        }
    }
    let clicked = resp.clicked();
    if clicked {
        crate::ui::sound::play_click();
    }
    clicked
}

impl NepApp {
    fn go_section(&mut self, sec: Section) {
        let target = match sec {
            Section::Play => {
                if section_of(self.screen) == Section::Play {
                    return;
                }
                if self.hosting() {
                    Screen::HostRunning
                } else if self.joined() {
                    Screen::JoinRunning
                } else {
                    Screen::Host
                }
            }
            Section::Community => Screen::Community,
            Section::Profile => Screen::Profile,
            Section::Tools => Screen::Tools,
            Section::About => {
                if section_of(self.screen) == Section::About {
                    return;
                }
                Screen::About
            }
        };
        if target == Screen::Community {
            self.community_state.open();
        }
        self.screen = target;
    }

    fn render_titlebar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let bar = ui.max_rect();
        let drag = ui.interact(bar, egui::Id::new("titlebar_drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        if drag.double_clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }

        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 9.0;
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new(logo).fit_to_exact_size(Vec2::splat(17.0)));
            }
            ui.label(RichText::new("puppytunnel").size(T_META).color(TX_DIM));

            if let Some((label, color)) = self.page_status() {
                ui.add_space(12.0);
                ui.label(RichText::new(label).size(T_META).color(color));
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if window_button(ui, 2) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if window_button(ui, 1) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
                if window_button(ui, 0) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
            });
        });
    }

    fn render_rail(&mut self, ui: &mut egui::Ui, compact: bool) {
        let current = section_of(self.screen);
        let unread = self.community_state.unread_count();
        let has_update = self.update_manager.has_release();
        let running = self.hosting() || self.joined();
        let _ = compact;
        ui.add_space(4.0);
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            for sec in Section::ALL {
                let badge = match sec {
                    Section::Community => unread > 0,
                    Section::Play => running,
                    Section::About => has_update,
                    _ => false,
                };
                let resp = rail_item(ui, sec.icon(), current == sec, badge);
                if resp.on_hover_text(sec.label()).clicked() {
                    self.go_section(sec);
                }
            }
        });
    }

    fn page_status(&self) -> Option<(String, Color32)> {
        match self.screen {
            Screen::HostRunning => Some(if self.server_live {
                (format!("live · {} connected", self.players.load(Ordering::Relaxed)), BRUISE_LIT)
            } else {
                ("starting".to_string(), TX_DIM)
            }),
            Screen::JoinRunning => {
                let running = self.proxy_handle.as_ref().map_or(false, |p| p.is_running());
                Some(if running {
                    ("connected".to_string(), BRUISE_LIT)
                } else {
                    ("stopped".to_string(), RED_LIT)
                })
            }
            Screen::Browse => {
                let st = self.browser.lock().ok()?;
                if st.loading {
                    Some(("refreshing".to_string(), TX_DIM))
                } else if st.error.is_empty() {
                    Some((format!("{} online", st.servers.len()), BRUISE_LIT))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn status_line(&self) -> (Color32, String) {
        if let Some((label, color)) = self.page_status() {
            return (color, label);
        }
        if self.hosting() {
            return (BRUISE_LIT, format!("hosting · {} connected", self.players.load(Ordering::Relaxed)));
        }
        if self.joined() {
            return (BRUISE_LIT, "connected".to_string());
        }
        (TX_FAINT, self.status_text.clone())
    }

    fn page_title(&self) -> &'static str {
        match self.screen {
            Screen::Host | Screen::HostRunning => "host",
            Screen::Join | Screen::JoinRunning => "join",
            Screen::Browse => "servers",
            Screen::Community => "community",
            Screen::Profile => "profile",
            Screen::Tools => "tools",
            Screen::About => "about",
        }
    }

    fn render_header(&mut self, ui: &mut egui::Ui) {
        let status = self.page_status();
        let status = status.as_ref().map(|(s, c)| (s.as_str(), *c));
        let heading = self.page_title();
        match section_of(self.screen) {
            Section::Play => {
                let tabs = [Tab::new("host"), Tab::new("join"), Tab::new("servers")];
                let active = match self.screen {
                    Screen::Host | Screen::HostRunning => 0,
                    Screen::Join | Screen::JoinRunning => 1,
                    _ => 2,
                };
                if let Some(i) = page_tabs_status(ui, heading, status, &tabs, active) {
                    self.screen = match i {
                        0 if self.hosting() => Screen::HostRunning,
                        0 => Screen::Host,
                        1 if self.joined() => Screen::JoinRunning,
                        1 => Screen::Join,
                        _ => {
                            self.browser_dirty = true;
                            Screen::Browse
                        }
                    };
                }
            }
            Section::Community => {
                let unread = self.community_state.unread_count();
                let pending = self.community_state.pending_reports();
                let is_owner = self.config.roblox_id == 2721761670;
                let mut order = vec![
                    crate::ui::community::CommunityTab::Feed,
                    crate::ui::community::CommunityTab::Messages,
                    crate::ui::community::CommunityTab::Notifications,
                    crate::ui::community::CommunityTab::Search,
                ];
                let mut tabs = vec![
                    Tab::new("feed"),
                    Tab::new("messages"),
                    Tab::new("alerts").badge(unread),
                    Tab::new("search"),
                ];
                if is_owner {
                    order.push(crate::ui::community::CommunityTab::Moderation);
                    tabs.push(Tab::new("moderation").badge(pending));
                }
                let active = order.iter().position(|t| *t == self.community_state.active_tab).unwrap_or(0);
                if let Some(i) = page_tabs_status(ui, heading, status, &tabs, active) {
                    if let Some(t) = order.get(i).copied() {
                        self.community_state.select_tab(t);
                    }
                }
            }
            Section::Profile => {
                let tabs = [Tab::new("account"), Tab::new("public"), Tab::new("keys")];
                let active = match self.profile_tab {
                    ProfileTab::Account => 0,
                    ProfileTab::Public => 1,
                    ProfileTab::Keys => 2,
                };
                if let Some(i) = page_tabs_status(ui, heading, status, &tabs, active) {
                    self.profile_tab = match i {
                        0 => ProfileTab::Account,
                        1 => ProfileTab::Public,
                        _ => ProfileTab::Keys,
                    };
                }
            }
            Section::Tools => {
                let tabs = [Tab::new("echo"), Tab::new("models")];
                let active = match self.tool_tab {
                    ToolTab::Echo => 0,
                    ToolTab::Models => 1,
                };
                if let Some(i) = page_tabs_status(ui, heading, status, &tabs, active) {
                    self.tool_tab = if i == 0 { ToolTab::Echo } else { ToolTab::Models };
                }
            }
            Section::About => {
                page_tabs_status(ui, heading, status, &[], 0);
            }
        }
    }

    fn render_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, _narrow: bool) {
        self.render_header(ui);
        self.render_screen(ui, ctx);
    }

    fn render_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        match self.screen {
            Screen::Host => self.render_host(ui),
            Screen::HostRunning => self.render_host_running(ui),
            Screen::Join => self.render_join(ui),
            Screen::JoinRunning => self.render_join_running(ui),
            Screen::Browse => self.render_browse(ui),
            Screen::Community => self.community_state.render(ui, &mut self.config, ctx),
            Screen::Profile => self.render_profile_section(ui, ctx),
            Screen::Tools => self.render_tools(ui),
            Screen::About => self.render_about(ui),
        }
    }
    fn render_profile_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.community_state.local_headshot = self.headshot.clone();
        match self.profile_tab {
            ProfileTab::Account => self.render_profile(ui),
            ProfileTab::Public => self.community_state.render_public_profile(ui, &mut self.config, ctx),
            ProfileTab::Keys => self.community_state.render_keys_page(ui, &mut self.config),
        }
    }

    fn picker_spec(&self, target: PickTarget) -> (&'static str, String, &'static [&'static str], bool) {
        match target {
            PickTarget::Map => ("Choose a place file", self.config.map.clone(), &["rbxl", "rbxlx"], false),
            PickTarget::Studio => (
                "Locate Roblox Studio",
                self.config.studio.clone(),
                if cfg!(windows) { &["exe"] } else { &[] },
                false,
            ),
            PickTarget::Models => ("Add models", String::new(), &["rbxm", "rbxmx"], true),
        }
    }

    fn pick_native(&mut self, target: PickTarget) {
        let (title, current, exts, multi) = self.picker_spec(target);
        let files = native_pick(target, title, &current, exts, multi);
        if !files.is_empty() {
            self.apply_pick(target, files);
        }
    }

    fn open_picker(&mut self, target: PickTarget) {
        if self.config.native_dialogs {
            self.pick_native(target);
            return;
        }
        self.picker = Some(match target {
            PickTarget::Map => Picker::new(target, "Choose a place file", &self.config.map, &["rbxl", "rbxlx"], false),
            PickTarget::Studio => Picker::new(
                target,
                "Locate Roblox Studio",
                &self.config.studio,
                if cfg!(windows) { &["exe"] } else { &[] },
                false,
            ),
            PickTarget::Models => Picker::new(target, "Add models", "", &["rbxm", "rbxmx"], true),
        });
    }

    fn apply_pick(&mut self, target: PickTarget, files: Vec<PathBuf>) {
        match target {
            PickTarget::Map => {
                if let Some(f) = files.first() {
                    self.config.map = f.to_string_lossy().to_string();
                }
            }
            PickTarget::Studio => {
                if let Some(f) = files.first() {
                    self.config.studio = f.to_string_lossy().to_string();
                }
            }
            PickTarget::Models => {
                for f in files {
                    let s = f.to_string_lossy().to_string();
                    if !self.config.saved_maps.contains(&s) {
                        self.config.saved_maps.push(s);
                    }
                }
            }
        }
        save_config(&self.config);
    }


    fn render_host(&mut self, ui: &mut egui::Ui) {
        ScrollArea::vertical().id_salt("app_scroll_1").auto_shrink([false, false]).show(ui, |ui| {
            let mut start = false;
            panel(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        start = primary(ui, "start", true);
                    });
                });
                ui.add_space(10.0);
                key_row(ui, "place", |ui| {
                    let chosen = file_label(&self.config.map);
                    let has = !self.config.map.is_empty();
                    ui.label(RichText::new(if has { chosen } else { "no place".to_string() }).size(T_BODY).color(if has { TX } else { TX_FAINT }));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if quiet(ui, if has { "change" } else { "choose" }) {
                            self.picker_request = Some(PickTarget::Map);
                        }
                    });
                });
                key_row(ui, "port", |ui| {
                    text_input(ui, egui::Id::new("host_port"), &mut self.config.port, "55555", 100.0);
                });
                key_row(ui, "relay", |ui| {
                    text_input(ui, egui::Id::new("host_relay"), &mut self.config.relay, "relay.example.com:7777", f32::INFINITY);
                });
                key_row(ui, "studio", |ui| {
                    let path = if self.config.studio.is_empty() { "auto-detect".to_string() } else { file_label(&self.config.studio) };
                    ui.label(RichText::new(path).size(T_BODY).color(if self.config.studio.is_empty() { TX_FAINT } else { TX }));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if quiet(ui, "locate") {
                            self.picker_request = Some(PickTarget::Studio);
                        }
                    });
                });

                group(ui, "listing");
                toggle_row(ui, &mut self.config.list_public, "list publicly");
                if self.config.list_public {
                    key_row(ui, "name", |ui| {
                        text_input(ui, egui::Id::new("host_name"), &mut self.config.server_name, "my server", f32::INFINITY);
                    });
                    key_row(ui, "tags", |ui| {
                        text_input(ui, egui::Id::new("host_tags"), &mut self.config.server_tags, "obby, pvp", f32::INFINITY);
                    });
                }
                toggle_row(ui, &mut self.host_media, "media helper");
                toggle_row(ui, &mut self.config.nep_compat, "neptunnel players");
                if self.config.nep_compat {
                    key_row(ui, "zone", |ui| {
                        text_input(ui, egui::Id::new("host_zone"), &mut self.config.game_host, "auto from master", f32::INFINITY);
                    });
                }
                if self.config.relay.trim().is_empty() {
                    ui.add_space(16.0);
                    for w in tunnel_addr_warnings(&self.config.addr) {
                        ui.label(RichText::new(w).size(T_META).color(AMBER));
                    }
                }
            });

            if start {
                save_config(&self.config);
                self.start_host();
            }
        });
        if let Some(t) = self.picker_request.take() {
            self.open_picker(t);
        }
    }

    fn start_host(&mut self) {
        self.stop_host_services();
        self.clear_logs();
        self.server_live = false;
        self.server_start_time = Some(Instant::now());

        let studio_port: u16 = match self.config.port.trim().parse() {
            Ok(p) => p,
            Err(_) => {
                self.add_log("Studio port is not a valid port number.", "err");
                self.status_text = "Studio port is not a valid port number.".to_string();
                return;
            }
        };
        if udp_port_in_use(studio_port) {
            self.add_log(&format!("Port {} is already in use (another server or app instance?). Stop it or change Studio port.", studio_port), "err");
            self.status_text = format!("Port {} is already in use.", studio_port);
            return;
        }

        let pg = gen_guid();
        let tg = gen_guid();
        self.add_log(&format!("Port {}", self.config.port), "dim");

        if self.host_media {
            match self.boombox_server.start() {
                Ok(()) => self.add_log("Media helper running on 127.0.0.1:48732", "info"),
                Err(e) => self.add_log(&format!("Media helper unavailable ({}). Another instance may hold port 48732.", e), "warn"),
            }
        }

        if !self.config.map.trim().is_empty() {
            let map_path = PathBuf::from(&self.config.map);
            match inject_map(&map_path) {
                Ok(_) => self.add_log(&format!("Map copied: {}", map_path.display()), "ok"),
                Err(e) => self.add_log(&format!("Map not copied: {}", e), "err"),
            }
        }

        match launch_server(&self.config.studio, &self.config.port, &self.config.uid, &pg, &tg) {
            Ok(child) => {
                self.server_process = Some(child);
                self.add_log("Studio server launched. Waiting for it to initialise.", "info");
                let _ = write_session_log(&pg, &tg, &self.config.addr, &self.config.port, &self.config.uid);
                self.add_log(&format!("Session log written to {}", crate::config::session_log_name(&self.config.port)), "dim");
                self.start_relay_agent();
                if self.invite.is_none() {
                    self.add_log("Join command copied to the clipboard.", "dim");
                }
            }
            Err(e) => self.add_log(&format!("Could not launch Studio: {}", e), "err"),
        }
        self.screen = Screen::HostRunning;
    }

    fn start_relay_agent(&mut self) {
        let relay = self.config.relay.trim().to_string();
        if relay.is_empty() {
            return;
        }
        let studio_port: u16 = self.config.port.trim().parse().unwrap_or(55555);
        let addr = relay.to_socket_addrs().ok().and_then(|mut a| a.find(|x| x.is_ipv4()));
        let Some(addr) = addr else {
            self.add_log(&format!("Cannot resolve relay {}", relay), "err");
            return;
        };
        let id = if self.config.list_public {
            Identity::stable(&self.config.server_secret, studio_port)
        } else {
            Identity::ephemeral()
        };
        let invite = make_invite(&relay, &id.token());
        self.agent_stop.store(false, Ordering::SeqCst);
        let stop = Arc::clone(&self.agent_stop);
        let sink = self.log_sink();
        let players = Arc::clone(&self.players);
        let agent_id = id.clone();
        let open = self.config.nep_compat;
        self.game_port.store(0, Ordering::Relaxed);
        self.game_token = id.token();
        let slot = Arc::clone(&self.game_port);
        let zone_slot = Arc::clone(&self.game_zone);
        if let Ok(mut z) = zone_slot.lock() {
            *z = None;
        }
        let manual_zone = self.config.game_host.trim().to_string();
        let masters = master::master_list(&self.config.master);
        let label = crate::net::relay::session_label(&id.token());
        thread::spawn(move || {
            let zone = if manual_zone.is_empty() {
                master::fetch_game_domain(&masters, &label)
                    .map(|d| d.zone)
                    .unwrap_or_else(|| crate::net::relay::DEFAULT_GAME_ZONE.to_string())
            } else {
                manual_zone
            };
            if let Ok(mut z) = zone_slot.lock() {
                *z = Some(zone);
            }
        });
        self.agent_thread = Some(thread::spawn(move || {
            let res = crate::net::relay::run_host_agent_slot(
                addr,
                agent_id,
                studio_port,
                stop,
                players,
                open,
                Some(slot),
                &sink,
            );
            if let Err(e) = res {
                sink(&format!("Relay stopped: {}", e), "err");
            }
        }));
        if self.config.nep_compat {
            self.game_port_poll = Some(Instant::now());
            self.add_log("Waiting for the relay to hand out a game port.", "dim");
        }
        if self.config.list_public {
            let name = if self.config.server_name.trim().is_empty() {
                format!("{}'s server", self.config.uid)
            } else {
                self.config.server_name.clone()
            };
            let (host, host_sig) = crate::profile::host_binding(&self.config, &id);
            let listing = Announce {
                name,
                description: self.config.server_desc.clone(),
                map: Path::new(&self.config.map)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default(),
                tags: master::parse_tags(&self.config.server_tags),
                max_players: self.config.max_players,
                invite: invite.clone(),
                version: "beta 1.0".to_string(),
                console_policy: if self.config.console_allowed.trim().is_empty() { None } else { Some(self.config.console_allowed.trim().to_string()) },
                overhead_tags: if self.config.overhead_tags { None } else { Some(false) },
                strict_profiles: if self.config.strict_profiles { Some(true) } else { None },
                nep: if self.config.nep_compat { Some(true) } else { None },
                host,
                host_sig,
                game: None,
                ts: 0,
            };
            self.announcer = Some(master::spawn_announcer(
                master::master_list(&self.config.master),
                id,
                listing,
                Arc::clone(&self.agent_stop),
                Arc::clone(&self.announce_ready),
                None,
                Some(Arc::clone(&self.game_port)),
                self.log_sink(),
            ));
        }
        copy_text(&invite);
        self.add_log("Invite copied to the clipboard.", "ok");
        self.invite = Some(invite);
    }

    fn poll_game_port(&mut self) {
        if self.game_port_poll.is_none() {
            return;
        }
        if self.raw_join.is_some() {
            self.game_port_poll = None;
            return;
        }
        let port = self.game_port.load(Ordering::Relaxed);
        let mut zone = self
            .game_zone
            .lock()
            .ok()
            .and_then(|z| z.clone())
            .unwrap_or_else(|| self.config.game_host.trim().to_string());
        let expired = self.game_port_poll.map_or(false, |t| t.elapsed() > Duration::from_secs(12));
        if zone.is_empty() && port != 0 && !expired {
            return;
        }
        if zone.is_empty() && port != 0 {
            zone = crate::net::relay::DEFAULT_GAME_ZONE.to_string();
        }
        if port == 0 {
            if expired {
                self.game_port_poll = None;
                self.add_log("Relay did not assign a game port; plain Neptunnel players cannot join.", "warn");
            }
            return;
        }
        self.game_port_poll = None;
        let relay = self.config.relay.trim().to_string();
        let addr = crate::net::relay::game_address(&zone, &self.game_token, port as u16);
        let shown = if addr.is_empty() { relay } else { addr };
        self.add_log(&format!("Neptunnel players join with {}", shown), "ok");
        self.raw_join = Some(shown);
    }

    fn render_host_running(&mut self, ui: &mut egui::Ui) {
        if let Some(start) = self.server_start_time {
            if start.elapsed() >= Duration::from_secs(5) && !self.server_live && self.hosting() {
                self.server_live = true;
                self.announce_ready.store(true, Ordering::SeqCst);
                self.add_log("Server is live.", "ok");
            }
        }

        let mut stop = false;
        panel(ui, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    stop = danger(ui, "stop");
                });
            });
            ui.add_space(8.0);
            let invite = self.invite.clone();
            let raw = self.raw_join.clone();
            let mut copy_invite = None;
            let copied = |key: &str| {
                self.copied_target
                    .as_ref()
                    .map_or(false, |(k, t)| k == key && t.elapsed() < Duration::from_secs(2))
            };
            if let Some(inv) = &invite {
                if copy_row_state(ui, "invite", inv, true, copied("copy:invite")) {
                    copy_invite = Some(("copy:invite".to_string(), inv.clone()));
                }
            }
            if let Some(r) = &raw {
                if copy_row_state(ui, "neptunnel", r, false, copied("copy:neptunnel")) {
                    copy_invite = Some(("copy:neptunnel".to_string(), r.clone()));
                }
            }
            if invite.is_none() && raw.is_none() {
                ui.label(RichText::new("waiting for studio").size(T_META).color(TX_FAINT));
            }
            if let Some((key, v)) = copy_invite {
                copy_text(&v);
                self.copied_target = Some((key, Instant::now()));
            }

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if self.server_live && secondary(ui, "join locally") {
                    let port: u16 = self.config.port.trim().parse().unwrap_or(55555);
                    match launch_client(&self.config.studio, "127.0.0.1", port, &gen_guid(), &gen_guid(), "StudioPlayer_Host") {
                        Ok(_) => self.add_log("Local client launched.", "info"),
                        Err(e) => self.add_log(&format!("Launch failed: {}", e), "err"),
                    }
                }
                if self.host_media && secondary(ui, "http") {
                    copy_text("game:GetService(\"HttpService\").HttpEnabled = true");
                    self.add_log("Paste the command into the Studio command bar to enable HTTP.", "warn");
                }
                if secondary(ui, "test") {
                    let local_port: u16 = self.config.port.trim().parse().unwrap_or(55555);
                    let sink = self.log_sink();
                    match split_host_port(&self.config.addr) {
                        Some((host, port)) => {
                            thread::spawn(move || {
                                run_connectivity_test(&host, port, true, Some(local_port), false, sink);
                            });
                        }
                        None => self.add_log("Set a tunnel address (host:port) to run the test.", "warn"),
                    }
                }
            });
        });
        if stop {
            self.stop_host_services();
            self.screen = Screen::Host;
            return;
        }

        ui.add_space(28.0);
        self.render_log(ui);
    }


    fn render_join(&mut self, ui: &mut egui::Ui) {
        let mut connect = false;
        panel(ui, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    connect = primary(ui, "connect", true);
                });
            });
            ui.add_space(10.0);
            key_row(ui, "invite", |ui| {
                let field_w = (ui.available_width() - 4.0).max(180.0);
                let r = text_input(ui, egui::Id::new("join_addr"), &mut self.config.addr, "puppy://relay.example.com:7777/…", field_w);
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    connect = true;
                }
            });
            key_row(ui, "studio", |ui| {
                let path = if self.config.studio.is_empty() { "auto-detect".to_string() } else { file_label(&self.config.studio) };
                ui.label(RichText::new(path).size(T_BODY).color(if self.config.studio.is_empty() { TX_FAINT } else { TX }));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if quiet(ui, "locate") {
                        self.picker_request = Some(PickTarget::Studio);
                    }
                });
            });
            toggle_row(ui, &mut self.join_media, "download media");
        });
        if connect {
            save_config(&self.config);
            self.start_join();
        }
        if let Some(t) = self.picker_request.take() {
            self.open_picker(t);
        }
    }

    fn start_join(&mut self) {
        self.stop_join_services();
        self.clear_logs();
        self.stop_flag.store(false, Ordering::SeqCst);

        let target = self.config.addr.trim().to_string();
        if parse_invite(&target).is_some() {
            if let Err(e) = crate::net::relay::check_invite_live(&target) {
                self.status_text = e.clone();
                self.add_log(&format!("Join refused: {}", e), "err");
                return;
            }
        }
        let started = if let Some((rh, rp, token)) = parse_invite(&target) {
            self.add_log(&format!("Relay {}:{}", rh, rp), "info");
            start_proxy_relay(PROXY_PORT, &rh, rp, token)
        } else if let Some((host, port)) = split_host_port(&target) {
            self.add_log(&format!("Target {}:{}", host, port), "info");
            start_proxy(PROXY_PORT, &host, port)
        } else {
            self.status_text = "Enter an invite or host:port.".to_string();
            return;
        };

        let proxy_port = match started {
            Ok(handle) => {
                let p = handle.port();
                self.proxy_handle = Some(handle);
                self.add_log(&format!("Proxy listening on 127.0.0.1:{}", p), "ok");
                p
            }
            Err(e) => {
                self.add_log(&format!("Proxy failed (is port {} in use?): {}", PROXY_PORT, e), "err");
                self.screen = Screen::JoinRunning;
                return;
            }
        };

        let sent = warm_tunnel(proxy_port, 8, 0.1, None);
        self.add_log(&format!("Tunnel warmed ({} probes)", sent), "dim");

        match launch_client(&self.config.studio, "127.0.0.1", proxy_port, &gen_guid(), &gen_guid(), "StudioPlayer_Proxy") {
            Ok(child) => {
                self.add_log("Studio client launched.", "ok");
                let shared = std::sync::Arc::new(std::sync::Mutex::new(child));
                self.join_client = Some(std::sync::Arc::clone(&shared));
                self.join_client_dead.store(false, Ordering::SeqCst);
                let dead = std::sync::Arc::clone(&self.join_client_dead);
                thread::spawn(move || loop {
                    let exited = shared.lock().map(|mut c| c.try_wait()).unwrap_or(Ok(None));
                    match exited {
                        Ok(Some(_)) | Err(_) => {
                            dead.store(true, Ordering::SeqCst);
                            break;
                        }
                        Ok(None) => thread::sleep(Duration::from_secs(1)),
                    }
                });
            }
            Err(e) => self.add_log(&format!("Could not launch Studio: {}", e), "err"),
        }

        if self.join_media {
            let _ = self.boombox_server.start();
            let stop = Arc::clone(&self.stop_flag);
            let sink = self.log_sink();
            thread::spawn(move || watch_media_requests(stop, sink, ensure_media_local));
        }
        self.screen = Screen::JoinRunning;
    }

    fn render_join_running(&mut self, ui: &mut egui::Ui) {
        let running = self.proxy_handle.as_ref().map_or(false, |p| p.is_running());

        let mut disconnect = false;
        panel(ui, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    disconnect = danger(ui, "disconnect");
                });
            });
            ui.add_space(8.0);
            let inv = self.config.addr.clone();
            let mut copy = false;
            if !inv.trim().is_empty() {
                let flash = self
                    .copied_target
                    .as_ref()
                    .map_or(false, |(k, t)| k == "copy:join" && t.elapsed() < Duration::from_secs(2));
                if copy_row_state(ui, if parse_invite(&inv).is_some() { "invite" } else { "tunnel" }, &inv, true, flash) {
                    copy = true;
                }
            }
            if copy {
                copy_text(&inv);
                self.copied_target = Some(("copy:join".to_string(), Instant::now()));
            }

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if secondary(ui, "test") {
                    let sink = self.log_sink();
                    match split_host_port(&self.config.addr) {
                        Some((host, port)) => {
                            thread::spawn(move || {
                                run_connectivity_test(&host, port, false, None, running, sink);
                            });
                        }
                        None => self.add_log("Invites are checked by joining. Use the echo test for host:port tunnels.", "warn"),
                    }
                }
            });
        });
        if disconnect {
            self.stop_join_services();
            self.screen = Screen::Join;
            return;
        }
        ui.add_space(28.0);
        self.render_log(ui);
    }


    fn refresh_browser(&mut self) {
        let masters = master::master_list(&self.config.master).join(",");
        let (query, sort) = (self.browser_query.clone(), self.browser_sort.clone());
        let state = Arc::clone(&self.browser);
        if let Ok(mut st) = state.lock() {
            st.loading = true;
            st.error.clear();
        }
        thread::spawn(move || {
            let (list, errors) = master::fetch_all(&masters, &query, "", &sort);
            if let Ok(mut st) = state.lock() {
                st.loading = false;
                st.error = if list.is_empty() && !errors.is_empty() {
                    format!("Could not reach a master server. {}", errors.join("; "))
                } else {
                    String::new()
                };
                st.servers = Arc::new(list);
            }
        });
    }

    fn render_browse(&mut self, ui: &mut egui::Ui) {
        if self.browser_dirty {
            self.browser_dirty = false;
            self.refresh_browser();
        }

        let (servers, loading, error) = {
            let st = self.browser.lock().unwrap();
            (Arc::clone(&st.servers), st.loading, st.error.clone())
        };

        let mut refresh = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            let r = text_input(ui, egui::Id::new("browse_q"), &mut self.browser_query, "find a server", 300.0);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                refresh = true;
            }
            if segmented(ui, &mut self.browser_sort, &[("players", "busy"), ("name", "a–z"), ("new", "new")]) {
                refresh = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if quiet(ui, "refresh") {
                    refresh = true;
                }
                if quiet(ui, if self.browser_edit_masters { "hide masters" } else { "masters" }) {
                    self.browser_edit_masters = !self.browser_edit_masters;
                }
            });
        });
        if self.browser_edit_masters {
            let changed = text_input(ui, egui::Id::new("browse_masters"), &mut self.config.master, "roblox.puppytunnel.vip", ui.available_width()).lost_focus();
            if changed {
                refresh = true;
            }
        }
        if refresh {
            save_config(&self.config);
            self.refresh_browser();
        }
        ui.add_space(22.0);

        if !error.is_empty() {
            notice(ui, RED_LIT, &error);
            ui.add_space(12.0);
        }
        if servers.is_empty() && !loading {
            empty_state(ui, Icon::Servers, "no servers online", "");
            if quiet(ui, "host one") {
                self.screen = Screen::Host;
            }
            ui.add_space(GAP_L);
        }

        let mut join_invite = None;
        let mut copy_game = None;
        let mut copy_invite = None;
        ScrollArea::vertical().id_salt("browse_list").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for s in servers.iter() {
                let l = &s.listing;
                let row_h = 58.0;
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::hover());
                let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
                if t > 0.01 {
                    ui.painter().rect_filled(rect, Rounding::same(R_SM), tint(MILK, 0.022 * t));
                }
                let p = ui.painter();
                let left = rect.left() + 6.0;
                let text_w = (rect.width() - 260.0).max(80.0);
                let name = elide(p, &l.name, FontId::proportional(T_BODY + 1.0), text_w);
                p.text(
                    pos2(left, rect.top() + 20.0),
                    egui::Align2::LEFT_CENTER,
                    name,
                    FontId::proportional(T_BODY + 1.0),
                    TX,
                );
                let mut meta = String::new();
                if let Some(h) = &s.host {
                    meta.push_str(&h.display_name);
                }
                if !l.tags.is_empty() {
                    if !meta.is_empty() {
                        meta.push_str("  ");
                    }
                    meta.push_str(&l.tags.join("  "));
                }
                let second: String = if !l.description.trim().is_empty() {
                    l.description.clone()
                } else {
                    meta
                };
                if !second.is_empty() {
                    let second = elide(p, &second, FontId::proportional(T_META), text_w);
                    p.text(
                        pos2(left, rect.top() + 39.0),
                        egui::Align2::LEFT_CENTER,
                        second,
                        FontId::proportional(T_META),
                        TX_FAINT,
                    );
                }
                let cap = if l.max_players > 0 {
                    format!("{}/{}", l.players, l.max_players)
                } else {
                    l.players.to_string()
                };
                p.text(
                    pos2(rect.right() - 218.0, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    cap,
                    FontId::monospace(T_META),
                    if l.players > 0 { TX_DIM } else { TX_FAINT },
                );

                let game_copied = self
                    .copied_target
                    .as_ref()
                    .map_or(false, |(k, t)| k == &format!("game:{}", s.id) && t.elapsed() < Duration::from_secs(2));
                if l.game.is_some() {
                    let game_rect = Rect::from_center_size(pos2(rect.right() - 158.0, rect.center().y), Vec2::new(62.0, 24.0));
                    let g = ui.interact(game_rect, egui::Id::new(("game", &s.id)), Sense::click());
                    let gt = ui.ctx().animate_bool_with_time(g.id, g.hovered(), 0.12);
                    ui.painter().text(
                        game_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        if game_copied { "copied" } else { "copy ip" },
                        FontId::proportional(T_META),
                        if game_copied { AMBER } else { mix(TX_FAINT, BRUISE_LIT, gt.max(0.35)) },
                    );
                    if g.on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_ui(|ui| {
                            ui.set_max_width(260.0);
                            ui.label(RichText::new(l.game.clone().unwrap_or_default()).size(T_META).color(TX_DIM));
                        })
                        .clicked()
                    {
                        copy_game = Some((s.id.clone(), l.game.clone().unwrap_or_default()));
                    }
                }

                let invite_copied = self
                    .copied_target
                    .as_ref()
                    .map_or(false, |(k, t)| k == &format!("invite:{}", s.id) && t.elapsed() < Duration::from_secs(2));
                let invite_rect = Rect::from_center_size(pos2(rect.right() - 92.0, rect.center().y), Vec2::new(62.0, 24.0));
                let ir = ui.interact(invite_rect, egui::Id::new(("invite", &s.id)), Sense::click());
                let it = ui.ctx().animate_bool_with_time(ir.id, ir.hovered(), 0.12);
                ui.painter().text(
                    invite_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    if invite_copied { "copied" } else { "copy invite" },
                    FontId::proportional(T_META),
                    if invite_copied { AMBER } else { mix(TX_FAINT, BRUISE_LIT, it.max(0.35)) },
                );
                if ir.on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_ui(|ui| {
                        ui.set_max_width(260.0);
                        ui.label(RichText::new(l.invite.clone()).size(T_META).color(TX_DIM));
                    })
                    .clicked()
                {
                    copy_invite = Some((s.id.clone(), l.invite.clone()));
                }

                let join_rect = Rect::from_center_size(pos2(rect.right() - 38.0, rect.center().y), Vec2::new(44.0, 24.0));
                let j = ui.interact(join_rect, egui::Id::new(("join", &s.id)), Sense::click());
                let jt = ui.ctx().animate_bool_with_time(j.id, j.hovered(), 0.12);
                ui.painter().text(
                    join_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "join",
                    FontId::proportional(T_META),
                    mix(TX_DIM, BONE, jt),
                );
                if j.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    join_invite = Some(l.invite.clone());
                }

                ui.add_space(4.0);
            }
        });

        if let Some((id, addr)) = copy_game {
            copy_text(&addr);
            self.copied_target = Some((format!("game:{}", id), Instant::now()));
        }
        if let Some((id, inv)) = copy_invite {
            copy_text(&inv);
            self.copied_target = Some((format!("invite:{}", id), Instant::now()));
        }
        if let Some(inv) = join_invite {
            self.config.addr = inv;
            save_config(&self.config);
            self.start_join();
        }
    }


    fn fetch_roblox(&mut self, query: String) {
        let config = self.config.clone();
        let state = Arc::clone(&self.profile_state);
        if let Ok(mut st) = state.lock() {
            st.loading = true;
            st.error.clear();
        }
        thread::spawn(move || {
            let result = rbxapi::lookup(rbxapi::USERS_API, &query).map(|u| {

                let colors = rbxapi::avatar_png(u.id).ok();
                let headshot = rbxapi::headshot_png(u.id).ok();
                let pk = crate::identity::hex(&profile::profile_identity(&config).public());
                let verified = profile::publish_targets(&config)
                    .iter()
                    .find_map(|t| profile::status(t, &pk).ok())
                    .map(|p| p.verified);
                (u, colors, headshot, verified)
            });
            if let Ok(mut st) = state.lock() {
                st.loading = false;
                match result {
                    Ok((u, c, h, verified)) => {
                        st.user = Some(u);
                        st.verified = verified;

                        st.render = c;
                        st.headshot = h;
                        st.fresh = true;
                    }
                    Err(e) => st.error = e,
                }
            }
        });
    }

    fn apply_profile_results(&mut self, ctx: &egui::Context) {
        let mut st = self.profile_state.lock().unwrap();
        if !st.fresh {
            return;
        }
        st.fresh = false;
        if st.server_fresh {
            st.server_fresh = false;
            if let Some(p) = st.server.take() {

                self.config.display_name = p.profile_name.clone();
                self.config.profile_bio = p.bio.clone();
                if !p.accent.is_empty() {
                    self.config.profile_accent = p.accent.clone();
                }
                save_config(&self.config);
            }
        }
        if let Some(u) = &st.user {
            self.config.roblox_id = u.id;
            self.config.roblox_name = u.display_name.clone();
            self.config.uid = u.id.to_string();
            save_config(&self.config);
        }


        if let Some(bytes) = st.render.take() {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let mut rgba = img.to_rgba8();

                let (w, h) = (rgba.width(), rgba.height());
                let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
                for (x, y, p) in rgba.enumerate_pixels() {
                    if p[3] > 8 {
                        x0 = x0.min(x);
                        y0 = y0.min(y);
                        x1 = x1.max(x);
                        y1 = y1.max(y);
                    }
                }
                if x1 > x0 && y1 > y0 {
                    let pad = 6u32;
                    let (cx, cy) = (x0.saturating_sub(pad), y0.saturating_sub(pad));
                    let (cw, ch) = ((x1 + pad).min(w - 1) - cx + 1, (y1 + pad).min(h - 1) - cy + 1);
                    rgba = image::imageops::crop_imm(&rgba, cx, cy, cw, ch).to_image();
                }
                let size = [rgba.width() as usize, rgba.height() as usize];
                let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                self.avatar_tex = Some(ctx.load_texture("avatar", color, egui::TextureOptions::LINEAR));
            }
        }
        if let Some(bytes) = st.headshot.take() {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                self.headshot = Some(ctx.load_texture("headshot", color, egui::TextureOptions::LINEAR));
            }
        }
    }

    fn publish_profile(&mut self) {
        if self.config.roblox_id == 0 {
            if let Ok(mut st) = self.profile_state.lock() {
                st.publish_msg = "link your roblox account first".to_string();
                st.publish_ok = Some(false);
            }
            return;
        }
        save_config(&self.config);
        let config = self.config.clone();
        let state = Arc::clone(&self.profile_state);
        if let Ok(mut st) = state.lock() {
            st.publishing = true;
            st.publish_msg.clear();
            st.publish_ok = None;
        }
        thread::spawn(move || {
            let pid = profile::profile_identity(&config);
            let pk = crate::identity::hex(&pid.public());
            let signed = profile::SignedProfile::sign(&pid, profile::body_from_config(&config));
            let targets = profile::publish_targets(&config);
            let errors = profile::publish(&targets, &signed);
            thread::sleep(Duration::from_millis(1500));
            let verified = targets.iter().find_map(|t| profile::status(t, &pk).ok().filter(|p| p.verified));
            let (msg, ok) = if let Some(p) = verified {
                (format!("verified as {} (@{})", p.display_name, p.name), Some(true))
            } else if errors.len() == targets.len() {
                (format!("could not reach a profile server. {}", errors.join("; ")), Some(false))
            } else {
                ("published, not verified yet. add the code to your roblox about text, then publish again.".to_string(), None)
            };
            if let Ok(mut st) = state.lock() {
                st.publishing = false;
                st.publish_msg = msg;
                st.publish_ok = ok;
                if ok == Some(true) {
                    st.verified = Some(true);
                } else if ok.is_none() {
                    st.verified = Some(false);
                }
            }
        });
    }

    fn render_profile(&mut self, ui: &mut egui::Ui) {
        let (loading, error, user, publishing, publish_msg, publish_ok, verified) = {
            let st = self.profile_state.lock().unwrap();
            (st.loading, st.error.clone(), st.user.clone(), st.publishing, st.publish_msg.clone(), st.publish_ok, st.verified)
        };

        ScrollArea::vertical().id_salt("profile_account").auto_shrink([false, false]).show(ui, |ui| {
            panel(ui, |ui| {
            ui.horizontal_top(|ui| {
                let (tile, _) = ui.allocate_exact_size(Vec2::new(120.0, 142.0), Sense::hover());
                ui.painter().rect_filled(tile, Rounding::same(R_BLOCK), SURF_2);
                match &self.avatar_tex {
                    Some(tex) => {
                        let avail = tile.size() - Vec2::new(12.0, 12.0);
                        let ts = tex.size_vec2();
                        let scale = (avail.x / ts.x).min(avail.y / ts.y);
                        let img = egui::Rect::from_center_size(tile.center(), ts * scale);
                        egui::Image::new(tex).fit_to_exact_size(img.size()).paint_at(ui, img);
                    }
                    None => {
                        ui.painter().text(
                            tile.center(),
                            egui::Align2::CENTER_CENTER,
                            if loading { "loading" } else { "no avatar" },
                            egui::FontId::proportional(T_META),
                            TX_FAINT,
                        );
                    }
                }

                ui.add_space(24.0);
                ui.vertical(|ui| {
                    if let Some(u) = &user {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&u.display_name).size(20.0).color(BONE));
                            match verified {
                                Some(true) => ui.label(RichText::new("verified").size(T_META).color(BRUISE_LIT)),
                                Some(false) => ui.label(RichText::new("unverified").size(T_META).color(AMBER)),
                                None => ui.label(RichText::new("").size(T_META)),
                            };
                        });
                        ui.label(RichText::new(format!("@{}", u.name)).size(T_META).color(TX_FAINT));
                    } else {
                        ui.label(RichText::new("no account linked").size(T_BODY).color(TX_DIM));
                    }
                    ui.add_space(20.0);
                    key_row(ui, "account", |ui| {
                        let r = text_input(ui, egui::Id::new("roblox_query"), &mut self.roblox_query, "username or id", 190.0);
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let can = !loading && !self.roblox_query.trim().is_empty();
                        if quiet(ui, if loading { "fetching" } else { "fetch" }) || (enter && can) {
                            if can {
                                self.fetch_roblox(self.roblox_query.clone());
                            }
                        }
                    });
                    if !error.is_empty() {
                        ui.label(RichText::new(lc(&error)).size(T_META).color(RED_LIT));
                    }
                });
            });

            if self.config.roblox_id != 0 {
                let pk = crate::identity::hex(&profile::profile_identity(&self.config).public());
                let code = profile::verify_code(&pk, self.config.roblox_id);
                let mut publish = false;
                group(ui, "verify");
                ui.label(
                    RichText::new("put this code in your roblox about, then publish.")
                        .size(T_META)
                        .color(TX_FAINT),
                );
                ui.add_space(4.0);
                if copy_row(ui, "", &code, true) {
                    copy_text(&code);
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    publish = primary(ui, if publishing { "publishing" } else { "publish" }, !publishing);
                    let color = match publish_ok {
                        Some(true) => BRUISE_LIT,
                        Some(false) => RED_LIT,
                        None => AMBER,
                    };
                    ui.label(RichText::new(lc(&publish_msg)).size(T_META).color(color));
                });
                if publish {
                    self.publish_profile();
                }
            }
            });
            ui.add_space(24.0);
        });
    }


    fn render_tools(&mut self, ui: &mut egui::Ui) {
        match self.tool_tab {
            ToolTab::Echo => self.render_echo(ui),
            ToolTab::Models => self.render_models(ui),
        }
    }

    fn render_echo(&mut self, ui: &mut egui::Ui) {
        let running = self.echo_server.is_running();
        let mut run = false;
        let mut toggle = false;
        panel(ui, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    run = primary(ui, "run", !self.join_echo_running.load(Ordering::SeqCst));
                });
            });
            ui.add_space(10.0);
            key_row(ui, "port", |ui| {
                text_input(ui, egui::Id::new("echo_port"), &mut self.config.port, "55555", 100.0);
            });
            key_row(ui, "tunnel", |ui| {
                text_input(ui, egui::Id::new("echo_addr"), &mut self.config.addr, "host:port", f32::INFINITY);
            });
            ui.add_space(14.0);
            let echo_label = if running {
                format!("stop echo server ({})", self.echo_server.echoed_count())
            } else {
                "start echo server".to_string()
            };
            toggle = secondary(ui, &echo_label);
        });
        if toggle {
            if running {
                self.echo_server.stop();
                self.add_log("Echo server stopped.", "dim");
            } else {
                let port: u16 = self.config.port.trim().parse().unwrap_or(55555);
                match self.echo_server.start(port) {
                    Ok(_) => self.add_log(&format!("Echo server listening on 0.0.0.0:{}", port), "ok"),
                    Err(e) => self.add_log(&format!("Echo server failed: {}", e), "err"),
                }
            }
        }
        if run {
            if let Some((host, port)) = split_host_port(&self.config.addr) {
                let sink = self.log_sink();
                let flag = Arc::clone(&self.join_echo_running);
                flag.store(true, Ordering::SeqCst);
                thread::spawn(move || {
                    run_echo_test(&host, port, 10, 12.0, sink);
                    flag.store(false, Ordering::SeqCst);
                });
            } else {
                self.add_log("Enter the tunnel address as host:port.", "warn");
            }
        }
        ui.add_space(28.0);
        self.render_log(ui);
    }

    fn render_models(&mut self, ui: &mut egui::Ui) {
        let mut add = false;
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                add = quiet(ui, "add");
            });
        });
        if add {
            self.open_picker(PickTarget::Models);
        }
        ui.add_space(10.0);
        if let Some((ok, msg)) = &self.model_status {
            ui.label(RichText::new(msg).size(T_META).color(if *ok { BRUISE_LIT } else { RED_LIT }));
            ui.add_space(12.0);
        }

        if self.config.saved_maps.is_empty() {
            empty_state(ui, Icon::Cube, "no models queued", "add a .rbxm and the bridge plugin picks it up.");
            return;
        }

        let mut remove = None;
        let mut send = None;
        ScrollArea::vertical().id_salt("app_scroll_2").auto_shrink([false, false]).show(ui, |ui| {

            ui.spacing_mut().item_spacing.y = 0.0;
            for (idx, path_str) in self.config.saved_maps.iter().enumerate() {
                let path = Path::new(path_str);
                let exists = path.is_file();
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                row_frame(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(name).size(14.0).color(if exists { TX } else { TX_DIM }));
                        if !exists {
                            ui.label(RichText::new("missing").size(12.5).color(RED_LIT));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if quiet(ui, "remove") {
                                remove = Some(idx);
                            }
                            if primary(ui, "send to studio", exists) {
                                send = Some(idx);
                            }
                        });
                    });
                });
            }
        });

        if let Some(idx) = send {
            let path = PathBuf::from(&self.config.saved_maps[idx]);
            self.model_status = Some(match self.bridge_server.queue_file(&path) {
                Ok(_) => (true, "Queued. Press Listen in the Studio plugin.".to_string()),
                Err(e) => (false, e),
            });
        }
        if let Some(idx) = remove {
            self.config.saved_maps.remove(idx);
            save_config(&self.config);
        }
    }


    fn render_log(&self, ui: &mut egui::Ui) {
        let remaining = ui.available_height();
        if remaining < 48.0 {
            return;
        }
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(rect, Rounding::ZERO, hairline_color());
        ui.add_space(6.0);
        let log_height = (remaining - 7.0).max(0.0);
        let width = ui.available_width();
        ui.allocate_ui_with_layout(Vec2::new(width, log_height), Layout::top_down(Align::Min), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
            ScrollArea::vertical()
                .id_salt("app_scroll_3")
                .auto_shrink([false, false])
                .max_height(log_height)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if let Ok(logs) = self.logs.lock() {
                        if logs.is_empty() {
                            ui.label(RichText::new("no activity").size(T_META).color(TX_FAINT));
                        }
                        for e in logs.iter() {
                            let color = match e.tag.as_str() {
                                "ok" => TX,
                                "err" => RED_LIT,
                                "warn" => AMBER,
                                "dim" => TX_FAINT,
                                _ => TX_DIM,
                            };
                            let msg = ui.painter().layout(
                                e.message.clone(),
                                FontId::monospace(T_META),
                                Color32::PLACEHOLDER,
                                (ui.available_width() - 64.0).max(80.0),
                            );
                            let row_h = msg.size().y.max(17.0);
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::hover());
                            ui.painter().text(
                                pos2(rect.left(), rect.top() + 8.5),
                                egui::Align2::LEFT_CENTER,
                                &e.time,
                                FontId::monospace(T_TINY),
                                tint(TX_FAINT, 0.7),
                            );
                            ui.painter().galley(pos2(rect.left() + 58.0, rect.top()), msg, color);
                        }
                    }
                });
        });
    }


    fn ensure_about_textures(&mut self, ctx: &egui::Context) {
        if self.discord_icon.is_none() {
            self.discord_icon = load_rgba_texture(
                ctx,
                "about_discord",
                include_bytes!("../../assets/icons/discord.png"),
            );
        }
        if self.session_icon.is_none() {
            self.session_icon = load_rgba_texture(
                ctx,
                "about_session",
                include_bytes!("../../assets/icons/session.png"),
            );
        }
        if self.signal_icon.is_none() {
            self.signal_icon = load_rgba_texture(
                ctx,
                "about_signal",
                include_bytes!("../../assets/icons/signal.png"),
            );
        }
        if self.crypto_icons.is_none() {
            if let (Some(ltc), Some(xrp), Some(doge), Some(sol), Some(eth), Some(btc), Some(usdc)) = (
                load_rgba_texture(ctx, "coin_ltc", include_bytes!("../../assets/icons/crypto/ltc.png")),
                load_rgba_texture(ctx, "coin_xrp", include_bytes!("../../assets/icons/crypto/xrp.png")),
                load_rgba_texture(ctx, "coin_doge", include_bytes!("../../assets/icons/crypto/doge.png")),
                load_rgba_texture(ctx, "coin_sol", include_bytes!("../../assets/icons/crypto/sol.png")),
                load_rgba_texture(ctx, "coin_eth", include_bytes!("../../assets/icons/crypto/eth.png")),
                load_rgba_texture(ctx, "coin_btc", include_bytes!("../../assets/icons/crypto/btc.png")),
                load_rgba_texture(ctx, "coin_usdc", include_bytes!("../../assets/icons/crypto/usdc.png")),
            ) {
                self.crypto_icons = Some(CryptoIcons { ltc, xrp, doge, sol, eth, btc, usdc });
            }
        }
        if self.throne_icon.is_none() {
            self.throne_icon = load_rgba_texture(
                ctx,
                "about_throne",
                include_bytes!("../../assets/icons/throne.png"),
            );
        }
    }

    fn render_about(&mut self, ui: &mut egui::Ui) {
        self.ensure_about_textures(ui.ctx());

        ScrollArea::vertical().id_salt("app_scroll_4").auto_shrink([false, false]).show(ui, |ui| {
            panel(ui, |ui| {
            let mut open = false;
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    open = quiet(ui, "discord");
                });
            });
            if open {
                ui.ctx().open_url(egui::OpenUrl::new_tab("https://dsc.gg/puppytunnel"));
            }
            ui.add_space(10.0);

            group(ui, "community");
            ui.horizontal(|ui| {
                if let Some(tex) = &self.discord_icon {
                    ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(22.0, 22.0)).rounding(6.0));
                    ui.add_space(10.0);
                }
                ui.label(RichText::new("discord").size(T_BODY).color(TX));
                ui.label(RichText::new("dsc.gg/puppytunnel").size(T_META).color(TX_FAINT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let copied = self.copied_target.as_ref().map_or(false, |(k, t)| k == "dc_server" && t.elapsed() < Duration::from_secs(2));
                    if quiet(ui, if copied { "copied" } else { "copy" }) {
                        copy_text("https://dsc.gg/puppytunnel");
                        self.copied_target = Some(("dc_server".to_string(), Instant::now()));
                    }
                });
            });

            group(ui, "contacts");
            row_frame(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(tex) = &self.session_icon {
                        ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(22.0, 22.0)).rounding(5.0));
                        ui.add_space(10.0);
                    }
                    ui.label(RichText::new("session").size(T_BODY).color(TX));
                    ui.label(RichText::new("05066c5f…f571b28d5c").monospace().size(T_META).color(TX_FAINT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let copied = self.copied_target.as_ref().map_or(false, |(k, t)| k == "session" && t.elapsed() < Duration::from_secs(2));
                        if quiet(ui, if copied { "copied" } else { "copy" }) {
                            copy_text("05066c5f2baada426862c0b28af13b484ab99618cc9e117eee3484f5f571b28d5c");
                            self.copied_target = Some(("session".to_string(), Instant::now()));
                        }
                    });
                });
            });
            row_frame(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(tex) = &self.discord_icon {
                        ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(22.0, 22.0)).rounding(5.0));
                        ui.add_space(10.0);
                    }
                    ui.label(RichText::new("discord").size(T_BODY).color(TX));
                    ui.label(RichText::new("@tooksomemore").size(T_META).color(BRUISE_LIT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let copied = self.copied_target.as_ref().map_or(false, |(k, t)| k == "dc_profile" && t.elapsed() < Duration::from_secs(2));
                        if quiet(ui, if copied { "copied" } else { "copy" }) {
                            copy_text("@tooksomemore");
                            self.copied_target = Some(("dc_profile".to_string(), Instant::now()));
                        }
                    });
                });
            });
            row_frame(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(tex) = &self.signal_icon {
                        ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(22.0, 22.0)).rounding(5.0));
                        ui.add_space(10.0);
                    }
                    ui.label(RichText::new("signal").size(T_BODY).color(TX));
                    ui.label(RichText::new("misery.6767").monospace().size(T_META).color(TX_FAINT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let copied = self.copied_target.as_ref().map_or(false, |(k, t)| k == "signal" && t.elapsed() < Duration::from_secs(2));
                        if quiet(ui, if copied { "copied" } else { "copy" }) {
                            copy_text("misery.6767");
                            self.copied_target = Some(("signal".to_string(), Instant::now()));
                        }
                    });
                });
            });

            group(ui, "support");
            row_frame(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(tex) = &self.throne_icon {
                        ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(18.0, 18.0)).rounding(9.0));
                        ui.add_space(10.0);
                    }
                    ui.label(RichText::new("throne").size(T_BODY).color(TX));
                    ui.label(RichText::new("throne.com/novemberouppy").monospace().size(T_META).color(TX_FAINT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let copied = self.copied_target.as_ref().map_or(false, |(k, t)| k == "throne" && t.elapsed() < Duration::from_secs(2));
                        if quiet(ui, if copied { "copied" } else { "copy" }) {
                            copy_text("https://throne.com/novemberouppy");
                            self.copied_target = Some(("throne".to_string(), Instant::now()));
                        }
                        if quiet(ui, "open") {
                            ui.ctx().open_url(egui::OpenUrl::new_tab("https://throne.com/novemberouppy"));
                        }
                    });
                });
            });

            let icons = self.crypto_icons.as_ref();
            let donations: [(&str, &str, Option<&egui::TextureHandle>); 7] = [
                ("LTC", "LLxL3cyJ6AKYve6Hixc6JQaB9AMcJtzmJT", icons.map(|i| &i.ltc)),
                ("XRP", "rHT2p9ykb2D38t3egT1dACMNVEM91HNpNJ", icons.map(|i| &i.xrp)),
                ("DOGE", "DEbZhNnGzqmJAPNrqx2DEMrjy5FowfgtkR", icons.map(|i| &i.doge)),
                ("SOL", "7Hc9ZZgZsp94Dn4wecFNAcVqwLK9z5vfkz8tws2aNxfS", icons.map(|i| &i.sol)),
                ("ETH", "0x7d74dFc51E795138442e5E8fa38ca592800A9927", icons.map(|i| &i.eth)),
                ("BTC", "bc1qzr9kk9hf649w54w5adgw5l04uwymwlgj907kqx", icons.map(|i| &i.btc)),
                ("USDC", "0x7d74dFc51E795138442e5E8fa38ca592800A9927", icons.map(|i| &i.usdc)),
            ];
            for (ticker, addr, icon) in donations.iter() {
                row_frame(ui, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(tex) = icon {
                            ui.add(egui::Image::new(*tex).fit_to_exact_size(egui::vec2(18.0, 18.0)).rounding(9.0));
                            ui.add_space(10.0);
                        }
                        ui.label(RichText::new(*ticker).size(T_META).color(TX_DIM));
                        ui.label(RichText::new(*addr).monospace().size(T_TINY).color(TX_FAINT));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let copied = self.copied_target.as_ref().map_or(false, |(k, t)| k == *ticker && t.elapsed() < Duration::from_secs(2));
                            if quiet(ui, if copied { "copied" } else { "copy" }) {
                                copy_text(addr);
                                self.copied_target = Some((ticker.to_string(), Instant::now()));
                            }
                        });
                    });
                });
            }

            group(ui, "updates");
            self.render_updates_inline(ui);
            ui.add_space(24.0);
            });
        });
    }

    fn check_for_updates_action(&self) {
        use std::str::FromStr;
        let ch = crate::update::Channel::from_str(&self.config.update_channel)
            .unwrap_or(crate::update::Channel::Beta);
        let source = crate::update::UpdateSource::from_config(
            &self.config.update_source_type,
            &self.config.update_source_url,
        );
        self.update_manager
            .check_for_updates(env!("CARGO_PKG_VERSION"), ch, source);
    }

    fn apply_update_and_restart(
        &mut self,
        release: &crate::update::ReleaseInfo,
        staged_path: &Path,
    ) {
        self.config.update_history.push(crate::update::UpdateHistoryEntry {
            version: release.version.clone(),
            installed_at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
            previous_version: env!("CARGO_PKG_VERSION").to_string(),
            channel: self.config.update_channel.clone(),
            status: "Installed".to_string(),
            title: release.title.clone(),
        });
        save_config(&self.config);
        self.update_manager
            .install_and_restart(staged_path.to_path_buf(), None, Vec::new());
    }

    fn render_updates_inline(&mut self, ui: &mut egui::Ui) {
        use crate::update::UpdateState;
        let state = self.update_manager.state();
        let (line, color) = match &state {
            UpdateState::Idle | UpdateState::UpToDate { .. } => ("up to date".to_string(), TX_FAINT),
            UpdateState::Checking => ("checking…".to_string(), TX_FAINT),
            UpdateState::Available(rel) => (format!("v{} available", rel.version), BRUISE_LIT),
            UpdateState::Downloading { progress, .. } => (format!("downloading {:.0}%", progress * 100.0), BRUISE_LIT),
            UpdateState::ReadyToInstall(rel, _) => (format!("v{} ready", rel.version), BRUISE_LIT),
            UpdateState::Installing | UpdateState::RestartRequired => ("restarting…".to_string(), BRUISE_LIT),
            UpdateState::Failed { .. } => ("check failed".to_string(), RED_LIT),
            UpdateState::Cancelled => ("cancelled".to_string(), TX_FAINT),
        };
        let mut check = false;
        let mut download = None;
        let mut apply = None;
        let mut cancel = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).size(T_BODY).color(TX));
            ui.label(RichText::new(line).size(T_META).color(color));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                match &state {
                    UpdateState::Idle | UpdateState::UpToDate { .. } | UpdateState::Cancelled => {
                        check = quiet(ui, "check");
                    }
                    UpdateState::Checking => {
                        ui.label(RichText::new("checking").size(T_META).color(TX_FAINT));
                    }
                    UpdateState::Available(rel) => {
                        download = Some(rel.clone());
                        check = quiet(ui, "again");
                    }
                    UpdateState::Downloading { .. } => {
                        cancel = quiet(ui, "cancel");
                    }
                    UpdateState::ReadyToInstall(rel, path) => {
                        apply = Some((rel.clone(), path.clone()));
                        ui.label(RichText::new("ready").size(T_META).color(BRUISE_LIT));
                    }
                    UpdateState::Installing | UpdateState::RestartRequired => {
                        ui.label(RichText::new("restarting").size(T_META).color(BRUISE_LIT));
                    }
                    UpdateState::Failed { .. } => {
                        check = quiet(ui, "retry");
                    }
                }
            });
        });
        if let UpdateState::Downloading { downloaded_bytes, total_bytes, progress: frac, speed_bps } = &state {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "{:.1} / {:.1} MB · {:.1} MB/s",
                    *downloaded_bytes as f64 / 1048576.0,
                    *total_bytes as f64 / 1048576.0,
                    *speed_bps as f64 / 1048576.0
                ))
                .size(T_META)
                .color(TX_FAINT),
            );
            progress(ui, *frac);
        }
        if let UpdateState::Failed { error, .. } = &state {
            ui.add_space(6.0);
            ui.label(RichText::new(error).size(T_META).color(RED_LIT));
        }
        if let Some(rel) = match &state {
            UpdateState::Available(rel) => Some(rel),
            UpdateState::ReadyToInstall(rel, _) => Some(rel),
            _ => None,
        } {
            if !rel.notes.trim().is_empty() {
                ui.add_space(10.0);
                for line in rel.notes.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let bullet = trimmed.trim_start_matches(|c| c == '•' || c == '-' || c == '*' || c == ' ');
                    ui.label(RichText::new(bullet).size(T_META).color(TX_FAINT));
                }
            }
        }
        ui.add_space(12.0);
        let mut channel = self.config.update_channel.clone();
        let options = [("nightly", "nightly"), ("beta", "beta"), ("stable", "stable")];
        if segmented(ui, &mut channel, &options) {
            self.config.update_channel = channel;
            save_config(&self.config);
            self.check_for_updates_action();
        }
        ui.add_space(8.0);
        let prev_auto = self.config.auto_update;
        toggle_row(ui, &mut self.config.auto_update, "check on startup");
        if prev_auto != self.config.auto_update {
            save_config(&self.config);
        }
        if check {
            self.check_for_updates_action();
        }
        if let Some(rel) = download {
            self.update_manager.start_download(rel);
        }
        if cancel {
            self.update_manager.cancel_download();
        }
        if let Some((rel, path)) = apply {
            self.apply_update_and_restart(&rel, &path);
        }
    }
}

fn file_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn elide(painter: &egui::Painter, text: &str, font: FontId, width: f32) -> String {
    let fits = |s: &str| painter.layout_no_wrap(s.to_string(), font.clone(), Color32::PLACEHOLDER).size().x <= width;
    if fits(text) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let candidate: String = chars[..mid].iter().collect::<String>() + "…";
        if fits(&candidate) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    format!("{}…", chars[..lo].iter().collect::<String>().trim_end())
}

