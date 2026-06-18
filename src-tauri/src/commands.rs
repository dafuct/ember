// 🦀 `#[tauri::command]` is a procedural attribute macro that wraps this async
//    fn into a Tauri IPC handler.  It generates the glue code that lets the
//    JavaScript frontend call this function by name via `invoke("connect_gmail")`.
//    Without it, the function is just a plain Rust fn — Tauri never sees it.

use crate::auth::tokens::load_token;
use crate::auth::{ensure_access_token, GoogleOAuth, PRIMARY_ACCOUNT};
use crate::error::Result;
use crate::gmail::types::MessagePreview;
use crate::gmail::GmailClient;

/// Run the interactive Google sign-in. Returns the connected email address.
// 🦀 These commands are `async` because they perform I/O (network calls, file
//    reads).  Tauri's async runtime awaits them on a thread-pool, so the GUI
//    thread is never blocked.  The JS `invoke()` call returns a Promise, which
//    resolves when the Rust future completes.
#[tauri::command]
pub async fn connect_gmail() -> Result<String> {
    let oauth = GoogleOAuth::from_env()?;
    let stored = oauth.connect().await?;
    Ok(stored.email)
}

/// The currently connected account email, if any.
// 🦀 `Result<Option<String>>` maps cleanly to the JS side: `Ok(Some(s))` →
//    the Promise resolves with a `string`; `Ok(None)` → resolves with `null`;
//    `Err(e)` → the Promise rejects with the serialized `AppError`.
//    `Option<String>` serializes to `string | null` in TypeScript.
#[tauri::command]
pub async fn get_connected_account() -> Result<Option<String>> {
    Ok(load_token(PRIMARY_ACCOUNT)?.map(|t| t.email))
}

/// Fetch a preview (from/subject/snippet) of the most recent inbox messages.
// 🦀 Returning `Result<T, AppError>` (aliased as `Result<T>` via `crate::error`)
//    means: `Ok(value)` serializes `value` with serde and resolves the JS Promise;
//    `Err(e)` serializes `AppError` (which derives `Serialize`) and rejects it.
//    This gives the frontend structured error information rather than a raw panic.
#[tauri::command]
pub async fn fetch_inbox_preview(max: u32) -> Result<Vec<MessagePreview>> {
    // 🦀 Clamp the caller-supplied count into [1, 50] so a stray large value can't
    //    fan out into hundreds of sequential Gmail requests. `.clamp(lo, hi)` is a
    //    standard numeric method that bounds a value between two limits.
    let max = max.clamp(1, 50);
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    let ids = client.list_inbox_message_ids(max).await?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(client.get_message_preview(&id).await?);
    }
    Ok(out)
}
