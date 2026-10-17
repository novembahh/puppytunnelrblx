use crate::config::get_script_dir;
use crate::identity::unix_now;
use crate::update::installer::{
    apply_update_to, download_asset, relaunch_and_terminate, verify_and_stage, DownloadProgress,
    InstallerError,
};
use crate::update::manifest::{is_newer, Channel, ReleaseInfo};
use crate::update::source::{UpdateSource, UpdateSourceFetcher};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

pub type UpdateEngine = UpdateManager;

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate {
        checked_at: String,
    },
    Available(ReleaseInfo),
    Downloading {
        downloaded: u64,
        total: u64,
        speed_bps: u64,
        percent: f32,
        info: ReleaseInfo,
    },
    ReadyToApply {
        info: ReleaseInfo,
        staged_path: PathBuf,
    },
    Applying,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate {
        checked_at: u64,
    },
    Available(ReleaseInfo),
    Downloading {
        downloaded_bytes: u64,
        total_bytes: u64,
        progress: f32,
        speed_bps: u64,
    },
    ReadyToInstall(ReleaseInfo, PathBuf),
    Installing,
    RestartRequired,
    Failed {
        error: String,
        can_retry: bool,
    },
    Cancelled,
}

impl UpdateState {
    pub fn is_idle(&self) -> bool {
        matches!(self, UpdateState::Idle)
    }

    pub fn is_checking(&self) -> bool {
        matches!(self, UpdateState::Checking)
    }

    pub fn is_downloading(&self) -> bool {
        matches!(self, UpdateState::Downloading { .. })
    }

    pub fn is_ready_to_install(&self) -> bool {
        matches!(self, UpdateState::ReadyToInstall(..))
    }

    pub fn status_text(&self) -> String {
        match self {
            UpdateState::Idle => "Idle".to_string(),
            UpdateState::Checking => "Checking for updates...".to_string(),
            UpdateState::UpToDate { .. } => "Up to date".to_string(),
            UpdateState::Available(rel) => format!("Update available: v{}", rel.version),
            UpdateState::Downloading {
                progress,
                speed_bps,
                ..
            } => {
                let mbps = *speed_bps as f64 / 1_000_000.0;
                format!("Downloading: {:.1}% ({:.2} MB/s)", progress * 100.0, mbps)
            }
            UpdateState::ReadyToInstall(rel, _) => {
                format!("Ready to install v{}", rel.version)
            }
            UpdateState::Installing => "Installing update...".to_string(),
            UpdateState::RestartRequired => "Restart required to complete update".to_string(),
            UpdateState::Failed { error, .. } => format!("Update failed: {}", error),
            UpdateState::Cancelled => "Update download cancelled".to_string(),
        }
    }
}

impl fmt::Display for UpdateState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.status_text())
    }
}

pub trait IntoUpdateCheck {
    fn resolve(self) -> (String, Channel, UpdateSource);
}

impl<'a, 'b, 'c> IntoUpdateCheck for (&'a str, &'b str, &'c str) {
    fn resolve(self) -> (String, Channel, UpdateSource) {
        let channel = self.0.parse::<Channel>().unwrap_or(Channel::Stable);
        let source = UpdateSource::from_config(self.1, self.2);
        (crate::update::CURRENT_VERSION.to_string(), channel, source)
    }
}

impl<'a, 'b, 'c> IntoUpdateCheck for (&'a String, &'b String, &'c String) {
    fn resolve(self) -> (String, Channel, UpdateSource) {
        let channel = self.0.parse::<Channel>().unwrap_or(Channel::Stable);
        let source = UpdateSource::from_config(self.1, self.2);
        (crate::update::CURRENT_VERSION.to_string(), channel, source)
    }
}

impl<'a> IntoUpdateCheck for (&'a str, Channel, UpdateSource) {
    fn resolve(self) -> (String, Channel, UpdateSource) {
        (self.0.to_string(), self.1, self.2)
    }
}

#[derive(Clone)]
pub struct UpdateManager {
    state: Arc<Mutex<UpdateState>>,
    cancel_flag: Arc<AtomicBool>,
    custom_pubkey: Arc<Mutex<Option<String>>>,
    active_release: Arc<Mutex<Option<ReleaseInfo>>>,
}

impl Default for UpdateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(UpdateState::Idle)),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            custom_pubkey: Arc::new(Mutex::new(None)),
            active_release: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_custom_pubkey(self, pubkey: Option<String>) -> Self {
        *self.custom_pubkey.lock().unwrap() = pubkey;
        self
    }

    pub fn set_custom_pubkey(&self, pubkey: Option<String>) {
        *self.custom_pubkey.lock().unwrap() = pubkey;
    }

    pub fn custom_pubkey(&self) -> Option<String> {
        self.custom_pubkey.lock().unwrap().clone()
    }

    pub fn state(&self) -> UpdateState {
        self.state.lock().unwrap().clone()
    }

    pub fn set_state(&self, new_state: UpdateState) {
        *self.state.lock().unwrap() = new_state;
    }

    pub fn get_status(&self) -> UpdateStatus {
        let st = self.state();
        let active = self.active_release.lock().unwrap().clone();
        match st {
            UpdateState::Idle => UpdateStatus::Idle,
            UpdateState::Checking => UpdateStatus::Checking,
            UpdateState::UpToDate { checked_at } => UpdateStatus::UpToDate {
                checked_at: if checked_at == 0 {
                    "just now".to_string()
                } else {
                    chrono::DateTime::from_timestamp(checked_at as i64, 0)
                        .map(|dt| dt.format("%H:%M:%S").to_string())
                        .unwrap_or_else(|| "recently".to_string())
                },
            },
            UpdateState::Available(rel) => UpdateStatus::Available(rel),
            UpdateState::Downloading {
                downloaded_bytes,
                total_bytes,
                progress,
                speed_bps,
            } => {
                let info = active.unwrap_or_else(|| ReleaseInfo {
                    version: String::new(),
                    channel: Channel::Stable,
                    release_date: String::new(),
                    title: String::new(),
                    notes: String::new(),
                    assets: vec![],
                });
                UpdateStatus::Downloading {
                    downloaded: downloaded_bytes,
                    total: total_bytes,
                    speed_bps,
                    percent: progress * 100.0,
                    info,
                }
            }
            UpdateState::ReadyToInstall(rel, path) => UpdateStatus::ReadyToApply {
                info: rel,
                staged_path: path,
            },
            UpdateState::Installing | UpdateState::RestartRequired => UpdateStatus::Applying,
            UpdateState::Failed { error, .. } => UpdateStatus::Failed(error),
            UpdateState::Cancelled => UpdateStatus::Idle,
        }
    }

    pub fn check_for_updates<A, B, C>(&self, a: A, b: B, c: C)
    where
        (A, B, C): IntoUpdateCheck,
    {
        let (current_version, channel, source) = (a, b, c).resolve();
        self.check_for_updates_internal(&current_version, channel, source);
    }

    pub fn check_for_updates_internal(
        &self,
        current_version: &str,
        channel: Channel,
        source: UpdateSource,
    ) {
        let current_ver = current_version.to_string();
        let state_clone = Arc::clone(&self.state);
        let active_rel_clone = Arc::clone(&self.active_release);

        *state_clone.lock().unwrap() = UpdateState::Checking;

        thread::spawn(move || {
            match source.fetch_manifest(channel) {
                Ok(manifest) => {
                    let latest_opt = manifest.latest_for_channel(channel);
                    match latest_opt {
                        Some(candidate) if is_newer(&current_ver, &candidate.version) => {
                            *active_rel_clone.lock().unwrap() = Some(candidate.clone());
                            *state_clone.lock().unwrap() =
                                UpdateState::Available(candidate.clone());
                        }
                        _ => {
                            *state_clone.lock().unwrap() = UpdateState::UpToDate {
                                checked_at: unix_now(),
                            };
                        }
                    }
                }
                Err(e) => {
                    *state_clone.lock().unwrap() = UpdateState::Failed {
                        error: e.to_string(),
                        can_retry: true,
                    };
                }
            }
        });
    }

    pub fn start_download(&self, release: ReleaseInfo) {
        let asset = match release.find_asset_for_current_platform() {
            Some(a) => a.clone(),
            None => {
                *self.state.lock().unwrap() = UpdateState::Failed {
                    error: format!(
                        "No compatible binary found for current platform in release v{}",
                        release.version
                    ),
                    can_retry: false,
                };
                return;
            }
        };

        *self.active_release.lock().unwrap() = Some(release.clone());
        self.cancel_flag.store(false, Ordering::SeqCst);
        let cancel_flag = Arc::clone(&self.cancel_flag);
        let state_clone = Arc::clone(&self.state);
        let custom_pubkey = self.custom_pubkey();
        let download_dir = get_script_dir();

        *state_clone.lock().unwrap() = UpdateState::Downloading {
            downloaded_bytes: 0,
            total_bytes: asset.size,
            progress: 0.0,
            speed_bps: 0,
        };

        thread::spawn(move || {
            let state_for_progress = Arc::clone(&state_clone);
            let download_result = download_asset(
                &asset,
                &download_dir,
                cancel_flag,
                move |p: DownloadProgress| {
                    if let Ok(mut st) = state_for_progress.lock() {
                        *st = UpdateState::Downloading {
                            downloaded_bytes: p.downloaded_bytes,
                            total_bytes: p.total_bytes,
                            progress: p.progress,
                            speed_bps: p.speed_bps,
                        };
                    }
                },
            );

            match download_result {
                Ok(temp_path) => {
                    match verify_and_stage(&temp_path, &asset, custom_pubkey.as_deref()) {
                        Ok(()) => {
                            *state_clone.lock().unwrap() =
                                UpdateState::ReadyToInstall(release, temp_path);
                        }
                        Err(e) => {
                            *state_clone.lock().unwrap() = UpdateState::Failed {
                                error: format!("Integrity verification failed: {}", e),
                                can_retry: true,
                            };
                        }
                    }
                }
                Err(InstallerError::Cancelled) => {
                    *state_clone.lock().unwrap() = UpdateState::Cancelled;
                }
                Err(e) => {
                    *state_clone.lock().unwrap() = UpdateState::Failed {
                        error: e.to_string(),
                        can_retry: true,
                    };
                }
            }
        });
    }

    pub fn cancel_download(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
        let mut st = self.state.lock().unwrap();
        if st.is_downloading() {
            *st = UpdateState::Cancelled;
        }
    }

    pub fn install_and_restart(
        &self,
        staged_path: PathBuf,
        target_exe_override: Option<PathBuf>,
        relaunch_args: Vec<String>,
    ) {
        *self.state.lock().unwrap() = UpdateState::Installing;
        let state_clone = Arc::clone(&self.state);

        thread::spawn(move || {
            let target_exe = match target_exe_override {
                Some(p) => p,
                None => match std::env::current_exe() {
                    Ok(p) => p,
                    Err(e) => {
                        *state_clone.lock().unwrap() = UpdateState::Failed {
                            error: format!("Cannot determine current executable path: {}", e),
                            can_retry: false,
                        };
                        return;
                    }
                },
            };

            match apply_update_to(&staged_path, &target_exe) {
                Ok(()) => {
                    *state_clone.lock().unwrap() = UpdateState::RestartRequired;
                    if let Err(e) = relaunch_and_terminate(&target_exe, &relaunch_args) {
                        *state_clone.lock().unwrap() = UpdateState::Failed {
                            error: format!("Failed to relaunch updated executable: {}", e),
                            can_retry: false,
                        };
                    }
                }
                Err(e) => {
                    *state_clone.lock().unwrap() = UpdateState::Failed {
                        error: format!("Failed to apply update: {}", e),
                        can_retry: false,
                    };
                }
            }
        });
    }

    pub fn apply_update_and_restart(&self, staged_path: &Path) {
        let _ = crate::update::installer::apply_update(staged_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manager_state_transitions() {
        let manager = UpdateManager::new();
        assert_eq!(manager.state(), UpdateState::Idle);

        manager.set_state(UpdateState::UpToDate { checked_at: 12345 });
        assert_eq!(manager.state(), UpdateState::UpToDate { checked_at: 12345 });

        manager.set_state(UpdateState::Downloading {
            downloaded_bytes: 50,
            total_bytes: 100,
            progress: 0.5,
            speed_bps: 1024,
        });
        assert!(manager.state().is_downloading());

        manager.cancel_download();
        assert_eq!(manager.state(), UpdateState::Cancelled);
    }

    #[test]
    fn test_manager_get_status_mapping() {
        let manager = UpdateManager::new();
        assert_eq!(manager.get_status(), UpdateStatus::Idle);

        manager.set_state(UpdateState::Checking);
        assert_eq!(manager.get_status(), UpdateStatus::Checking);

        manager.set_state(UpdateState::UpToDate { checked_at: 0 });
        assert_eq!(
            manager.get_status(),
            UpdateStatus::UpToDate {
                checked_at: "just now".into()
            }
        );

        let rel = ReleaseInfo {
            version: "1.0.1".into(),
            channel: Channel::Stable,
            release_date: "".into(),
            title: "Puppy 1.0.1".into(),
            notes: "Notes".into(),
            assets: vec![],
        };
        manager.set_state(UpdateState::Available(rel.clone()));
        assert_eq!(manager.get_status(), UpdateStatus::Available(rel.clone()));

        manager.set_state(UpdateState::Downloading {
            downloaded_bytes: 10,
            total_bytes: 100,
            progress: 0.1,
            speed_bps: 500,
        });
        match manager.get_status() {
            UpdateStatus::Downloading { percent, downloaded, total, speed_bps, .. } => {
                assert_eq!(downloaded, 10);
                assert_eq!(total, 100);
                assert_eq!(speed_bps, 500);
                assert!((percent - 10.0).abs() < 0.01);
            }
            other => panic!("Expected Downloading, got {:?}", other),
        }

        manager.set_state(UpdateState::ReadyToInstall(rel.clone(), PathBuf::from("staged.exe")));
        assert_eq!(
            manager.get_status(),
            UpdateStatus::ReadyToApply {
                info: rel,
                staged_path: PathBuf::from("staged.exe")
            }
        );

        manager.set_state(UpdateState::Installing);
        assert_eq!(manager.get_status(), UpdateStatus::Applying);

        manager.set_state(UpdateState::RestartRequired);
        assert_eq!(manager.get_status(), UpdateStatus::Applying);

        manager.set_state(UpdateState::Failed {
            error: "checksum error".into(),
            can_retry: true,
        });
        assert_eq!(
            manager.get_status(),
            UpdateStatus::Failed("checksum error".into())
        );
    }

    #[test]
    fn test_into_update_check() {
        let (ver, ch, src) = ("1.0.0", Channel::Beta, UpdateSource::master("https://pt.local")).resolve();
        assert_eq!(ver, "1.0.0");
        assert_eq!(ch, Channel::Beta);
        assert_eq!(src, UpdateSource::MasterApi("https://pt.local".into()));

        let (_, ch2, src2) = ("nightly", "github", "org/repo").resolve();
        assert_eq!(ch2, Channel::Nightly);
        assert_eq!(src2, UpdateSource::GitHubReleases { owner: "org".into(), repo: "repo".into() });
    }
}
