use crate::config::{save_config, AppConfig};
use crate::identity::Identity;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::Path;

pub const KEYSTORE_BACKUP_VERSION: u32 = 1;
pub const KEYSTORE_MAGIC: &str = "puppytunnel-identity-v1";

#[derive(Debug, Clone)]
pub enum KeystoreError {
    InvalidSecret(String),
    ParseError(String),
    IoError(String),
    VersionMismatch(u32),
}

impl fmt::Display for KeystoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSecret(e) => write!(f, "Invalid secret key: {}", e),
            Self::ParseError(e) => write!(f, "Failed to parse key backup: {}", e),
            Self::IoError(e) => write!(f, "Disk I/O error: {}", e),
            Self::VersionMismatch(v) => write!(f, "Unsupported backup version: {}", v),
        }
    }
}

impl std::error::Error for KeystoreError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityBackup {
    pub magic: String,
    pub schema_version: u32,
    pub created_at: String,
    pub server_secret: String,
    pub ed25519_pubkey: String,
    pub x25519_pubkey: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub roblox_id: u64,
}

pub fn validate_secret_hex(secret_hex: &str) -> bool {
    let s = secret_hex.trim();
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn derive_ed25519_pubkey_hex(secret_hex: &str) -> String {
    let id = Identity::profile(secret_hex.trim());
    id.public().iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn create_backup(config: &AppConfig) -> IdentityBackup {
    let secret_hex = config.server_secret.trim().to_string();
    let ed25519_pubkey = derive_ed25519_pubkey_hex(&secret_hex);
    let x25519_pubkey = crate::community::e2ee::get_x25519_public_hex(&secret_hex);

    IdentityBackup {
        magic: KEYSTORE_MAGIC.to_string(),
        schema_version: KEYSTORE_BACKUP_VERSION,
        created_at: Utc::now().to_rfc3339(),
        server_secret: secret_hex,
        ed25519_pubkey,
        x25519_pubkey,
        username: config.roblox_name.clone(),
        display_name: if !config.roblox_name.is_empty() {
            config.roblox_name.clone()
        } else {
            "Puppy User".to_string()
        },
        roblox_id: config.roblox_id,
    }
}

pub fn export_backup_json(config: &AppConfig) -> Result<String, KeystoreError> {
    let backup = create_backup(config);
    serde_json::to_string_pretty(&backup)
        .map_err(|e| KeystoreError::ParseError(format!("JSON serialization failed: {}", e)))
}

pub fn export_backup_file(path: &Path, config: &AppConfig) -> Result<(), KeystoreError> {
    let json = export_backup_json(config)?;
    fs::write(path, json).map_err(|e| KeystoreError::IoError(e.to_string()))
}

pub fn import_backup_str(
    input: &str,
    config: &mut AppConfig,
) -> Result<IdentityBackup, KeystoreError> {
    let trimmed = input.trim();

    if validate_secret_hex(trimmed) {
        config.server_secret = trimmed.to_lowercase();
        save_config(config);
        return Ok(create_backup(config));
    }

    if !trimmed.starts_with('{') && trimmed.contains(' ') {
        if let Ok(secret) = mnemonic_to_secret(trimmed) {
            config.server_secret = secret;
            save_config(config);
            return Ok(create_backup(config));
        }
    }

    let backup: IdentityBackup = serde_json::from_str(trimmed)
        .map_err(|e| KeystoreError::ParseError(format!("Invalid JSON backup payload: {}", e)))?;

    if !validate_secret_hex(&backup.server_secret) {
        return Err(KeystoreError::InvalidSecret(
            "Backup contains invalid server_secret hex".into(),
        ));
    }

    config.server_secret = backup.server_secret.trim().to_lowercase();
    if !backup.username.is_empty() && config.roblox_name.is_empty() {
        config.roblox_name = backup.username.clone();
    }
    if backup.roblox_id > 0 && config.roblox_id == 0 {
        config.roblox_id = backup.roblox_id;
    }

    save_config(config);
    Ok(backup)
}

pub fn import_backup_file(
    path: &Path,
    config: &mut AppConfig,
) -> Result<IdentityBackup, KeystoreError> {
    let content = fs::read_to_string(path).map_err(|e| KeystoreError::IoError(e.to_string()))?;
    import_backup_str(&content, config)
}

const WORDLIST: [&str; 256] = [
    "puppy", "tunnel", "roblox", "studio", "spark", "alpha", "bravo", "cloud",
    "cyber", "delta", "eagle", "flare", "ghost", "haven", "ivory", "jelly",
    "karma", "lunar", "magic", "nexus", "orbit", "pixel", "quest", "radar",
    "solar", "titan", "ultra", "vivid", "waves", "xenon", "yield", "zenith",
    "amber", "blaze", "coral", "drift", "ember", "frost", "glade", "hazel",
    "inlet", "jade", "koala", "lotus", "maple", "noble", "oasis", "pearl",
    "quill", "river", "stone", "topaz", "unity", "valve", "willow", "zebra",
    "acorn", "bison", "cedar", "dune", "elm", "flint", "gecko", "harbor",
    "iris", "juniper", "kelp", "lark", "meadow", "newt", "otter", "pine",
    "quail", "raven", "spruce", "trout", "urchin", "viper", "walrus", "yarrow",
    "anchor", "bridge", "beacon", "canyon", "dome", "forge", "gate", "haven",
    "island", "jetty", "knoll", "lighthouse", "marsh", "needle", "orchard", "peak",
    "quarry", "ridge", "spire", "tower", "vale", "wharf", "yard", "vault",
    "arrow", "blade", "compass", "dagger", "engine", "feather", "glove", "helmet",
    "iron", "journal", "kettle", "lantern", "mirror", "needle", "paddle", "quiver",
    "rope", "shield", "telescope", "urn", "vessel", "wheel", "yoke", "zipper",
    "apple", "berry", "cherry", "date", "fig", "grape", "honey", "juice",
    "kiwi", "lemon", "mango", "nutmeg", "olive", "peach", "quince", "radish",
    "sage", "thyme", "vanilla", "wheat", "yam", "zest", "clove", "mint",
    "azure", "bronze", "copper", "denim", "emerald", "fuchsia", "gold", "indigo",
    "jet", "khaki", "lime", "magenta", "navy", "ochre", "plum", "ruby",
    "silver", "teal", "umber", "violet", "white", "yellow", "cyan", "sepia",
    "atomic", "binary", "crypto", "digital", "echo", "fiber", "gateway", "hybrid",
    "input", "kernel", "logic", "matrix", "network", "output", "packet", "quantum",
    "router", "socket", "terminal", "uplink", "vector", "widget", "proxy", "daemon",
    "bubble", "canvas", "glaze", "pastel", "velvet", "ribbon", "shimmer", "sparkle",
    "cotton", "breeze", "puddle", "button", "marshmallow", "cookie", "candy", "sugar",
    "cherry", "biscuit", "butter", "waffle", "muffin", "donut", "frosting", "sprinkles",
    "bounce", "cozy", "dream", "giggle", "happy", "honey", "joyful", "kitten",
    "lively", "melody", "nifty", "petal", "quirky", "rainbow", "sunny", "twinkle",
    "comet", "daisy", "fable", "glimmer", "blossom", "cascade", "journey", "nectar",
    "pebble", "ripple", "starlight", "timber", "whisper", "zephyr", "horizon", "aurora",
];

pub fn generate_mnemonic(secret_hex: &str) -> String {
    let s = secret_hex.trim();
    if !validate_secret_hex(s) {
        return String::new();
    }
    let bytes: Vec<u8> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0))
        .collect();

    let words: Vec<&str> = bytes.iter().map(|&b| WORDLIST[b as usize]).collect();
    words.join(" ")
}

pub fn mnemonic_to_secret(phrase: &str) -> Result<String, KeystoreError> {
    let words: Vec<&str> = phrase.trim().split_whitespace().collect();
    if words.len() != 32 {
        return Err(KeystoreError::ParseError(format!(
            "Mnemonic phrase must contain exactly 32 words, got {}",
            words.len()
        )));
    }

    let mut bytes = Vec::with_capacity(32);
    for word in words {
        let clean = word.to_ascii_lowercase();
        let idx = WORDLIST
            .iter()
            .position(|&w| w == clean)
            .ok_or_else(|| KeystoreError::ParseError(format!("Unknown recovery word: '{}'", word)))?;
        bytes.push(idx as u8);
    }

    Ok(bytes.iter().map(|b| format!("{:02x}", b)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_validation_and_derivation() {
        let valid_secret = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert!(validate_secret_hex(valid_secret));

        let pubkey = derive_ed25519_pubkey_hex(valid_secret);
        assert_eq!(pubkey.len(), 64);

        assert!(!validate_secret_hex("short"));
        assert!(!validate_secret_hex("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg"));
    }

    #[test]
    fn test_backup_json_roundtrip() {
        let mut config = AppConfig::default();
        config.server_secret = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".into();
        config.roblox_name = "builder_pro".into();
        config.roblox_id = 998877;

        let json = export_backup_json(&config).expect("export json failed");
        assert!(json.contains("puppytunnel-identity-v1"));
        assert!(json.contains("builder_pro"));

        let mut fresh_config = AppConfig::default();
        let restored = import_backup_str(&json, &mut fresh_config).expect("import failed");

        assert_eq!(fresh_config.server_secret, config.server_secret);
        assert_eq!(restored.roblox_id, 998877);
        assert_eq!(restored.username, "builder_pro");
    }

    #[test]
    fn test_mnemonic_roundtrip() {
        let secret = "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";
        let mnemonic = generate_mnemonic(secret);
        let words: Vec<&str> = mnemonic.split_whitespace().collect();
        assert_eq!(words.len(), 32);

        let recovered = mnemonic_to_secret(&mnemonic).expect("mnemonic decode failed");
        assert_eq!(recovered, secret);
    }
}
