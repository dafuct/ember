use crate::error::{AppError, Result};

const DEFAULT_BASE: &str = "https://api.zoom.us";

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ZoomAccount {
    pub email: String,
    pub account_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ZoomMeeting {
    pub id: String,
    pub join_url: String,
    pub password: Option<String>,
}

pub struct ZoomClient {
    base_url: String,
    access_token: String,
    http: reqwest::Client,
}

impl ZoomClient {
    pub fn new(access_token: String) -> Self {
        Self { base_url: DEFAULT_BASE.to_string(), access_token, http: reqwest::Client::new() }
    }

    pub fn with_base_url(access_token: String, base_url: String) -> Self {
        Self { base_url, access_token, http: reqwest::Client::new() }
    }

    async fn check(&self, resp: reqwest::Response) -> Result<reqwest::Response> {
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AppError::Auth("Zoom access expired — reconnect Zoom in Settings.".into()));
        }
        Ok(resp.error_for_status()?)
    }

    pub async fn get_me(&self) -> Result<ZoomAccount> {
        let url = format!("{}/v2/users/me", self.base_url);
        let resp = self.http.get(&url).bearer_auth(&self.access_token).send().await?;
        let resp = self.check(resp).await?;
        let w: MeWire = resp.json().await?;
        Ok(ZoomAccount { email: w.email.unwrap_or_default(), account_id: w.id })
    }

    pub async fn create_meeting(
        &self,
        topic: &str,
        start_rfc3339: &str,
        duration_min: u32,
        timezone: &str,
    ) -> Result<ZoomMeeting> {
        let url = format!("{}/v2/users/me/meetings", self.base_url);
        let body = CreateReq { topic, type_: 2, start_time: start_rfc3339, duration: duration_min, timezone };
        let resp = self.http.post(&url).bearer_auth(&self.access_token).json(&body).send().await?;
        let resp = self.check(resp).await?;
        let w: MeetingWire = resp.json().await?;
        Ok(ZoomMeeting { id: w.id.to_string(), join_url: w.join_url, password: w.password })
    }
}

#[derive(serde::Serialize)]
struct CreateReq<'a> {
    topic: &'a str,
    #[serde(rename = "type")]
    type_: u8,
    #[serde(rename = "start_time")]
    start_time: &'a str,
    duration: u32,
    timezone: &'a str,
}

#[derive(serde::Deserialize)]
struct MeetingWire {
    id: i64,
    join_url: String,
    #[serde(default)]
    password: Option<String>,
}

#[derive(serde::Deserialize)]
struct MeWire {
    id: String,
    #[serde(default)]
    email: Option<String>,
}
