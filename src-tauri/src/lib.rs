// 🦀 `mod error;` declares a submodule.  Rust will look for either
//    `src/error.rs` or `src/error/mod.rs` and compile it as part of this crate.
//    Items inside are accessed as `error::AppError`, or you can `use` them.
mod error;

// 🦀 `pub mod auth;` declares the `auth` submodule and makes it public so that
//    Tauri commands (and future crate consumers) can reference `auth::tokens::…`
//    directly.  Rust will look for `src/auth/mod.rs` and compile it as the module
//    root, which in turn declares `pub mod tokens;` — wiring up the full path
//    `ember_lib::auth::tokens::StoredToken`.
pub mod auth;

// 🦀 `#[cfg_attr(mobile, tauri::mobile_entry_point)]` is a *conditional
//    attribute*.  `cfg_attr` applies the inner attribute (`tauri::mobile_entry_point`)
//    only when the `mobile` cfg flag is set (i.e. compiling for iOS/Android).
//    On desktop the attribute is a no-op, so this single function works on
//    both platforms without duplicating code.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
// 🦀 `pub fn run()` declares a *public function* named `run`.  `pub` makes it
//    visible outside this module — specifically, `main.rs` calls it via
//    `ember_lib::run()`.  Without `pub` it would be private to `lib.rs`.
pub fn run() {
    // Load src-tauri/.env in dev so Google client id/secret are available.
    // 🦀 `let _ = ...` intentionally discards the `Result` returned by
    //    `from_path`.  The leading underscore tells the compiler "I know I'm
    //    ignoring this value" — suppressing the unused-result warning.
    //    In release builds there is no .env file and that is fine.
    let _ = dotenvy::from_path(
        // 🦀 `env!("CARGO_MANIFEST_DIR")` is a compile-time macro that expands
        //    to the absolute path of the directory containing Cargo.toml.
        //    Using it here means the path is baked in at compile time and never
        //    relies on the working directory at runtime.
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env"),
    );

    // 🦀 `tauri::Builder::default()` starts a *method-chaining builder*.
    //    Each `.method()` call configures one aspect of the app and returns
    //    `Self` so the next call can be chained.  The chain ends with `.run()`
    //    which consumes the builder and starts the event loop.
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
