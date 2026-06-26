// Assisted BlackHole install. BlackHole is a system audio driver that captures a meeting's
// output so Ember can transcribe it. It is a kernel-extension driver that CANNOT be installed
// silently (macOS requires the user's admin password), and Existential Audio does NOT attach a
// .pkg to their GitHub releases — the supported install path is Homebrew. So we run
// `brew install --cask blackhole-2ch` in Terminal (the user watches progress and types their
// admin password), falling back to opening the official install page when Homebrew isn't present.
use crate::error::{AppError, Result};

// 🦀 Where Homebrew's `brew` lives per Mac arch (Apple Silicon vs Intel). Tool/GUI shells don't
//    source the user's login profile, so we probe the known paths on disk instead of trusting $PATH.
const BREW_PATHS: [&str; 2] = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"];
// 🦀 Fallback when Homebrew isn't installed: the official BlackHole install instructions.
const INSTALL_PAGE: &str = "https://github.com/ExistentialAudio/BlackHole#installation";

// 🦀 The first Homebrew binary that actually exists on disk, if any. `iter().copied()` turns the
//    array's `&&str` items into `&'static str` so the return type stays clean.
fn find_brew() -> Option<&'static str> {
    BREW_PATHS
        .iter()
        .copied()
        .find(|p| std::path::Path::new(p).exists())
}

// 🦀 The AppleScript that opens Terminal and runs the cask install. Pure → unit-testable.
//    We pass the full `brew` path so it works regardless of Terminal's PATH.
fn brew_install_applescript(brew: &str) -> String {
    format!("tell application \"Terminal\"\nactivate\ndo script \"{brew} install --cask blackhole-2ch\"\nend tell")
}

/// Install BlackHole 2ch. With Homebrew present, run the cask install in Terminal so the user can
/// watch it and enter their admin password; otherwise open the official install page and report
/// that Homebrew is needed. (A kernel-extension driver can never install silently.)
pub async fn install_2ch() -> Result<()> {
    match find_brew() {
        Some(brew) => {
            // 🦀 `osascript -e <script>` runs the AppleScript; the first run may prompt macOS for
            //    permission to control Terminal (expected). `open --cask` will then ask for sudo.
            let script = brew_install_applescript(brew);
            let status = std::process::Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .status()
                .map_err(|e| {
                    AppError::Other(format!("couldn't launch Terminal for the BlackHole install: {e}"))
                })?;
            if !status.success() {
                return Err(AppError::Other(
                    "couldn't start the BlackHole install in Terminal".into(),
                ));
            }
            Ok(())
        }
        None => {
            // 🦀 No Homebrew — hand the install page to the default browser, then surface a clear
            //    message (returned as an error so the UI shows it).
            open::that(INSTALL_PAGE).map_err(|e| {
                AppError::Other(format!("couldn't open the BlackHole install page: {e}"))
            })?;
            Err(AppError::Other(
                "Homebrew isn't installed, so I opened the BlackHole install page instead. \
                 Install Homebrew (brew.sh) and try again, or follow the manual download."
                    .into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applescript_runs_the_cask_install_with_the_brew_path() {
        let s = brew_install_applescript("/opt/homebrew/bin/brew");
        assert!(
            s.contains("/opt/homebrew/bin/brew install --cask blackhole-2ch"),
            "got: {s}"
        );
        assert!(s.contains("tell application \"Terminal\""));
        assert!(s.contains("do script"));
    }
}
