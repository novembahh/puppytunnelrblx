use crate::config::{join_log_write, JOIN_LOG_FILE};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

pub fn linux_roblox_dirs() -> Vec<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        bases.push(home.join(".var/app/org.vinegarhq.Vinegar/data"));
        bases.push(match std::env::var_os("XDG_DATA_HOME") {
            Some(x) => PathBuf::from(x),
            None => home.join(".local/share"),
        });
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    for base in bases {
        dirs.push(base.join("Roblox"));
        let users = base.join("vinegar/prefixes/studio/drive_c/users");
        if let Ok(entries) = std::fs::read_dir(users) {
            for entry in entries.flatten() {
                dirs.push(entry.path().join("AppData/Local/Roblox"));
            }
        }
    }
    dirs.into_iter().filter(|d| d.is_dir()).collect()
}

pub fn roblox_log_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        let base = std::env::var("LOCALAPPDATA").ok()?;
        let p = PathBuf::from(base).join("Roblox").join("logs");
        if p.is_dir() {
            Some(p)
        } else {
            None
        }
    } else if cfg!(target_os = "macos") {
        let home = std::env::var("HOME").ok()?;
        let p = PathBuf::from(home).join("Library/Logs/Roblox");
        if p.is_dir() {
            Some(p)
        } else {
            None
        }
    } else if cfg!(target_os = "linux") {
        let mut hits: Vec<PathBuf> = linux_roblox_dirs()
            .into_iter()
            .map(|d| d.join("logs"))
            .filter(|p| p.is_dir())
            .collect();
        hits.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
        hits.pop()
    } else {
        None
    }
}

pub fn watch_media_requests<F, E>(
    stop_flag: Arc<AtomicBool>,
    mut log_fn: F,
    ensure_media_fn: E,
) where
    F: FnMut(&str, &str),
    E: Fn(&str, &str, &str) -> (bool, String),
{
    log_fn("-- Media watcher starting --", "info");

    let logs_dir = match roblox_log_dir() {
        Some(d) => d,
        None => {
            log_fn(
                "Roblox log folder NOT FOUND - auto media fetch disabled.",
                "err",
            );
            if let Ok(base) = std::env::var("LOCALAPPDATA") {
                log_fn(
                    &format!("Expected: {}\\Roblox\\logs", base),
                    "err",
                );
            }
            return;
        }
    };

    log_fn(&format!("Watching: {}", logs_dir.display()), "ok");
    log_fn(
        &format!("Activity is also written to: {}", JOIN_LOG_FILE),
        "dim",
    );

    let nep_re = match Regex::new(
        r"\[NEPMEDIA\]\s*url=(?P<url>.*?)\s*\|\s*audio=(?P<audio>.*?)\s*\|\s*video=(?P<video>[^\r\n]*)",
    ) {
        Ok(r) => r,
        Err(_) => return,
    };

    let mut offsets: HashMap<PathBuf, u64> = HashMap::new();
    let mut handled: HashSet<(String, String)> = HashSet::new();
    let started_at = SystemTime::now();
    let mut last_heartbeat = Instant::now();
    let mut fetched_count = 0usize;

    while !stop_flag.load(Ordering::SeqCst) {
        if last_heartbeat.elapsed() >= Duration::from_secs(15) {
            last_heartbeat = Instant::now();
            let msg = format!(
                "watching... {} active log file(s), {} media fetched",
                offsets.len(),
                fetched_count
            );
            log_fn(&msg, "dim");
            join_log_write(&msg);
        }


        if let Ok(entries) = std::fs::read_dir(&logs_dir) {
            let mut recent_files: Vec<PathBuf> = Vec::new();
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("log") {
                    if let Ok(meta) = entry.metadata() {
                        if let Ok(mtime) = meta.modified() {
                            if let Ok(dur) = started_at.duration_since(mtime) {
                                if dur.as_secs() < 300 {
                                    recent_files.push(path);
                                }
                            } else {

                                recent_files.push(path);
                            }
                        }
                    }
                }
            }

            for path in recent_files {
                let meta = match std::fs::metadata(&path) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let size = meta.len();

                let start_offset = match offsets.get(&path) {
                    Some(&off) => off,
                    None => {
                        offsets.insert(path.clone(), size);
                        continue;
                    }
                };

                if size <= start_offset {
                    offsets.insert(path, size);
                    continue;
                }

                let mut file = match File::open(&path) {
                    Ok(f) => f,
                    Err(_) => continue,
                };

                if file.seek(SeekFrom::Start(start_offset)).is_err() {
                    continue;
                }

                let mut chunk = String::new();
                let _ = file.read_to_string(&mut chunk);
                offsets.insert(path, size);

                for cap in nep_re.captures_iter(&chunk) {
                    let url = cap.name("url").map(|m| m.as_str().trim()).unwrap_or("");
                    let audio = cap.name("audio").map(|m| m.as_str().trim()).unwrap_or("");
                    let video = cap.name("video").map(|m| m.as_str().trim()).unwrap_or("");

                    if audio.is_empty() && video.is_empty() {
                        continue;
                    }

                    let key = (audio.to_string(), video.to_string());
                    if handled.contains(&key) {
                        continue;
                    }
                    handled.insert(key);

                    let media_name = if !audio.is_empty() { audio } else { video };
                    let client_need_msg = format!("Client needs: {}", media_name);
                    log_fn(&client_need_msg, "warn");
                    join_log_write(&client_need_msg);

                    let (ok, detail) = ensure_media_fn(url, audio, video);
                    if ok {
                        fetched_count += 1;
                        let ok_msg = format!("  [ok] {}  ({})", media_name, detail);
                        log_fn(&ok_msg, "ok");
                        join_log_write(&ok_msg);
                    } else {
                        let err_msg = format!("  [fail] {}", detail);
                        log_fn(&err_msg, "err");
                        join_log_write(&err_msg);
                    }
                }
            }
        }

        std::thread::sleep(Duration::from_millis(1000));
    }
}
