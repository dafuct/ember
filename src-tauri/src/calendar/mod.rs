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

    // 🦀 Shared 401/403 handling (extracted from get_json so writes reuse it): map a missing-scope
    //    403 to AppError::Auth (reconnect helps) and any other 403 to the Google message.
    async fn check_auth_status(&self, resp: reqwest::Response) -> Result<reqwest::Response> {
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            let body = resp.text().await.unwrap_or_default();
            let msg = google_error_message(&body);
            let lower = msg.to_lowercase();
            if status == reqwest::StatusCode::UNAUTHORIZED
                || lower.contains("scope")
                || lower.contains("insufficient")
                || lower.contains("credential")
            {
                return Err(AppError::Auth(
                    "Calendar access not granted — reconnect Google to enable it.".into(),
                ));
            }
            return Err(AppError::Other(format!("Google Calendar API error: {msg}")));
        }
        Ok(resp.error_for_status()?)
    }

    // 🦀 GET + bearer auth + JSON parse. 401/403 need care: Google returns the SAME status for
    //    two very different problems — (a) the token lacks the calendar scope → reconnecting fixes
    //    it; (b) the Calendar API isn't enabled for the Cloud project (or another permission issue)
    //    → reconnecting can NEVER fix it. We read the JSON error body and tell them apart, so we
    //    don't trap the user in an endless "reconnect" loop and so the real cause is visible.
    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let resp = self.http.get(url).bearer_auth(&self.access_token).send().await?;
        let resp = self.check_auth_status(resp).await?;
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

    /// Create an event (optionally a meeting with a Google Meet link). `sendUpdates=all` emails
    /// the guests. Returns the created event, normalized.
    pub async fn create_event(
        &self,
        calendar_id: &str,
        ev: &types::EventWrite,
        add_meet: bool,
    ) -> Result<CalendarEvent> {
        // 🦀 Percent-encode the calendar id in the path — real ids contain '@'/'#' (same as list_events).
        let cal = url::form_urlencoded::byte_serialize(calendar_id.as_bytes()).collect::<String>();
        let url = format!(
            "{}/calendar/v3/calendars/{}/events?conferenceDataVersion=1&sendUpdates=all",
            self.base_url, cal
        );
        let mut body = event_body(ev);
        if add_meet {
            // 🦀 A unique requestId per create (mirrors M17's boundary) so retries don't collide.
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            body.conference_data = Some(ConferenceDataBody {
                create_request: CreateConferenceRequest {
                    request_id: format!("ember-meet-{nanos}"),
                    conference_solution_key: ConferenceSolutionKey { type_: "hangoutsMeet" },
                },
            });
        }
        let resp = self.http.post(&url).bearer_auth(&self.access_token).json(&body).send().await?;
        let resp = self.check_auth_status(resp).await?;
        let g: GEvent = resp.json().await?;
        // 🦀 Created events come back without the owning calendar's color; the next week refetch
        //    re-colors them. `ok_or_else` turns the None (e.g. a cancelled echo) into an error.
        map_event(g, calendar_id, None)
            .ok_or_else(|| AppError::Other("calendar returned an unusable event".into()))
    }

    /// Edit an existing event (PATCH — partial update). Sending the body fields replaces them;
    /// omitting `conferenceData` PRESERVES any existing Meet link. `sendUpdates=all` notifies guests.
    pub async fn update_event(
        &self,
        calendar_id: &str,
        event_id: &str,
        ev: &types::EventWrite,
    ) -> Result<CalendarEvent> {
        // 🦀 Percent-encode both path segments — real calendar/event ids contain '@'/'#'.
        let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
        let url = format!(
            "{}/calendar/v3/calendars/{}/events/{}?sendUpdates=all",
            self.base_url, enc(calendar_id), enc(event_id)
        );
        let body = event_body(ev); // conference_data stays None → existing Meet link preserved
        let resp = self.http.patch(&url).bearer_auth(&self.access_token).json(&body).send().await?;
        let resp = self.check_auth_status(resp).await?;
        let g: GEvent = resp.json().await?;
        map_event(g, calendar_id, None)
            .ok_or_else(|| AppError::Other("calendar returned an unusable event".into()))
    }

    /// Delete an event. `sendUpdates=all` sends guests the cancellation.
    pub async fn delete_event(&self, calendar_id: &str, event_id: &str) -> Result<()> {
        let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
        let url = format!(
            "{}/calendar/v3/calendars/{}/events/{}?sendUpdates=all",
            self.base_url, enc(calendar_id), enc(event_id)
        );
        let resp = self.http.delete(&url).bearer_auth(&self.access_token).send().await?;
        // 🦀 We don't need a body back; `check_auth_status` still maps 401/403 + `error_for_status`.
        self.check_auth_status(resp).await?;
        Ok(())
    }

    /// Fetch one event (used by RSVP to read the current attendee list).
    pub async fn get_event(&self, calendar_id: &str, event_id: &str) -> Result<GEvent> {
        let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
        let url = format!(
            "{}/calendar/v3/calendars/{}/events/{}",
            self.base_url, enc(calendar_id), enc(event_id)
        );
        self.get_json(&url).await
    }

    /// RSVP to an event: set your own `responseStatus` (accepted/declined/tentative) while
    /// preserving every other guest's status. GET-then-PATCH so a stale client can't clobber
    /// statuses set by others since the last fetch. `sendUpdates=all` notifies the organizer.
    pub async fn respond_to_event(
        &self,
        calendar_id: &str,
        event_id: &str,
        response_status: &str,
        self_email: &str,
    ) -> Result<CalendarEvent> {
        let g = self.get_event(calendar_id, event_id).await?;
        // 🦀 Own the attendees so the borrowed PATCH body stays valid until `send()`.
        let attendees = g.attendees.unwrap_or_default();
        let mut found = false;
        let patch: Vec<AttendeeResponseBody> = attendees
            .iter()
            .map(|a| {
                // You = Google's `self` flag, or an email match as a fallback.
                let is_me = a.is_self || a.email.eq_ignore_ascii_case(self_email);
                if is_me {
                    found = true;
                }
                AttendeeResponseBody {
                    email: &a.email,
                    response_status: if is_me { Some(response_status) } else { a.response_status.as_deref() },
                }
            })
            .collect();
        if !found {
            return Err(AppError::Other("you are not a guest on this event".into()));
        }
        let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
        let url = format!(
            "{}/calendar/v3/calendars/{}/events/{}?sendUpdates=all",
            self.base_url, enc(calendar_id), enc(event_id)
        );
        let body = AttendeesPatchBody { attendees: patch };
        let resp = self.http.patch(&url).bearer_auth(&self.access_token).json(&body).send().await?;
        let resp = self.check_auth_status(resp).await?;
        let g: GEvent = resp.json().await?;
        map_event(g, calendar_id, None)
            .ok_or_else(|| AppError::Other("calendar returned an unusable event".into()))
    }
}

// 🦀 Serialize-only shapes for the Google event-write body. Lifetimes (`'a`) let the body
//    borrow strings from the EventWrite instead of cloning. `skip_serializing_if` omits absent
//    optional keys entirely (Google rejects some explicit nulls).
#[derive(serde::Serialize)]
struct EventDateTimeBody<'a> {
    #[serde(rename = "dateTime", skip_serializing_if = "Option::is_none")]
    date_time: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<&'a str>,
}
#[derive(serde::Serialize)]
struct AttendeeBody<'a> {
    email: &'a str,
}
#[derive(serde::Serialize)]
struct ConferenceSolutionKey {
    #[serde(rename = "type")]
    type_: &'static str,
}
#[derive(serde::Serialize)]
struct CreateConferenceRequest {
    #[serde(rename = "requestId")]
    request_id: String,
    #[serde(rename = "conferenceSolutionKey")]
    conference_solution_key: ConferenceSolutionKey,
}
#[derive(serde::Serialize)]
struct ConferenceDataBody {
    #[serde(rename = "createRequest")]
    create_request: CreateConferenceRequest,
}
// 🦀 Narrow RSVP PATCH body: just the attendee list with each one's responseStatus.
//    Borrows (`'a`) the emails/statuses from the GET'd event — no clones.
#[derive(serde::Serialize)]
struct AttendeeResponseBody<'a> {
    email: &'a str,
    #[serde(rename = "responseStatus", skip_serializing_if = "Option::is_none")]
    response_status: Option<&'a str>,
}
#[derive(serde::Serialize)]
struct AttendeesPatchBody<'a> {
    attendees: Vec<AttendeeResponseBody<'a>>,
}
#[derive(serde::Serialize)]
struct EventBody<'a> {
    summary: &'a str,
    start: EventDateTimeBody<'a>,
    end: EventDateTimeBody<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<&'a str>,
    attendees: Vec<AttendeeBody<'a>>,
    #[serde(rename = "conferenceData", skip_serializing_if = "Option::is_none")]
    conference_data: Option<ConferenceDataBody>,
}

// 🦀 Build the start/end blocks from the EventWrite: all-day → `{date}`, timed → `{dateTime}`.
fn date_time_body(value: &str, all_day: bool) -> EventDateTimeBody<'_> {
    if all_day {
        EventDateTimeBody { date_time: None, date: Some(value) }
    } else {
        EventDateTimeBody { date_time: Some(value), date: None }
    }
}

// 🦀 The shared event body (everything except conferenceData, which only `create` adds).
fn event_body(ev: &types::EventWrite) -> EventBody<'_> {
    EventBody {
        summary: &ev.title,
        start: date_time_body(&ev.start, ev.all_day),
        end: date_time_body(&ev.end, ev.all_day),
        description: ev.description.as_deref(),
        location: ev.location.as_deref(),
        attendees: ev.attendees.iter().map(|e| AttendeeBody { email: e }).collect(),
        conference_data: None,
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
    // 🦀 Map guests to the frontend Attendee shape; remember your own status for the RSVP control.
    let attendees: Vec<types::Attendee> = ev
        .attendees
        .unwrap_or_default()
        .into_iter()
        .map(|g| types::Attendee {
            email: g.email,
            response_status: g.response_status,
            is_self: g.is_self,
        })
        .collect();
    let my_response_status = attendees
        .iter()
        .find(|a| a.is_self)
        .and_then(|a| a.response_status.clone());
    Some(CalendarEvent {
        id: ev.id,
        calendar_id: calendar_id.to_string(),
        title: ev.summary.filter(|s| !s.is_empty()).unwrap_or_else(|| "(no title)".to_string()),
        start: start_s,
        end: end_s,
        all_day,
        location: ev.location,
        color: color.map(|c| c.to_string()),
        description: ev.description,
        meet_link: ev.hangout_link,
        html_link: ev.html_link,
        attendees,
        my_response_status,
    })
}

// 🦀 Pull the human-readable message out of Google's error JSON ({"error":{"message":"…"}}),
//    falling back to the raw (truncated) body when it doesn't parse. `serde_json::Value` is a
//    dynamically-typed JSON tree we can index without declaring a struct.
fn google_error_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .map(String::from)
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let trimmed = body.trim();
            if trimmed.is_empty() {
                "permission denied".to_string()
            } else {
                trimmed.chars().take(300).collect()
            }
        })
}

// 🦀 Guard for `open_external`: only let the OS open web links, never file:/javascript:/etc.
pub fn is_safe_url(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    u.starts_with("https://") || u.starts_with("http://")
}
