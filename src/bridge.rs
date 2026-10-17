use crate::config::BRIDGE_PORT;
use serde_json::json;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use crate::privacy::is_local_request;
use std::io::Read;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

pub struct BridgeServer {
    running: Arc<AtomicBool>,
    pending: Arc<Mutex<Option<String>>>,
    staging_dir: PathBuf,
    thread: Option<JoinHandle<()>>,
}

impl BridgeServer {
    pub fn new() -> Self {
        let staging_dir = std::env::temp_dir().join("rbxm_bridge");
        let _ = std::fs::create_dir_all(&staging_dir);

        Self {
            running: Arc::new(AtomicBool::new(false)),
            pending: Arc::new(Mutex::new(None)),
            staging_dir,
            thread: None,
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        self.stop();

        let bind_addr = format!("127.0.0.1:{}", BRIDGE_PORT);
        let server = Server::http(&bind_addr)
            .map_err(|e| format!("Failed to bind Studio bridge on {}: {}", bind_addr, e))?;

        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);
        let pending = Arc::clone(&self.pending);
        let staging_dir = self.staging_dir.clone();

        self.thread = Some(thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                match server.recv_timeout(Duration::from_millis(500)) {
                    Ok(Some(request)) => handle(request, &pending, &staging_dir),
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

    pub fn queue_file(&self, file_path: &Path) -> Result<String, String> {
        let client = ureq::AgentBuilder::new()
            .timeout_read(Duration::from_secs(5))
            .build();
        let resp = client
            .post(&format!("http://127.0.0.1:{}/queue", BRIDGE_PORT))
            .send_json(json!({ "path": file_path.to_string_lossy().to_string() }))
            .map_err(|e| format!("Bridge queue request failed: {}", e))?;

        let val: serde_json::Value = resp
            .into_json()
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        if let Some(err) = val.get("error").and_then(|v| v.as_str()) {
            return Err(err.to_string());
        }

        Ok(val["staged"].as_str().unwrap_or("").to_string())
    }
}


const ALLOWED_EXTS: &[&str] = &["rbxm", "rbxmx", "rbxl", "rbxlx"];

fn json_response(request: Request, code: u16, body: serde_json::Value) {
    let resp = Response::from_string(body.to_string())
        .with_status_code(StatusCode(code))
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
    let _ = request.respond(resp);
}

fn handle(mut request: Request, pending: &Mutex<Option<String>>, staging_dir: &Path) {
    if !is_local_request(&request, BRIDGE_PORT) {
        return json_response(request, 403, json!({"error": "forbidden"}));
    }
    let method = request.method().clone();
    let url = request.url().to_string();

    match (method, url.as_str()) {
        (Method::Get, "/poll") => {
            let body = match &*pending.lock().unwrap() {
                None => json!({"status": "idle"}),
                Some(fname) => json!({
                    "status": "ready",
                    "name": fname,
                    "staging_dir": staging_dir.to_string_lossy()
                }),
            };
            json_response(request, 200, body);
        }
        (Method::Get, "/download") => {
            let fname = pending.lock().unwrap().clone();
            let Some(fname) = fname else {
                return json_response(request, 404, json!({"error": "no file pending"}));
            };
            let fpath = staging_dir.join(&fname);
            match File::open(&fpath) {
                Ok(file) if fpath.is_file() => {
                    let disp = format!("attachment; filename=\"{}\"", fname);
                    let resp = Response::from_file(file)
                        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/octet-stream"[..]).unwrap())
                        .with_header(Header::from_bytes(&b"Content-Disposition"[..], disp.as_bytes()).unwrap());
                    let _ = request.respond(resp);
                }
                _ => json_response(request, 404, json!({"error": "staged file missing"})),
            }
        }
        (Method::Post, "/queue") => {
            let mut body = String::new();
            let _ = request.as_reader().take(64 * 1024).read_to_string(&mut body);
            let Ok(val) = serde_json::from_str::<serde_json::Value>(&body) else {
                return json_response(request, 400, json!({"error": "invalid json body"}));
            };
            let src_path = val.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let src = Path::new(src_path);
            let ext_ok = src
                .extension()
                .and_then(|e| e.to_str())
                .map_or(false, |e| ALLOWED_EXTS.contains(&e.to_ascii_lowercase().as_str()));
            if !src.is_file() || !ext_ok {
                return json_response(
                    request,
                    400,
                    json!({"error": format!("not a Roblox model/place file: {}", src_path)}),
                );
            }
            let fname = src.file_name().unwrap_or_default().to_string_lossy().to_string();
            let dst = staging_dir.join(&fname);
            match std::fs::copy(src, &dst) {
                Ok(_) => {
                    *pending.lock().unwrap() = Some(fname);
                    json_response(request, 200, json!({"status": "queued", "staged": dst.to_string_lossy()}));
                }
                Err(e) => json_response(request, 500, json!({"error": format!("Failed to stage file: {}", e)})),
            }
        }
        (Method::Post, "/clear") => {
            *pending.lock().unwrap() = None;
            json_response(request, 200, json!({"status": "cleared"}));
        }
        _ => json_response(request, 404, json!({"error": "not found"})),
    }
}

impl Drop for BridgeServer {
    fn drop(&mut self) {
        self.stop();
    }
}
