pub mod types;

use types::{
    BusySpan, CalendarEvent, CalendarListEntry, CalendarListResponse, EventsResponse,
    FreeBusyResult, GEvent, PersonFreeBusy,
};

use crate::error::{AppError, Result};

const DEFAULT_BASE: &str = "https://www.googleapis.com";

pub struct CalendarClient {
    base_url: String,
    access_token: String,
    http: reqwest::Client,
}

impl CalendarClient {
    pub fn new(access_token: String) -> Self {
        Self { base_url: DEFAULT_BASE.to_string(), access_token, http: reqwest::Client::new() }
    }

    pub fn with_base_url(access_token: String, base_url: String) -> Self {
        Self { base_url, access_token, http: reqwest::Client::new() }
    }

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

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let resp = self.http.get(url).bearer_auth(&self.access_token).send().await?;
        let resp = self.check_auth_status(resp).await?;
        Ok(resp.json::<T>().await?)
    }

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

    pub async fn list_events(
        &self,
        calendar_id: &str,
        time_min: &str,
        time_max: &str,
    ) -> Result<Vec<GEvent>> {
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

    pub async fn create_event(
        &self,
        calendar_id: &str,
        ev: &types::EventWrite,
        conferencing: types::Conferencing,
        zoom_join_url: Option<&str>,
    ) -> Result<CalendarEvent> {
        let cal = url::form_urlencoded::byte_serialize(calendar_id.as_bytes()).collect::<String>();
        let url = format!(
            "{}/calendar/v3/calendars/{}/events?conferenceDataVersion=1&sendUpdates=all",
            self.base_url, cal
        );
        let mut body = event_body(ev);
        let description_owned: Option<String>;
        match conferencing {
            types::Conferencing::None => {}
            types::Conferencing::Meet => {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                body.conference_data = Some(ConferenceDataBody::Create {
                    create_request: CreateConferenceRequest {
                        request_id: format!("ember-meet-{nanos}"),
                        conference_solution_key: ConferenceSolutionKey { type_: "hangoutsMeet" },
                    },
                });
            }
            types::Conferencing::Zoom => {
                let uri = zoom_join_url.unwrap_or_default().to_string();
                body.conference_data = Some(ConferenceDataBody::Manual {
                    conference_solution: ConferenceSolutionBody {
                        key: ConferenceSolutionKey { type_: "addOn" },
                        name: "Zoom Meeting",
                    },
                    entry_points: vec![EntryPointBody {
                        entry_point_type: "video",
                        uri: uri.clone(),
                        label: "Zoom Meeting",
                    }],
                });
                // description fallback so the link is never lost
                let base = ev.description.clone().unwrap_or_default();
                let joined = if base.is_empty() {
                    format!("Join Zoom Meeting: {uri}")
                } else {
                    format!("Join Zoom Meeting: {uri}\n\n{base}")
                };
                description_owned = Some(joined);
                body.description = description_owned.as_deref();
            }
        }
        let resp = self.http.post(&url).bearer_auth(&self.access_token).json(&body).send().await?;
        let resp = self.check_auth_status(resp).await?;
        let g: GEvent = resp.json().await?;
        map_event(g, calendar_id, None)
            .ok_or_else(|| AppError::Other("calendar returned an unusable event".into()))
    }

    pub async fn update_event(
        &self,
        calendar_id: &str,
        event_id: &str,
        ev: &types::EventWrite,
    ) -> Result<CalendarEvent> {
        let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
        let url = format!(
            "{}/calendar/v3/calendars/{}/events/{}?sendUpdates=all",
            self.base_url, enc(calendar_id), enc(event_id)
        );
        let body = event_body(ev);
        let resp = self.http.patch(&url).bearer_auth(&self.access_token).json(&body).send().await?;
        let resp = self.check_auth_status(resp).await?;
        let g: GEvent = resp.json().await?;
        map_event(g, calendar_id, None)
            .ok_or_else(|| AppError::Other("calendar returned an unusable event".into()))
    }

    pub async fn delete_event(&self, calendar_id: &str, event_id: &str) -> Result<()> {
        let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
        let url = format!(
            "{}/calendar/v3/calendars/{}/events/{}?sendUpdates=all",
            self.base_url, enc(calendar_id), enc(event_id)
        );
        let resp = self.http.delete(&url).bearer_auth(&self.access_token).send().await?;
        self.check_auth_status(resp).await?;
        Ok(())
    }

    pub async fn get_event(&self, calendar_id: &str, event_id: &str) -> Result<GEvent> {
        let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
        let url = format!(
            "{}/calendar/v3/calendars/{}/events/{}",
            self.base_url, enc(calendar_id), enc(event_id)
        );
        self.get_json(&url).await
    }

    pub async fn free_busy(
        &self,
        emails: &[String],
        time_min: &str,
        time_max: &str,
    ) -> Result<FreeBusyResult> {
        let url = format!("{}/calendar/v3/freeBusy", self.base_url);
        let req = FreeBusyReq {
            time_min,
            time_max,
            items: emails.iter().map(|e| FreeBusyReqItem { id: e }).collect(),
        };
        let resp = self.http.post(&url).bearer_auth(&self.access_token).json(&req).send().await?;
        let resp = self.check_auth_status(resp).await?;
        let wire: FbWireResp = resp.json().await?;
        let calendars = wire
            .calendars
            .into_iter()
            .map(|(k, v)| {
                let error = v.errors.into_iter().find_map(|e| e.reason);
                (
                    k,
                    PersonFreeBusy {
                        busy: v
                            .busy
                            .into_iter()
                            .map(|b| BusySpan { start: b.start, end: b.end })
                            .collect(),
                        error,
                    },
                )
            })
            .collect();
        Ok(FreeBusyResult { calendars })
    }

    pub async fn respond_to_event(
        &self,
        calendar_id: &str,
        event_id: &str,
        response_status: &str,
        self_email: &str,
    ) -> Result<CalendarEvent> {
        let g = self.get_event(calendar_id, event_id).await?;
        let attendees = g.attendees.unwrap_or_default();
        let mut found = false;
        let patch: Vec<AttendeeResponseBody> = attendees
            .iter()
            .map(|a| {
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

#[derive(serde::Serialize)]
struct FreeBusyReqItem<'a> {
    id: &'a str,
}
#[derive(serde::Serialize)]
struct FreeBusyReq<'a> {
    #[serde(rename = "timeMin")]
    time_min: &'a str,
    #[serde(rename = "timeMax")]
    time_max: &'a str,
    items: Vec<FreeBusyReqItem<'a>>,
}
#[derive(serde::Deserialize)]
struct FbWireBusy {
    start: String,
    end: String,
}
#[derive(serde::Deserialize)]
struct FbWireError {
    #[serde(default)]
    reason: Option<String>,
}
#[derive(serde::Deserialize)]
struct FbWireCal {
    #[serde(default)]
    busy: Vec<FbWireBusy>,
    #[serde(default)]
    errors: Vec<FbWireError>,
}
#[derive(serde::Deserialize)]
struct FbWireResp {
    #[serde(default)]
    calendars: std::collections::HashMap<String, FbWireCal>,
}

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
#[serde(untagged)]
enum ConferenceDataBody {
    Create {
        #[serde(rename = "createRequest")]
        create_request: CreateConferenceRequest,
    },
    Manual {
        #[serde(rename = "conferenceSolution")]
        conference_solution: ConferenceSolutionBody,
        #[serde(rename = "entryPoints")]
        entry_points: Vec<EntryPointBody>,
    },
}

#[derive(serde::Serialize)]
struct ConferenceSolutionBody {
    key: ConferenceSolutionKey,
    name: &'static str,
}
#[derive(serde::Serialize)]
struct EntryPointBody {
    #[serde(rename = "entryPointType")]
    entry_point_type: &'static str,
    uri: String,
    label: &'static str,
}
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

fn date_time_body(value: &str, all_day: bool) -> EventDateTimeBody<'_> {
    if all_day {
        EventDateTimeBody { date_time: None, date: Some(value) }
    } else {
        EventDateTimeBody { date_time: Some(value), date: None }
    }
}

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

pub fn map_event(ev: GEvent, calendar_id: &str, color: Option<&str>) -> Option<CalendarEvent> {
    if ev.status.as_deref() == Some("cancelled") {
        return None;
    }
    let start = ev.start?;
    let end = ev.end?;
    let all_day = start.date.is_some();
    let start_s = start.date_time.or(start.date)?;
    let end_s = end.date_time.or(end.date)?;
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
        meet_link: ev.hangout_link.clone().or_else(|| {
            ev.conference_data.as_ref().and_then(|c| {
                c.entry_points
                    .iter()
                    .find(|e| e.entry_point_type.as_deref() == Some("video"))
                    .and_then(|e| e.uri.clone())
            })
        }),
        html_link: ev.html_link,
        attendees,
        my_response_status,
    })
}

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

pub fn is_safe_url(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    u.starts_with("https://") || u.starts_with("http://")
}
