// 🦀 `pub mod types;` exposes the sibling `types.rs` as `ember_lib::calendar::types`.
pub mod types;

use types::{CalendarEvent, CalendarListEntry, CalendarListResponse, EventsResponse, GEvent};

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

    /// Events in [time_min, time_max) for one calendar. `singleEvents=true` expands recurring
    /// events into individual instances; `orderBy=startTime` requires it. Follows pagination.
    pub async fn list_events(
        &self,
        calendar_id: &str,
        time_min: &str,
        time_max: &str,
    ) -> Result<Vec<GEvent>> {
        // 🦀 Percent-encode path + query values: calendar ids contain '@'/'#', and timeMin/Max
        //    contain ':' and '+', all of which must be escaped to stay URL-safe.
        let enc = |s: &str| -> String { url::form_urlencoded::byte_serialize(s.as_bytes()).collect() };
        let cal = enc(calendar_id);
        let (tmin, tmax) = (enc(time_min), enc(time_max));
        let mut out = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/calendar/v3/calendars/{}/events\
                 ?singleEvents=true&orderBy=startTime&maxResults=250&timeMin={}&timeMax={}",
                self.base_url, cal, tmin, tmax
            );
            if let Some(t) = &page_token {
                url.push_str(&format!("&pageToken={t}"));
            }
            let page: EventsResponse = self.get_json(&url).await?;
            out.extend(page.items);
            match page.next_page_token {
                Some(t) => page_token = Some(t),
                None => break,
            }
        }
        Ok(out)
    }
}

// 🦀 Pure mapping (no I/O → trivially unit-testable). Returns None for cancelled or malformed
//    events. `all_day` is detected by the presence of `start.date` (vs `start.dateTime`). The
//    `?` on an Option returns None early when a field is missing.
pub fn map_event(ev: GEvent, calendar_id: &str, color: Option<&str>) -> Option<CalendarEvent> {
    if ev.status.as_deref() == Some("cancelled") {
        return None;
    }
    let start = ev.start?;
    let end = ev.end?;
    let all_day = start.date.is_some();
    let start_s = start.date_time.or(start.date)?;
    let end_s = end.date_time.or(end.date)?;
    Some(CalendarEvent {
        id: ev.id,
        calendar_id: calendar_id.to_string(),
        // 🦀 filter() drops an empty summary so it falls through to the default title.
        title: ev.summary.filter(|s| !s.is_empty()).unwrap_or_else(|| "(no title)".to_string()),
        start: start_s,
        end: end_s,
        all_day,
        location: ev.location,
        color: color.map(|c| c.to_string()),
    })
}
