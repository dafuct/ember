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
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "htmlLink", default)]
    pub html_link: Option<String>,
    #[serde(rename = "hangoutLink", default)]
    pub hangout_link: Option<String>,
    #[serde(default)]
    pub attendees: Option<Vec<GAttendee>>,
}

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

#[derive(Debug, Serialize, PartialEq)]
pub struct Attendee {
    pub email: String,
    pub response_status: Option<String>,
    #[serde(rename = "self")]
    pub is_self: bool,
}

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
    pub description: Option<String>,
    pub meet_link: Option<String>,
    pub html_link: Option<String>,
    pub attendees: Vec<Attendee>,
    pub my_response_status: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CalendarSummary {
    pub id: String,
    pub summary: String,
    pub primary: bool,
    pub writable: bool,
}
