use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Beta,
    Nightly,
}

impl Channel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
            Channel::Nightly => "nightly",
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for Channel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stable" => Ok(Channel::Stable),
            "beta" => Ok(Channel::Beta),
            "nightly" => Ok(Channel::Nightly),
            other => Err(format!("Unknown update channel: '{}'", other)),
        }
    }
}

impl From<&str> for Channel {
    fn from(s: &str) -> Self {
        s.parse().unwrap_or(Channel::Stable)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    WindowsX64,
    WindowsArm64,
    LinuxX64,
    LinuxArm64,
    MacosX64,
    MacosArm64,
    Other(String),
}

impl Platform {
    pub fn current() -> Self {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            Platform::WindowsX64
        }
        #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
        {
            Platform::WindowsArm64
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            Platform::LinuxX64
        }
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            Platform::LinuxArm64
        }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            Platform::MacosX64
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            Platform::MacosArm64
        }
        #[cfg(not(any(
            all(target_os = "windows", any(target_arch = "x86_64", target_arch = "aarch64")),
            all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")),
            all(target_os = "macos", any(target_arch = "x86_64", target_arch = "aarch64"))
        )))]
        {
            Platform::Other(format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH))
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Platform::WindowsX64 => "windows-x86_64",
            Platform::WindowsArm64 => "windows-aarch64",
            Platform::LinuxX64 => "linux-x86_64",
            Platform::LinuxArm64 => "linux-aarch64",
            Platform::MacosX64 => "macos-x86_64",
            Platform::MacosArm64 => "macos-aarch64",
            Platform::Other(s) => s.as_str(),
        }
    }

    pub fn matches_filename(&self, filename: &str) -> bool {
        let f = filename.to_ascii_lowercase();
        match self {
            Platform::WindowsX64 => {
                (f.contains("win") || f.ends_with(".exe"))
                    && (f.contains("x86_64") || f.contains("x64") || f.contains("amd64") || !f.contains("arm"))
            }
            Platform::WindowsArm64 => {
                (f.contains("win") || f.ends_with(".exe"))
                    && (f.contains("arm64") || f.contains("aarch64"))
            }
            Platform::LinuxX64 => {
                f.contains("linux")
                    && (f.contains("x86_64") || f.contains("x64") || f.contains("amd64"))
                    && !f.ends_with(".exe")
            }
            Platform::LinuxArm64 => {
                f.contains("linux")
                    && (f.contains("arm64") || f.contains("aarch64"))
                    && !f.ends_with(".exe")
            }
            Platform::MacosX64 => {
                (f.contains("macos") || f.contains("darwin") || f.contains("osx") || f.ends_with(".dmg"))
                    && (f.contains("x86_64") || f.contains("x64") || f.contains("intel"))
            }
            Platform::MacosArm64 => {
                (f.contains("macos") || f.contains("darwin") || f.contains("osx") || f.ends_with(".dmg"))
                    && (f.contains("arm64") || f.contains("aarch64") || f.contains("m1") || f.contains("apple"))
            }
            Platform::Other(custom) => f.contains(&custom.to_ascii_lowercase()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetInfo {
    pub name: String,
    pub download_url: String,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default)]
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub version: String,
    pub channel: Channel,
    #[serde(default)]
    pub release_date: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub assets: Vec<AssetInfo>,
}

impl ReleaseInfo {
    pub fn find_asset_for_platform(&self, platform: &Platform) -> Option<&AssetInfo> {
        let plat_str = platform.as_str().to_ascii_lowercase();

        for asset in &self.assets {
            if let Some(ref p) = asset.platform {
                if p.to_ascii_lowercase() == plat_str {
                    return Some(asset);
                }
            }
        }

        for asset in &self.assets {
            if platform.matches_filename(&asset.name) {
                return Some(asset);
            }
        }

        if self.assets.len() == 1 {
            return self.assets.first();
        }

        None
    }

    pub fn find_asset_for_current_platform(&self) -> Option<&AssetInfo> {
        self.find_asset_for_platform(&Platform::current())
    }
}

fn default_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseManifest {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub releases: Vec<ReleaseInfo>,
}

impl ReleaseManifest {
    pub fn latest_for_channel(&self, channel: Channel) -> Option<&ReleaseInfo> {
        let mut matching: Vec<&ReleaseInfo> = self
            .releases
            .iter()
            .filter(|r| r.channel == channel)
            .collect();

        matching.sort_by(|a, b| {
            if is_newer(&a.version, &b.version) {
                Ordering::Less
            } else if is_newer(&b.version, &a.version) {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        });

        matching.last().copied()
    }

    pub fn find_version(&self, version: &str) -> Option<&ReleaseInfo> {
        let target = version.trim_start_matches(|c| c == 'v' || c == 'V');
        self.releases.iter().find(|r| {
            r.version.trim_start_matches(|c| c == 'v' || c == 'V') == target
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateHistoryEntry {
    pub version: String,
    #[serde(default)]
    pub installed_at: String,
    #[serde(default)]
    pub previous_version: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub prerelease: Option<String>,
}

impl ParsedVersion {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches(|c| c == 'v' || c == 'V');
        if s.is_empty() {
            return None;
        }

        let (core, prerelease) = match s.split_once('-') {
            Some((c, pre)) => (c, Some(pre.to_string())),
            None => (s, None),
        };

        let mut parts = core.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
        let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;

        Some(Self {
            major,
            minor,
            patch,
            prerelease,
        })
    }
}

impl Ord for ParsedVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.major.cmp(&other.major) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.minor.cmp(&other.minor) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.patch.cmp(&other.patch) {
            Ordering::Equal => {}
            ord => return ord,
        }

        match (&self.prerelease, &other.prerelease) {
            (None, None) => Ordering::Equal,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(pre_a), Some(pre_b)) => compare_prereleases(pre_a, pre_b),
        }
    }
}

impl PartialOrd for ParsedVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn compare_prereleases(a: &str, b: &str) -> Ordering {
    let parts_a: Vec<&str> = a.split('.').collect();
    let parts_b: Vec<&str> = b.split('.').collect();

    let len = parts_a.len().min(parts_b.len());
    for i in 0..len {
        let p_a = parts_a[i];
        let p_b = parts_b[i];

        let num_a = p_a.parse::<u64>().ok();
        let num_b = p_b.parse::<u64>().ok();

        let ord = match (num_a, num_b) {
            (Some(na), Some(nb)) => na.cmp(&nb),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => p_a.cmp(p_b),
        };

        if ord != Ordering::Equal {
            return ord;
        }
    }

    parts_a.len().cmp(&parts_b.len())
}

pub fn is_newer(current: &str, candidate: &str) -> bool {
    let cur_parsed = match ParsedVersion::parse(current) {
        Some(v) => v,
        None => return false,
    };
    let cand_parsed = match ParsedVersion::parse(candidate) {
        Some(v) => v,
        None => return false,
    };

    cand_parsed > cur_parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semver_comparisons() {
        assert!(is_newer("1.0.0", "1.0.1"));
        assert!(is_newer("1.0.0", "1.1.0"));
        assert!(is_newer("1.0.0", "2.0.0"));
        assert!(is_newer("v1.0.0", "v1.0.1"));
        assert!(!is_newer("1.0.1", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));

        assert!(is_newer("1.0.0-beta.1", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0-beta.1"));

        assert!(is_newer("1.0.0-alpha", "1.0.0-beta"));
        assert!(is_newer("1.0.0-beta.1", "1.0.0-beta.2"));
        assert!(is_newer("1.0.0-beta.2", "1.0.0-beta.10"));
        assert!(is_newer("1.0.0-beta", "1.0.0-beta.1"));
        assert!(!is_newer("1.0.0-beta.2", "1.0.0-beta.1"));

        assert!(is_newer("1.0.0-beta.1", "1.0.0-beta.2"));
        assert!(is_newer("1.0.0-beta.1", "1.0.0"));
    }

    #[test]
    fn test_channel_parsing() {
        assert_eq!("stable".parse::<Channel>().unwrap(), Channel::Stable);
        assert_eq!("BETA".parse::<Channel>().unwrap(), Channel::Beta);
        assert_eq!("nightly".parse::<Channel>().unwrap(), Channel::Nightly);
        assert!("invalid".parse::<Channel>().is_err());
    }

    #[test]
    fn test_manifest_latest_for_channel() {
        let manifest = ReleaseManifest {
            schema_version: 1,
            releases: vec![
                ReleaseInfo {
                    version: "1.0.0".into(),
                    channel: Channel::Stable,
                    release_date: "2026-01-01".into(),
                    title: "Initial".into(),
                    notes: "Release 1.0.0".into(),
                    assets: vec![],
                },
                ReleaseInfo {
                    version: "1.0.1".into(),
                    channel: Channel::Stable,
                    release_date: "2026-02-01".into(),
                    title: "Patch".into(),
                    notes: "Release 1.0.1".into(),
                    assets: vec![],
                },
                ReleaseInfo {
                    version: "1.1.0-beta.1".into(),
                    channel: Channel::Beta,
                    release_date: "2026-03-01".into(),
                    title: "Beta".into(),
                    notes: "Beta 1".into(),
                    assets: vec![],
                },
            ],
        };

        let latest_stable = manifest.latest_for_channel(Channel::Stable).unwrap();
        assert_eq!(latest_stable.version, "1.0.1");

        let latest_beta = manifest.latest_for_channel(Channel::Beta).unwrap();
        assert_eq!(latest_beta.version, "1.1.0-beta.1");

        assert!(manifest.latest_for_channel(Channel::Nightly).is_none());
    }

    #[test]
    fn test_platform_matching() {
        let asset_win = AssetInfo {
            name: "puppytunnel-windows-x86_64.exe".into(),
            download_url: "https://example.com/win.exe".into(),
            sha256: "abc".into(),
            signature: None,
            size: 1024,
            platform: Some("windows-x86_64".into()),
        };
        let asset_linux = AssetInfo {
            name: "puppytunnel-linux-x86_64".into(),
            download_url: "https://example.com/linux".into(),
            sha256: "def".into(),
            signature: None,
            size: 2048,
            platform: Some("linux-x86_64".into()),
        };

        let release = ReleaseInfo {
            version: "1.0.0".into(),
            channel: Channel::Stable,
            release_date: "".into(),
            title: "".into(),
            notes: "".into(),
            assets: vec![asset_win.clone(), asset_linux.clone()],
        };

        let found_win = release.find_asset_for_platform(&Platform::WindowsX64).unwrap();
        assert_eq!(found_win.name, "puppytunnel-windows-x86_64.exe");

        let found_linux = release.find_asset_for_platform(&Platform::LinuxX64).unwrap();
        assert_eq!(found_linux.name, "puppytunnel-linux-x86_64");
    }
}
