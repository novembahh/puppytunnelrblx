use crate::update::manifest::{AssetInfo, Channel, ReleaseInfo, ReleaseManifest};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

const USER_AGENT: &str = "PuppyTunnel-Updater";
const DEFAULT_TIMEOUT_SECS: u64 = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateSourceError {
    NetworkError(String),
    HttpError(u16, String),
    ParseError(String),
    InvalidSource(String),
}

impl fmt::Display for UpdateSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateSourceError::NetworkError(e) => write!(f, "Network error fetching updates: {}", e),
            UpdateSourceError::HttpError(code, e) => {
                write!(f, "HTTP error {} fetching updates: {}", code, e)
            }
            UpdateSourceError::ParseError(e) => {
                write!(f, "Failed to parse update manifest: {}", e)
            }
            UpdateSourceError::InvalidSource(e) => write!(f, "Invalid update source: {}", e),
        }
    }
}

impl std::error::Error for UpdateSourceError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UpdateSource {
    MasterApi(String),
    GitHubReleases { owner: String, repo: String },
    DirectUrl(String),
}

impl UpdateSource {
    pub fn master<S: Into<String>>(url: S) -> Self {
        UpdateSource::MasterApi(url.into())
    }

    pub fn github<S1: Into<String>, S2: Into<String>>(owner: S1, repo: S2) -> Self {
        UpdateSource::GitHubReleases {
            owner: owner.into(),
            repo: repo.into(),
        }
    }

    pub fn direct<S: Into<String>>(url: S) -> Self {
        UpdateSource::DirectUrl(url.into())
    }

    pub fn from_config(source_type: &str, url_or_repo: &str) -> Self {
        let t = source_type.trim().to_ascii_lowercase();
        let target = url_or_repo.trim();

        match t.as_str() {
            "github" => {
                if let Some((owner, repo)) = target.split_once('/') {
                    UpdateSource::GitHubReleases {
                        owner: owner.trim().to_string(),
                        repo: repo.trim().to_string(),
                    }
                } else {
                    UpdateSource::GitHubReleases {
                        owner: "PuppyTunnel".to_string(),
                        repo: if target.is_empty() {
                            "puppytunnel".to_string()
                        } else {
                            target.to_string()
                        },
                    }
                }
            }
            "direct" => UpdateSource::DirectUrl(target.to_string()),
            _ => {
                let base = if target.is_empty() {
                    "https://updates.puppytunnel.vip"
                } else {
                    target
                };
                UpdateSource::MasterApi(base.to_string())
            }
        }
    }

    pub fn build_url(&self, channel: Channel) -> String {
        match self {
            UpdateSource::MasterApi(base) => {
                let trimmed = base.trim_end_matches('/');
                format!("{}/api/updates/check?channel={}", trimmed, channel.as_str())
            }
            UpdateSource::GitHubReleases { owner, repo } => {
                format!("https://api.github.com/repos/{}/{}/releases", owner, repo)
            }
            UpdateSource::DirectUrl(url) => url.clone(),
        }
    }
}

pub trait UpdateSourceFetcher: Send + Sync {
    fn fetch_manifest(&self, channel: Channel) -> Result<ReleaseManifest, UpdateSourceError>;
}

impl UpdateSourceFetcher for UpdateSource {
    fn fetch_manifest(&self, channel: Channel) -> Result<ReleaseManifest, UpdateSourceError> {
        let url = self.build_url(channel);
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .user_agent(USER_AGENT)
            .build();

        let req = match self {
            UpdateSource::GitHubReleases { .. } => agent
                .get(&url)
                .set("Accept", "application/vnd.github.v3+json"),
            _ => agent.get(&url).set("Accept", "application/json"),
        };

        let resp = req.call().map_err(|e| match e {
            ureq::Error::Status(code, resp) => {
                let body = resp.into_string().unwrap_or_default();
                UpdateSourceError::HttpError(code, body)
            }
            ureq::Error::Transport(t) => UpdateSourceError::NetworkError(t.to_string()),
        })?;

        let body_str = resp
            .into_string()
            .map_err(|e| UpdateSourceError::NetworkError(e.to_string()))?;

        match self {
            UpdateSource::GitHubReleases { .. } => parse_github_releases(&body_str),
            UpdateSource::MasterApi(_) | UpdateSource::DirectUrl(_) => {
                parse_manifest_or_release(&body_str, channel)
            }
        }
    }
}

pub fn parse_manifest_or_release(
    json_str: &str,
    default_channel: Channel,
) -> Result<ReleaseManifest, UpdateSourceError> {
    if let Ok(manifest) = serde_json::from_str::<ReleaseManifest>(json_str) {
        return Ok(manifest);
    }

    if let Ok(mut release) = serde_json::from_str::<ReleaseInfo>(json_str) {
        release.channel = default_channel;
        return Ok(ReleaseManifest {
            schema_version: 1,
            releases: vec![release],
        });
    }

    Err(UpdateSourceError::ParseError(
        "Response is neither a valid ReleaseManifest nor ReleaseInfo".to_string(),
    ))
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

fn parse_github_releases(json_str: &str) -> Result<ReleaseManifest, UpdateSourceError> {
    let gh_releases: Vec<GitHubRelease> = serde_json::from_str(json_str)
        .map_err(|e| UpdateSourceError::ParseError(format!("Invalid GitHub releases JSON: {}", e)))?;

    let mut releases = Vec::with_capacity(gh_releases.len());

    for gh in gh_releases {
        let tag = gh.tag_name.trim();
        let version = tag.trim_start_matches(|c| c == 'v' || c == 'V').to_string();

        let channel = if tag.to_ascii_lowercase().contains("nightly") {
            Channel::Nightly
        } else if gh.prerelease || tag.contains('-') {
            Channel::Beta
        } else {
            Channel::Stable
        };

        let body = gh.body.unwrap_or_default();
        let mut assets = Vec::with_capacity(gh.assets.len());

        for a in gh.assets {
            let sha256 = extract_sha256_for_asset(&a.name, &body);
            assets.push(AssetInfo {
                name: a.name,
                download_url: a.browser_download_url,
                sha256,
                signature: None,
                size: a.size,
                platform: None,
            });
        }

        releases.push(ReleaseInfo {
            version,
            channel,
            release_date: gh.published_at.unwrap_or_default(),
            title: gh.name.unwrap_or_else(|| tag.to_string()),
            notes: body,
            assets,
        });
    }

    Ok(ReleaseManifest {
        schema_version: 1,
        releases,
    })
}

fn extract_sha256_for_asset(asset_name: &str, body: &str) -> String {
    for line in body.lines() {
        if line.contains(asset_name) {
            for word in line.split_whitespace() {
                let clean = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
                if clean.len() == 64 && clean.chars().all(|c| c.is_ascii_hexdigit()) {
                    return clean.to_ascii_lowercase();
                }
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_urls() {
        let master = UpdateSource::master("https://master.puppytunnel.com");
        assert_eq!(
            master.build_url(Channel::Stable),
            "https://master.puppytunnel.com/api/updates/check?channel=stable"
        );
        assert_eq!(
            master.build_url(Channel::Beta),
            "https://master.puppytunnel.com/api/updates/check?channel=beta"
        );

        let gh = UpdateSource::github("owner", "repo");
        assert_eq!(
            gh.build_url(Channel::Stable),
            "https://api.github.com/repos/owner/repo/releases"
        );

        let direct = UpdateSource::direct("https://mycdn.com/manifest.json");
        assert_eq!(direct.build_url(Channel::Nightly), "https://mycdn.com/manifest.json");
    }

    #[test]
    fn test_from_config() {
        let gh = UpdateSource::from_config("github", "myorg/myrepo");
        assert_eq!(gh, UpdateSource::GitHubReleases { owner: "myorg".into(), repo: "myrepo".into() });

        let direct = UpdateSource::from_config("direct", "https://example.com/manifest.json");
        assert_eq!(direct, UpdateSource::DirectUrl("https://example.com/manifest.json".into()));

        let master = UpdateSource::from_config("master", "https://relay.puppytunnel.com");
        assert_eq!(master, UpdateSource::MasterApi("https://relay.puppytunnel.com".into()));
    }

    #[test]
    fn test_parse_manifest_json() {
        let json = r#"{
            "schema_version": 1,
            "releases": [
                {
                    "version": "1.0.1",
                    "channel": "stable",
                    "title": "v1.0.1",
                    "notes": "Bug fixes",
                    "assets": [
                        {
                            "name": "puppytunnel.exe",
                            "download_url": "https://example.com/download.exe",
                            "sha256": "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
                            "size": 1024
                        }
                    ]
                }
            ]
        }"#;

        let parsed = parse_manifest_or_release(json, Channel::Stable).unwrap();
        assert_eq!(parsed.releases.len(), 1);
        assert_eq!(parsed.releases[0].version, "1.0.1");
    }

    #[test]
    fn test_parse_github_json() {
        let json = r#"[
            {
                "tag_name": "v1.0.2",
                "name": "Release 1.0.2",
                "body": "SHA256 puppytunnel-win.exe: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "prerelease": false,
                "published_at": "2026-10-17T05:00:00Z",
                "assets": [
                    {
                        "name": "puppytunnel-win.exe",
                        "browser_download_url": "https://github.com/...",
                        "size": 5000000
                    }
                ]
            }
        ]"#;

        let parsed = parse_github_releases(json).unwrap();
        assert_eq!(parsed.releases.len(), 1);
        assert_eq!(parsed.releases[0].version, "1.0.2");
        assert_eq!(parsed.releases[0].channel, Channel::Stable);
        assert_eq!(parsed.releases[0].assets.len(), 1);
        assert_eq!(
            parsed.releases[0].assets[0].sha256,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
    }
}
