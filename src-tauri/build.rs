use std::fs;
use std::path::Path;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let env_path = Path::new(&manifest_dir).join(".env");
    println!("cargo:rerun-if-changed=.env");
    if let Ok(contents) = fs::read_to_string(&env_path) {
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim();
                if key == "EMBER_GOOGLE_CLIENT_ID" || key == "EMBER_GOOGLE_CLIENT_SECRET" {
                    let val = val.trim().trim_matches('"').trim_matches('\'');
                    println!("cargo:rustc-env={key}={val}");
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        println!("cargo:rerun-if-changed=native/syscapture.m");
        cc::Build::new()
            .file("native/syscapture.m")
            .flag("-fobjc-arc")
            .flag("-mmacosx-version-min=13.0")
            .compile("ember_syscapture");
        for fw in ["ScreenCaptureKit", "CoreMedia", "Foundation", "CoreGraphics"] {
            println!("cargo:rustc-link-lib=framework={fw}");
        }
    }

    tauri_build::build()
}
