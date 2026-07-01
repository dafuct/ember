mod error;

pub mod auth;

pub mod gmail;

pub mod calendar;

pub mod db;

mod commands;

mod html;

pub mod scorer;

pub mod mime;

pub mod ollama;

pub mod transcript;

pub mod audio;

pub mod capture;

pub mod transcribe;

pub mod model;

pub mod decode;

pub mod syscapture;

pub mod scheduling;

pub mod people;

pub mod zoom;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = dotenvy::from_path(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env"),
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = rusqlite::Connection::open(dir.join("ember.db"))?;
            crate::db::init(&conn)?;
            if let Err(e) = commands::migrate_legacy_primary_account(&conn) {
                eprintln!("[ember] legacy account migration failed: {e}");
            }
            app.manage(std::sync::Arc::new(std::sync::Mutex::new(conn)));
            app.manage(std::sync::Arc::new(std::sync::Mutex::new(
                None::<crate::transcribe::Transcriber>,
            )) as crate::transcribe::TranscriberState);
            app.manage(std::sync::Arc::new(std::sync::Mutex::new(
                None::<crate::syscapture::SysSession>,
            )) as crate::syscapture::SysCaptureState);
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
            commands::zoom_connect,
            commands::zoom_status,
            commands::zoom_disconnect,
            commands::set_zoom_credentials,
            commands::zoom_credentials_status,
            commands::clear_zoom_credentials,
            commands::open_external,
            commands::respond_to_event,
            commands::get_meeting_note,
            commands::save_meeting_note,
            commands::delete_meeting_note,
            commands::list_meeting_notes,
            commands::summarize_meeting_note,
            commands::read_transcript_file,
            commands::transcribe_recording,
            commands::prepare_transcription,
            syscapture::start_system_capture,
            syscapture::stop_system_capture,
            commands::get_settings,
            commands::set_settings,
            commands::google_credentials_status,
            commands::set_google_credentials,
            commands::clear_google_credentials,
            commands::disconnect,
            commands::remove_account,
            commands::list_accounts,
            commands::set_active_account,
            commands::search_people,
            commands::find_meeting_times,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
