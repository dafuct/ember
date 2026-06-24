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

// 🦀 `pub mod gmail;` wires in the Gmail API client as a public submodule.
//    Integration tests in `tests/gmail_test.rs` (a *separate crate*) can then
//    reach it as `ember_lib::gmail::GmailClient` — same as any external user.
pub mod gmail;

// 🦀 The read-only Google Calendar client, mirroring `gmail`. `pub` so integration
//    tests in `tests/calendar_test.rs` (a separate crate) can reach `ember_lib::calendar`.
pub mod calendar;

// 🦀 `pub mod db;` declares the local SQLite store module. Rust resolves this
//    to `src/db/mod.rs` (the `mod.rs` convention for a module that is itself a
//    directory) or `src/db.rs` (single-file form) — whichever exists.
pub mod db;

// 🦀 `mod commands;` pulls in the Tauri command handlers defined in
//    `src/commands.rs`.  It is `mod` (not `pub mod`) because external crates
//    never need to call these directly — only the `invoke_handler!` macro and
//    the JS frontend reach them through Tauri's IPC bridge.
mod commands;

// 🦀 email HTML sanitizer — strips scripts/events, optionally blocks tracking pixels
mod html;

// 🦀 `pub mod scorer;` wires in the pure smart-inbox classifier (no I/O, fully
//    unit-testable). `pub` so integration tests / future callers can reach it.
pub mod scorer;

// 🦀 Pure RFC822 message builder for outgoing mail (no I/O, fully unit-testable).
pub mod mime;

// 🦀 Local Ollama client for meeting-note summarization (M21). `pub` so the wiremock
//    integration test in tests/ollama_test.rs (a separate crate) can reach it.
pub mod ollama;

// 🦀 Pure transcript helpers (WebVTT→text, summary-input builder) — no I/O, unit-tested (M22).
pub mod transcript;

// 🦀 Local Whisper STT client (M23) — the audio twin of `ollama`. `pub` so the wiremock
//    integration test in tests/whisper_test.rs (a separate crate) can reach it.
pub mod whisper;

// 🦀 Pure audio helpers (downmix/resample/WAV) for live capture — no I/O, unit-tested (M24).
pub mod audio;

// 🦀 Live audio capture session + commands (M24). `pub` is not required (only the IPC bridge
//    calls these), but mirrors the sibling modules.
pub mod capture;

// 🦀 In-process Whisper STT engine (whisper-rs). Replaces the external whisper-server.
pub mod transcribe;

// 🦀 Whisper model management: app-data path + first-use auto-download (with progress).
pub mod model;

use tauri::Manager;

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
    // 🦀 `tauri::generate_handler![...]` is a macro that registers the listed
    //    Rust async fns as IPC handlers.  After this, the JS frontend can call
    //    `invoke("connect_gmail")` and Tauri will route it to `commands::connect_gmail`.
    tauri::Builder::default()
        // 🦀 `.plugin(...)` registers a Tauri plugin's commands + setup on the builder.
        //    `tauri_plugin_notification::init()` returns the plugin value; the JS side
        //    reaches it through `@tauri-apps/plugin-notification`.
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 🦀 The setup hook runs once at startup with the App handle. `app.path()`
            //    (Manager trait) resolves OS-standard dirs; on macOS app_data_dir is
            //    ~/Library/Application Support/<bundle-identifier>/.
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = rusqlite::Connection::open(dir.join("ember.db"))?;
            crate::db::init(&conn)?;
            // 🦀 One-time migration of a pre-multi-account "primary" token into the
            //    email-keyed scheme. Non-fatal: on failure we log and continue (the user
            //    can simply reconnect, which re-registers the account on next sign-in).
            if let Err(e) = commands::migrate_legacy_primary_account(&conn) {
                eprintln!("[ember] legacy account migration failed: {e}");
            }
            // 🦀 `app.manage(...)` stores a value in Tauri's typed state registry;
            //    commands receive it later via `tauri::State<'_, Db>`.
            app.manage(std::sync::Arc::new(std::sync::Mutex::new(conn)));
            // 🦀 Live-capture session state (M24): starts empty; start/stop_capture fill/clear it.
            app.manage(crate::capture::CaptureState::default());
            // 🦀 In-process Whisper engine: loaded lazily by prepare_transcription, then reused.
            app.manage(std::sync::Arc::new(std::sync::Mutex::new(
                None::<crate::transcribe::Transcriber>,
            )) as crate::transcribe::TranscriberState);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::connect_gmail,
            commands::get_connected_account,
            commands::fetch_inbox_preview,
            commands::sync_inbox,
            commands::sync_all_accounts,
            commands::fetch_message_body,
            commands::download_attachment,
            commands::set_message_read,
            commands::set_message_starred,
            commands::batch_modify_messages,
            commands::send_email,
            commands::get_reply_context,
            commands::search_messages,
            commands::fetch_folder,
            commands::list_labels,
            commands::create_label,
            commands::fetch_label,
            commands::get_draft,
            commands::save_draft,
            commands::send_draft,
            commands::delete_draft,
            commands::restore_message,
            commands::delete_message_forever,
            commands::snooze_message,
            commands::unsnooze_message,
            commands::wake_due_snoozes,
            commands::list_snoozed,
            commands::batch_restore_messages,
            commands::batch_delete_messages,
            commands::fetch_calendar_week,
            commands::list_calendars,
            commands::create_calendar_event,
            commands::update_calendar_event,
            commands::delete_calendar_event,
            commands::get_meeting_note,
            commands::save_meeting_note,
            commands::delete_meeting_note,
            commands::list_meeting_notes,
            commands::summarize_meeting_note,
            commands::read_transcript_file,
            commands::transcribe_recording,
            commands::prepare_transcription,
            commands::transcription_status,
            capture::list_input_devices,
            capture::start_capture,
            capture::stop_capture,
            commands::get_settings,
            commands::set_settings,
            commands::google_credentials_status,
            commands::set_google_credentials,
            commands::clear_google_credentials,
            commands::disconnect,
            commands::remove_account,
            commands::list_accounts,
            commands::set_active_account,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
