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
}

#[derive(Debug, Deserialize)]
pub struct EventsResponse {
    #[serde(default)]
    pub items: Vec<GEvent>,
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
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
}
