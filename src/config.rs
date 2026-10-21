use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use crate::update::UpdateHistoryEntry;

pub const USER_ID: &str = "<ur user id here>";
pub const STATIC_PORT: &str = "55555";
pub const TUNNEL_ADDR: &str = "<[Address:Port] here>";
pub const PROXY_PORT: u16 = 55555;
pub const BOOMBOX_PORT: u16 = 48732;
pub const BRIDGE_PORT: u16 = 7878;
pub const RELAY_PORT: u16 = 7777;


pub const CONFIG_FILE: &str = "puppytunnel.json";
pub const LOG_FILE: &str = "SESSION_INFO.txt";
pub const JOIN_LOG_FILE: &str = "media_join.log";

pub const WARM_PACKETS: usize = 8;
pub const WARM_INTERVAL: f32 = 0.1;

pub const TEST_PROBE_COUNT: usize = 5;
pub const TEST_PROBE_TIMEOUT: f64 = 2.5;

pub const ECHO_REQ: &[u8] = b"NEP_TEST\x00";
pub const ECHO_RESP: &[u8] = b"NEP_ECHO\x00";

pub const STUDIO_PLUGIN_NAME: &str = "PuppyTunnelHttp.lua";
pub const STUDIO_PLUGIN_SOURCE: &str = r#"-- Installed by PuppyTunnel. Enables HttpService requests so the local host can
-- reach the helper on 127.0.0.1. Safe to delete if you no longer use PuppyTunnel.
local HttpService = game:GetService("HttpService")

local function enableHttp()
	if HttpService.HttpEnabled then
		return true, "already enabled"
	end
	local ok, err = pcall(function()
		HttpService.HttpEnabled = true
	end)
	return ok, ok and "enabled" or tostring(err)
end

local ok, status = enableHttp()
print("[PuppyTunnel] HTTP requests: " .. status)
if not ok then
	warn("[PuppyTunnel] Could not enable HTTP requests automatically. "
		.. "Set it in Game Settings > Security > Allow HTTP Requests.")
end

-- Toolbar button as a fallback: covers places that were already open when the
-- plugin first landed, and keeps the behaviour visible rather than silent.
local toolbar = plugin:CreateToolbar("PuppyTunnel")
local button = toolbar:CreateButton(
	"Enable HTTP",
	"Enable HttpService requests for this place",
	"rbxasset://textures/ui/GuiImagePlaceholder.png"
)

button.Click:Connect(function()
	local clickOk, clickStatus = enableHttp()
	print("[PuppyTunnel] HTTP requests: " .. clickStatus)
	if not clickOk then
		warn("[PuppyTunnel] Set it in Game Settings > Security > Allow HTTP Requests.")
	end
end)
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub uid: String,
    pub port: String,
    pub addr: String,
    pub studio: String,
    pub map: String,
    #[serde(default)]
    pub saved_maps: Vec<String>,
    #[serde(default)]
    pub relay: String,
    #[serde(default)]
    pub master: String,
    #[serde(default)]
    pub server_name: String,
    #[serde(default)]
    pub server_desc: String,
    #[serde(default)]
    pub server_tags: String,
    #[serde(default)]
    pub max_players: u32,
    #[serde(default)]
    pub list_public: bool,
    #[serde(default)]
    pub nep_compat: bool,
    #[serde(default)]
    pub game_host: String,
    #[serde(default)]
    pub native_dialogs: bool,
    #[serde(default)]
    pub server_secret: String,
    #[serde(default)]
    pub roblox_id: u64,
    #[serde(default)]
    pub roblox_name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub profile_bio: String,
    #[serde(default)]
    pub profile_accent: String,
    #[serde(default)]
    pub profile_servers: String,
    #[serde(default = "default_true")]
    pub show_profile: bool,
    #[serde(default = "default_channel")]
    pub update_channel: String,
    #[serde(default = "default_source")]
    pub update_source_type: String,
    #[serde(default)]
    pub update_source_url: String,
    #[serde(default = "default_true")]
    pub auto_update: bool,
    #[serde(default)]
    pub update_history: Vec<UpdateHistoryEntry>,
    #[serde(default)]
    pub community_instance: String,
    #[serde(default)]
    pub console_allowed: String,
    #[serde(default = "default_true")]
    pub overhead_tags: bool,
    #[serde(default)]
    pub strict_profiles: bool,
}

fn default_channel() -> String {
    "nightly".to_string()
}

fn default_source() -> String {
    "github".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            uid: String::new(),
            port: STATIC_PORT.to_string(),
            addr: String::new(),
            studio: String::new(),
            map: String::new(),
            saved_maps: Vec::new(),
            relay: "relay.puppytunnel.vip:7777".to_string(),
            master: "https://roblox.puppytunnel.vip".to_string(),
            server_name: String::new(),
            server_desc: String::new(),
            server_tags: String::new(),
            max_players: 0,
            list_public: false,
            nep_compat: false,
            game_host: String::new(),
            native_dialogs: false,
            server_secret: String::new(),
            roblox_id: 0,
            roblox_name: String::new(),
            display_name: String::new(),
            profile_bio: String::new(),
            profile_accent: "mint".to_string(),
            profile_servers: String::new(),
            show_profile: true,
            update_channel: "nightly".to_string(),
            update_source_type: "github".to_string(),
            update_source_url: "novembahh/puppytunnelrblx".to_string(),
            auto_update: true,
            update_history: vec![UpdateHistoryEntry {
                version: "1.0.0-beta.1".to_string(),
                installed_at: "Initial install".to_string(),
                previous_version: String::new(),
                title: "PuppyTunnel Beta 1".to_string(),
                channel: "beta".to_string(),
                status: "installed".to_string(),
            }],
            community_instance: String::new(),
            console_allowed: String::new(),
            overhead_tags: true,
            strict_profiles: false,
        }
    }
}

impl AppConfig {
    pub fn console_allowed_ids(&self) -> Vec<u64> {
        self.console_allowed.split(',').filter_map(|s| s.trim().parse().ok()).collect()
    }
    pub fn console_allowed_for(&self, id: u64) -> bool {
        let ids = self.console_allowed_ids();
        ids.is_empty() || ids.contains(&id)
    }
    pub fn community_base(&self) -> String {
        let own = self.community_instance.trim();
        if !own.is_empty() {
            return crate::master::normalize_url(own);
        }
        crate::master::master_list(&self.master).remove(0)
    }

    pub fn effective_display_name(&self) -> &str {
        let dn = self.display_name.trim();
        if !dn.is_empty() {
            dn
        } else if !self.roblox_name.trim().is_empty() {
            &self.roblox_name
        } else {
            "pup_anon"
        }
    }
}

pub fn get_script_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.to_path_buf();
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn get_config_path() -> PathBuf {
    if std::path::Path::new(CONFIG_FILE).is_file() {
        return PathBuf::from(CONFIG_FILE);
    }
    get_script_dir().join(CONFIG_FILE)
}

pub fn session_log_name(port: &str) -> String {
    match port.trim().parse::<u16>() {
        Ok(p) => format!("SESSION_INFO_{}.txt", p),
        Err(_) => LOG_FILE.to_string(),
    }
}

pub fn get_session_log_path(port: &str) -> PathBuf {
    get_script_dir().join(session_log_name(port))
}

pub fn get_join_log_path() -> PathBuf {
    get_script_dir().join(JOIN_LOG_FILE)
}

pub fn load_config() -> AppConfig {
    let mut config = AppConfig::default();
    let path = get_config_path();
    let legacy = get_script_dir().join("nep_config.json");
    if let Ok(content) = fs::read_to_string(&path).or_else(|_| fs::read_to_string(&legacy)) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(uid) = parsed.get("uid").and_then(|v| v.as_str()) {
                config.uid = uid.to_string();
            }
            if let Some(port) = parsed.get("port").and_then(|v| v.as_str()) {
                config.port = port.to_string();
            }
            if let Some(addr) = parsed.get("addr").and_then(|v| v.as_str()) {
                config.addr = addr.to_string();
            }
            if let Some(studio) = parsed.get("studio").and_then(|v| v.as_str()) {
                config.studio = studio.to_string();
            }
            if let Some(map) = parsed.get("map").and_then(|v| v.as_str()) {
                config.map = map.to_string();
            }
            if let Some(relay) = parsed.get("relay").and_then(|v| v.as_str()) {
                config.relay = relay.to_string();
            }
            for (key, field) in [
                ("master", &mut config.master),
                ("community_instance", &mut config.community_instance),
                ("console_allowed", &mut config.console_allowed),
                ("server_name", &mut config.server_name),
                ("server_desc", &mut config.server_desc),
                ("server_tags", &mut config.server_tags),
                ("server_secret", &mut config.server_secret),
                ("roblox_name", &mut config.roblox_name),
                ("display_name", &mut config.display_name),
                ("profile_bio", &mut config.profile_bio),
                ("profile_accent", &mut config.profile_accent),
                ("profile_servers", &mut config.profile_servers),
                ("update_channel", &mut config.update_channel),
                ("update_source_type", &mut config.update_source_type),
                ("update_source_url", &mut config.update_source_url),
            ] {
                if let Some(v) = parsed.get(key).and_then(|v| v.as_str()) {
                    *field = v.to_string();
                }
            }
            if let Some(v) = parsed.get("update_source").and_then(|v| v.as_str()) {
                config.update_source_type = v.to_string();
            }
            if let Some(v) = parsed.get("update_custom_url").and_then(|v| v.as_str()) {
                config.update_source_url = v.to_string();
            }
            if let Some(v) = parsed.get("max_players").and_then(|v| v.as_u64()) {
                config.max_players = v.min(1000) as u32;
            }
            if let Some(v) = parsed.get("list_public").and_then(|v| v.as_bool()) {
                config.list_public = v;
            }
            if let Some(v) = parsed.get("nep_compat").and_then(|v| v.as_bool()) {
                config.nep_compat = v;
            }
            if let Some(v) = parsed.get("game_host").and_then(|v| v.as_str()) {
                config.game_host = v.to_string();
            }
            if let Some(v) = parsed.get("overhead_tags").and_then(|v| v.as_bool()) {
                config.overhead_tags = v;
            }
            if let Some(v) = parsed.get("strict_profiles").and_then(|v| v.as_bool()) {
                config.strict_profiles = v;
            }
            if let Some(v) = parsed.get("native_dialogs").and_then(|v| v.as_bool()) {
                config.native_dialogs = v;
            }
            if let Some(v) = parsed.get("roblox_id").and_then(|v| v.as_u64()) {
                config.roblox_id = v;
            }
            if let Some(v) = parsed.get("show_profile").and_then(|v| v.as_bool()) {
                config.show_profile = v;
            }
            if let Some(v) = parsed.get("auto_update").and_then(|v| v.as_bool()) {
                config.auto_update = v;
            }
            if let Some(history) = parsed.get("update_history").and_then(|v| v.as_array()) {
                if let Ok(entries) = serde_json::from_value(serde_json::Value::Array(history.clone())) {
                    config.update_history = entries;
                }
            }
            if let Some(saved) = parsed.get("saved_maps").and_then(|v| v.as_array()) {
                config.saved_maps = saved
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
            }
        }
    }
    if config.update_history.is_empty() {
        config.update_history.push(UpdateHistoryEntry {
            version: "1.0.0-beta.1".to_string(),
            installed_at: "Initial install".to_string(),
            previous_version: String::new(),
            title: "PuppyTunnel Beta 1".to_string(),
            channel: "beta".to_string(),
            status: "installed".to_string(),
        });
    }
    let changed = crate::roblox::autoconfigure(&mut config);
    if changed || config.server_secret.is_empty() {
        if config.server_secret.is_empty() {
            config.server_secret = crate::identity::new_secret();
        }
        save_config(&config);
    }
    config
}

fn write_atomic(path: &std::path::Path, content: &[u8]) {
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, content).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

pub fn save_config(config: &AppConfig) {
    let path = get_config_path();
    if let Ok(content) = serde_json::to_string_pretty(config) {
        write_atomic(&path, content.as_bytes());
    }
}

pub fn ts() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

pub fn gen_guid() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn join_log_write(message: &str) {
    let path = get_join_log_path();
    let time_str = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let line = format!("[{}] {}\n", time_str, message);
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn write_session_log(
    pg: &str,
    tg: &str,
    tunnel_addr: &str,
    port: &str,
    uid: &str,
) -> String {
    let (host, dp) = if tunnel_addr.contains(':') {
        tunnel_addr.rsplit_once(':').unwrap_or((tunnel_addr, port))
    } else {
        (tunnel_addr, port)
    };

    let win_cmd = format!(
        "powershell -ExecutionPolicy Bypass -Command \"$p = Get-ChildItem -Path $env:LOCALAPPDATA\\Roblox\\Versions -Filter RobloxStudioBeta.exe -Recurse | Select-Object -First 1 -ExpandProperty FullName; Start-Process -FilePath $p -ArgumentList '-task StartClient -placeId 0 -universeId 0 -placeVersion 0 -server {} -port {} -parentSessionGuid {} -playTestSessionGuid {} -instanceId StudioPlayer_0'\"",
        host, dp, pg, tg
    );

    let mac_cmd = format!(
        "\"/Applications/RobloxStudio.app/Contents/MacOS/RobloxStudio\" -task StartClient -placeId 0 -universeId 0 -placeVersion 0 -server {} -port {} -parentSessionGuid {} -playTestSessionGuid {} -instanceId StudioPlayer_0",
        host, dp, pg, tg
    );

    let lin_cmd = format!(
        "flatpak run org.vinegarhq.Vinegar -task StartClient -placeId 0 -universeId 0 -placeVersion 0 -server {} -port {} -parentSessionGuid {} -playTestSessionGuid {} -instanceId StudioPlayer_0",
        host, dp, pg, tg
    );

    let date_str = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let date_line = format!("Date       : {}", date_str);
    let uid_line = format!("User ID    : {}", uid);
    let addr_line = format!("Address    : {}", tunnel_addr);
    let port_line = format!("Server Local Port: {}", port);

    let lines = vec![
        "+--------------------------------------------------------+",
        "|  PUPPYTUNNEL - ROBLOX STUDIO SESSION LOG               |",
        "+--------------------------------------------------------+",
        &date_line,
        &uid_line,
        &addr_line,
        &port_line,
        "",
        "-- WINDOWS (Command Prompt) --",
        &win_cmd,
        "",
        "-- MAC (Terminal) --",
        &mac_cmd,
        "",
        "-- LINUX / VINEGAR --",
        &lin_cmd,
        "",
        "----------------------------------------------------------",
    ];

    let content = lines.join("\n");
    write_atomic(&get_session_log_path(port), content.as_bytes());

    let clip_cmd = if cfg!(target_os = "linux") {
        &lin_cmd
    } else if cfg!(target_os = "macos") {
        &mac_cmd
    } else {
        &win_cmd
    };

    copy_text(clip_cmd);

    win_cmd
}

pub fn split_host_port(addr: &str) -> Option<(String, u16)> {
    let (h, p) = addr.trim().rsplit_once(':')?;
    let h = h.trim().trim_matches(|c| c == '[' || c == ']');
    if h.is_empty() {
        return None;
    }
    Some((h.to_string(), p.trim().parse().ok()?))
}

pub fn copy_text(text: &str) {
    #[cfg(feature = "gui")]
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        let _ = clipboard.set_text(text.to_string());
    }
    #[cfg(not(feature = "gui"))]
    let _ = text;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_config_update_settings_round_trip() {
        let mut config = AppConfig::default();
        config.auto_update = false;
        config.update_channel = "nightly".to_string();
        config.update_source_type = "github".to_string();
        config.update_source_url = "PuppyTunnel/puppytunnel".to_string();
        config.update_history.push(UpdateHistoryEntry {
            version: "1.0.1".to_string(),
            installed_at: "2026-10-17 06:00".to_string(),
            previous_version: "1.0.0-beta.1".to_string(),
            channel: "nightly".to_string(),
            status: "success".to_string(),
            title: "Nightly Build".to_string(),
        });

        let json = serde_json::to_string(&config).unwrap();
        let decoded: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.auto_update, false);
        assert_eq!(decoded.update_channel, "nightly");
        assert_eq!(decoded.update_source_type, "github");
        assert_eq!(decoded.update_source_url, "PuppyTunnel/puppytunnel");
        assert_eq!(decoded.update_history.len(), 2);
        assert_eq!(decoded.update_history[1].version, "1.0.1");
    }

    #[test]
    fn test_console_approvals_and_server_options() {
        let config = AppConfig::default();
        assert!(config.console_allowed_for(1));
        assert!(config.overhead_tags);
        assert!(!config.strict_profiles);
        let mut gated = AppConfig::default();
        gated.console_allowed = "12, bad, 34".to_string();
        assert_eq!(gated.console_allowed_ids(), vec![12, 34]);
        assert!(gated.console_allowed_for(12));
        assert!(!gated.console_allowed_for(99));
    }

    #[test]
    fn test_default_update_source_is_github() {
        let config = AppConfig::default();
        assert_eq!(config.update_channel, "nightly");
        assert_eq!(config.update_source_type, "github");
        assert_eq!(config.update_source_url, "novembahh/puppytunnelrblx");
        let src = crate::update::UpdateSource::from_config(
            &config.update_source_type,
            &config.update_source_url,
        );
        assert_eq!(
            src,
            crate::update::UpdateSource::GitHubReleases {
                owner: "novembahh".into(),
                repo: "puppytunnelrblx".into(),
            }
        );
    }
}
