use crate::privacy::public_only_resolver;
use std::io::Read;
use std::time::Duration;

pub const USERS_API: &str = "https://users.roblox.com";
const THUMBS_API: &str = "https://thumbnails.roblox.com";
const MAX_IMAGE_BYTES: u64 = 2 * 1024 * 1024;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RobloxUser {
    pub id: u64,
    pub name: String,
    pub display_name: String,
    pub description: String,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).build()
}

fn describe(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(429, _) => "Roblox is rate limiting requests, try again in a minute".to_string(),
        ureq::Error::Status(code, _) => format!("Roblox answered {}", code),
        e => format!("could not reach Roblox: {}", e),
    }
}

pub fn user_by_id(base: &str, id: u64) -> Result<RobloxUser, String> {
    let v: serde_json::Value = agent()
        .get(&format!("{}/v1/users/{}", base.trim_end_matches('/'), id))
        .call()
        .map_err(describe)?
        .into_json()
        .map_err(|e| e.to_string())?;
    if v["id"].as_u64() != Some(id) {
        return Err("user not found".to_string());
    }
    Ok(RobloxUser {
        id,
        name: v["name"].as_str().unwrap_or_default().to_string(),
        display_name: v["displayName"].as_str().unwrap_or_default().to_string(),
        description: v["description"].as_str().unwrap_or_default().to_string(),
    })
}

pub fn lookup(base: &str, query: &str) -> Result<RobloxUser, String> {
    let q = query.trim().trim_start_matches('@');
    if q.is_empty() {
        return Err("enter a Roblox username or ID".to_string());
    }
    let id = if q.chars().all(|c| c.is_ascii_digit()) {
        q.parse::<u64>().map_err(|_| "invalid user ID".to_string())?
    } else {
        let v: serde_json::Value = agent()
            .post(&format!("{}/v1/usernames/users", base.trim_end_matches('/')))
            .send_json(serde_json::json!({"usernames": [q], "excludeBannedUsers": false}))
            .map_err(describe)?
            .into_json()
            .map_err(|e| e.to_string())?;
        v["data"][0]["id"].as_u64().ok_or_else(|| format!("no Roblox user named {}", q))?
    };
    user_by_id(base, id)
}

pub fn headshot_png(id: u64) -> Result<Vec<u8>, String> {
    thumbnail_png("avatar-headshot", id, "150x150")
}

pub fn avatar_png(id: u64) -> Result<Vec<u8>, String> {
    thumbnail_png("avatar", id, "720x720")
}

pub fn thumbnail_url(kind: &str, id: u64, size: &str) -> Result<String, String> {
    for _ in 0..3 {
        let v: serde_json::Value = agent()
            .get(&format!("{}/v1/users/{}", THUMBS_API, kind))
            .query("userIds", &id.to_string())
            .query("size", size)
            .query("format", "Png")
            .call()
            .map_err(describe)?
            .into_json()
            .map_err(|e| e.to_string())?;
        let state = v["data"][0]["state"].as_str().unwrap_or("");
        if state == "Completed" {
            if let Some(u) = v["data"][0]["imageUrl"].as_str() {
                return Ok(u.to_string());
            }
            return Err("avatar image is not available yet".to_string());
        }
        std::thread::sleep(Duration::from_millis(800));
    }
    Err("avatar image is not available yet".to_string())
}

fn thumbnail_png(kind: &str, id: u64, size: &str) -> Result<Vec<u8>, String> {
    let url = thumbnail_url(kind, id, size)?;
    let url = url.as_str();
    let rest = url.strip_prefix("https://").ok_or("unexpected avatar URL")?;
    let host = rest.split('/').next().unwrap_or_default();
    if !host.ends_with(".rbxcdn.com") {
        return Err("unexpected avatar host".to_string());
    }
    let cdn = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .resolver(public_only_resolver)
        .build();
    let mut bytes = Vec::new();
    cdn.get(url)
        .call()
        .map_err(describe)?
        .into_reader()
        .take(MAX_IMAGE_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct BodyColors {
    pub ids: [u32; 6],
    pub height: f32,
    pub width: f32,
}

#[allow(dead_code)]
pub fn body_colors(id: u64) -> Result<BodyColors, String> {
    let v: serde_json::Value = agent()
        .get(&format!("https://avatar.roblox.com/v1/users/{}/avatar", id))
        .call()
        .map_err(describe)?
        .into_json()
        .map_err(|e| e.to_string())?;
    let b = &v["bodyColors"];
    let get = |k: &str| b[k].as_u64().unwrap_or(194) as u32;
    Ok(BodyColors {
        ids: [
            get("headColorId"),
            get("torsoColorId"),
            get("rightArmColorId"),
            get("leftArmColorId"),
            get("rightLegColorId"),
            get("leftLegColorId"),
        ],
        height: v["scales"]["height"].as_f64().unwrap_or(1.0) as f32,
        width: v["scales"]["width"].as_f64().unwrap_or(1.0) as f32,
    })
}
