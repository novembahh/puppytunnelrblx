use crate::config::BOOMBOX_PORT;
use regex::Regex;
use serde_json::json;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tiny_http::{Header, Response, Server, StatusCode};

#[derive(Clone, Debug)]
pub enum JobState {
    Processing,
    Done(serde_json::Value),
    Error(String),
}

pub struct BoomboxServer {
    running: Arc<AtomicBool>,
    jobs: Arc<Mutex<HashMap<String, JobState>>>,
    thread: Option<JoinHandle<()>>,
}

impl BoomboxServer {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            jobs: Arc::new(Mutex::new(HashMap::new())),
            thread: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn start(&mut self) -> Result<(), String> {
        self.stop();

        let bind_addr = format!("127.0.0.1:{}", BOOMBOX_PORT);
        let server = Server::http(&bind_addr)
            .map_err(|e| format!("Failed to bind Boombox server on {}: {}", bind_addr, e))?;

        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);
        let jobs = Arc::clone(&self.jobs);

        self.thread = Some(thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                match server.recv_timeout(Duration::from_millis(500)) {
                    Ok(Some(request)) => {
                        handle_boombox_request(request, &jobs);
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        }));

        Ok(())
    }

    pub fn stop(&mut self) {
        if self.running.swap(false, Ordering::SeqCst) {
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

impl Drop for BoomboxServer {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn roblox_versions_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        let base = std::env::var("LOCALAPPDATA").ok()?;
        Some(PathBuf::from(base).join("Roblox").join("Versions"))
    } else if cfg!(target_os = "macos") {
        let home = std::env::var("HOME").ok()?;
        Some(PathBuf::from(home).join("Library/Application Support/Roblox/Versions"))
    } else if cfg!(target_os = "linux") {
        let home = std::env::var("HOME").ok()?;
        Some(PathBuf::from(home).join(".var/app/org.vinegarhq.Vinegar/data/Roblox/Versions"))
    } else {
        None
    }
}

pub fn find_all_sounds_dirs() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(versions) = roblox_versions_dir() {
        if let Ok(entries) = std::fs::read_dir(&versions) {
            for entry in entries.flatten() {
                let sounds = entry.path().join("content").join("sounds");
                if sounds.is_dir() {
                    candidates.push(sounds);
                }
            }
        }
    }
    candidates
}

pub fn downloads_dir() -> PathBuf {
    let dir = crate::config::get_script_dir()
        .join("local-boombox-data")
        .join("downloads");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn find_ytdlp() -> Option<String> {
    let local = crate::config::get_script_dir().join("yt-dlp.exe");
    if local.is_file() {
        return Some(local.to_string_lossy().to_string());
    }
    if let Ok(out) = Command::new("yt-dlp").arg("--version").output() {
        if out.status.success() {
            return Some("yt-dlp".to_string());
        }
    }
    None
}

pub fn find_ffmpeg() -> Option<String> {
    let local = crate::config::get_script_dir().join("ffmpeg.exe");
    if local.is_file() {
        return Some(local.to_string_lossy().to_string());
    }
    if let Ok(out) = Command::new("ffmpeg").arg("-version").output() {
        if out.status.success() {
            return Some("ffmpeg".to_string());
        }
    }
    None
}

pub fn media_key(filename: &str) -> String {
    let path = Path::new(filename);
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let re_roblox = Regex::new(r"(?i)_roblox$").unwrap();
    let stem = re_roblox.replace(&stem, "");
    let re_id = Regex::new(r"_[A-Za-z0-9_-]{6,15}$").unwrap();
    let stem = re_id.replace(&stem, "");
    let re_non_alnum = Regex::new(r"[^a-z0-9]+").unwrap();
    re_non_alnum.replace_all(&stem.to_lowercase(), "").to_string()
}

pub fn find_existing_media(target: &str, search_dirs: &[PathBuf]) -> Option<PathBuf> {
    if target.is_empty() {
        return None;
    }
    for dir in search_dirs {
        let p = dir.join(target);
        if p.is_file() {
            return Some(p);
        }
    }

    let target_key = media_key(target);
    if target_key.is_empty() {
        return None;
    }
    let target_ext = Path::new(target)
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();

    for dir in search_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let ext = path
                        .extension()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase();
                    if ext == target_ext {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        if media_key(&name) == target_key {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }

    None
}


const MAX_MEDIA_BYTES: u64 = 256 * 1024 * 1024;

fn install_media(out_path: &Path, filename: &str, sounds_dirs: &[PathBuf]) {
    for s_dir in sounds_dirs {
        let _ = std::fs::copy(out_path, s_dir.join(filename));
    }
}

fn fetch_media(url: &str, filename: &str, dl_dir: &Path, sounds_dirs: &[PathBuf]) -> Result<(), String> {
    let out_path = dl_dir.join(filename);
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .resolver(crate::privacy::public_only_resolver)
        .build();

    let direct = agent.get(url).call().map_err(|e| e.to_string()).and_then(|resp| {
        let mut f = File::create(&out_path).map_err(|e| format!("File create error: {}", e))?;
        let mut reader = std::io::Read::take(resp.into_reader(), MAX_MEDIA_BYTES + 1);
        let copied = std::io::copy(&mut reader, &mut f).map_err(|e| e.to_string())?;
        if copied > MAX_MEDIA_BYTES {
            return Err("file too large".to_string());
        }
        Ok(())
    });

    match direct {
        Ok(()) => {
            install_media(&out_path, filename, sounds_dirs);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&out_path);
            if let Some(ytdlp) = find_ytdlp() {
                let ok = Command::new(ytdlp)
                    .args(["-x", "--audio-format", "mp3", "-o"])
                    .arg(&out_path)
                    .arg("--")
                    .arg(url)
                    .output()
                    .map_or(false, |o| o.status.success());
                if ok {
                    install_media(&out_path, filename, sounds_dirs);
                    return Ok(());
                }
            }
            Err(format!("Download failed: {}", e))
        }
    }
}

fn handle_boombox_request(
    request: tiny_http::Request,
    jobs: &Arc<Mutex<HashMap<String, JobState>>>,
) {
    if !crate::privacy::is_local_request(&request, BOOMBOX_PORT) {
        send_json_response(request, StatusCode(403), r#"{"ok":false,"error":"forbidden"}"#);
        return;
    }
    let url_str = request.url().to_string();
    let (path, query) = if let Some(idx) = url_str.find('?') {
        (&url_str[..idx], &url_str[idx + 1..])
    } else {
        (url_str.as_str(), "")
    };

    let params: HashMap<String, String> = query
        .split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let k = parts.next()?;
            let v = parts.next().unwrap_or("");
            Some((
                urlencoding_decode(k),
                urlencoding_decode(v),
            ))
        })
        .collect();

    match path {
        "/health" => {
            let sounds_dirs = find_all_sounds_dirs();
            let first_sounds = sounds_dirs
                .first()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let sounds_str_list: Vec<String> = sounds_dirs
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            let body = json!({
                "ok": true,
                "soundsDir": first_sounds,
                "soundsDirs": sounds_str_list,
                "ytdlp": find_ytdlp().unwrap_or_else(|| "not found (will auto-download)".to_string()),
                "ffmpeg": find_ffmpeg().unwrap_or_else(|| "not found".to_string())
            })
            .to_string();

            send_json_response(request, StatusCode(200), &body);
        }
        "/job-status" => {
            let id = params.get("id").map(|s| s.as_str()).unwrap_or("");
            if id.is_empty() {
                send_json_response(request, StatusCode(400), r#"{"ok":false,"error":"Missing id query parameter."}"#);
                return;
            }

            let state = {
                let guard = jobs.lock().unwrap();
                guard.get(id).cloned()
            };

            match state {
                Some(JobState::Processing) => {
                    send_json_response(request, StatusCode(200), r#"{"ok":true,"status":"processing"}"#);
                }
                Some(JobState::Done(mut result)) => {
                    result["status"] = json!("done");
                    let body = result.to_string();
                    jobs.lock().unwrap().remove(id);
                    send_json_response(request, StatusCode(200), &body);
                }
                Some(JobState::Error(err)) => {
                    let body = json!({"ok": false, "status": "error", "error": err}).to_string();
                    jobs.lock().unwrap().remove(id);
                    send_json_response(request, StatusCode(400), &body);
                }
                None => {
                    send_json_response(request, StatusCode(404), r#"{"ok":false,"error":"Job not found."}"#);
                }
            }
        }
        "/ensure" => {
            let url = params.get("url").cloned().unwrap_or_default();
            let raw_audio = params.get("audio").map(|s| s.as_str()).unwrap_or("");
            let raw_video = params.get("video").map(|s| s.as_str()).unwrap_or("");
            let clean = |raw: &str| if raw.trim().is_empty() { Some(String::new()) } else { crate::privacy::sanitize_media_name(raw) };
            let (Some(audio), Some(video)) = (clean(raw_audio), clean(raw_video)) else {
                send_json_response(request, StatusCode(400), r#"{"ok":false,"error":"Invalid media filename."}"#);
                return;
            };
            if !url.is_empty() && !crate::privacy::is_http_url(&url) {
                send_json_response(request, StatusCode(400), r#"{"ok":false,"error":"Only http(s) URLs are allowed."}"#);
                return;
            }

            if audio.is_empty() && video.is_empty() {
                send_json_response(request, StatusCode(400), r#"{"ok":false,"error":"Missing audio/video filename."}"#);
                return;
            }

            let job_id = uuid::Uuid::new_v4().to_string();
            jobs.lock().unwrap().insert(job_id.clone(), JobState::Processing);

            let jobs_clone = Arc::clone(jobs);
            let job_id_clone = job_id.clone();

            thread::spawn(move || {
                let sounds_dirs = find_all_sounds_dirs();
                let dl_dir = downloads_dir();
                let mut search_dirs = vec![dl_dir.clone()];
                search_dirs.extend_from_slice(&sounds_dirs);

                let target = if !audio.is_empty() { &audio } else { &video };
                if let Some(existing) = find_existing_media(target, &search_dirs) {
                    for s_dir in &sounds_dirs {
                        let dst = s_dir.join(existing.file_name().unwrap_or_default());
                        if !dst.is_file() {
                            let _ = std::fs::copy(&existing, &dst);
                        }
                    }
                    jobs_clone.lock().unwrap().insert(
                        job_id_clone,
                        JobState::Done(json!({"ok": true, "action": "reused", "filename": existing.file_name().unwrap().to_string_lossy().to_string()})),
                    );
                    return;
                }

                if !url.is_empty() {
                    let filename = if !audio.is_empty() { &audio } else { &video };
                    let state = match fetch_media(&url, filename, &dl_dir, &sounds_dirs) {
                        Ok(()) => JobState::Done(json!({"ok": true, "action": "downloaded", "filename": filename})),
                        Err(e) => JobState::Error(e),
                    };
                    jobs_clone.lock().unwrap().insert(job_id_clone, state);
                    return;
                }

                jobs_clone.lock().unwrap().insert(
                    job_id_clone,
                    JobState::Error(format!("Media file not found and cannot download: {}", target)),
                );
            });

            let body = json!({"ok": true, "status": "processing", "jobId": job_id}).to_string();
            send_json_response(request, StatusCode(200), &body);
        }
        "/avatar" => {
            let username = params.get("username").map(|s| s.as_str()).unwrap_or("");
            let user_id_param = params.get("userId").map(|s| s.as_str()).unwrap_or("");

            let client = ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(10))
                .build();

            let (user_id, res_name) = if !user_id_param.is_empty() {
                (user_id_param.parse::<i64>().unwrap_or(0), username.to_string())
            } else if !username.is_empty() {
                let res = client
                    .post("https://users.roblox.com/v1/usernames/users")
                    .send_json(json!({
                        "usernames": [username],
                        "excludeBannedUsers": false
                    }));
                match res {
                    Ok(r) => match r.into_json::<serde_json::Value>() {
                        Ok(data) => {
                            let entry = data["data"].as_array().and_then(|a| a.first());
                            let id = entry.and_then(|e| e["id"].as_i64()).unwrap_or(0);
                            let name = entry.and_then(|e| e["name"].as_str()).unwrap_or(username).to_string();
                            (id, name)
                        }
                        Err(_) => (0, username.to_string()),
                    },
                    Err(_) => (0, username.to_string()),
                }
            } else {
                send_json_response(request, StatusCode(400), r#"{"ok":false,"error":"Provide username or userId."}"#);
                return;
            };

            if user_id == 0 {
                send_json_response(request, StatusCode(404), r#"{"ok":false,"error":"User not found."}"#);
                return;
            }

            match client
                .get(&format!("https://avatar.roblox.com/v1/users/{}/avatar", user_id))
                .call()
            {
                Ok(resp) => match resp.into_json::<serde_json::Value>() {
                    Ok(avatar) => {
                        let mut assets = Vec::new();
                        if let Some(arr) = avatar["assets"].as_array() {
                            for a in arr {
                                assets.push(json!({
                                    "id": a["id"],
                                    "name": a["name"],
                                    "assetTypeId": a["assetType"]["id"],
                                    "assetTypeName": a["assetType"]["name"]
                                }));
                            }
                        }
                        let body = json!({
                            "ok": true,
                            "userId": user_id,
                            "username": res_name,
                            "playerAvatarType": avatar["playerAvatarType"],
                            "assets": assets,
                            "bodyColors": avatar["bodyColors"],
                            "scales": avatar["scales"]
                        })
                        .to_string();
                        send_json_response(request, StatusCode(200), &body);
                    }
                    Err(e) => {
                        let body = json!({"ok": false, "error": format!("JSON parse error: {}", e)}).to_string();
                        send_json_response(request, StatusCode(500), &body);
                    }
                },
                Err(e) => {
                    let body = json!({"ok": false, "error": format!("Roblox API error: {}", e)}).to_string();
                    send_json_response(request, StatusCode(400), &body);
                }
            }
        }
        "/outfits" => {
            let username = params.get("username").map(|s| s.as_str()).unwrap_or("");
            if username.is_empty() {
                send_json_response(request, StatusCode(400), r#"{"ok":false,"error":"Missing username query parameter."}"#);
                return;
            }

            let client = ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(10))
                .build();

            let user_id = match client
                .post("https://users.roblox.com/v1/usernames/users")
                .send_json(json!({ "usernames": [username], "excludeBannedUsers": false }))
            {
                Ok(r) => match r.into_json::<serde_json::Value>() {
                    Ok(data) => data["data"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|e| e["id"].as_i64())
                        .unwrap_or(0),
                    Err(_) => 0,
                },
                Err(_) => 0,
            };

            if user_id == 0 {
                send_json_response(request, StatusCode(404), r#"{"ok":false,"error":"User not found."}"#);
                return;
            }

            match client
                .get(&format!("https://avatar.roblox.com/v1/users/{}/outfits?page=1&itemsPerPage=50", user_id))
                .call()
            {
                Ok(resp) => match resp.into_json::<serde_json::Value>() {
                    Ok(outfits) => {
                        let body = json!({ "ok": true, "data": outfits["data"] }).to_string();
                        send_json_response(request, StatusCode(200), &body);
                    }
                    Err(e) => {
                        let body = json!({"ok": false, "error": format!("JSON parse error: {}", e)}).to_string();
                        send_json_response(request, StatusCode(500), &body);
                    }
                },
                Err(e) => {
                    let body = json!({"ok": false, "error": format!("Roblox API error: {}", e)}).to_string();
                    send_json_response(request, StatusCode(400), &body);
                }
            }
        }
        _ => {
            send_json_response(request, StatusCode(404), r#"{"ok":false,"error":"Not found"}"#);
        }
    }
}

fn send_json_response(request: tiny_http::Request, code: StatusCode, body: &str) {
    let resp = Response::from_string(body)
        .with_status_code(code)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
    let _ = request.respond(resp);
}

pub(crate) fn urlencoding_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 3 <= bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                if let Some(v) = hex {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn ensure_media_local(url: &str, audio: &str, video: &str) -> (bool, String) {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(90)).build();
    let base = format!("http://127.0.0.1:{}", BOOMBOX_PORT);
    let started = agent
        .get(&format!("{}/ensure", base))
        .query("url", url)
        .query("audio", audio)
        .query("video", video)
        .call()
        .map_err(|e| e.to_string())
        .and_then(|r| r.into_json::<serde_json::Value>().map_err(|e| e.to_string()));
    let job = match started {
        Ok(v) if v["ok"].as_bool() == Some(true) => v,
        Ok(v) => return (false, v["error"].as_str().unwrap_or("fetch failed").to_string()),
        Err(e) => return (false, e),
    };
    let Some(id) = job["jobId"].as_str() else {
        return (true, "media fetched".to_string());
    };
    for _ in 0..180 {
        thread::sleep(Duration::from_millis(500));
        let resp = agent.get(&format!("{}/job-status", base)).query("id", id).call();
        let body = match resp {
            Ok(r) => r.into_json::<serde_json::Value>().ok(),
            Err(ureq::Error::Status(_, r)) => r.into_json::<serde_json::Value>().ok(),
            Err(e) => return (false, e.to_string()),
        };
        match body.as_ref().and_then(|b| b["status"].as_str()) {
            Some("processing") => continue,
            Some("done") => return (true, body.unwrap()["action"].as_str().unwrap_or("media fetched").to_string()),
            _ => {
                let err = body.as_ref().and_then(|b| b["error"].as_str()).unwrap_or("fetch failed");
                return (false, err.to_string());
            }
        }
    }
    (false, "timed out waiting for download".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_browser_requests_and_bad_names() {
        let mut srv = BoomboxServer::new();
        srv.start().unwrap();
        let url = format!("http://127.0.0.1:{}", BOOMBOX_PORT);

        assert_eq!(ureq::get(&format!("{}/health", url)).call().unwrap().status(), 200);

        let blocked = ureq::get(&format!("{}/health", url)).set("Origin", "https://evil.example").call();
        assert!(matches!(blocked, Err(ureq::Error::Status(403, _))));

        let traversal = ureq::get(&format!("{}/ensure", url))
            .query("audio", "../../evil.exe")
            .query("url", "http://127.0.0.1/x")
            .call();
        assert!(matches!(traversal, Err(ureq::Error::Status(400, _))));

        assert_eq!(urlencoding_decode("a%20b+c%C3%A9%zz%4"), "a b cé%zz%4");
        srv.stop();
    }
}
