use std::path::{Path, PathBuf};
use std::process::{Child, Command};

pub const VINEGAR: &str = "__VINEGAR__";

pub fn get_studio_path() -> String {
    if cfg!(windows) {

        if let Ok(out) = Command::new("reg")
            .args(["query", r"HKCU\Software\ROBLOX Corporation\Environments\roblox-studio", "/v", "clientExe"])
            .output()
        {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                for line in text.lines() {
                    if let Some(pos) = line.find("REG_SZ") {
                        let path = line[pos + 6..].trim();
                        if Path::new(path).is_file() {
                            return path.to_string();
                        }
                    }
                }
            }
        }


        if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
            let versions_dir = Path::new(&local_app).join("Roblox").join("Versions");
            if let Ok(entries) = std::fs::read_dir(&versions_dir) {
                let mut hits: Vec<PathBuf> = Vec::new();
                for entry in entries.flatten() {
                    let exe = entry.path().join("RobloxStudioBeta.exe");
                    if exe.is_file() {
                        hits.push(exe);
                    }
                }
                hits.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
                if let Some(newest) = hits.last() {
                    return newest.to_string_lossy().to_string();
                }
            }

            let alt = Path::new(&local_app).join("Roblox").join("RobloxStudioBeta.exe");
            if alt.is_file() {
                return alt.to_string_lossy().to_string();
            }
        }


        for pf in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Ok(dir) = std::env::var(pf) {
                let versions = Path::new(&dir).join("Roblox").join("Versions");
                if let Ok(entries) = std::fs::read_dir(&versions) {
                    let mut hits: Vec<PathBuf> = Vec::new();
                    for entry in entries.flatten() {
                        let exe = entry.path().join("RobloxStudioBeta.exe");
                        if exe.is_file() {
                            hits.push(exe);
                        }
                    }
                    hits.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
                    if let Some(newest) = hits.last() {
                        return newest.to_string_lossy().to_string();
                    }
                }
            }
        }

        String::new()
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

pub fn build_cmd(studio: &str, args: &[&str]) -> Command {
    let mut cmd = if studio == VINEGAR {
        let mut c = Command::new("flatpak");
        c.args(["run", "org.vinegarhq.Vinegar", "studio", "--"]);
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
