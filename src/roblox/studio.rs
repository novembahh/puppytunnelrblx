use std::path::{Path, PathBuf};
use std::process::{Child, Command};

pub const VINEGAR: &str = "__VINEGAR__";

pub fn get_studio_path() -> String {
    if cfg!(windows) {
        let mut hits: Vec<PathBuf> = Vec::new();

        if let Ok(out) = Command::new("reg")
            .args(["query", r"HKCU\Software\ROBLOX Corporation\Environments\roblox-studio", "/v", "clientExe"])
            .output()
        {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                for line in text.lines() {
                    if let Some(pos) = line.find("REG_SZ") {
                        let path = Path::new(line[pos + 6..].trim());
                        if path.is_file() {
                            hits.push(path.to_path_buf());
                        }
                    }
                }
            }
        }

        let mut roots: Vec<PathBuf> = Vec::new();
        if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
            roots.push(Path::new(&local_app).join("Roblox"));
        }
        for pf in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Ok(dir) = std::env::var(pf) {
                roots.push(Path::new(&dir).join("Roblox"));
            }
        }
        for root in roots {
            if let Ok(entries) = std::fs::read_dir(root.join("Versions")) {
                for entry in entries.flatten() {
                    let exe = entry.path().join("RobloxStudioBeta.exe");
                    if exe.is_file() {
                        hits.push(exe);
                    }
                }
            }
            let alt = root.join("RobloxStudioBeta.exe");
            if alt.is_file() {
                hits.push(alt);
            }
        }

        hits.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
        hits.last().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
    } else if cfg!(target_os = "macos") {
        let home = std::env::var("HOME").unwrap_or_default();
        let candidates = [
            PathBuf::from("/Applications/RobloxStudio.app/Contents/MacOS/RobloxStudio"),
            PathBuf::from(&home).join("Applications/RobloxStudio.app/Contents/MacOS/RobloxStudio"),
        ];
        for p in &candidates {
            if p.is_file() {
                return p.to_string_lossy().to_string();
            }
        }
        String::new()
    } else if cfg!(target_os = "linux") {
        if let Ok(out) = Command::new("flatpak")
            .args(["info", "org.vinegarhq.Vinegar"])
            .output()
        {
            if out.status.success() {
                return VINEGAR.to_string();
            }
        }
        if let Some(bin) = native_vinegar() {
            return bin.to_string_lossy().to_string();
        }
        if let Ok(home) = std::env::var("HOME") {
            let grapejuice = PathBuf::from(home).join(".local/share/grapejuice/prefixes/player/drive_c/Program Files (x86)/Roblox/Versions");
            if let Ok(entries) = std::fs::read_dir(&grapejuice) {
                let mut hits: Vec<PathBuf> = entries.flatten().map(|e| e.path().join("RobloxStudioBeta.exe")).filter(|p| p.is_file()).collect();
                hits.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
                if let Some(newest) = hits.last() {
                    return newest.to_string_lossy().to_string();
                }
            }
        }
        String::new()
    } else {
        String::new()
    }
}

fn native_vinegar() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join("vinegar"))
        .find(|bin| bin.is_file())
}

fn vinegar_managed_exe(studio: &str) -> Option<String> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let lower = studio.to_ascii_lowercase();
    if !lower.contains("vinegar") || !lower.ends_with("robloxstudiobeta.exe") {
        return None;
    }
    if lower.contains("org.vinegarhq.vinegar") {
        return Some(VINEGAR.to_string());
    }
    native_vinegar().map(|bin| bin.to_string_lossy().to_string())
}

pub fn build_cmd(studio: &str, args: &[&str]) -> Command {
    let routed = vinegar_managed_exe(studio);
    let studio = routed.as_deref().unwrap_or(studio);
    let mut cmd = if studio == VINEGAR {
        let mut c = Command::new("flatpak");
        c.args(["run", "org.vinegarhq.Vinegar", "studio", "--"]);
        c.args(args);
        c
    } else if Path::new(studio).file_stem().map_or(false, |n| n == "vinegar") {
        let mut c = Command::new(studio);
        c.args(["studio", "--"]);
        c.args(args);
        c
    } else {
        let mut c = Command::new(studio);
        c.args(args);
        c
    };

    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    cmd
}

pub fn launch_server(
    studio: &str,
    port: &str,
    uid: &str,
    pg: &str,
    tg: &str,
) -> std::io::Result<Child> {
    let target = if studio.trim().is_empty() {
        get_studio_path()
    } else {
        studio.to_string()
    };
    if target.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Roblox Studio executable path not configured or detected",
        ));
    }

    let args = [
        "-task",
        "StartServer",
        "-placeId",
        "0",
        "-universeId",
        "0",
        "-placeVersion",
        "1",
        "-port",
        port,
        "-creatorId",
        uid,
        "-creatorType",
        "1",
        "-numTestServerPlayersUponStartup",
        "1",
        "-userid",
        uid,
        "-parentSessionGuid",
        pg,
        "-playTestSessionGuid",
        tg,
        "-instanceId",
        "StudioServer",
    ];

    build_cmd(&target, &args).spawn()
}

pub fn launch_client(
    studio: &str,
    server: &str,
    port: u16,
    pg: &str,
    tg: &str,
    inst: &str,
) -> std::io::Result<Child> {
    let target = if studio.trim().is_empty() {
        get_studio_path()
    } else {
        studio.to_string()
    };
    if target.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Roblox Studio executable path not configured or detected",
        ));
    }

    let port_str = port.to_string();
    let args = [
        "-task",
        "StartClient",
        "-placeId",
        "0",
        "-universeId",
        "0",
        "-placeVersion",
        "1",
        "-server",
        server,
        "-port",
        &port_str,
        "-parentSessionGuid",
        pg,
        "-playTestSessionGuid",
        tg,
        "-instanceId",
        inst,
    ];

    build_cmd(&target, &args).spawn()
}
