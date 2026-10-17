use crate::config::{STUDIO_PLUGIN_NAME, STUDIO_PLUGIN_SOURCE};
use std::path::PathBuf;

pub fn studio_plugins_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        let base = std::env::var("LOCALAPPDATA").ok()?;
        Some(PathBuf::from(base).join("Roblox").join("Plugins"))
    } else if cfg!(target_os = "macos") {
        let home = std::env::var("HOME").ok()?;
        Some(PathBuf::from(home).join("Documents").join("Roblox").join("Plugins"))
    } else if cfg!(target_os = "linux") {
        let home = std::env::var("HOME").ok()?;
        Some(
            PathBuf::from(home)
                .join(".var/app/org.vinegarhq.Vinegar/data/Roblox/Plugins"),
        )
    } else {
        None
    }
}

pub fn install_studio_plugin() -> Result<(bool, String), String> {
    let folder = studio_plugins_dir()
        .ok_or_else(|| "Unsupported platform for Roblox Studio plugin installation".to_string())?;

    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("Failed to create plugins folder: {}", e))?;

    let _ = std::fs::remove_file(folder.join("NepTunnelHttp.lua"));
    let target = folder.join(STUDIO_PLUGIN_NAME);
    if target.is_file() {
        if let Ok(existing) = std::fs::read_to_string(&target) {
            if existing == STUDIO_PLUGIN_SOURCE {
                return Ok((true, "already installed".to_string()));
            }
        }
    }

    std::fs::write(&target, STUDIO_PLUGIN_SOURCE)
        .map_err(|e| format!("Failed to write plugin file: {}", e))?;

    Ok((true, format!("installed to {}", target.display())))
}
