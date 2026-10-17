pub mod installer;
pub mod manifest;
pub mod manager;
pub mod source;
pub mod verifier;

use std::path::PathBuf;
use std::sync::Arc;

pub const CURRENT_VERSION: &str = "1.0.0-beta.1";
pub use manifest::ReleaseInfo as UpdateInfo;

pub use installer::{
    apply_update, apply_update_to, download_asset, relaunch_and_terminate, verify_and_stage, DownloadProgress,
    InstallerError,
};
pub use manifest::{
    is_newer, AssetInfo, Channel, ParsedVersion, Platform, ReleaseInfo, ReleaseManifest,
    UpdateHistoryEntry,
};
pub use manager::{UpdateManager, UpdateState};
pub use source::{UpdateSource, UpdateSourceError, UpdateSourceFetcher};
pub use verifier::{
    compute_file_sha256, compute_sha256, verify_file_sha256, verify_payload_file, verify_sha256,
    verify_signature, VerifierError, EMBEDDED_RELEASE_PUBKEY,
};

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate { checked_at: String },
    Available(ReleaseInfo),
    Downloading { downloaded: u64, total: u64, speed_bps: f64, percent: f32, info: ReleaseInfo },
    ReadyToApply { info: ReleaseInfo, staged_path: PathBuf },
    Applying,
    Failed(String),
}

#[derive(Clone)]
pub struct UpdateEngine {
    pub manager: UpdateManager,
    pub last_available: Arc<std::sync::Mutex<Option<ReleaseInfo>>>,
}

impl Default for UpdateEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateEngine {
    pub fn new() -> Self {
        Self {
            manager: UpdateManager::new(),
            last_available: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn get_status(&self) -> UpdateStatus {
        match self.manager.state() {
            UpdateState::Idle => UpdateStatus::Idle,
            UpdateState::Checking => UpdateStatus::Checking,
            UpdateState::UpToDate { .. } => UpdateStatus::UpToDate {
                checked_at: chrono::Local::now().format("%H:%M:%S").to_string(),
            },
            UpdateState::Available(info) => {
                *self.last_available.lock().unwrap() = Some(info.clone());
                UpdateStatus::Available(info)
            }
            UpdateState::Downloading { downloaded_bytes, total_bytes, progress, speed_bps } => {
                let info = self.last_available.lock().unwrap().clone().unwrap_or_else(|| ReleaseInfo {
                    version: "1.0.0".into(),
                    channel: Channel::Beta,
                    release_date: "".into(),
                    title: "PuppyTunnel".into(),
                    notes: "".into(),
                    assets: vec![],
                });
                UpdateStatus::Downloading {
                    downloaded: downloaded_bytes,
                    total: total_bytes,
                    speed_bps: speed_bps as f64,
                    percent: progress * 100.0,
                    info,
                }
            }
            UpdateState::ReadyToInstall(info, path) => UpdateStatus::ReadyToApply {
                info,
                staged_path: path,
            },
            UpdateState::Installing | UpdateState::RestartRequired => UpdateStatus::Applying,
            UpdateState::Failed { error, .. } => UpdateStatus::Failed(error),
            UpdateState::Cancelled => UpdateStatus::Idle,
        }
    }

    pub fn check_for_updates(&self, channel: &str, source: &str, url: &str) {
        use std::str::FromStr;
        let ch = Channel::from_str(channel).unwrap_or(Channel::Beta);
        let src = UpdateSource::from_config(source, url);
        self.manager.check_for_updates(CURRENT_VERSION, ch, src);
    }

    pub fn start_download(&self, info: ReleaseInfo) {
        *self.last_available.lock().unwrap() = Some(info.clone());
        self.manager.start_download(info);
    }

    pub fn cancel_download(&self) {
        self.manager.cancel_download();
    }

    pub fn simulate_available_update(&self) {
        let fake = ReleaseInfo {
            version: "1.0.1".into(),
            channel: Channel::Beta,
            release_date: "2026-10-17".into(),
            title: "PuppyTunnel 1.0.1 Beta".into(),
            notes: "• Community Feed with interactive posts and likes\n• End-to-End Encrypted (E2EE) DMs and Private Groups\n• Key Backup, Export & Migration between devices\n• Instance Global Moderation & Guild Ban/Kick tools".into(),
            assets: vec![],
        };
        *self.last_available.lock().unwrap() = Some(fake.clone());
        self.manager.set_state(UpdateState::Available(fake));
    }
}
