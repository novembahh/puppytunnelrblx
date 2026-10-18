use crate::config::AppConfig;
use crate::identity::{self, hex, unhex, unix_now, Identity};
use crate::master;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;

pub const ACCENTS: &[(&str, &str)] = &[
    ("mint", "#5fe3ac"),
    ("sky", "#6cb8ff"),
    ("violet", "#b69cff"),
    ("amber", "#f0b455"),
    ("rose", "#ff8fa3"),
];

pub fn accent_hex(name: &str) -> &'static str {
    ACCENTS.iter().find(|(n, _)| *n == name).map(|(_, h)| *h).unwrap_or(ACCENTS[0].1)
}

pub fn clean_accent(raw: &str) -> String {
    let t = raw.trim().to_ascii_lowercase();
    if let Some((_, h)) = ACCENTS.iter().find(|(n, _)| *n == t) {
        return h.to_string();
    }
    let ok = t.len() == 7 && t.starts_with('#') && t[1..].chars().all(|c| c.is_ascii_hexdigit());
    if ok { t } else { ACCENTS[0].1.to_string() }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ProfileBody {
    pub roblox_id: u64,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub bio: String,
    #[serde(default)]
    pub accent: String,
    #[serde(default)]
    pub ts: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SignedProfile {
    pub pubkey: String,
    pub sig: String,
    #[serde(flatten)]
    pub body: ProfileBody,
}

fn signing_bytes(b: &ProfileBody) -> Vec<u8> {
    let mut v = b"puppy-profile\0".to_vec();
    v.extend_from_slice(&serde_json::to_vec(b).unwrap_or_default());
    v
}

impl SignedProfile {
    pub fn sign(id: &Identity, mut body: ProfileBody) -> Self {
        body.ts = unix_now();
        let sig = id.sign(&signing_bytes(&body));
        Self { pubkey: hex(&id.public()), sig: hex(&sig), body }
    }

    pub fn verify(&self, max_age: u64) -> Result<[u8; 32], &'static str> {
        let pk = unhex::<32>(&self.pubkey).ok_or("bad public key")?;
        let sig = unhex::<64>(&self.sig).ok_or("bad signature")?;
        if unix_now().abs_diff(self.body.ts) > max_age {
            return Err("stale or future timestamp");
        }
        if self.body.roblox_id == 0 {
            return Err("roblox_id is required");
        }
        if !identity::verify(&pk, &signing_bytes(&self.body), &sig) {
            return Err("signature does not match");
        }
        Ok(pk)
    }
}

pub fn verify_code(pubkey_hex: &str, roblox_id: u64) -> String {
    let mut h = Sha256::new();
    h.update(b"puppy-verify");
    h.update(pubkey_hex.as_bytes());
    h.update(roblox_id.to_be_bytes());
    format!("puppytunnel-{}", &hex(&h.finalize())[..12])
}

pub fn host_message(server_pk: &[u8; 32]) -> Vec<u8> {
    let mut m = b"puppy-host\0".to_vec();
    m.extend_from_slice(server_pk);
    m
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct PublicProfile {
    pub id: String,
    pub verified: bool,
    #[serde(default)]
    pub profile_name: String,
    #[serde(default)]
    pub roblox_id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub bio: String,
    #[serde(default)]
    pub accent: String,
    #[serde(default)]
    pub code: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct HostInfo {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub accent: String,
}

pub fn profile_identity(config: &AppConfig) -> Identity {
    Identity::profile(&config.server_secret)
}

pub fn sanitize_text(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

pub fn body_from_config(config: &AppConfig) -> ProfileBody {
    ProfileBody {
        roblox_id: config.roblox_id,
        display_name: sanitize_text(&config.display_name, 40),
        bio: sanitize_text(&config.profile_bio, 200),
        accent: clean_accent(&config.profile_accent),
        ts: 0,
    }
}

pub fn publish_targets(config: &AppConfig) -> Vec<String> {
    let mut out = master::master_list(&config.profile_servers);
    for m in master::master_list(&config.master) {
        if !out.contains(&m) {
            out.push(m);
        }
    }
    out
}

pub fn host_binding(config: &AppConfig, server: &Identity) -> (String, String) {
    if config.roblox_id == 0 || !config.show_profile {
        return (String::new(), String::new());
    }
    let pid = profile_identity(config);
    (hex(&pid.public()), hex(&pid.sign(&host_message(&server.public()))))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(8)).build()
}

pub fn publish(servers: &[String], signed: &SignedProfile) -> Vec<String> {
    let body = serde_json::to_value(signed).unwrap_or_default();
    servers
        .iter()
        .filter_map(|s| {
            agent()
                .post(&format!("{}/api/profile", master::normalize_url(s)))
                .send_json(body.clone())
                .err()
                .map(|e| match e {
                    ureq::Error::Status(c, r) => {
                        let m = r.into_json::<serde_json::Value>().ok().and_then(|v| v["error"].as_str().map(String::from));
                        format!("{}: {}{}", s, c, m.map(|m| format!(" {}", m)).unwrap_or_default())
                    }
                    e => format!("{}: {}", s, e),
                })
        })
        .collect()
}

pub fn status(server: &str, pubkey_hex: &str) -> Result<PublicProfile, String> {
    agent()
        .get(&format!("{}/api/profile", master::normalize_url(server)))
        .query("id", pubkey_hex)
        .call()
        .map_err(|e| e.to_string())?
        .into_json::<PublicProfile>()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_and_codes() {
        let id = Identity::profile("secret");
        let p = SignedProfile::sign(&id, ProfileBody { roblox_id: 1, display_name: String::new(), bio: "hi".into(), accent: "mint".into(), ts: 0 });
        assert!(p.verify(60).is_ok());
        let mut t = p.clone();
        t.body.roblox_id = 2;
        assert!(t.verify(60).is_err());
        assert_ne!(verify_code(&p.pubkey, 1), verify_code(&p.pubkey, 2));
        assert_ne!(verify_code(&p.pubkey, 1), verify_code("other", 1));
        assert_eq!(sanitize_text("a\x00b\x07c", 10), "abc");
        assert_eq!(sanitize_text("  hi  ", 10), "hi");
        assert_eq!(sanitize_text("abcdef", 3), "abc");
        assert_eq!(clean_accent("nope"), "#5fe3ac");
        assert_eq!(clean_accent("#AABBCC"), "#aabbcc");
    }
}
