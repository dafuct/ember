use std::fs;
use std::path::Path;

fn main() {
    // 🦀 Bake the Google OAuth credentials from `src-tauri/.env` INTO the binary at build
    //    time. At runtime the app first tries a real env var (set by dotenvy in dev), then
    //    falls back to these baked values (see auth::GoogleOAuth::from_env). Without this, a
    //    packaged release copied to another Mac has no credentials — the runtime `.env` path
    //    is baked via CARGO_MANIFEST_DIR and points at THIS machine's source tree, which
    //    doesn't exist on the target machine. (Desktop "installed-app" OAuth client secrets
    //    are not treated as confidential by Google, so embedding them is acceptable — just
    //    don't publish the binary itself.)
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let env_path = Path::new(&manifest_dir).join(".env");
    // 🦀 Re-run this build script (re-baking the values) only when `.env` changes.
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
                    // 🦀 Strip optional surrounding quotes. `cargo:rustc-env=K=V` sets a
                    //    compile-time env var that `option_env!("K")` reads, embedding V.
                    let val = val.trim().trim_matches('"').trim_matches('\'');
                    println!("cargo:rustc-env={key}={val}");
                }
            }
        }
    }

    tauri_build::build()
}
