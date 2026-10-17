use crate::config::{load_config, save_config};
use std::io::{self, Write};

fn prompt(label: &str, default: &str) -> String {
    if default.is_empty() {
        print!("{}: ", label);
    } else {
        print!("{} [{}]: ", label, default);
    }
    let _ = io::stdout().flush();
    let mut s = String::new();
    if io::stdin().read_line(&mut s).is_err() {
        return default.to_string();
    }
    let t = s.trim();
    if t.is_empty() {
        default.to_string()
    } else {
        t.to_string()
    }
}

fn prompt_u32(label: &str, default: u32) -> u32 {
    prompt(label, &default.to_string()).trim().parse().unwrap_or(default)
}

pub fn run_setup() -> i32 {
    let mut config = load_config();
    println!("PuppyTunnel server setup. Empty answer keeps the [default].");
    println!("Relay and master default to your domain; just name the server.");

    let relay_def = if config.relay.trim().is_empty() {
        "relay.puppytunnel.vip:7777".to_string()
    } else {
        config.relay.clone()
    };
    config.relay = prompt("Relay host:port", &relay_def);

    let master_def = if config.master.trim().is_empty() {
        "https://roblox.puppytunnel.vip".to_string()
    } else {
        config.master.clone()
    };
    config.master = prompt("Master URL(s), comma separated", &master_def);

    let name_def = config.server_name.clone();
    let name_label = "Server name";
    if name_def.trim().is_empty() {
        config.server_name = prompt(name_label, "My server");
    } else {
        let owned = name_def.clone();
        config.server_name = prompt(name_label, &owned);
    }

    let max_def = if config.max_players == 0 { 99 } else { config.max_players };
    config.max_players = prompt_u32("Player cap", max_def);

    let port_def = if config.port.trim().is_empty() {
        "55555".to_string()
    } else {
        config.port.clone()
    };
    let port_in = prompt("Studio game port", &port_def);
    if port_in.trim().parse::<u16>().is_err() {
        eprintln!("Port must be 1-65535; keeping {}.", port_def);
    } else {
        config.port = port_in.trim().to_string();
    }

    let uid_def = config.uid.clone();
    config.uid = prompt("Roblox user id (creatorId/userid)", &uid_def);

    let map_def = config.map.clone();
    config.map = prompt("Map file (.rbxl, empty = none)", &map_def);
    if !config.map.trim().is_empty() && !std::path::Path::new(config.map.trim()).is_file() {
        println!("Note: map file not found; hosting will fail until the path is fixed.");
    }

    let ans = prompt("List in the server browser (Y/n)", "Y");
    let lower = ans.trim().to_ascii_lowercase();
    config.list_public = !(lower == "n" || lower == "no");

    if config.server_secret.trim().is_empty() {
        let a = uuid::Uuid::new_v4().simple().to_string();
        let b = uuid::Uuid::new_v4().simple().to_string();
        config.server_secret = format!("{}{}", a, b);
        println!("Generated a fresh server identity (the invite stays stable across restarts).");
    }

    let detected = crate::roblox::get_studio_path();
    if config.studio.is_empty() && detected.is_empty() {
        println!("Roblox Studio was not auto-detected; set `studio` in puppytunnel.json later.");
    } else {
        let studio_def = if config.studio.is_empty() { detected } else { config.studio.clone() };
        config.studio = prompt("Studio executable path", &studio_def);
    }

    save_config(&config);
    println!("Saved. Host with: puppytunnel host --relay {} --list", config.relay);
    0
}
