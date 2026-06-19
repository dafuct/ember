// 🦀 `pub mod types;` exposes the sibling `types.rs` as `ember_lib::calendar::types`.
pub mod types;

use types::{CalendarListEntry, CalendarListResponse};

use crate::error::{AppError, Result};

const DEFAULT_BASE: &str = "https://www.googleapis.com";

// 🦀 Same shape as GmailClient: a base URL (swappable in tests), the bearer token, and a
//    reusable reqwest client (connection-pooled, cheap to hold).
pub struct CalendarClient {
    base_url: String,
    access_token: String,
    http: reqwest::Client,
}

impl CalendarClient {
    pub fn new(access_token: String) -> Self {
        Self { base_url: DEFAULT_BASE.to_string(), access_token, http: reqwest::Client::new() }
    }

    /// Point the client at a mock server in tests.
    pub fn with_base_url(access_token: String, base_url: String) -> Self {
        Self { base_url, access_token, http: reqwest::Client::new() }
    }

    // 🦀 GET + bearer auth + JSON parse. We peek at the status BEFORE `error_for_status()` so a
    //    401/403 (no calendar scope) becomes a friendly, actionable AppError::Auth instead of a
    //    generic "http error: 403" — the same "inspect status first" trick GmailClient uses for 404.
    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let resp = self.http.get(url).bearer_auth(&self.access_token).send().await?;
        if matches!(
            resp.status(),
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        ) {
            return Err(AppError::Auth(
                "Calendar access not granted — reconnect Google to enable it.".into(),
            ));
        }
        let resp = resp.error_for_status()?;
        Ok(resp.json::<T>().await?)
    }

    /// All calendars in the user's list (following pagination).
    pub async fn list_calendars(&self) -> Result<Vec<CalendarListEntry>> {
        let mut out = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/calendar/v3/users/me/calendarList?maxResults=250",
                self.base_url
            );
            if let Some(t) = &page_token {
                url.push_str(&format!("&pageToken={t}"));
            }
            let page: CalendarListResponse = self.get_json(&url).await?;
            out.extend(page.items);
            match page.next_page_token {
                Some(t) => page_token = Some(t),
                None => break,
            }
        }
        Ok(out)
    }
}
