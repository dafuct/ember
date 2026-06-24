// Assisted BlackHole install. BlackHole is a system audio driver that captures a meeting's
// output so Ember can transcribe it — but a driver can't install silently (macOS requires the
// user's admin password). We make it one-click: fetch the official signed 2ch installer (.pkg)
// from Existential Audio's GitHub releases on demand and hand it to macOS's Installer. We do NOT
// bundle/redistribute BlackHole (it's GPL-3.0); we fetch the upstream installer, mirroring the
// Whisper-model download pattern in `model.rs`.
use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, Result};

// 🦀 GitHub's "latest release" API for BlackHole — returns JSON listing the release's assets.
const RELEASES_API: &str =
    "https://api.github.com/repos/ExistentialAudio/BlackHole/releases/latest";
// 🦀 GitHub rejects API requests that don't send a User-Agent header.
const USER_AGENT: &str = "ember-mail-client";

#[derive(Deserialize)]
struct GhRelease {
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

/// Pick the BlackHole **2ch** installer (.pkg) from a release's assets. Pure → unit-testable.
/// Matches case-insensitively on "2ch" + ".pkg" so it survives upstream naming/version changes.
fn pick_2ch_pkg(assets: &[GhAsset]) -> Option<&GhAsset> {
    assets.iter().find(|a| {
        let n = a.name.to_lowercase();
        n.contains("2ch") && n.ends_with(".pkg")
    })
}

/// Fetch the latest BlackHole 2ch installer and open it in macOS Installer.
/// The user still authenticates (a system driver can't install silently). Returns once the
/// installer is handed off; any failure leaves the manual download link as the fallback.
pub async fn install_2ch() -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(AppError::from)?;

    // 1. Resolve the installer's download URL from the latest release.
    let resp = client
        .get(RELEASES_API)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| AppError::Other(format!("couldn't reach GitHub for the BlackHole installer: {e}")))?;
    let resp = resp
        .error_for_status()
        .map_err(|e| AppError::Other(format!("GitHub returned an error fetching the installer: {e}")))?;
    let release: GhRelease = resp
        .json()
        .await
        .map_err(|e| AppError::Other(format!("couldn't read the BlackHole release info: {e}")))?;
    let asset = pick_2ch_pkg(&release.assets).ok_or_else(|| {
        AppError::Other("no BlackHole 2ch installer found in the latest release".into())
    })?;

    // 2. Download the .pkg to a temp file (small — a few MB — so a single read is fine).
    let resp = client
        .get(&asset.browser_download_url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| AppError::Other(format!("downloading the BlackHole installer failed: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::Other(format!("downloading the BlackHole installer failed: {e}")))?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::Other(format!("downloading the BlackHole installer failed: {e}")))?;
    let pkg_path = std::env::temp_dir().join(&asset.name);
    std::fs::write(&pkg_path, &bytes)
        .map_err(|e| AppError::Other(format!("couldn't save the BlackHole installer: {e}")))?;

    // 3. Hand it to macOS Installer (GUI; it prompts for the admin password). `open` returns as
    //    soon as Installer launches — we don't wait for the install to finish.
    let status = std::process::Command::new("open")
        .arg(&pkg_path)
        .status()
        .map_err(|e| AppError::Other(format!("couldn't launch the BlackHole installer: {e}")))?;
    if !status.success() {
        return Err(AppError::Other("the BlackHole installer could not be opened".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> GhAsset {
        GhAsset { name: name.into(), browser_download_url: format!("https://x/{name}") }
    }

    #[test]
    fn picks_the_2ch_pkg_case_insensitively() {
        let assets = vec![
            asset("BlackHole.16ch.v0.6.0.pkg"),
            asset("BlackHole.2ch.v0.6.0.pkg"),
            asset("SomeReadme.txt"),
        ];
        assert_eq!(pick_2ch_pkg(&assets).unwrap().name, "BlackHole.2ch.v0.6.0.pkg");
    }

    #[test]
    fn returns_none_when_no_2ch_pkg_present() {
        let assets = vec![asset("BlackHole.16ch.pkg"), asset("notes.txt")];
        assert!(pick_2ch_pkg(&assets).is_none());
    }
}
