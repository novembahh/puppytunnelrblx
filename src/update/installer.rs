use crate::update::manifest::AssetInfo;
use crate::update::verifier::{verify_payload_file, VerifierError};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub progress: f32,
    pub speed_bps: u64,
}

#[derive(Debug)]
pub enum InstallerError {
    IoError(io::Error),
    NetworkError(String),
    VerificationFailed(VerifierError),
    Cancelled,
    ProcessError(String),
}

impl fmt::Display for InstallerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallerError::IoError(e) => write!(f, "Installer I/O error: {}", e),
            InstallerError::NetworkError(e) => write!(f, "Download network error: {}", e),
            InstallerError::VerificationFailed(e) => write!(f, "Verification failed: {}", e),
            InstallerError::Cancelled => write!(f, "Update download was cancelled"),
            InstallerError::ProcessError(e) => write!(f, "Process management error: {}", e),
        }
    }
}

impl std::error::Error for InstallerError {}

impl From<io::Error> for InstallerError {
    fn from(err: io::Error) -> Self {
        InstallerError::IoError(err)
    }
}

impl From<VerifierError> for InstallerError {
    fn from(err: VerifierError) -> Self {
        InstallerError::VerificationFailed(err)
    }
}



pub fn download_asset<F>(
    asset: &AssetInfo,
    download_dir: &Path,
    cancel_flag: Arc<AtomicBool>,
    on_progress: F,
) -> Result<PathBuf, InstallerError>
where
    F: Fn(DownloadProgress) + Send + Sync + 'static,
{
    fs::create_dir_all(download_dir)?;
    let temp_name = format!("puppytunnel_update_{}.tmp", uuid::Uuid::new_v4());
    let temp_path = download_dir.join(temp_name);

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .user_agent("PuppyTunnel-Updater")
        .build();

    let resp = agent
        .get(&asset.download_url)
        .call()
        .map_err(|e| InstallerError::NetworkError(e.to_string()))?;

    let total_bytes = resp
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(asset.size);

    let mut reader = resp.into_reader();
    let mut file = File::create(&temp_path)?;

    let mut buffer = [0u8; 16384];
    let mut downloaded_bytes: u64 = 0;
    let start_time = Instant::now();
    let mut last_progress_report = Instant::now();
    let mut bytes_since_last_report: u64 = 0;
    let mut current_speed: u64 = 0;

    loop {
        if cancel_flag.load(Ordering::SeqCst) {
            drop(file);
            let _ = fs::remove_file(&temp_path);
            return Err(InstallerError::Cancelled);
        }

        let bytes_read = reader
            .read(&mut buffer)
            .map_err(|e| InstallerError::NetworkError(e.to_string()))?;

        if bytes_read == 0 {
            break;
        }

        file.write_all(&buffer[..bytes_read])?;
        downloaded_bytes += bytes_read as u64;
        bytes_since_last_report += bytes_read as u64;

        let elapsed = last_progress_report.elapsed();
        if elapsed >= Duration::from_millis(200) {
            let secs = elapsed.as_secs_f64();
            if secs > 0.0 {
                current_speed = (bytes_since_last_report as f64 / secs) as u64;
            }
            bytes_since_last_report = 0;
            last_progress_report = Instant::now();

            let progress = if total_bytes > 0 {
                (downloaded_bytes as f32 / total_bytes as f32).min(1.0)
            } else {
                0.0
            };

            on_progress(DownloadProgress {
                downloaded_bytes,
                total_bytes,
                progress,
                speed_bps: current_speed,
            });
        }
    }

    file.flush()?;
    drop(file);


    let total_elapsed = start_time.elapsed().as_secs_f64();
    let avg_speed = if total_elapsed > 0.0 {
        (downloaded_bytes as f64 / total_elapsed) as u64
    } else {
        0
    };

    on_progress(DownloadProgress {
        downloaded_bytes,
        total_bytes: downloaded_bytes,
        progress: 1.0,
        speed_bps: avg_speed,
    });

    Ok(temp_path)
}



pub fn verify_and_stage(
    temp_file: &Path,
    asset: &AssetInfo,
    custom_pubkey: Option<&str>,
) -> Result<(), InstallerError> {
    if let Err(e) = verify_payload_file(
        temp_file,
        &asset.sha256,
        asset.signature.as_deref(),
        custom_pubkey,
    ) {
        let _ = fs::remove_file(temp_file);
        return Err(InstallerError::VerificationFailed(e));
    }
    Ok(())
}

pub fn apply_update(staged_path: &Path) -> Result<(), InstallerError> {
    let current = std::env::current_exe()
        .map_err(|e| InstallerError::ProcessError(format!("Cannot find current exe: {}", e)))?;
    apply_update_to(staged_path, &current)
}

pub fn apply_update_to(staged_path: &Path, target_exe: &Path) -> Result<(), InstallerError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(staged_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(staged_path, perms)?;
    }

    #[cfg(windows)]
    {
        let old_exe = target_exe.with_extension("exe.old");
        let _ = fs::remove_file(&old_exe);

        if target_exe.exists() {
            if let Err(rename_err) = fs::rename(target_exe, &old_exe) {
                return spawn_windows_file_swap(staged_path, target_exe, &old_exe)
                    .map_err(|e| InstallerError::ProcessError(format!("Windows swap failed: {} (orig: {})", e, rename_err)));
            }
        }

        fs::rename(staged_path, target_exe)?;
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        fs::rename(staged_path, target_exe)?;
        Ok(())
    }
}

#[cfg(windows)]
fn spawn_windows_file_swap(
    staged: &Path,
    target: &Path,
    old: &Path,
) -> Result<(), io::Error> {
    let pid = std::process::id();
    let script_dir = target.parent().unwrap_or_else(|| Path::new("."));
    let bat_path = script_dir.join("puppytunnel_updater.bat");

    let script = format!(
        "@echo off\r\n\
        :wait_pid\r\n\
        tasklist /fi \"PID eq {pid}\" 2>nul | find \"{pid}\" >nul\r\n\
        if not errorlevel 1 (\r\n\
            timeout /t 1 /nobreak >nul\r\n\
            goto wait_pid\r\n\
        )\r\n\
        del \"{old}\" 2>nul\r\n\
        move /y \"{target}\" \"{old}\" 2>nul\r\n\
        move /y \"{staged}\" \"{target}\" >nul\r\n\
        del \"%~f0\" 2>nul\r\n",
        pid = pid,
        old = old.display(),
        target = target.display(),
        staged = staged.display()
    );

    fs::write(&bat_path, script)?;

    Command::new("cmd.exe")
        .args(["/C", "start", "/b", "", &bat_path.to_string_lossy()])
        .spawn()?;

    Ok(())
}

pub fn relaunch_and_terminate(target_exe: &Path, args: &[String]) -> Result<(), InstallerError> {
    let mut cmd = Command::new(target_exe);
    cmd.args(args);

    cmd.spawn()
        .map_err(|e| InstallerError::ProcessError(format!("Failed to spawn updated process: {}", e)))?;

    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_apply_update_mock() {
        let temp_dir = std::env::temp_dir().join(format!("pt_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        let target_exe = temp_dir.join("mock_app.exe");
        let staged_file = temp_dir.join("mock_app_new.tmp");

        {
            let mut f = File::create(&target_exe).unwrap();
            f.write_all(b"version 1.0").unwrap();
        }

        {
            let mut f = File::create(&staged_file).unwrap();
            f.write_all(b"version 1.1").unwrap();
        }

        assert!(apply_update_to(&staged_file, &target_exe).is_ok());
        assert!(target_exe.exists());

        let content = fs::read_to_string(&target_exe).unwrap();
        assert_eq!(content, "version 1.1");

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
