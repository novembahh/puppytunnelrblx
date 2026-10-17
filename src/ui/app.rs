use crate::boombox::{ensure_media_local, BoomboxServer};
use crate::bridge::BridgeServer;
use crate::config::{
    copy_text, gen_guid, load_config, save_config, split_host_port, ts, write_session_log,
    AppConfig, PROXY_PORT,
};
use crate::identity::Identity;
use crate::master::{self, Announce, RemoteServer};
use crate::net::relay::{make_invite, parse_invite, run_host_agent};
use crate::net::{
    run_connectivity_test, run_echo_test, start_proxy, start_proxy_relay, warm_tunnel, EchoServer,
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
    self, Align, Color32, Layout, Margin, RichText, Rounding, ScrollArea, Sense, Vec2,
};
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};


pub const CONTENT: f32 = 1040.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Screen {
    Home,
    Host,
    HostRunning,
    Join,
    JoinRunning,
    Browse,
    Community,
    Profile,
    Tools,
    About,
    Updates,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Section {
    Home,
    Play,
    Community,
    Profile,
    Tools,
    About,
}

impl Section {
    const ALL: [Section; 6] = [Section::Home, Section::Play, Section::Community, Section::Profile, Section::Tools, Section::About];

    fn label(self) -> &'static str {
        match self {
            Section::Home => "home",
            Section::Play => "play",
            Section::Community => "community",
            Section::Profile => "profile",
            Section::Tools => "tools",
            Section::About => "about",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Section::Home => Icon::Home,
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
        Screen::Home => Section::Home,
        Screen::Host | Screen::HostRunning | Screen::Join | Screen::JoinRunning | Screen::Browse => Section::Play,
        Screen::Community => Section::Community,
        Screen::Profile => Section::Profile,
        Screen::Tools => Section::Tools,
        Screen::About | Screen::Updates => Section::About,
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
    servers: Vec<RemoteServer>,
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
        let (start_screen, start_profile_tab, start_tool_tab) = (Screen::Home, ProfileTab::Account, ToolTab::Echo);

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

            logs: Arc::new(Mutex::new(Vec::new())),
            stop_flag: Arc::new(AtomicBool::new(false)),

            server_process: None,
            server_start_time: None,
            server_live: false,
            host_media: false,
            join_media: false,

            agent_stop: Arc::new(AtomicBool::new(false)),
            agent_thread: None,
            invite: None,
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

    fn joined(&self) -> bool {
        self.proxy_handle.is_some()
    }
}

impl Drop for NepApp {
    fn drop(&mut self) {
        self.stop_host_services();
        self.stop_join_services();
    }
}

impl eframe::App for NepApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(100));
        handle_resize(ctx);

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

        self.community_state.local_headshot = self.headshot.clone();
        self.community_state.local_avatar = self.avatar_tex.clone();

        if let Some(query) = self.community_state.request_fetch_roblox.take() {
            self.roblox_query = query.clone();
            self.fetch_roblox(query);
        }

        if let Ok(st) = self.profile_state.lock() {
            self.community_state.is_roblox_loading = st.loading;
            self.community_state.roblox_error = st.error.clone();
        }

        let compact = ctx.screen_rect().width() < 900.0;
        egui::TopBottomPanel::top("titlebar")
            .exact_height(60.0)
            .frame(egui::Frame::none().fill(BG).inner_margin(Margin::symmetric(18.0, 0.0)))
            .show(ctx, |ui| self.render_titlebar(ui, ctx, compact));

        let section = section_of(self.screen);
        if section != Section::Home {
            egui::TopBottomPanel::top("subnav")
                .exact_height(48.0)
                .frame(egui::Frame::none().fill(BG).inner_margin(Margin::symmetric(18.0, 0.0)))
                .show(ctx, |ui| self.render_subnav(ui, section, compact));
        }

        egui::TopBottomPanel::bottom("status")
            .exact_height(32.0)
            .frame(egui::Frame::none().fill(BG).inner_margin(Margin::symmetric(24.0, 0.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new(&self.status_text).size(12.0).color(DIM));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("v{}", crate::update::CURRENT_VERSION)).size(12.0).color(DIM));
                    });
                });
            });

        let content_w = (ctx.screen_rect().width() - 48.0).min(CONTENT);
        let side = ((ctx.screen_rect().width() - content_w) / 2.0).max(20.0);
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG).inner_margin(Margin { left: side, right: side, top: 10.0, bottom: 8.0 }))
            .show(ctx, |ui| {
                ui.set_max_width(content_w);
                match self.screen {
                    Screen::Home => self.render_home(ui),
                    Screen::Host => self.render_host(ui),
                    Screen::HostRunning => self.render_host_running(ui),
                    Screen::Join => self.render_join(ui),
                    Screen::JoinRunning => self.render_join_running(ui),
                    Screen::Browse => self.render_browse(ui),
                    Screen::Community => self.community_state.render(ui, &mut self.config, ctx),
                    Screen::Profile => self.render_profile_section(ui, ctx),
                    Screen::Tools => self.render_tools(ui),
                    Screen::About => self.render_about(ui),
                    Screen::Updates => self.render_updates(ui),
                }
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
        "updates" => (Screen::Updates, ProfileTab::Account, ToolTab::Echo),
        _ => (Screen::Home, ProfileTab::Account, ToolTab::Echo),
    }
}

fn load_rgba_texture(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Option<egui::TextureHandle> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Some(ctx.load_texture(name, color, egui::TextureOptions::LINEAR))
}

fn home_card(ui: &mut egui::Ui, w: f32, icon: Icon, title: &str, sub: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 120.0), Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
    let p = ui.painter();
    let fill = CARD.lerp_to_gamma(FIELD, 0.6 * hover);
    p.rect_filled(rect, Rounding::same(14.0), fill);
    let badge = egui::pos2(rect.left() + 34.0, rect.top() + 34.0);
    crate::ui::icons::paint(p, badge, 22.0, icon, MINT);
    p.text(egui::pos2(rect.left() + 18.0, rect.top() + 72.0), egui::Align2::LEFT_CENTER, lc(title), egui::FontId::proportional(15.0), INK);
    let galley = p.layout(lc(sub), egui::FontId::proportional(12.0), DIM, rect.width() - 36.0);
    p.galley(egui::pos2(rect.left() + 18.0, rect.top() + 86.0), galley, DIM);
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        crate::ui::sound::play_click();
    }
    resp.clicked()
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
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.hovered() {
        let fill = if kind == 2 { ERR } else { FIELD_HI };
        ui.painter().circle_filled(rect.center(), 16.0, fill);
    }
    let c = rect.center();
    let col = if resp.hovered() && kind == 2 { egui::Color32::WHITE } else { INK };
    let st = egui::Stroke::new(1.5_f32, col);
    let p = ui.painter();
    match kind {
        0 => {
            p.line_segment([c + egui::vec2(-5.0, 3.0), c + egui::vec2(5.0, 3.0)], st);
        }
        1 => {
            p.rect_stroke(egui::Rect::from_center_size(c, egui::vec2(10.0, 10.0)), 2.0, st);
        }
        _ => {
            p.line_segment([c + egui::vec2(-4.5, -4.5), c + egui::vec2(4.5, 4.5)], st);
            p.line_segment([c + egui::vec2(-4.5, 4.5), c + egui::vec2(4.5, -4.5)], st);
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
        save_config(&self.config);
        let target = match sec {
            Section::Home => Screen::Home,
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

    fn render_titlebar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, compact: bool) {
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
            ui.spacing_mut().item_spacing.x = 4.0;
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new(logo).fit_to_exact_size(Vec2::splat(40.0)));
            }
            ui.add_space(if compact { 10.0 } else { 22.0 });

            let current = section_of(self.screen);
            let unread = self.community_state.unread_count();
            for sec in Section::ALL {
                let mut label = if compact { String::new() } else { sec.label().to_string() };
                let badge = match sec {
                    Section::Community => unread > 0,
                    Section::About => matches!(
                        self.update_manager.state(),
                        crate::update::UpdateState::Available(_) | crate::update::UpdateState::ReadyToInstall(..)
                    ),
                    Section::Play => self.hosting() || self.joined(),
                    _ => false,
                };
                if badge && !compact {
                    label.push_str(" •");
                }
                let resp = pill_icon(ui, Some(sec.icon()), &label, current == sec, false);
                let resp = if compact { resp.on_hover_text(sec.label()) } else { resp };
                if resp.clicked() {
                    self.go_section(sec);
                }
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

    fn render_subnav(&mut self, ui: &mut egui::Ui, section: Section, compact: bool) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let shown = |label: &str| if compact { String::new() } else { label.to_string() };
            match section {
                Section::Play => {
                    let items = [
                        ("host", Icon::Host, matches!(self.screen, Screen::Host | Screen::HostRunning), if self.hosting() { Screen::HostRunning } else { Screen::Host }),
                        ("join", Icon::Join, matches!(self.screen, Screen::Join | Screen::JoinRunning), if self.joined() { Screen::JoinRunning } else { Screen::Join }),
                        ("servers", Icon::Servers, self.screen == Screen::Browse, Screen::Browse),
                    ];
                    for (label, icon, active, target) in items {
                        if pill_icon(ui, Some(icon), &shown(label), active, true).on_hover_text(label).clicked() && !active {
                            save_config(&self.config);
                            if target == Screen::Browse {
                                self.browser_dirty = true;
                            }
                            self.screen = target;
                        }
                    }
                }
                Section::Community => {
                    use crate::ui::community::CommunityTab as T;
                    let unread = self.community_state.unread_count();
                    let pending = self.community_state.pending_reports();
                    let is_owner = self.config.roblox_id == 2721761670;
                    let mut items = vec![
                        ("feed", Icon::Feed, T::Feed, 0),
                        ("messages", Icon::Messages, T::Messages, 0),

                        ("notifications", Icon::Bell, T::Notifications, unread),
                        ("search", Icon::Search, T::Search, 0),
                    ];
                    if is_owner {
                        items.push(("moderation", Icon::Gavel, T::Moderation, pending));
                    }
                    for (label, icon, tab, count) in items {
                        let active = self.community_state.active_tab == tab;
                        let text = if count > 0 && !compact { format!("{} {}", label, count) } else { shown(label) };
                        if pill_icon(ui, Some(icon), &text, active, true).on_hover_text(label).clicked() && !active {
                            self.community_state.select_tab(tab);
                        }
                    }
                }
                Section::Profile => {
                    let items = [
                        ("account", Icon::Profile, ProfileTab::Account),
                        ("public profile", Icon::Community, ProfileTab::Public),
                        ("keys", Icon::Key, ProfileTab::Keys),
                    ];
                    for (label, icon, tab) in items {
                        let active = self.profile_tab == tab;
                        if pill_icon(ui, Some(icon), &shown(label), active, true).on_hover_text(label).clicked() {
                            self.profile_tab = tab;
                        }
                    }
                }
                Section::Tools => {
                    let items = [("echo test", Icon::Echo, ToolTab::Echo), ("models", Icon::Cube, ToolTab::Models)];
                    for (label, icon, tab) in items {
                        let active = self.tool_tab == tab;
                        if pill_icon(ui, Some(icon), &shown(label), active, true).on_hover_text(label).clicked() {
                            self.tool_tab = tab;
                        }
                    }
                }
                Section::About => {
                    let has_update = matches!(
                        self.update_manager.state(),
                        crate::update::UpdateState::Available(_) | crate::update::UpdateState::ReadyToInstall(..)
                    );
                    let items = [
                        ("about", Icon::About, Screen::About, false),
                        ("updates", Icon::Update, Screen::Updates, has_update),
                    ];
                    for (label, icon, target, dot) in items {
                        let active = self.screen == target;
                        let text = if dot && !compact { format!("{} •", label) } else { shown(label) };
                        if pill_icon(ui, Some(icon), &text, active, true).on_hover_text(label).clicked() {
                            self.screen = target;
                        }
                    }
                }
                Section::Home => {}
            }
        });
    }


    fn render_home(&mut self, ui: &mut egui::Ui) {
        ScrollArea::vertical().id_salt("home_scroll").auto_shrink([false, false]).show(ui, |ui| {
            let name = if self.config.display_name.trim().is_empty() && self.config.roblox_name.trim().is_empty() {
                String::new()
            } else {
                self.config.effective_display_name().to_string()
            };
            let hello = if name.is_empty() { "hey there".to_string() } else { format!("hey, {}", name) };
            ui.add_space(6.0);
            ui.label(RichText::new(lc(&hello)).size(28.0).color(INK));
            ui.label(muted("what do you want to do?"));
            ui.add_space(16.0);

            let gap = 12.0;
            let w = (ui.available_width() - gap * 2.0) / 3.0;
            let cards = [
                (Icon::Host, "host a server", "start a studio server and invite people", Screen::Host),
                (Icon::Join, "join a server", "paste an invite or an address", Screen::Join),
                (Icon::Servers, "browse servers", "public servers from your masters", Screen::Browse),
            ];
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (icon, title, sub, target) in cards {
                    if home_card(ui, w, icon, title, sub) {
                        if target == Screen::Browse {
                            self.browser_dirty = true;
                        }
                        self.screen = if target == Screen::Host && self.hosting() {
                            Screen::HostRunning
                        } else if target == Screen::Join && self.joined() {
                            Screen::JoinRunning
                        } else {
                            target
                        };
                    }
                }
            });
            ui.add_space(14.0);

            card(ui, Some("session"), |ui| {
                if self.hosting() {
                    ui.horizontal(|ui| {
                        chip(ui, if self.server_live { "hosting · live" } else { "hosting · starting" }, MINT);
                        ui.label(muted(format!("{} connected", self.players.load(Ordering::Relaxed))));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if secondary_icon(ui, Icon::Host, "open") {
                                self.screen = Screen::HostRunning;
                            }
                            if let Some(inv) = self.invite.clone() {
                                if secondary_icon(ui, Icon::Copy, "copy invite") {
                                    copy_text(&inv);
                                }
                            }
                        });
                    });
                } else if self.joined() {
                    ui.horizontal(|ui| {
                        chip(ui, "connected", MINT);
                        ui.label(muted(&self.config.addr));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if secondary_icon(ui, Icon::Join, "open") {
                                self.screen = Screen::JoinRunning;
                            }
                        });
                    });
                } else {
                    ui.label(muted("nothing running. host a server or join one above."));
                }
            });
            ui.add_space(12.0);

            card(ui, Some("you"), |ui| {
                ui.horizontal(|ui| {
                    if let Some(tex) = &self.headshot {
                        ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::splat(44.0)).rounding(22.0));
                    }
                    ui.vertical(|ui| {
                        if self.config.roblox_id != 0 {
                            ui.label(RichText::new(lc(self.config.effective_display_name())).size(16.0).color(INK));
                            let verified = self.profile_state.lock().map(|s| s.verified).unwrap_or(None);
                            match verified {
                                Some(true) => chip(ui, "verified", MINT),
                                _ => chip(ui, "unverified", WARN),
                            }
                        } else {
                            ui.label(muted("no roblox account linked"));
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if secondary_icon(ui, Icon::Profile, "open profile") {
                            self.screen = Screen::Profile;
                        }
                    });
                });
            });
            ui.add_space(12.0);
            card(ui, Some("community"), |ui| {
                ui.horizontal(|ui| {
                    let unread = self.community_state.unread_count();
                    ui.vertical(|ui| {
                        ui.label(RichText::new(if unread > 0 { format!("{} unread", unread) } else { "all caught up".to_string() }).size(16.0).color(INK));
                        ui.label(muted(format!("on {}", self.community_state.instance_label(&self.config))));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if secondary_icon(ui, Icon::Community, "open community") {
                            self.community_state.open();
                            self.screen = Screen::Community;
                        }
                    });
                });
            });
            ui.add_space(12.0);
        });
    }


    fn render_profile_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.community_state.local_headshot = self.headshot.clone();
        self.community_state.local_avatar = self.avatar_tex.clone();
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

            page_header(ui, "Host a server", "Start a Roblox Studio server and let people join it.");

            card(ui, Some("Server"), |ui| {
                field(ui, "Studio port", &mut self.config.port, "55555");
                field(ui, "User ID", &mut self.config.uid, "Your Roblox user ID");
                path_field(ui, "Map (optional)", &mut self.config.map, "Choose a .rbxl file", || self.picker_request = Some(PickTarget::Map));
                path_field(ui, "Roblox Studio", &mut self.config.studio, "Path to RobloxStudioBeta", || self.picker_request = Some(PickTarget::Studio));
            });
            ui.add_space(12.0);

            card(ui, Some("Connection"), |ui| {
                field(ui, "Relay", &mut self.config.relay, "relay.example.com:7777");
                field(ui, "Tunnel address (when no relay is set)", &mut self.config.addr, "host:port");
                notes(ui, &self.config.addr, &self.config.relay);
            });
            ui.add_space(12.0);

            card(ui, Some("Options"), |ui| {
                toggle(ui, &mut self.host_media, "Run the media helper");
                toggle(ui, &mut self.config.list_public, "List in the server browser");
                toggle(ui, &mut self.config.native_dialogs, "Use the system file dialog");
                if self.config.list_public {
                    ui.add_space(8.0);
                    field(ui, "Name", &mut self.config.server_name, "My server");
                    field(ui, "Description", &mut self.config.server_desc, "");
                    field(ui, "Tags", &mut self.config.server_tags, "obby, pvp, chill");
                    field(ui, "Master servers", &mut self.config.master, "roblox.puppytunnel.vip (comma-separate several)");
                    ui.label(muted("Max players"));
                    ui.add(egui::DragValue::new(&mut self.config.max_players).range(0..=1000));
                }
            });

            ui.add_space(16.0);
            if primary(ui, "Start server", true) {
                save_config(&self.config);
                self.start_host();
            }
            ui.add_space(8.0);
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

        let pg = gen_guid();
        let tg = gen_guid();
        self.add_log(&format!("Port {}", self.config.port), "dim");

        if self.host_media {
            let _ = self.boombox_server.start();
            self.add_log("Media helper running on 127.0.0.1:48732", "info");
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
        self.agent_thread = Some(thread::spawn(move || {
            if let Err(e) = run_host_agent(addr, agent_id, studio_port, stop, players, &sink) {
                sink(&format!("Relay stopped: {}", e), "err");
            }
        }));
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
                host,
                host_sig,
                ts: 0,
            };
            self.announcer = Some(master::spawn_announcer(
                master::master_list(&self.config.master),
                id,
                listing,
                Arc::clone(&self.agent_stop),
                Arc::clone(&self.announce_ready),
                self.log_sink(),
            ));
        }
        copy_text(&invite);
        self.add_log("Invite copied to the clipboard.", "ok");
        self.invite = Some(invite);
    }

    fn render_host_running(&mut self, ui: &mut egui::Ui) {
        if let Some(start) = self.server_start_time {
            if start.elapsed() >= Duration::from_secs(5) && !self.server_live && self.hosting() {
                self.server_live = true;
                self.announce_ready.store(true, Ordering::SeqCst);
                self.add_log("Server is live.", "ok");
            }
        }

        ui.horizontal(|ui| {
            ui.label(RichText::new("Server").size(24.0).color(INK));
            let (text, color) = if self.server_live { ("Live", MINT) } else { ("Starting", DIM) };
            ui.label(RichText::new(text).size(13.0).color(color));
        });
        if self.invite.is_some() {
            ui.label(muted(format!("{} connected", self.players.load(Ordering::Relaxed))));
        }
        ui.add_space(12.0);

        if let Some(inv) = self.invite.clone() {
            egui::Frame::none()
                .fill(CARD)
                .rounding(14.0)
                .inner_margin(Margin::symmetric(16.0, 12.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&inv).monospace().size(12.5).color(INK));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if secondary(ui, "Copy invite") {
                                copy_text(&inv);
                            }
                        });
                    });
                    if let Some((rh, rp, _)) = parse_invite(&inv) {
                        ui.horizontal(|ui| {
                            ui.label(muted(format!("join ip {}:{}", rh, rp)));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if secondary(ui, "Copy IP") {
                                    copy_text(&format!("{}:{}", rh, rp));
                                }
                            });
                        });
                    }
                });
            ui.add_space(8.0);
        }

        ui.horizontal(|ui| {
            if primary(ui, "Join locally", self.server_live) {
                let port: u16 = self.config.port.trim().parse().unwrap_or(55555);
                match launch_client(&self.config.studio, "127.0.0.1", port, &gen_guid(), &gen_guid(), "StudioPlayer_Host") {
                    Ok(_) => self.add_log("Local client launched.", "info"),
                    Err(e) => self.add_log(&format!("Launch failed: {}", e), "err"),
                }
            }
            if self.host_media && secondary(ui, "Copy HTTP command") {
                copy_text("game:GetService(\"HttpService\").HttpEnabled = true");
                self.add_log("Paste the command into the Studio command bar to enable HTTP.", "warn");
            }
            if secondary(ui, "Test") {
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
            if secondary(ui, "Stop") {
                self.stop_host_services();
                self.screen = Screen::Host;
            }
        });

        ui.add_space(12.0);
        self.render_log(ui);
    }


    fn render_join(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Join a server", "Paste an invite or a host address.");
        card(ui, None, |ui| {
            field(ui, "Invite or host:port", &mut self.config.addr, "puppy://relay.example.com:7777/...");
            notes(ui, &self.config.addr, "");
            ui.add_space(6.0);
            toggle(ui, &mut self.join_media, "Download media the host asks for");
            if self.join_media {
                ui.label(RichText::new("The media site will see your IP.").size(12.5).color(WARN));
            }
        });
        ui.add_space(16.0);
        if primary(ui, "Connect", true) {
            save_config(&self.config);
            self.start_join();
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
        ui.horizontal(|ui| {
            ui.label(RichText::new("Connection").size(24.0).color(INK));
            let running = self.proxy_handle.as_ref().map_or(false, |p| p.is_running());
            let (text, color) = if running { ("Connected", MINT) } else { ("Stopped", ERR) };
            ui.label(RichText::new(text).size(13.0).color(color));
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if secondary(ui, "Test") {
                let running = self.proxy_handle.as_ref().map_or(false, |p| p.is_running());
                let sink = self.log_sink();
                match split_host_port(&self.config.addr) {
                    Some((host, port)) => {
                        thread::spawn(move || {
                            run_connectivity_test(&host, port, false, None, running, sink);
                        });
                    }
                    None => self.add_log("Invites are checked by joining. Use Tools > Echo test for host:port tunnels.", "warn"),
                }
            }
            if secondary(ui, "Disconnect") {
                self.stop_join_services();
                self.screen = Screen::Join;
            }
        });
        ui.add_space(12.0);
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
                st.servers = list;
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
            (st.servers.clone(), st.loading, st.error.clone())
        };

        ui.horizontal(|ui| {
            ui.label(RichText::new("servers").size(24.0).color(INK));
            if loading {
                chip(ui, "refreshing", DIM);
            } else if error.is_empty() {
                chip(ui, &format!("{} online", servers.len()), MINT);
            }
        });
        ui.label(muted("public servers from your masters. refreshing shows them your ip."));
        ui.add_space(12.0);

        let mut refresh = false;
        ui.horizontal(|ui| {
            let r = text_input(ui, egui::Id::new("browse_q"), &mut self.browser_query, "search name, map or tag", 300.0);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                refresh = true;
            }
            if segmented(ui, &mut self.browser_sort, &[("players", "players"), ("name", "name"), ("new", "newest")]) {
                refresh = true;
            }
            if icon_button(ui, Icon::Refresh, "refresh") {
                refresh = true;
            }
            if icon_button(ui, Icon::Tools, "choose masters") {
                self.browser_edit_masters = !self.browser_edit_masters;
            }
        });
        if self.browser_edit_masters {
            ui.add_space(4.0);
            ui.label(muted("masters (comma separated, empty uses roblox.puppytunnel.vip)"));
            let w = ui.available_width();
            let changed = text_input(ui, egui::Id::new("browse_masters"), &mut self.config.master, "roblox.puppytunnel.vip", w).lost_focus();
            if changed {
                refresh = true;
            }
        }
        if refresh {
            save_config(&self.config);
            self.refresh_browser();
        }
        ui.add_space(10.0);

        if !error.is_empty() {
            card(ui, None, |ui| {
                ui.label(RichText::new(lc(&error)).size(13.0).color(ERR));
            });
        } else if servers.is_empty() && !loading {
            card(ui, None, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.label(RichText::new("no servers online right now").size(16.0).color(INK));
                    ui.label(muted("host one from the play tab and it shows up here."));
                    ui.add_space(12.0);
                    if primary_icon(ui, Icon::Host, "host a server", true) {
                        self.screen = Screen::Host;
                    }
                    ui.add_space(24.0);
                });
            });
        }

        let mut join_invite = None;
        ScrollArea::vertical().id_salt("browse_list").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            for s in &servers {
                let l = &s.listing;
                row_frame(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&l.name).size(15.5).color(INK));
                                if let Some(h) = &s.host {
                                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                                    ui.painter().circle_filled(dot.center(), 4.5, hex_color(&h.accent));
                                    ui.label(muted(format!("by {}", h.display_name)));
                                }
                            });
                            if !l.tags.is_empty() {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                                    for t in &l.tags {
                                        chip(ui, t, DIM);
                                    }
                                });
                            }
                            if !l.description.is_empty() {
                                let desc: String = l.description.chars().take(140).collect();
                                ui.label(muted(desc));
                            }
                            if let Some((rh, rp, _)) = parse_invite(&l.invite) {
                                ui.horizontal(|ui| {
                                    ui.label(muted(format!("{}:{}", rh, rp)));
                                    if secondary_icon(ui, Icon::Copy, "copy server address") {
                                        copy_text(&format!("{}:{}", rh, rp));
                                    }
                                });
                            }
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if primary_icon(ui, Icon::Join, "join", true) {
                                join_invite = Some(l.invite.clone());
                            }
                            ui.add_space(8.0);
                            let cap = if l.max_players > 0 {
                                format!("{}/{}", l.players, l.max_players)
                            } else {
                                l.players.to_string()
                            };
                            ui.label(RichText::new(cap).size(14.0).color(INK));
                            let (ic, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                            crate::ui::icons::paint(ui.painter(), ic.center(), 15.0, Icon::Community, DIM);
                        });
                    });
                });
            }
        });

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
            page_header(ui, "account", "your roblox identity. it only shows as verified once you prove the account is yours.");

            card(ui, Some("roblox account"), |ui| {
                ui.horizontal(|ui| {
                    let (tile, _) = ui.allocate_exact_size(Vec2::new(230.0, 270.0), Sense::hover());
                    ui.painter().rect_filled(tile, Rounding::same(20.0), FIELD);
                    match &self.avatar_tex {
                        Some(tex) => {
                            let avail = tile.size() - Vec2::new(24.0, 24.0);
                            let ts = tex.size_vec2();
                            let scale = (avail.x / ts.x).min(avail.y / ts.y);
                            let img = egui::Rect::from_center_size(tile.center(), ts * scale);
                            egui::Image::new(tex).fit_to_exact_size(img.size()).paint_at(ui, img);
                        }
                        None => {
                            ui.painter().text(
                                tile.center(),
                                egui::Align2::CENTER_CENTER,
                                if loading { "loading avatar" } else { "no avatar yet" },
                                egui::FontId::proportional(13.0),
                                DIM,
                            );
                        }
                    }
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        if let Some(u) = &user {
                            ui.horizontal(|ui| {
                                if let Some(tex) = &self.headshot {
                                    ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::splat(44.0)).rounding(22.0));
                                }
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(lc(&u.display_name)).size(20.0).color(INK));
                                        match verified {
                                            Some(true) => chip(ui, "verified", MINT),
                                            _ => chip(ui, "unverified", WARN),
                                        }
                                    });
                                    ui.label(muted(format!("@{}  ·  {}", u.name, u.id)));
                                });
                            });
                        } else {
                            ui.label(muted("no roblox account linked yet."));
                        }
                        ui.add_space(8.0);
                        ui.label(muted("roblox username or user id"));
                        ui.horizontal(|ui| {
                            let r = text_input(ui, egui::Id::new("roblox_query"), &mut self.roblox_query, "e.g. roblox", 200.0);
                            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            let can = !loading && !self.roblox_query.trim().is_empty();
                            if (primary_icon(ui, Icon::Refresh, if loading { "fetching" } else { "fetch" }, can) || (enter && can)) && can {
                                self.fetch_roblox(self.roblox_query.clone());
                            }
                        });
                        if !error.is_empty() {
                            ui.label(RichText::new(lc(&error)).size(12.5).color(ERR));
                        }
                        ui.add_space(6.0);
                        ui.label(muted("this is roblox's own render of your avatar."));
                        ui.label(muted("fetching asks roblox's public api, so roblox sees your ip."));
                    });
                });
            });
            ui.add_space(12.0);

            card(ui, Some("verification"), |ui| {
                if self.config.roblox_id == 0 {
                    ui.label(muted("link your roblox account above to get a verification code."));
                } else {
                    let pk = crate::identity::hex(&profile::profile_identity(&self.config).public());
                    let code = profile::verify_code(&pk, self.config.roblox_id);
                    ui.label(muted("paste this code anywhere in your roblox profile about text, then publish. the server reads your public about text to confirm the account is yours."));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        egui::Frame::none().fill(FIELD).rounding(14.0).inner_margin(Margin::symmetric(14.0, 9.0)).show(ui, |ui| {
                            ui.label(RichText::new(&code).monospace().size(13.0).color(INK));
                        });
                        if secondary_icon(ui, Icon::Copy, "copy") {
                            copy_text(&code);
                        }
                    });
                    ui.add_space(8.0);
                    field(ui, "profile server", &mut self.config.profile_servers, "roblox.puppytunnel.vip");
                    ui.horizontal(|ui| {
                        if primary_icon(ui, Icon::Shield, if publishing { "publishing" } else { "publish and verify" }, !publishing) {
                            self.publish_profile();
                        }
                        let color = match publish_ok {
                            Some(true) => MINT,
                            Some(false) => ERR,
                            None => WARN,
                        };
                        ui.label(RichText::new(lc(&publish_msg)).size(13.0).color(color));
                    });
                }
            });
            ui.add_space(8.0);
        });
    }


    fn render_tools(&mut self, ui: &mut egui::Ui) {
        match self.tool_tab {
            ToolTab::Echo => {
                page_header(ui, "echo test", "check a tunnel end to end before you start a session.");
                self.render_echo(ui);
            }
            ToolTab::Models => {
                page_header(ui, "models", "send .rbxm files into studio through the bridge plugin.");
                self.render_models(ui);
            }
        }
    }

    fn render_echo(&mut self, ui: &mut egui::Ui) {
        card(ui, None, |ui| {
            field(ui, "Studio port (host)", &mut self.config.port, "55555");
            field(ui, "Tunnel address (joiner)", &mut self.config.addr, "host:port");
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let running = self.echo_server.is_running();
            let label = if running {
                format!("Stop echo server ({})", self.echo_server.echoed_count())
            } else {
                "Start echo server".to_string()
            };
            if secondary(ui, &label) {
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
            let busy = self.join_echo_running.load(Ordering::SeqCst);
            if primary(ui, "Run echo test", !busy) {
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
        });
        ui.add_space(12.0);
        self.render_log(ui);
    }

    fn render_models(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if secondary(ui, "Add models") {
                self.open_picker(PickTarget::Models);
            }
            if let Some((ok, msg)) = &self.model_status {
                ui.label(RichText::new(msg).size(13.0).color(if *ok { MINT } else { ERR }));
            }
        });
        ui.add_space(8.0);

        if self.config.saved_maps.is_empty() {
            ui.label(muted("No models added yet."));
            return;
        }

        let mut remove = None;
        let mut send = None;
        ScrollArea::vertical().id_salt("app_scroll_2").auto_shrink([false, false]).show(ui, |ui| {

            ui.spacing_mut().item_spacing.y = 8.0;
            for (idx, path_str) in self.config.saved_maps.iter().enumerate() {
                let path = Path::new(path_str);
                let exists = path.is_file();
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                row_frame(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(name).size(14.0).color(if exists { INK } else { DIM }));
                        if !exists {
                            ui.label(RichText::new("missing").size(12.5).color(ERR));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if secondary(ui, "Remove") {
                                remove = Some(idx);
                            }
                            if primary(ui, "Send to Studio", exists) {
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
        let height = (ui.available_height() - 6.0).max(120.0);
        egui::Frame::none()
            .fill(CARD)
            .rounding(18.0)
            .inner_margin(Margin::same(16.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(height - 32.0);
                ScrollArea::vertical().id_salt("app_scroll_3").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                    if let Ok(logs) = self.logs.lock() {
                        for e in logs.iter() {
                            let color = match e.tag.as_str() {
                                "ok" => MINT,
                                "err" => ERR,
                                "warn" => WARN,
                                "dim" => DIM,
                                _ => INK,
                            };
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(&e.time).monospace().size(12.0).color(DIM));
                                ui.label(RichText::new(&e.message).monospace().size(12.5).color(color));
                            });
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

            page_header(
                ui,
                "About",
                "Community and contacts.",
            );


            card(ui, Some("DISCORD"), |ui| {
                ui.horizontal(|ui| {
                    if let Some(tex) = &self.discord_icon {
                        ui.add(
                            egui::Image::new(tex)
                                .fit_to_exact_size(egui::vec2(44.0, 44.0))
                                .rounding(10.0),
                        );
                    }
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("PuppyTunnel Discord").size(17.0).color(INK));
                        ui.label(RichText::new("dsc.gg/puppytunnel").size(13.0).color(MINT));
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let copied = self.copied_target.as_ref().map_or(false, |(k, t)| {
                            k == "dc_server" && t.elapsed() < Duration::from_secs(2)
                        });
                        if secondary(ui, if copied { "Copied!" } else { "Copy Link" }) {
                            copy_text("https://dsc.gg/puppytunnel");
                            self.copied_target = Some(("dc_server".to_string(), Instant::now()));
                        }
                        ui.add_space(4.0);

                        let blurple = Color32::from_rgb(88, 101, 242);
                        let blurple_hover = Color32::from_rgb(107, 119, 245);
                        let clicked = ui
                            .scope(|ui| {
                                let w = &mut ui.visuals_mut().widgets;
                                w.inactive.weak_bg_fill = blurple;
                                w.hovered.weak_bg_fill = blurple_hover;
                                w.active.weak_bg_fill = blurple;
                                for s in [&mut w.inactive, &mut w.hovered, &mut w.active] {
                                    s.fg_stroke.color = Color32::WHITE;
                                }
                                ui.add(
                                    egui::Button::new(
                                        RichText::new("Join Discord ->")
                                            .size(14.0)
                                            .color(Color32::WHITE),
                                    )
                                    .rounding(egui::Rounding::same(PILL))
                                    .min_size(egui::vec2(0.0, 34.0)),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            })
                            .inner;

                        if clicked {
                            crate::ui::sound::play_click();
                            ui.ctx().open_url(egui::OpenUrl::new_tab("https://dsc.gg/puppytunnel"));
                        }
                    });
                });
            });

            ui.add_space(14.0);


            card(ui, Some("CONTACTS"), |ui| {

                row_frame(ui, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(tex) = &self.session_icon {
                            ui.add(
                                egui::Image::new(tex)
                                    .fit_to_exact_size(egui::vec2(28.0, 28.0))
                                    .rounding(6.0),
                            );
                        }
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(RichText::new("Session").size(13.5).color(INK));
                            let session_id = "05066c5f2baada426862c0b28af13b484ab99618cc9e117eee3484f5f571b28d5c";
                            egui::Frame::none()
                                .fill(FIELD)
                                .rounding(6.0)
                                .inner_margin(Margin::symmetric(6.0, 2.0))
                                .show(ui, |ui| {
                                    ui.label(RichText::new(session_id).monospace().size(11.5).color(INK));
                                });
                        });

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let copied = self.copied_target.as_ref().map_or(false, |(k, t)| {
                                k == "session" && t.elapsed() < Duration::from_secs(2)
                            });
                            if secondary(ui, if copied { "Copied ID!" } else { "Copy ID" }) {
                                copy_text("05066c5f2baada426862c0b28af13b484ab99618cc9e117eee3484f5f571b28d5c");
                                self.copied_target = Some(("session".to_string(), Instant::now()));
                            }
                        });
                    });
                });

                ui.add_space(8.0);


                row_frame(ui, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(tex) = &self.discord_icon {
                            ui.add(
                                egui::Image::new(tex)
                                    .fit_to_exact_size(egui::vec2(28.0, 28.0))
                                    .rounding(6.0),
                            );
                        }
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(RichText::new("Discord Profile").size(13.5).color(INK));
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("@tooksomemore").size(13.5).color(MINT));
                                ui.label(RichText::new("(might be banned -w-)").size(12.0).color(WARN));
                            });
                        });

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let copied = self.copied_target.as_ref().map_or(false, |(k, t)| {
                                k == "dc_profile" && t.elapsed() < Duration::from_secs(2)
                            });
                            if secondary(ui, if copied { "Copied!" } else { "Copy Username" }) {
                                copy_text("@tooksomemore");
                                self.copied_target = Some(("dc_profile".to_string(), Instant::now()));
                            }
                        });
                    });
                });

                ui.add_space(8.0);


                row_frame(ui, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(tex) = &self.signal_icon {
                            ui.add(
                                egui::Image::new(tex)
                                    .fit_to_exact_size(egui::vec2(28.0, 28.0))
                                    .rounding(6.0),
                            );
                        }
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(RichText::new("Signal").size(13.5).color(INK));
                            ui.label(RichText::new("misery.6767").monospace().size(13.5).color(INK));
                        });

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let copied = self.copied_target.as_ref().map_or(false, |(k, t)| {
                                k == "signal" && t.elapsed() < Duration::from_secs(2)
                            });
                            if secondary(ui, if copied { "Copied!" } else { "Copy Username" }) {
                                copy_text("misery.6767");
                                self.copied_target = Some(("signal".to_string(), Instant::now()));
                            }
                        });
                    });
                });
            });

            ui.add_space(14.0);


            card(ui, Some("DONATE"), |ui| {

                egui::Frame::none()
                    .fill(FIELD)
                    .rounding(10.0)
                    .inner_margin(Margin::symmetric(12.0, 7.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            if let Some(tex) = &self.throne_icon {
                                ui.add(
                                    egui::Image::new(tex)
                                        .fit_to_exact_size(egui::vec2(22.0, 22.0))
                                        .rounding(11.0),
                                );
                                ui.add_space(4.0);
                            }
                            ui.label(RichText::new("Throne").size(13.0).strong().color(INK));
                            ui.label(RichText::new("Wishlist").size(12.5).color(DIM));
                            ui.add_space(6.0);
                            ui.label(RichText::new("throne.com/novemberouppy").monospace().size(11.5).color(MINT));

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let copied = self.copied_target.as_ref().map_or(false, |(k, t)| {
                                    k == "throne" && t.elapsed() < Duration::from_secs(2)
                                });
                                if secondary(ui, if copied { "Copied!" } else { "Copy Link" }) {
                                    copy_text("https://throne.com/novemberouppy");
                                    self.copied_target = Some(("throne".to_string(), Instant::now()));
                                }
                                ui.add_space(4.0);

                                let purple = Color32::from_rgb(139, 92, 246);
                                let purple_hover = Color32::from_rgb(167, 139, 250);
                                let clicked = ui
                                    .scope(|ui| {
                                        let w = &mut ui.visuals_mut().widgets;
                                        w.inactive.weak_bg_fill = purple;
                                        w.hovered.weak_bg_fill = purple_hover;
                                        w.active.weak_bg_fill = purple;
                                        for s in [&mut w.inactive, &mut w.hovered, &mut w.active] {
                                            s.fg_stroke.color = Color32::WHITE;
                                        }
                                        ui.add(
                                            egui::Button::new(
                                                RichText::new("Open Throne ->")
                                                    .size(13.0)
                                                    .color(Color32::WHITE),
                                            )
                                            .rounding(egui::Rounding::same(PILL))
                                            .min_size(egui::vec2(0.0, 30.0)),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    })
                                    .inner;

                                if clicked {
                                    crate::ui::sound::play_click();
                                    ui.ctx().open_url(egui::OpenUrl::new_tab("https://throne.com/novemberouppy"));
                                }
                            });
                        });
                    });

                ui.add_space(8.0);

                let icons = self.crypto_icons.as_ref();
                let donations: [(&str, &str, &str, Option<&egui::TextureHandle>); 7] = [
                    ("LTC", "Litecoin", "LLxL3cyJ6AKYve6Hixc6JQaB9AMcJtzmJT", icons.map(|i| &i.ltc)),
                    ("XRP", "XRP", "rHT2p9ykb2D38t3egT1dACMNVEM91HNpNJ", icons.map(|i| &i.xrp)),
                    ("DOGE", "Dogecoin", "DEbZhNnGzqmJAPNrqx2DEMrjy5FowfgtkR", icons.map(|i| &i.doge)),
                    ("SOL", "Solana", "7Hc9ZZgZsp94Dn4wecFNAcVqwLK9z5vfkz8tws2aNxfS", icons.map(|i| &i.sol)),
                    ("ETH", "Ethereum", "0x7d74dFc51E795138442e5E8fa38ca592800A9927", icons.map(|i| &i.eth)),
                    ("BTC", "Bitcoin", "bc1qzr9kk9hf649w54w5adgw5l04uwymwlgj907kqx", icons.map(|i| &i.btc)),
                    ("USDC", "USD Coin", "0x7d74dFc51E795138442e5E8fa38ca592800A9927", icons.map(|i| &i.usdc)),
                ];

                for (i, (ticker, name, addr, icon)) in donations.iter().enumerate() {
                    if i > 0 {
                        ui.add_space(6.0);
                    }
                    egui::Frame::none()
                        .fill(FIELD)
                        .rounding(10.0)
                        .inner_margin(Margin::symmetric(12.0, 7.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                if let Some(tex) = icon {
                                    ui.add(
                                        egui::Image::new(*tex)
                                            .fit_to_exact_size(egui::vec2(22.0, 22.0))
                                            .rounding(11.0),
                                    );
                                    ui.add_space(4.0);
                                }
                                ui.label(RichText::new(*ticker).size(13.0).strong().color(INK));
                                ui.label(RichText::new(*name).size(12.5).color(DIM));
                                ui.add_space(6.0);
                                ui.label(RichText::new(*addr).monospace().size(11.5).color(INK));

                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let copied = self.copied_target.as_ref().map_or(false, |(k, t)| {
                                        k == *ticker && t.elapsed() < Duration::from_secs(2)
                                    });
                                    if secondary(ui, if copied { "Copied!" } else { "Copy" }) {
                                        copy_text(addr);
                                        self.copied_target = Some((ticker.to_string(), Instant::now()));
                                    }
                                });
                            });
                        });
                }
            });
            ui.add_space(16.0);
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

    fn render_updates(&mut self, ui: &mut egui::Ui) {
        let state = self.update_manager.state();

        ScrollArea::vertical().id_salt("app_scroll_5").auto_shrink([false, false]).show(ui, |ui| {


            page_header(
                ui,
                "Updates",
                "Release channels and software updates.",
            );


            card(ui, None, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Installed  v{}", env!("CARGO_PKG_VERSION"))).size(14.0).color(INK));

                    ui.add_space(6.0);


                    match &state {
                        crate::update::UpdateState::Idle
                        | crate::update::UpdateState::UpToDate { .. } => {
                            ui.label(RichText::new("·").size(14.0).color(DIM));
                            ui.label(RichText::new("up to date").size(13.0).color(MINT));
                        }
                        crate::update::UpdateState::Checking => {
                            ui.label(RichText::new("·").size(14.0).color(DIM));
                            ui.label(RichText::new("checking...").size(13.0).color(DIM));
                        }
                        crate::update::UpdateState::Available(rel) => {
                            ui.label(RichText::new("·").size(14.0).color(DIM));
                            ui.label(RichText::new(format!("v{} available", rel.version)).size(13.0).color(MINT).strong());
                        }
                        crate::update::UpdateState::Downloading { progress, .. } => {
                            ui.label(RichText::new("·").size(14.0).color(DIM));
                            ui.label(RichText::new(format!("downloading {:.0}%", progress * 100.0)).size(13.0).color(MINT));
                        }
                        crate::update::UpdateState::ReadyToInstall(rel, _) => {
                            ui.label(RichText::new("·").size(14.0).color(DIM));
                            ui.label(RichText::new(format!("v{} ready to apply", rel.version)).size(13.0).color(MINT).strong());
                        }
                        crate::update::UpdateState::Installing
                        | crate::update::UpdateState::RestartRequired => {
                            ui.label(RichText::new("·").size(14.0).color(DIM));
                            ui.label(RichText::new("restarting...").size(13.0).color(MINT));
                        }
                        crate::update::UpdateState::Failed { .. } => {
                            ui.label(RichText::new("·").size(14.0).color(DIM));
                            ui.label(RichText::new("check failed").size(13.0).color(ERR));
                        }
                        crate::update::UpdateState::Cancelled => {
                            ui.label(RichText::new("·").size(14.0).color(DIM));
                            ui.label(RichText::new("cancelled").size(13.0).color(DIM));
                        }
                    }


                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let mut channel = self.config.update_channel.clone();
                        let channel_options = [
                            ("nightly", "nightly"),
                            ("beta", "beta"),
                            ("stable", "stable"),
                        ];
                        if segmented(ui, &mut channel, &channel_options) {
                            self.config.update_channel = channel;
                            save_config(&self.config);
                            self.check_for_updates_action();
                        }
                    });
                });

                ui.add_space(10.0);


                ui.horizontal(|ui| {
                    match &state {
                        crate::update::UpdateState::Idle
                        | crate::update::UpdateState::UpToDate { .. }
                        | crate::update::UpdateState::Cancelled => {
                            if primary(ui, "Check for updates", true) {
                                self.check_for_updates_action();
                            }
                        }
                        crate::update::UpdateState::Checking => {
                            primary(ui, "Checking...", false);
                        }
                        crate::update::UpdateState::Available(rel) => {
                            let rel_clone = rel.clone();
                            if primary(ui, "Download & install", true) {
                                self.update_manager.start_download(rel_clone);
                            }
                            if secondary(ui, "Check again") {
                                self.check_for_updates_action();
                            }
                        }
                        crate::update::UpdateState::Downloading { .. } => {
                            if secondary(ui, "Cancel download") {
                                self.update_manager.cancel_download();
                            }
                        }
                        crate::update::UpdateState::ReadyToInstall(rel, staged_path) => {
                            let rel_clone = rel.clone();
                            let path_clone = staged_path.clone();
                            if primary(ui, "Restart & apply", true) {
                                self.apply_update_and_restart(&rel_clone, &path_clone);
                            }
                        }
                        crate::update::UpdateState::Installing
                        | crate::update::UpdateState::RestartRequired => {
                            primary(ui, "Restarting...", false);
                        }
                        crate::update::UpdateState::Failed { .. } => {
                            if primary(ui, "Retry check", true) {
                                self.check_for_updates_action();
                            }
                        }
                    }
                });

                if let crate::update::UpdateState::Failed { error, .. } = &state {
                    ui.add_space(8.0);
                    ui.label(RichText::new(error).size(12.0).color(ERR));
                }


                if let crate::update::UpdateState::Downloading {
                    downloaded_bytes,
                    total_bytes,
                    progress,
                    speed_bps,
                } = &state
                {
                    ui.add_space(8.0);
                    let (rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 6.0),
                        Sense::hover(),
                    );
                    ui.painter().rect_filled(rect, Rounding::same(3.0), FIELD_LO);
                    let fill_w = (rect.width() * progress.clamp(0.0, 1.0)).clamp(0.0, rect.width());
                    if fill_w > 0.0 {
                        let fill_rect = egui::Rect::from_min_size(rect.min, Vec2::new(fill_w, 6.0));
                        ui.painter().rect_filled(fill_rect, Rounding::same(3.0), MINT);
                    }
                    ui.add_space(4.0);
                    let dl_mb = *downloaded_bytes as f64 / (1024.0 * 1024.0);
                    let tot_mb = *total_bytes as f64 / (1024.0 * 1024.0);
                    let spd_mb = *speed_bps as f64 / (1024.0 * 1024.0);
                    ui.label(muted(format!("{:.1} / {:.1} MB · {:.1} MB/s · {:.0}%", dl_mb, tot_mb, spd_mb, progress * 100.0)));
                }
            });

            ui.add_space(10.0);


            let release_opt = match &state {
                crate::update::UpdateState::Available(rel) => Some(rel),
                crate::update::UpdateState::ReadyToInstall(rel, _) => Some(rel),
                _ => None,
            };

            if let Some(rel) = release_opt {
                card(ui, Some("Release notes"), |ui| {
                    ui.horizontal(|ui| {
                        let title = if rel.title.is_empty() {
                            format!("v{}", rel.version)
                        } else {
                            rel.title.clone()
                        };
                        ui.label(RichText::new(&title).size(15.0).color(INK).strong());
                        if !rel.release_date.is_empty() {
                            ui.label(muted(&rel.release_date));
                        }
                    });
                    ui.add_space(6.0);
                    for line in rel.notes.lines() {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        let bullet = trimmed.trim_start_matches(|c| c == '•' || c == '-' || c == '*' || c == ' ');
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("•").size(13.0).color(MINT));
                            ui.label(RichText::new(bullet).size(13.0).color(INK));
                        });
                    }
                });
                ui.add_space(10.0);
            }


            card(ui, Some("Preferences"), |ui| {
                let prev_auto = self.config.auto_update;
                toggle(
                    ui,
                    &mut self.config.auto_update,
                    "Automatically check for updates on startup",
                );
                if prev_auto != self.config.auto_update {
                    save_config(&self.config);
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(muted("Update source"));
                    let mut source_type = self.config.update_source_type.clone();
                    let sources = [
                        ("master", "default"),
                        ("github", "github"),
                        ("direct", "custom url"),
                    ];
                    if segmented(ui, &mut source_type, &sources) {
                        self.config.update_source_type = source_type;
                        save_config(&self.config);
                    }
                });

                if self.config.update_source_type == "direct" {
                    ui.add_space(6.0);
                    let mut custom_url = self.config.update_source_url.clone();
                    let changed = text_input(
                        ui,
                        egui::Id::new("custom_update_source_url"),
                        &mut custom_url,
                        "https://example.com/manifest.json",
                        f32::INFINITY,
                    )
                    .changed();
                    if changed {
                        self.config.update_source_url = custom_url;
                        save_config(&self.config);
                    }
                } else if self.config.update_source_type == "master" {
                    ui.add_space(4.0);
                    let host = if self.config.update_source_url.is_empty() {
                        "https://updates.puppytunnel.vip"
                    } else {
                        &self.config.update_source_url
                    };
                    ui.label(muted(format!("Endpoint: {}", host)));
                } else if self.config.update_source_type == "github" {
                    ui.add_space(4.0);
                    ui.label(muted("Endpoint: github.com/PuppyTunnel/puppytunnel"));
                }
            });

            ui.add_space(10.0);


            if !self.config.update_history.is_empty() {
                card(ui, Some("History"), |ui| {
                    for entry in self.config.update_history.iter().rev().take(5) {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&entry.version).size(13.0).color(MINT).strong());
                            ui.label(muted("·"));
                            ui.label(RichText::new(&entry.title).size(13.0).color(INK));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(muted(&entry.installed_at));
                            });
                        });
                        ui.add_space(3.0);
                    }
                });
                ui.add_space(10.0);
            }
        });
    }
}

fn notes(ui: &mut egui::Ui, addr: &str, relay: &str) {
    if !relay.trim().is_empty() {
        ui.label(RichText::new("Traffic goes through your relay. Players never see your IP.").size(12.5).color(MINT));
        return;
    }
    for w in tunnel_addr_warnings(addr) {
        ui.label(RichText::new(w).size(12.5).color(WARN));
    }
}

fn path_field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str, mut browse: impl FnMut()) {
    ui.label(muted(label));
    ui.horizontal(|ui| {
        let w = ui.available_width() - 96.0;
        text_input(ui, egui::Id::new(("path", label)), value, hint, w);
        if secondary(ui, "Browse") {
            browse();
        }
    });
    ui.add_space(6.0);
}
