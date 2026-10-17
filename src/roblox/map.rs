use std::path::{Path, PathBuf};

pub fn get_runtime_server_place() -> Option<PathBuf> {
    if cfg!(windows) {
        let base = std::env::var("LOCALAPPDATA").ok()?;
        Some(Path::new(&base).join("Roblox").join("server.rbxl"))
    } else if cfg!(target_os = "macos") {
        let home = std::env::var("HOME").ok()?;
        Some(Path::new(&home).join("Library/Application Support/Roblox/server.rbxl"))
    } else if cfg!(target_os = "linux") {
        let home = std::env::var("HOME").ok()?;
        Some(Path::new(&home).join(".var/app/org.vinegarhq.Vinegar/data/Roblox/server.rbxl"))
    } else {
        None
    }
}

pub fn inject_map(map_path: &Path) -> Result<(), String> {
    if !map_path.is_file() {
        return Err(format!("Map file does not exist: {}", map_path.display()));
    }

    let target = get_runtime_server_place()
        .ok_or_else(|| "Could not determine runtime Roblox server place location".to_string())?;

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }

    if target.exists() {
        let _ = std::fs::remove_file(&target);
    }

    std::fs::copy(map_path, &target)
        .map_err(|e| format!("Failed to copy map to {}: {}", target.display(), e))?;

    Ok(())
}
