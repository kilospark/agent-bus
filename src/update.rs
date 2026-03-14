use anyhow::{Context, Result, anyhow, bail};
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

const API_BASE: &str = "https://agentbus.site";
const DOWNLOAD_BASE: &str = "https://agentbus.site/download";
const UPDATE_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60; // 24 hours

fn platform_arch() -> Result<(&'static str, &'static str)> {
    let platform = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        bail!("Unsupported OS for auto-update");
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else {
        bail!("Unsupported architecture for auto-update");
    };
    Ok((platform, arch))
}

fn data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".agentbus")
}

fn last_check_file() -> PathBuf {
    data_dir().join("last-update-check")
}

pub fn should_check_for_update() -> bool {
    let path = last_check_file();
    match std::fs::metadata(&path) {
        Ok(meta) => {
            if let Ok(modified) = meta.modified() {
                modified.elapsed().map_or(true, |age| age.as_secs() >= UPDATE_CHECK_INTERVAL_SECS)
            } else {
                true
            }
        }
        Err(_) => true,
    }
}

fn touch_last_check() {
    let path = last_check_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, "");
}

/// Check GitHub for the latest release. Returns Some(version) if newer.
pub fn check_for_update() -> Result<Option<String>> {
    touch_last_check();
    let current = env!("CARGO_PKG_VERSION");

    let url = format!("{API_BASE}/v1/version?current={current}");
    let body = ureq::get(&url)
        .timeout(Duration::from_secs(5))
        .call()
        .context("Failed to check for updates")?
        .into_string()
        .context("Failed to read response")?;

    let info: serde_json::Value = serde_json::from_str(&body)
        .context("Failed to parse version response")?;

    let is_latest = info
        .get("current_is_latest")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if is_latest {
        return Ok(None);
    }

    let latest = info
        .get("latest")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if latest.is_empty() || latest == current {
        return Ok(None);
    }

    // Compare semver: only update if latest is strictly newer.
    // Strip prerelease/build suffixes (e.g. "10-dev" → "10") before parsing.
    let parse = |s: &str| -> (u32, u32, u32) {
        let mut parts = s.splitn(3, '.');
        let num = |p: Option<&str>| -> u32 {
            p.unwrap_or("0")
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0)
        };
        (num(parts.next()), num(parts.next()), num(parts.next()))
    };
    let has_prerelease = |s: &str| s.contains('-');
    let latest_v = parse(&latest);
    let current_v = parse(current);
    if latest_v > current_v {
        Ok(Some(latest))
    } else if latest_v == current_v && has_prerelease(current) && !has_prerelease(&latest) {
        // Prerelease (e.g. 0.4.0-dev) is older than the matching release (0.4.0)
        Ok(Some(latest))
    } else {
        Ok(None)
    }
}

/// Download and install the specified version.
pub fn self_update(version: &str) -> Result<()> {
    let lock_path = last_check_file().with_extension("lock");

    // Break stale locks (>5 min)
    if let Ok(meta) = std::fs::metadata(&lock_path) {
        if let Ok(modified) = meta.modified() {
            if modified.elapsed().map_or(false, |age| age.as_secs() >= 300) {
                let _ = std::fs::remove_file(&lock_path);
            }
        }
    }

    // Atomic lock
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .and_then(|mut f| {
            use std::io::Write;
            write!(f, "{}", std::process::id())
        })
        .map_err(|_| anyhow!("Another update is in progress"))?;

    let result = do_self_update(version);

    let _ = std::fs::remove_file(&lock_path);
    result
}

fn do_self_update(version: &str) -> Result<()> {
    let (platform, arch) = platform_arch()?;
    let asset = format!("agentbus-{platform}-{arch}");
    let url = format!("{DOWNLOAD_BASE}/v{version}/{asset}.tar.gz");

    let resp = ureq::get(&url)
        .timeout(Duration::from_secs(120))
        .call()
        .map_err(|e| anyhow!("Download failed: {e}"))?;

    let mut bytes = Vec::new();
    resp.into_reader()
        .read_to_end(&mut bytes)
        .context("Failed to read download")?;

    // Extract to unique temp dir
    let unique = format!(
        "agentbus-update-{version}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let tmp_dir = std::env::temp_dir().join(&unique);
    std::fs::create_dir_all(&tmp_dir)?;

    let gz = flate2::read::GzDecoder::new(&bytes[..]);
    let mut archive = tar::Archive::new(gz);
    archive.unpack(&tmp_dir)?;

    let extracted = tmp_dir.join(&asset);
    if !extracted.exists() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        bail!("Expected binary not found in archive: {asset}");
    }

    let current_exe = std::env::current_exe()
        .context("Cannot determine current executable path")?;

    // Stage then atomic rename
    let staged = current_exe.with_extension(format!("new-{}", std::process::id()));
    std::fs::copy(&extracted, &staged)
        .context("Failed to copy new binary to install directory")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
    }

    std::fs::rename(&staged, &current_exe)
        .context("Failed to replace binary (permission denied?)")?;

    let _ = std::fs::remove_dir_all(&tmp_dir);
    Ok(())
}
