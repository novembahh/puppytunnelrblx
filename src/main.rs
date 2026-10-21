pub mod boombox;
pub mod bridge;
pub mod community;
pub mod config;
pub mod identity;
pub mod master;
pub mod net;
pub mod privacy;
pub mod profile;
pub mod rbxapi;
pub mod roblox;
pub mod setup;
#[cfg(feature = "gui")]
pub mod ui;
pub mod update;

use clap::{Parser, Subcommand};
use config::{load_config, save_config, session_log_name, split_host_port, ts, write_session_log, PROXY_PORT, RELAY_PORT};
use identity::Identity;
use update::source::UpdateSourceFetcher;
use net::relay::{make_invite, parse_invite, RelayServer};
use net::{
    run_connectivity_test, run_echo_test, start_proxy, start_proxy_relay, warm_tunnel, EchoServer,
    ProxyHandle,
};
use roblox::{get_studio_path, inject_map, launch_client, launch_server, watch_media_requests};
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "puppytunnel", version = "beta 1.0")]
#[command(about = "PuppyTunnel: host and join Roblox Studio sessions through a tunnel or private relay")]
#[command(after_help = "Report issues on https://github.com/novembahh/puppytunnelrblx/issues/")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,

    #[arg(long)]
    headless: bool,

    #[arg(long)]
    host: bool,

    #[arg(long)]
    nep_host: bool,

    #[arg(long)]
    join: Option<String>,

    #[arg(long)]
    nep_join: Option<String>,

    #[arg(long)]
    echo_server: Option<u16>,

    #[arg(long)]
    echo_client: Option<String>,

    #[arg(long)]
    test: Option<String>,

    #[arg(long, global = true)]
    map: Option<String>,

    #[arg(long, global = true)]
    uid: Option<String>,

    #[arg(long, global = true)]
    port: Option<String>,

    #[arg(long, global = true)]
    relay: Option<String>,

    #[arg(long, global = true)]
    master: Option<String>,

    #[arg(long, global = true)]
    list: bool,

    #[arg(long, global = true)]
    name: Option<String>,

    #[arg(long, global = true)]
    desc: Option<String>,

    #[arg(long, global = true)]
    tags: Option<String>,

    #[arg(long, global = true)]
    max_players: Option<u32>,

    #[arg(long, global = true)]
    external_studio: bool,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    Relay {
        #[arg(long, default_value_t = format!("0.0.0.0:{}", RELAY_PORT))]
        listen: String,
        #[arg(long)]
        with_master: Option<String>,
        #[arg(long)]
        strict_profiles: bool,
        #[arg(long)]
        trust_proxy: bool,
        #[arg(long)]
        peer: Vec<String>,
        #[arg(long)]
        allow_private_relays: bool,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        pool_ports: usize,
    },
    Master {
        #[arg(long)]
        strict_profiles: bool,
        #[arg(long, default_value = "0.0.0.0:8080")]
        listen: String,
        #[arg(long)]
        trust_proxy: bool,
        #[arg(long)]
        peer: Vec<String>,
        #[arg(long)]
        allow_private_relays: bool,
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
    Browse {
        query: Option<String>,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long, default_value = "players")]
        sort: String,
    },
    Fleet {
        #[arg(long, default_value_t = 2)]
        count: u16,
        #[arg(long, default_value_t = 8)]
        stagger: u64,
    },
    Host {
        #[arg(long, alias = "nep")]
        media: bool,
    },
    Join {
        target: String,
        #[arg(long, alias = "nep")]
        media: bool,
    },
    Doctor,
    Profile {
        #[arg(long)]
        roblox: Option<String>,
        #[arg(long)]
        bio: Option<String>,
        #[arg(long)]
        accent: Option<String>,
        #[arg(long)]
        servers: Option<String>,
        #[arg(long)]
        publish: bool,
    },
    Community {
        #[arg(long)]
        instance: Option<String>,

        #[command(subcommand)]
        sub: CommunityCmd,
    },
    Update {
        #[arg(long)]
        check: bool,

        #[arg(long)]
        install: bool,

        #[arg(long)]
        channel: Option<String>,

        #[arg(long)]
        source: Option<String>,

        #[arg(long)]
        history: bool,
    },
    Setup,
    Status,
    Echo {
        #[arg(long)]
        server: Option<u16>,
        #[arg(long)]
        client: Option<String>,
    },
    Test {
        target: String,
        #[arg(long)]
        host: bool,
        #[arg(long)]
        port: Option<u16>,
    },
}

#[derive(Subcommand, Debug)]
pub enum CommunityCmd {
    Feed {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Post {
        content: String,
        #[arg(long)]
        tag: Option<String>,
    },
    Dm {
        target: String,
        message: String,
    },
    Dms {
        #[arg(long)]
        conv: Option<String>,
    },
    Guilds,
    Channel {
        id: String,
        #[arg(long)]
        send: Option<String>,
        #[arg(long)]
        guild: Option<String>,
    },
    ExportKeys {
        #[arg(long)]
        file: Option<PathBuf>,
    },
    ImportKeys {
        input: String,
    },
    Words,
    Ban {
        target: String,
        #[arg(long, default_value = "Violation of community guidelines")]
        reason: String,
    },
    Reports,
    AuditLog,
}

fn main() {
    let mut args = Cli::parse();

    let mut headless = args.headless
        || args.host
        || args.nep_host
        || args.join.is_some()
        || args.nep_join.is_some()
        || args.echo_server.is_some()
        || args.echo_client.is_some()
        || args.test.is_some();

    let mut relay_listen: Option<(String, Option<String>, usize, master::MasterOptions)> = None;
    let mut doctor = false;
    let mut profile_cmd = None;
    let mut master_cmd: Option<(String, master::MasterOptions)> = None;
    let mut browse: Option<(Option<String>, Option<String>, String)> = None;
    let mut fleet: Option<(u16, u64)> = None;
    if let Some(cmd) = args.cmd.take() {
        headless = true;
        match cmd {
            Cmd::Relay { listen, with_master, trust_proxy, peer, allow_private_relays, data_dir, strict_profiles, pool_ports } => {
                relay_listen = Some((listen, with_master, pool_ports, master::MasterOptions { trust_proxy, peers: peer, allow_private_relays, strict_profiles, roblox_api: None, persist_profiles_cache: true, data_dir: data_dir.clone(), ..Default::default() }))
            }
            Cmd::Master { listen, trust_proxy, peer, allow_private_relays, data_dir, strict_profiles } => {
                master_cmd = Some((listen, master::MasterOptions { trust_proxy, peers: peer, allow_private_relays, strict_profiles, roblox_api: None, persist_profiles_cache: true, data_dir: data_dir.clone(), ..Default::default() }))
            }
            Cmd::Browse { query, tag, sort } => browse = Some((query, tag, sort)),
            Cmd::Fleet { count, stagger } => fleet = Some((count, stagger)),
            Cmd::Host { media } => {
                args.host = !media;
                args.nep_host = media;
            }
            Cmd::Join { target, media } => {
                if media {
                    args.nep_join = Some(target);
                } else {
                    args.join = Some(target);
                }
            }
            Cmd::Doctor => doctor = true,
            Cmd::Profile { roblox, bio, accent, servers, publish } => profile_cmd = Some((roblox, bio, accent, servers, publish)),
            Cmd::Community { instance, sub } => std::process::exit(run_community_cli(instance, sub)),
            Cmd::Update { check, install, channel, source, history } => {
                std::process::exit(run_update_cli(check, install, channel, source, history));
            }
            Cmd::Setup => {
                std::process::exit(setup::run_setup());
            }
            Cmd::Status => {
                std::process::exit(run_status_cli(&args));
            }
            Cmd::Echo { server, client } => {
                if server.is_some() {
                    args.echo_server = server;
                }
                if client.is_some() {
                    args.echo_client = client;
                }
            }
            Cmd::Test { target, host, port } => {
                std::process::exit(run_test_cli(&target, host, port));
            }
        }
    }

    if headless {
        let code = if let Some((listen, with_master, pool, opts)) = relay_listen {
            run_relay(&listen, with_master.as_deref(), pool, opts)
        } else if let Some((listen, opts)) = master_cmd {
            run_master(&listen, opts)
        } else if let Some((query, tag, sort)) = browse {
            run_browse(&args, query.as_deref().unwrap_or(""), tag.as_deref().unwrap_or(""), &sort)
        } else if let Some((count, stagger)) = fleet {
            run_fleet(args, count, stagger)
        } else if let Some((roblox, bio, accent, servers, publish)) = profile_cmd {
            run_profile(roblox, bio, accent, servers, publish)
        } else if doctor {
            run_doctor(&args)
        } else {
            run_headless(args)
        };
        std::process::exit(code);
    }

    launch_gui();
}

#[cfg(feature = "gui")]
fn launch_gui() {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Console::FreeConsole();
    }
    let icon = image::load_from_memory(include_bytes!("../assets/icon/puppytunnel.png"))
        .ok()
        .map(|img| {
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            eframe::egui::IconData { rgba: rgba.into_raw(), width, height }
        });
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("PuppyTunnel")
            .with_icon(icon.unwrap_or_default())
            .with_decorations(false)
            .with_inner_size([900.0, 680.0])
            .with_min_inner_size([640.0, 520.0]),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "PuppyTunnel",
        options,
        Box::new(|cc| Ok(Box::new(ui::NepApp::new(cc)))),
    ) {
        eprintln!("GUI failed: {}", e);
        std::process::exit(1);
    }
}

#[cfg(not(feature = "gui"))]
fn launch_gui() {
    eprintln!("This build has no GUI. Run a subcommand (see --help), e.g. `puppytunnel relay`.");
    std::process::exit(2);
}

fn stop_flag() -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let s = Arc::clone(&stop);
    let _ = ctrlc::set_handler(move || s.store(true, Ordering::SeqCst));
    stop
}

fn sleep_until_stopped(stop: &AtomicBool) {
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn wait_child(child: &mut Child, stop: &AtomicBool) {
    while !stop.load(Ordering::SeqCst) {
        match child.try_wait() {
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            _ => return,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn log_line(msg: &str, _tag: &str) {
    println!("[{}] {}", ts(), msg);
}

fn resolve_relay(relay: &str) -> Result<SocketAddr, String> {
    relay
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve relay '{}': {}", relay, e))?
        .find(|a| a.is_ipv4())
        .ok_or_else(|| format!("relay '{}' has no IPv4 address", relay))
}

fn run_master(listen: &str, opts: master::MasterOptions) -> i32 {
    let stop = stop_flag();
    match master::MasterServer::start(listen, opts) {
        Ok(mut m) => {
            println!("[{}] Master server on http://{} (web list at /). Ctrl+C to stop.", ts(), listen);
            println!("[{}] Cloudflare: proxy this HTTP port freely. Do NOT proxy the UDP relay port through it.", ts());
            sleep_until_stopped(&stop);
            m.stop();
            0
        }
        Err(e) => {
            eprintln!("{}", e);
            1
        }
    }
}

fn run_browse(args: &Cli, query: &str, tag: &str, sort: &str) -> i32 {
    let config = load_config();
    let masters = args.master.clone().unwrap_or(config.master);
    let (list, errors) = master::fetch_all(&masters, query, tag, sort);
    for e in &errors {
        eprintln!("warning: master unreachable: {}", e);
    }
    if list.is_empty() {
        println!("No servers found.");
        return if errors.is_empty() { 0 } else { 1 };
    }
    println!("{:<28} {:<16} {:>9}  {:<20} INVITE", "NAME", "MAP", "PLAYERS", "TAGS");
    for s in &list {
        let l = &s.listing;
        let cap = if l.max_players > 0 { format!("{}/{}", l.players, l.max_players) } else { l.players.to_string() };
        println!("{:<28} {:<16} {:>9}  {:<20} {}", l.name, l.map, cap, l.tags.join(","), l.invite);
    }
    println!("\nJoin one with: puppytunnel join <INVITE>");
    0
}

fn run_fleet(args: Cli, count: u16, stagger: u64) -> i32 {
    let mut config = load_config();
    apply_overrides(&mut config, &args);
    let base: u16 = match config.port.trim().parse() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Server port '{}' is not a valid port", config.port);
            return 2;
        }
    };
    if count == 0 || u32::from(base) + u32::from(count) > 65535 {
        eprintln!("--count must be 1 or more and fit within the port range");
        return 2;
    }
    if config.relay.trim().is_empty() {
        eprintln!("fleet needs --relay so every server gets its own invite.");
        return 2;
    }
    let studio = if config.studio.is_empty() { get_studio_path() } else { config.studio.clone() };
    let stop = stop_flag();
    let nep = args.nep_host;
    let mut handles = Vec::new();
    for i in 0..count {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let mut c = config.clone();
        c.port = (base + i).to_string();
        let name = if c.server_name.is_empty() { "PuppyTunnel server".to_string() } else { c.server_name.clone() };
        c.server_name = format!("{} #{}", name, i + 1);
        let (studio_c, stop_c) = (studio.clone(), Arc::clone(&stop));
        handles.push(std::thread::spawn(move || {
            run_host(&c, &studio_c, nep, &stop_c, args.external_studio);
        }));
        for _ in 0..stagger * 5 {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    for h in handles {
        let _ = h.join();
    }
    0
}

fn run_relay(listen: &str, with_master: Option<&str>, pool: usize, opts: master::MasterOptions) -> i32 {
    let addr: SocketAddr = match listen.parse() {
        Ok(a) => a,
        Err(_) => {
            eprintln!("Invalid --listen address '{}'. Expected ip:port", listen);
            return 2;
        }
    };
    let stop = stop_flag();
    let mut master = None;
    if let Some(ml) = with_master {
        match master::MasterServer::start(ml, opts) {
            Ok(m) => {
                println!("[{}] Master server list on http://{} (put Cloudflare in front of this one).", ts(), ml);
                master = Some(m);
            }
            Err(e) => {
                eprintln!("{}", e);
                return 1;
            }
        }
    }
    match RelayServer::start_with_ingress(addr, pool, |m| println!("[{}] relay: {}", ts(), m)) {
        Ok(mut relay) => {
            println!("[{}] Relay listening on udp://{}. Ctrl+C to stop.", ts(), addr);
            println!("[{}] Open this UDP port on the VPS firewall. Hosts run: puppytunnel host --relay <this-ip>:{}", ts(), addr.port());
            if pool > 0 {
                println!(
                    "[{}] Game slots use udp/{}-{}. Forward that whole range to this host for plain Neptunnel joins.",
                    ts(),
                    addr.port() + 1,
                    addr.port() + pool as u16
                );
            }
            sleep_until_stopped(&stop);
            relay.stop();
            drop(master);
            0
        }
        Err(e) => {
            eprintln!("Failed to start relay on {}: {}", addr, e);
            1
        }
    }
}

fn port_free(port: u16) -> bool {
    !crate::net::udp_port_in_use(port)
}

fn run_doctor(args: &Cli) -> i32 {
    let config = load_config();
    let studio = if config.studio.is_empty() { get_studio_path() } else { config.studio.clone() };
    println!("Roblox Studio : {}", if studio.is_empty() { "NOT FOUND (set `studio` in puppytunnel.json)" } else { &studio });
    println!("Proxy  :{}  {}", PROXY_PORT, if port_free(PROXY_PORT) { "free" } else { "IN USE" });
    let relay = args.relay.clone().unwrap_or(config.relay.clone());
    println!("Relay         : {}", if relay.is_empty() { "none (direct tunnel)" } else { &relay });
    println!("Tunnel address: {}", config.addr);
    let warnings = privacy::tunnel_addr_warnings(&config.addr);
    for w in &warnings {
        println!("  ! {}", w);
    }
    println!("Privacy: PuppyTunnel contacts no third-party servers on its own. Only media fetches");
    println!("         triggered by --nep-join, and the Roblox API for avatar lookups, go out.");
    if relay.is_empty() && warnings.is_empty() {
        println!("Tip: `puppytunnel host --relay <vps>:{}` hides your IP from joiners.", RELAY_PORT);
    }
    0
}

fn start_join_proxy(target: &str) -> Result<(ProxyHandle, String), String> {
    if let Some((rh, rp, token)) = parse_invite(target) {
        net::relay::check_invite_live(target)?;
        let h = start_proxy_relay(PROXY_PORT, &rh, rp, token)
            .map_err(|e| format!("Failed to start UDP proxy: {}", e))?;
        return Ok((h, format!("relay {}:{}", rh, rp)));
    }
    let Some((host, port)) = split_host_port(target) else {
        return Err("Format must be host:port or a puppy:// invite".to_string());
    };
    for w in privacy::tunnel_addr_warnings(target) {
        println!("[{}] ! {}", ts(), w);
    }
    let h = start_proxy(PROXY_PORT, &host, port)
        .map_err(|e| format!("Failed to start UDP proxy (port {} busy?): {}", PROXY_PORT, e))?;
    Ok((h, format!("{}:{}", host, port)))
}

fn run_profile(roblox: Option<String>, bio: Option<String>, accent: Option<String>, servers: Option<String>, publish: bool) -> i32 {
    let mut config = load_config();
    if let Some(q) = roblox {
        match rbxapi::lookup(rbxapi::USERS_API, &q) {
            Ok(u) => {
                config.roblox_id = u.id;
                config.roblox_name = u.display_name.clone();
                config.uid = u.id.to_string();
                println!("Roblox account: {} (@{}), id {}", u.display_name, u.name, u.id);
            }
            Err(e) => {
                eprintln!("Roblox lookup failed: {}", e);
                return 1;
            }
        }
    }
    if let Some(b) = bio {
        config.profile_bio = b;
    }
    if let Some(a) = accent {
        config.profile_accent = a;
    }
    if let Some(s) = servers {
        config.profile_servers = s;
    }
    config::save_config(&config);

    if config.roblox_id == 0 {
        eprintln!("No Roblox account set. Run: puppytunnel profile --roblox <username or id>");
        return 2;
    }
    let pid = profile::profile_identity(&config);
    let pk = identity::hex(&pid.public());
    let code = profile::verify_code(&pk, config.roblox_id);
    println!("Profile id    : {}", &pk[..8]);
    println!("Accent        : {}", profile::clean_accent(&config.profile_accent));
    println!("Verification  : put this anywhere in your Roblox profile About text: {}", code);

    if publish {
        let targets = profile::publish_targets(&config);
        let signed = profile::SignedProfile::sign(&pid, profile::body_from_config(&config));
        let errors = profile::publish(&targets, &signed);
        for e in &errors {
            eprintln!("publish failed: {}", e);
        }
        if errors.len() == targets.len() {
            return 1;
        }
        std::thread::sleep(Duration::from_secs(2));
        for t in &targets {
            match profile::status(t, &pk) {
                Ok(p) if p.verified => println!("{} : verified as {} (@{})", t, p.display_name, p.name),
                Ok(_) => println!("{} : published, not verified yet (add the code to your Roblox About, then run again)", t),
                Err(e) => println!("{} : could not read status: {}", t, e),
            }
        }
    }
    0
}

fn run_status_cli(args: &Cli) -> i32 {
    let config = load_config();
    let current_version = update::CURRENT_VERSION;
    println!("=== PuppyTunnel Status ===");
    println!("Version         : v{}", current_version);
    println!("Update channel  : {}", config.update_channel);
    println!("Update source   : {} ({})", config.update_source_type, if config.update_source_url.is_empty() { "default" } else { &config.update_source_url });

    let studio = if config.studio.is_empty() { get_studio_path() } else { config.studio.clone() };
    println!("Roblox Studio   : {}", if studio.is_empty() { "NOT FOUND" } else { &studio });
    println!("Local proxy     : 127.0.0.1:{} ({})", PROXY_PORT, if port_free(PROXY_PORT) { "free" } else { "IN USE / RUNNING" });

    let relay = args.relay.clone().unwrap_or(config.relay.clone());
    println!("Configured relay: {}", if relay.is_empty() { "none (direct)" } else { &relay });
    println!("Default master  : {}", config.master);
    println!("User UID        : {}", if config.uid.is_empty() { "none" } else { &config.uid });

    let identity = Identity::profile(&config.server_secret);
    println!("Node Public Key : {}", identity::hex(&identity.public()));
    0
}

fn run_test_cli(target: &str, is_host: bool, local_port: Option<u16>) -> i32 {
    let (host, port) = match split_host_port(target) {
        Some(hp) => hp,
        None => {
            eprintln!("[fail] Target must be in host:port format");
            return 2;
        }
    };
    run_connectivity_test(&host, port, is_host, local_port, !port_free(PROXY_PORT), log_line);
    0
}

fn run_update_cli(
    mut check_only: bool,
    install: bool,
    channel_opt: Option<String>,
    source_opt: Option<String>,
    history: bool,
) -> i32 {
    let mut config = load_config();

    if history {
        println!("Update History:");
        if config.update_history.is_empty() {
            println!("  (No updates recorded)");
        } else {
            for entry in config.update_history.iter().rev() {
                println!("  * v{:<12} {:<24} {}", entry.version, entry.title, entry.installed_at);
            }
        }
        return 0;
    }

    let mut dirty = false;
    if let Some(ch) = &channel_opt {
        let ch_lower = ch.to_lowercase();
        if ["stable", "beta", "nightly"].contains(&ch_lower.as_str()) {
            config.update_channel = ch_lower;
            dirty = true;
        } else {
            eprintln!("[fail] Invalid channel '{}'. Options: stable, beta, nightly", ch);
            return 2;
        }
    }

    if let Some(src) = &source_opt {
        if src.starts_with("http://") || src.starts_with("https://") {
            if src.ends_with(".json") {
                config.update_source_type = "direct".into();
                config.update_source_url = src.clone();
            } else {
                config.update_source_type = "master".into();
                config.update_source_url = src.clone();
            }
            dirty = true;
        } else if src == "github" {
            config.update_source_type = "github".into();
            config.update_source_url = "".into();
            dirty = true;
        } else if src == "default" || src == "master" || src == "github" {
            config.update_source_type = "github".into();
            config.update_source_url = "".into();
            dirty = true;
        } else {
            eprintln!("[fail] Invalid source '{}'. Provide a URL or 'github' or 'default'.", src);
            return 2;
        }
    }

    if dirty {
        save_config(&config);
        println!("[ok] Configuration updated.");
    }

    if !check_only && !install {
        check_only = true;
    }

    use std::str::FromStr;
    let ch = update::Channel::from_str(&config.update_channel).unwrap_or(update::Channel::Beta);
    let src = update::UpdateSource::from_config(&config.update_source_type, &config.update_source_url);
    let current_version = update::CURRENT_VERSION;

    let endpoint_display = match &src {
        update::UpdateSource::MasterApi(url) => url.clone(),
        update::UpdateSource::GitHubReleases { owner, repo } => format!("https://api.github.com/repos/{}/{}", owner, repo),
        update::UpdateSource::DirectUrl(url) => url.clone(),
    };

    println!("Checking for updates...");
    println!("  Current version : v{}", current_version);
    println!("  Channel         : {}", config.update_channel);
    println!("  Source endpoint : {}", endpoint_display);

    let manifest = match src.fetch_manifest(ch) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("\n[fail] Update check failed: {}", e);
            return 1;
        }
    };

    let release_opt = manifest.latest_for_channel(ch);
    let update_available = match release_opt {
        Some(rel) => update::is_newer(current_version, &rel.version),
        None => false,
    };

    if !update_available {
        println!("\n[ok] PuppyTunnel v{} is up to date.", current_version);
        return 0;
    }

    let release = release_opt.unwrap();
    println!("\n[ok] Update available: v{} ({})", release.version, release.title);
    if !release.release_date.is_empty() {
        println!("  Release date: {}", release.release_date);
    }
    if !release.notes.is_empty() {
        println!("\nRelease notes:");
        for line in release.notes.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                println!("  - {}", trimmed.trim_start_matches(|c| c == '•' || c == '-' || c == '*' || c == ' '));
            }
        }
    }

    if check_only && !install {
        println!("\nRun `puppytunnel update --install` to download and apply this update.");
        return 0;
    }

    let asset = release.find_asset_for_current_platform();
    let Some(asset) = asset else {
        eprintln!("\n[fail] No compatible build in release for current platform");
        return 1;
    };

    println!("\nDownloading {} ({:.1} MB)...", asset.name, asset.size as f64 / (1024.0 * 1024.0));
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let dl_dir = std::env::temp_dir().join("puppytunnel_downloads");
    let temp_path = match update::download_asset(
        asset,
        &dl_dir,
        cancel_flag,
        |prog| {
            let mb = prog.downloaded_bytes as f64 / (1024.0 * 1024.0);
            let tot = prog.total_bytes as f64 / (1024.0 * 1024.0);
            let spd = prog.speed_bps as f64 / (1024.0 * 1024.0);
            let pct = (prog.progress * 100.0) as u32;
            let bar_len = 24;
            let filled = ((pct as usize * bar_len) / 100).min(bar_len);
            let empty = bar_len - filled;
            print!("\r  [{}{}] {:>3}%  {:.1} / {:.1} MB  ({:.1} MB/s)",
                "=".repeat(filled),
                " ".repeat(empty),
                pct, mb, tot, spd);
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }
    ) {
        Ok(p) => {
            println!();
            p
        }
        Err(e) => {
            println!();
            eprintln!("[fail] Download failed: {}", e);
            return 1;
        }
    };

    print!("Verifying integrity (SHA-256 + Ed25519 signature)... ");
    use std::io::Write;
    let _ = std::io::stdout().flush();
    if let Err(e) = update::verify_and_stage(&temp_path, asset, None) {
        println!("[fail]");
        eprintln!("[fail] Cryptographic verification failed: {}", e);
        return 1;
    }
    println!("[ok]");

    print!("Applying update to executable... ");
    let _ = std::io::stdout().flush();
    if let Err(e) = update::apply_update(&temp_path) {
        println!("[fail]");
        eprintln!("[fail] Failed to apply update: {}", e);
        return 1;
    }
    println!("[ok]");

    config.update_history.push(update::UpdateHistoryEntry {
        version: release.version.clone(),
        installed_at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
        previous_version: current_version.to_string(),
        channel: config.update_channel.clone(),
        status: "success".to_string(),
        title: release.title.clone(),
    });
    save_config(&config);

    println!("\n[ok] Successfully updated to v{}!", release.version);
    println!("Restart puppytunnel to run the new version.");
    0
}

fn run_community_cli(instance_override: Option<String>, sub: CommunityCmd) -> i32 {
    let mut config = load_config();
    let instance_url = instance_override
        .or_else(|| {
            if !config.community_instance.is_empty() {
                Some(config.community_instance.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "https://puppytunnel-community.puppytunnel.workers.dev".to_string());
    let id = if !config.server_secret.is_empty() {
        Some(Identity::profile(&config.server_secret))
    } else {
        None
    };
    let client = crate::community::client::CommunityClient::new(&instance_url, id);

    match sub {
        CommunityCmd::Feed { limit } => {
            println!("Feed from {}:", instance_url);
            match client.get_feed(limit, None) {
                Ok(posts) => {
                    if posts.is_empty() {
                        println!("No posts yet.");
                    } else {
                        for p in posts {
                            let author = if !p.author_handle.is_empty() {
                                p.author_handle
                            } else {
                                format!("pup_{}", &p.author_pubkey[..p.author_pubkey.len().min(8)])
                            };
                            println!("--------------------------------------------------");
                            println!("@{}  (likes: {}  reposts: {}  replies: {})", author, p.likes_count, p.reposts_count, p.replies_count);
                            println!("{}", p.content);
                            if !p.tags.is_empty() {
                                println!("tags: {}", p.tags.join(", "));
                            }
                        }
                        println!("--------------------------------------------------");
                    }
                    0
                }
                Err(e) => {
                    eprintln!("Failed to fetch feed: {}", e);
                    1
                }
            }
        }
        CommunityCmd::Post { content, tag } => {
            let tags = tag.map(|t| vec![t]).unwrap_or_default();
            match client.create_post(&content, vec![], None, None, tags, "public") {
                Ok(post) => {
                    println!("Post published! ID: {}", post.id);
                    0
                }
                Err(e) => {
                    eprintln!("Failed to publish post: {}", e);
                    1
                }
            }
        }
        CommunityCmd::Dm { target, message } => {
            if config.server_secret.is_empty() {
                eprintln!("No server secret configured for E2EE.");
                return 1;
            }
            let conv = match client.create_conversation(vec![target.clone()], None) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to create conversation: {}", e);
                    return 1;
                }
            };
            let recipient_x25519 = crate::community::e2ee::get_x25519_public_hex(&config.server_secret);
            match client.send_e2ee_direct_message(&conv.id, &recipient_x25519, &message, &config.server_secret) {
                Ok(_) => {
                    println!("E2EE message sent in conversation {}", conv.id);
                    0
                }
                Err(e) => {
                    eprintln!("Failed to send encrypted message: {}", e);
                    1
                }
            }
        }
        CommunityCmd::Dms { conv } => {
            if let Some(conv_id) = conv {
                match client.get_messages(&conv_id, 50) {
                    Ok(msgs) => {
                        println!("Messages in {}:", conv_id);
                        for m in msgs {
                            let text = if crate::community::e2ee::is_encrypted_payload(&m.content) {
                                crate::community::e2ee::decrypt_direct_message(&config.server_secret, &m.sender_pubkey, &m.content)
                                    .unwrap_or_else(|_| "[Encrypted Message]".to_string())
                            } else {
                                m.content
                            };
                            let sender = &m.sender_pubkey[..m.sender_pubkey.len().min(8)];
                            println!("pup_{}: {}", sender, text);
                        }
                        0
                    }
                    Err(e) => {
                        eprintln!("Failed to fetch messages: {}", e);
                        1
                    }
                }
            } else {
                match client.get_conversations() {
                    Ok(convs) => {
                        println!("Active conversations:");
                        for c in convs {
                            let title = c.title.unwrap_or_else(|| format!("DM ({})", &c.id[..c.id.len().min(8)]));
                            println!("  {} - {}", c.id, title);
                        }
                        0
                    }
                    Err(e) => {
                        eprintln!("Failed to fetch conversations: {}", e);
                        1
                    }
                }
            }
        }
        CommunityCmd::Guilds => {
            println!("Connected instance: {}", instance_url);
            println!("Run `puppytunnel community channel <channel_id>` to view channel messages.");
            0
        }
        CommunityCmd::Channel { id, send, guild } => {
            if let Some(content) = send {
                let gid = guild.unwrap_or_else(|| "default".to_string());
                match client.send_guild_message(&gid, &id, &content, vec![], None) {
                    Ok(_) => {
                        println!("Message sent to channel {}!", id);
                        0
                    }
                    Err(e) => {
                        eprintln!("Failed to send channel message: {}", e);
                        1
                    }
                }
            } else {
                match client.get_guild_messages(&id, 50) {
                    Ok(msgs) => {
                        println!("Messages in channel {}:", id);
                        for m in msgs {
                            let author = &m.author_pubkey[..m.author_pubkey.len().min(8)];
                            println!("pup_{}: {}", author, m.content);
                        }
                        0
                    }
                    Err(e) => {
                        eprintln!("Failed to fetch channel messages: {}", e);
                        1
                    }
                }
            }
        }
        CommunityCmd::ExportKeys { file } => {
            match crate::community::keystore::export_backup_json(&config) {
                Ok(json) => {
                    if let Some(p) = file {
                        if let Err(e) = std::fs::write(&p, &json) {
                            eprintln!("Failed to write backup file: {}", e);
                            return 1;
                        }
                        println!("Backup exported to {}", p.display());
                    } else {
                        println!("{}", json);
                    }
                    0
                }
                Err(e) => {
                    eprintln!("Failed to export backup: {}", e);
                    1
                }
            }
        }
        CommunityCmd::ImportKeys { input } => {
            match crate::community::keystore::import_backup_str(&input, &mut config) {
                Ok(backup) => {
                    config::save_config(&config);
                    println!("Identity restored! Username: {}", backup.username);
                    0
                }
                Err(e) => {
                    eprintln!("Failed to import keys: {}", e);
                    1
                }
            }
        }
        CommunityCmd::Words => {
            if config.server_secret.is_empty() {
                eprintln!("No server secret configured.");
                return 1;
            }
            let words = crate::community::keystore::generate_mnemonic(&config.server_secret);
            println!("32-word Account Recovery Phrase:");
            println!("{}", words);
            0
        }
        CommunityCmd::Ban { target, reason } => {
            match client.moderate(&target, crate::community::types::ModActionType::Ban, &reason, None, None) {
                Ok(entry) => {
                    println!("User {} banned! Action ID: {}", target, entry.id);
                    0
                }
                Err(e) => {
                    eprintln!("Failed to execute ban: {}", e);
                    1
                }
            }
        }
        CommunityCmd::Reports => {
            match client.get_reports() {
                Ok(reports) => {
                    println!("Pending Reports Queue ({}):", reports.len());
                    for r in reports {
                        println!("ID: {} | Target: {:?} {} | Reason: {}", r.id, r.target_type, r.target_id, r.reason);
                        if let Some(d) = r.details {
                            println!("  Details: {}", d);
                        }
                    }
                    0
                }
                Err(e) => {
                    eprintln!("Failed to fetch reports: {}", e);
                    1
                }
            }
        }
        CommunityCmd::AuditLog => {
            match client.get_audit_log() {
                Ok(entries) => {
                    println!("Moderation Audit Trail ({}):", entries.len());
                    for entry in entries {
                        let actor = &entry.actor_pubkey[..entry.actor_pubkey.len().min(8)];
                        println!("{} | Action: {} | by pup_{} | target: {}", entry.created_at, entry.action, actor, entry.target_id);
                    }
                    0
                }
                Err(e) => {
                    eprintln!("Failed to fetch audit log: {}", e);
                    1
                }
            }
        }
    }
}

fn apply_overrides(config: &mut config::AppConfig, args: &Cli) {
    if let Some(v) = &args.relay { config.relay = v.clone(); }
    if let Some(v) = &args.master { config.master = v.clone(); }
    if let Some(v) = &args.name { config.server_name = v.clone(); }
    if let Some(v) = &args.desc { config.server_desc = v.clone(); }
    if let Some(v) = &args.tags { config.server_tags = v.clone(); }
    if let Some(v) = args.max_players { config.max_players = v; }
    if args.list { config.list_public = true; }
}

fn run_headless(args: Cli) -> i32 {
    println!("[{}] PuppyTunnel beta 1.0 starting in headless mode...", ts());
    let mut config = load_config();
    let stop = stop_flag();
    apply_overrides(&mut config, &args);

    if let Some(uid) = args.uid {
        config.uid = uid;
    }
    if let Some(port) = args.port {
        if port.trim().parse::<u16>().is_err() {
            eprintln!("--port must be a number between 1 and 65535");
            return 2;
        }
        config.port = port.trim().to_string();
    }
    if let Some(map) = args.map {
        config.map = map;
    }
    let studio = if config.studio.is_empty() { get_studio_path() } else { config.studio.clone() };

    if let Some(echo_port) = args.echo_server {
        println!("[{}] Starting Echo Server on 0.0.0.0:{}...", ts(), echo_port);
        let mut echo = EchoServer::new();
        if let Err(e) = echo.start(echo_port) {
            eprintln!("[{}] Failed to start echo server: {}", ts(), e);
            return 1;
        }
        println!("[{}] Echo server active. Press Ctrl+C to stop.", ts());
        sleep_until_stopped(&stop);
        echo.stop();
        return 0;
    }

    if let Some(target) = args.echo_client {
        return match split_host_port(&target) {
            Some((h, port)) => {
                println!("[{}] Running Echo Test against {}:{}...", ts(), h, port);
                run_echo_test(&h, port, 10, 12.0, log_line);
                0
            }
            None => {
                eprintln!("Invalid format for --echo-client. Expected host:port");
                2
            }
        };
    }

    if let Some(target) = args.test {
        return match split_host_port(&target) {
            Some((h, port)) => {
                println!("[{}] Running Connectivity Test against {}:{}...", ts(), h, port);
                run_connectivity_test(&h, port, false, None, false, log_line);
                0
            }
            None => {
                eprintln!("Invalid format for --test. Expected host:port");
                2
            }
        };
    }

    if args.host || args.nep_host {
        return run_host(&config, &studio, args.nep_host, &stop, args.external_studio);
    }

    if let Some(target) = args.join.clone().or(args.nep_join.clone()) {
        return run_join(&studio, &target, args.nep_join.is_some(), &stop);
    }

    eprintln!("Nothing to do. Try `puppytunnel host`, `puppytunnel join <host:port|puppy://...>` or `puppytunnel --help`.");
    2
}

fn run_host(config: &config::AppConfig, studio: &str, nep: bool, stop: &Arc<AtomicBool>, external: bool) -> i32 {
    let studio_port: u16 = match config.port.trim().parse() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Server port '{}' is not a valid port", config.port);
            return 2;
        }
    };
    if crate::net::udp_port_in_use(studio_port) {
        eprintln!("Port {} is already in use (another server or app instance?). Stop it or use a different port.", studio_port);
        return 1;
    }
    if studio.is_empty() {
        eprintln!("Roblox Studio not found. Set `studio` in puppytunnel.json.");
        return 1;
    }
    println!(
        "[{}] Starting {} on port {}...",
        ts(),
        if nep { "HOST (media helper)" } else { "HOST" },
        config.port
    );

    let svc = Arc::new(AtomicBool::new(false));
    let pg = config::gen_guid();
    let tg = config::gen_guid();

    let mut boombox = boombox::BoomboxServer::new();
    if nep {
        println!("[{}] Starting LocalBoombox helper on 127.0.0.1:48732...", ts());
        if let Err(e) = boombox.start() {
            eprintln!("[{}] LocalBoombox failed: {}", ts(), e);
        }
    }

    if !config.map.trim().is_empty() {
        let map_path = PathBuf::from(&config.map);
        println!("[{}] Injecting map: {}", ts(), map_path.display());
        if let Err(e) = inject_map(&map_path) {
            eprintln!("[{}] Map injection failed: {}", ts(), e);
            return 1;
        }
    }

    let relay = config.relay.trim();
    let mut invite = None;
    let mut agent = None;
    let mut announcer = None;
    let players = Arc::new(AtomicUsize::new(0));
    let ready = Arc::new(AtomicBool::new(false));
    {
        let (ready_w, stop_w) = (Arc::clone(&ready), Arc::clone(stop));
        std::thread::spawn(move || {
            while !stop_w.load(Ordering::SeqCst) {
                if net::relay::udp_port_bound(studio_port) {
                    if !ready_w.swap(true, Ordering::SeqCst) {
                        println!("[{}] Game port {} is live; the server is now listed.", ts(), studio_port);
                    }
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        });
    }
    if !relay.is_empty() {
        let addr = match resolve_relay(relay) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("{}", e);
                return 1;
            }
        };
        let id = if config.list_public { Identity::stable(&config.server_secret, studio_port) } else { Identity::ephemeral() };
        let inv = make_invite(relay, &id.token());
        let st = Arc::clone(&svc);
        let pl = Arc::clone(&players);
        let agent_id = id.clone();
        let open = config.nep_compat;
        let slot = Arc::new(AtomicUsize::new(0));
        let slot_w = Arc::clone(&slot);
        agent = Some(std::thread::spawn(move || {
            let res = net::relay::run_host_agent_slot(
                addr,
                agent_id,
                studio_port,
                st,
                pl,
                open,
                Some(slot_w),
                log_line,
            );
            if let Err(e) = res {
                eprintln!("relay agent stopped: {}", e);
            }
        }));
        if config.nep_compat {
            let mut printed = false;
            for _ in 0..120 {
                let port = slot.load(Ordering::Relaxed);
                if port != 0 {
                    let addr = net::relay::game_address(&config.game_host, &id.token(), port as u16);
                    if addr.is_empty() {
                        println!("[{}] Neptunnel players join with {}:{}", ts(), relay, port);
                    } else {
                        println!("[{}] Neptunnel players join with {}", ts(), addr);
                    }
                    printed = true;
                    break;
                }
                if !svc.load(Ordering::SeqCst) && stop.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if !printed {
                eprintln!("[{}] Relay assigned no game port; plain Neptunnel players cannot join.", ts());
            }
        }
        if config.list_public {
            let name = if config.server_name.trim().is_empty() { format!("{}'s server", config.uid) } else { config.server_name.clone() };
            let (host, host_sig) = profile::host_binding(config, &id);
            let listing = master::Announce {
                name,
                description: config.server_desc.clone(),
                map: PathBuf::from(&config.map).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                tags: master::parse_tags(&config.server_tags),
                max_players: config.max_players,
                invite: inv.clone(),
                version: "beta 1.0".to_string(),
                console_policy: if config.console_allowed.trim().is_empty() { None } else { Some(config.console_allowed.trim().to_string()) },
                overhead_tags: if config.overhead_tags { None } else { Some(false) },
                strict_profiles: if config.strict_profiles { Some(true) } else { None },
                host,
                host_sig,
                ts: 0,
            };
            announcer = Some(master::spawn_announcer(
                master::master_list(&config.master),
                id,
                listing,
                Arc::clone(&svc),
                Arc::clone(&ready),
                log_line,
            ));
        }
        invite = Some(inv);
    } else {
        for w in privacy::tunnel_addr_warnings(&config.addr) {
            println!("[{}] ! {}", ts(), w);
        }
    }

    if external {
        match &invite {
            Some(inv) => println!("[{}] Invite (share this; your IP stays hidden): {}", ts(), inv),
            None => println!("[{}] No relay set; running standalone with no invite.", ts()),
        }
        println!("[{}] External Studio mode: relay agent + announcer running. Ctrl+C to stop.", ts());
        sleep_until_stopped(&stop);
        svc.store(true, Ordering::SeqCst);
        if let Some(a) = agent {
            let _ = a.join();
        }
        if let Some(a) = announcer {
            let _ = a.join();
        }
        boombox.stop();
        return 0;
    }

    println!("[{}] Launching Roblox Studio Server...", ts());
    let code = match launch_server(studio, &config.port, &config.uid, &pg, &tg) {
        Ok(mut child) => {
            let _ = write_session_log(&pg, &tg, &config.addr, &config.port, &config.uid);
            println!("[{}] Session log written to {}", ts(), session_log_name(&config.port));
            if let Some(inv) = &invite {
                println!("[{}] Invite (share this; your IP stays hidden): {}", ts(), inv);
                config::copy_text(inv);
            }
            println!("[{}] Server process running. Ctrl+C to stop.", ts());
            wait_child(&mut child, stop);
            0
        }
        Err(e) => {
            eprintln!("[{}] Failed to launch Roblox Studio Server: {}", ts(), e);
            1
        }
    };
    svc.store(true, Ordering::SeqCst);
    if let Some(a) = agent {
        let _ = a.join();
    }
    if let Some(a) = announcer {
        let _ = a.join();
    }
    boombox.stop();
    code
}

fn run_join(studio: &str, target: &str, nep: bool, stop: &Arc<AtomicBool>) -> i32 {
    if studio.is_empty() {
        eprintln!("Roblox Studio not found. Set `studio` in puppytunnel.json.");
        return 1;
    }
    println!("[{}] Starting UDP proxy...", ts());
    let (mut proxy, shown) = match start_join_proxy(target) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[{}] {}", ts(), e);
            return 1;
        }
    };
    let proxy_port = proxy.port();
    println!("[{}] Proxy 127.0.0.1:{} -> {}", ts(), proxy_port, shown);

    println!("[{}] Warming tunnel (8 probes)...", ts());
    warm_tunnel(proxy_port, 8, 0.1, None);

    let mut boombox = boombox::BoomboxServer::new();
    let mut watcher = None;
    if nep {
        match boombox.start() {
            Ok(()) => {
                let st = Arc::clone(stop);
                watcher = Some(std::thread::spawn(move || {
                    watch_media_requests(st, log_line, boombox::ensure_media_local);
                }));
            }
            Err(e) => eprintln!("[{}] LocalBoombox failed: {}", ts(), e),
        }
    }

    let pg = config::gen_guid();
    let tg = config::gen_guid();
    println!("[{}] Launching Roblox Studio Client...", ts());
    let code = match launch_client(studio, "127.0.0.1", proxy_port, &pg, &tg, "StudioPlayer_Proxy") {
        Ok(mut child) => {
            println!("[{}] Client launched. Session active. Ctrl+C to stop.", ts());
            wait_child(&mut child, stop);
            0
        }
        Err(e) => {
            eprintln!("[{}] Failed to launch Roblox Studio client: {}", ts(), e);
            1
        }
    };
    stop.store(true, Ordering::SeqCst);
    if let Some(w) = watcher {
        let _ = w.join();
    }
    boombox.stop();
    proxy.stop();
    code
}
