// 🦀 serde "shapes": these structs mirror the JSON Google returns. `#[serde(rename = "...")]`
//    maps a camelCase JSON key to a snake_case Rust field. `Option<T>` means "the key may be
//    absent" — serde fills it with `None` instead of erroring.
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct CalendarListEntry {
    pub id: String,
    pub summary: Option<String>,
    #[serde(rename = "backgroundColor")]
    pub background_color: Option<String>,
    pub selected: Option<bool>,
    pub primary: Option<bool>,
    #[serde(rename = "accessRole", default)]
    pub access_role: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CalendarListResponse {
    // 🦀 `#[serde(default)]` → if "items" is missing, use Vec::default() ([]) rather than failing.
    #[serde(default)]
    pub items: Vec<CalendarListEntry>,
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GEventDateTime {
    #[serde(rename = "dateTime")]
    pub date_time: Option<String>,
    pub date: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GEvent {
    pub id: String,
    pub summary: Option<String>,
    pub start: Option<GEventDateTime>,
    pub end: Option<GEventDateTime>,
    pub location: Option<String>,
    pub status: Option<String>,
    // 🦀 New read fields. `#[serde(default)]` → absent key becomes None/empty, never an error.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "htmlLink", default)]
    pub html_link: Option<String>,
    #[serde(rename = "hangoutLink", default)]
    pub hangout_link: Option<String>,
    #[serde(default)]
    pub attendees: Option<Vec<GAttendee>>,
}

// 🦀 One guest on an event. `self` marks the signed-in user (Google sets it on GET);
//    `rename = "self"` maps the JSON key to the Rust field `is_self` (`self` is reserved).
#[derive(Debug, Deserialize)]
pub struct GAttendee {
    pub email: String,
    #[serde(rename = "responseStatus", default)]
    pub response_status: Option<String>,
    #[serde(rename = "self", default)]
    pub is_self: bool,
}

#[derive(Debug, Deserialize)]
pub struct EventsResponse {
    #[serde(default)]
    pub items: Vec<GEvent>,
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
}

// 🦀 The create/edit input from the frontend. `Deserialize` so a Tauri command can accept it;
//    snake_case fields → JS passes snake_case object keys. `start`/`end` are RFC3339 dateTime
//    (timed) or "YYYY-MM-DD" (all-day, end already exclusive — the frontend handles the +1).
#[derive(Debug, Deserialize)]
pub struct EventWrite {
    pub title: String,
    pub start: String,
    pub end: String,
    pub all_day: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub attendees: Vec<String>,
}

// 🦀 A guest as the frontend sees it: email + their RSVP status + whether it's you.
//    `rename = "self"` so the JS reads `attendee.self`. `PartialEq` lets tests assert_eq!.
#[derive(Debug, Serialize, PartialEq)]
pub struct Attendee {
    pub email: String,
    pub response_status: Option<String>,
    #[serde(rename = "self")]
    pub is_self: bool,
}

// 🦀 The normalized event we send to the frontend. `Serialize` lets Tauri turn it into JSON.
//    `PartialEq` lets unit tests compare values with assert_eq!.
#[derive(Debug, Serialize, PartialEq)]
pub struct CalendarEvent {
    pub id: String,
    pub calendar_id: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub all_day: bool,
    pub location: Option<String>,
    pub color: Option<String>,
    // 🦀 New fields for the detail popover + meeting features.
    pub description: Option<String>,
    pub meet_link: Option<String>,
    pub html_link: Option<String>,
    pub attendees: Vec<Attendee>,
    // 🦀 The signed-in user's own RSVP status (None ⇒ not a guest ⇒ no RSVP control).
    pub my_response_status: Option<String>,
}

/// A calendar the user can write to (for the create-event picker).
#[derive(Debug, Serialize)]
pub struct CalendarSummary {
    pub id: String,
    pub summary: String,
    pub primary: bool,
    pub writable: bool,
}
