
use crate::config::AppConfig;
use crate::master::DEFAULT_MASTER;
use crate::rbxapi::{self, USERS_API};
use crate::roblox::plugin::install_studio_plugin;
use crate::roblox::studio::{get_studio_path, VINEGAR};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_RELAY: &str = "relay.puppytunnel.vip:7777";
pub const FALLBACK_UID: &str = "2721761670";


pub fn detect_roblox_user() -> Option<u64> {
    if cfg!(windows) {
        if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
            let roblox_dir = Path::new(&local_app).join("Roblox");


            let wv2_dir = roblox_dir.join("RobloxStudio").join("WebView2").join("EBWebView");
            if let Ok(entries) = fs::read_dir(&wv2_dir) {
                let mut hits: Vec<(u64, std::time::SystemTime)> = Vec::new();
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if let Some(uid_str) = name.strip_prefix("WV2Profile_user_") {
                        if let Ok(uid) = uid_str.parse::<u64>() {
                            let mtime = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            hits.push((uid, mtime));
                        }
                    }
                }
                hits.sort_by_key(|h| h.1);
                if let Some((uid, _)) = hits.last() {
                    return Some(*uid);
                }
            }


            let asst_dir = roblox_dir.join("AssistantSettings");
            if let Ok(entries) = fs::read_dir(&asst_dir) {
                let mut hits: Vec<(u64, std::time::SystemTime)> = Vec::new();
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if let Some(uid_str) = name.strip_suffix(".json") {
                        if let Ok(uid) = uid_str.parse::<u64>() {
                            let mtime = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            hits.push((uid, mtime));
                        }
                    }
                }
                hits.sort_by_key(|h| h.1);
                if let Some((uid, _)) = hits.last() {
                    return Some(*uid);
                }
            }


            if let Ok(entries) = fs::read_dir(&roblox_dir) {
                let mut hits: Vec<(u64, std::time::SystemTime)> = Vec::new();
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.chars().all(|c| c.is_ascii_digit()) && name.len() >= 6 {
                            if let Ok(uid) = name.parse::<u64>() {
                                let mtime = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                                hits.push((uid, mtime));
                            }
                        }
                    }
                }
                hits.sort_by_key(|h| h.1);
                if let Some((uid, _)) = hits.last() {
                    return Some(*uid);
                }
            }


            let logs_dir = roblox_dir.join("logs");
            if let Ok(entries) = fs::read_dir(&logs_dir) {
                let mut logs: Vec<PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().map_or(false, |ext| ext == "log"))
                    .collect();
                logs.sort_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok());
                if let Some(latest) = logs.last() {
                    if let Ok(content) = fs::read_to_string(latest) {
                        for line in content.lines().rev() {
                            if let Some(pos) = line.find("user profile ID: ") {
                                let rest = &line[pos + 17..];
                                let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                                if let Ok(uid) = num.parse::<u64>() {
                                    return Some(uid);
                                }
                            }
                            if let Some(pos) = line.find("userid:") {
                                let rest = &line[pos + 7..];
                                let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                                if let Ok(uid) = num.parse::<u64>() {
                                    if uid > 1000 {
                                        return Some(uid);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    } else if cfg!(target_os = "linux") {
        if let Ok(home) = std::env::var("HOME") {
            let roblox_dir = PathBuf::from(home).join(".var/app/org.vinegarhq.Vinegar/data/Roblox");
            let logs_dir = roblox_dir.join("logs");
            if let Ok(entries) = fs::read_dir(&logs_dir) {
                let mut logs: Vec<PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().map_or(false, |ext| ext == "log"))
                    .collect();
                logs.sort_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok());
                if let Some(latest) = logs.last() {
                    if let Ok(content) = fs::read_to_string(latest) {
                        for line in content.lines().rev() {
                            if let Some(pos) = line.find("user profile ID: ") {
                                let rest = &line[pos + 17..];
                                let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                                if let Ok(uid) = num.parse::<u64>() {
                                    return Some(uid);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None
}



pub fn autoconfigure(config: &mut AppConfig) -> bool {
    let mut changed = false;


    let studio_invalid = config.studio.is_empty()
        || (config.studio != VINEGAR && !Path::new(&config.studio).is_file());
    if studio_invalid {
        let detected = get_studio_path();
        if !detected.is_empty() {
            config.studio = detected;
            changed = true;
        }
    }


    let uid_invalid = config.uid.is_empty()
        || config.uid == "<ur user id here>"
        || config.uid.trim() == "0"
        || config.roblox_id == 0;
    if uid_invalid {
        if let Some(uid) = detect_roblox_user() {
            config.uid = uid.to_string();
            config.roblox_id = uid;
            changed = true;


            if config.roblox_name.is_empty() || config.roblox_name == "builder_pro" {
                if let Ok(user) = rbxapi::user_by_id(USERS_API, uid) {
                    config.roblox_name = user.name;
                    if config.profile_bio.is_empty() && !user.description.is_empty() {
                        config.profile_bio = user.description;
                    }
                } else {
                    config.roblox_name = format!("User{}", uid);
                }
            }
        } else {

            config.uid = FALLBACK_UID.to_string();
            config.roblox_id = FALLBACK_UID.parse().unwrap_or(2721761670);
            if config.roblox_name.is_empty() || config.roblox_name == "builder_pro" {
                config.roblox_name = "melody".to_string();
            }
            changed = true;
        }
    }


    if config.master.trim().is_empty() || config.master.contains("cabin-canberra") {
        config.master = DEFAULT_MASTER.to_string();
        changed = true;
    }


    if config.relay.trim().is_empty() {
        config.relay = DEFAULT_RELAY.to_string();
        changed = true;
    }


    if config.update_source_url.trim().is_empty()
        || crate::update::source::is_retired_update_host(&config.update_source_url)
    {
        config.update_source_type = "github".to_string();
        config.update_source_url = format!(
            "{}/{}",
            crate::update::source::DEFAULT_GITHUB_OWNER,
            crate::update::source::DEFAULT_GITHUB_REPO
        );
        if config.update_channel.trim().eq_ignore_ascii_case("beta") {
            config.update_channel = "nightly".to_string();
        }
        changed = true;
    }


    if config.port.trim().is_empty() {
        config.port = "55555".to_string();
        changed = true;
    }


    if config.addr == "<[Address:Port] here>" {
        config.addr = String::new();
        changed = true;
    }


    let _ = install_studio_plugin();

    changed
}
