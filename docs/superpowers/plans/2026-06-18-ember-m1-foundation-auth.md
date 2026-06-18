# Ember — Milestone 1: Foundation & Auth — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the Ember Tauri app so a user can sign into Google (OAuth loopback + PKCE), have tokens stored in the macOS Keychain and auto-refreshed, and see their real Gmail address plus a preview of recent inbox messages — proving local, backend-free Gmail access.

**Architecture:** Tauri 2 desktop app. Rust backend owns OAuth, Keychain token storage/refresh, and a typed Gmail REST client; it exposes async `#[tauri::command]`s. A minimal React + TypeScript frontend calls those commands via `@tauri-apps/api/core`. No server of our own — the app talks directly to Google.

**Tech Stack:** Tauri 2, Rust (`oauth2` v5, `reqwest`, `keyring` v3, `tokio`, `serde`, `open`, `dotenvy`, `thiserror`; `wiremock` for tests), React 18 + TypeScript + Vite.

---

## Milestone roadmap (context — only M1 is in this plan)

- **M1 — Foundation & auth (this plan):** scaffold, OAuth loopback + PKCE, Keychain tokens + refresh, first real Gmail fetch, minimal UI.
- **M2 — Local store & mail sync:** SQLite schema, 30-day backfill, message parse, Gmail History API incremental sync + polling.
- **M3 — Smart-inbox scorer:** People / Notifications / Newsletters classifier (+ table tests).
- **M4 — Ember mail UI:** 3-pane layout, Ember/Slate/Bloom themes, bundled fonts, Phosphor icons, sanitized HTML reading pane + remote-image blocking.
- **M5 — Actions:** read/unread, star, archive, labels, pin (local), snooze (local + scheduler).
- **M6 — Compose & send:** RFC822 builder, drafts, outbox, signature.
- **M7 — Settings & onboarding wiring.**
- **M8 — Calendar:** read-only Google Calendar week view.

Each later milestone gets its own plan. The smart-inbox model, scopes, and design are defined in `docs/superpowers/specs/2026-06-18-ownmail-gmail-client-design.md`.

---

## Prerequisites (one-time Google Cloud setup — do before Task 6)

These are manual console steps; capture the two secret values for Task 6.

1. Go to <https://console.cloud.google.com/>, create a project named `Ember`.
2. **APIs & Services → Library →** enable **Gmail API**. (Calendar API is added in M8.)
3. **APIs & Services → OAuth consent screen:** User type **External**; app name `Ember`; add your own Google address under **Test users**. (Test-user mode needs no Google verification.)
4. **APIs & Services → Credentials → Create credentials → OAuth client ID →** Application type **Desktop app**. Copy the **Client ID** and **Client secret**.
5. You'll place these in `src-tauri/.env` in Task 6. Loopback redirect URIs (`http://127.0.0.1:<port>`) need no registration for Desktop clients.

---

## File structure (created across M1, repo root = `/Users/makar/dev/ownmail`)

```
package.json                     frontend deps + scripts (scaffolded, then trimmed)
index.html                       Vite entry
vite.config.ts                   Vite + React, Tauri dev server on :1420
tsconfig.json / tsconfig.node.json
src/
  main.tsx                       React mount
  App.tsx                        minimal M1 screen: Connect + inbox preview
  lib/api.ts                     typed wrappers over Tauri invoke
src-tauri/
  Cargo.toml                     Rust deps (overwritten in Task 2)
  build.rs                       tauri_build::build()
  tauri.conf.json                app identity, window, dev/build commands
  .env                           EMBER_GOOGLE_CLIENT_ID/SECRET (gitignored)
  .env.example                   committed template
  capabilities/default.json      scaffolded core capability (left as-is)
  src/
    main.rs                      calls ember_lib::run()
    lib.rs                       Tauri builder + command registration
    error.rs                     AppError + serializable Result
    auth/
      mod.rs                     OAuth orchestration: connect, refresh, ensure_access_token
      tokens.rs                  StoredToken + Keychain storage (+ unit tests)
      loopback.rs                one-shot loopback listener + query parse (+ unit tests)
    gmail/
      mod.rs                     GmailClient (profile, list ids, message preview)
      types.rs                   serde types + MessagePreview
    commands.rs                  #[tauri::command]s
  tests/
    gmail_test.rs                wiremock tests of GmailClient
```

---

### Task 1: Scaffold the Tauri 2 + React-TS project into the repo

**Files:**
- Create: the scaffold (`package.json`, `index.html`, `vite.config.ts`, `tsconfig*.json`, `src/`, `src-tauri/`).

The repo root already has `docs/`, `scripts/`, `.git`, `.gitignore`. `create-tauri-app` wants a clean target, so scaffold into a temp dir and move the generated files in.

- [ ] **Step 1: Scaffold into a temp directory**

Run:
```bash
cd /tmp && rm -rf ember-scaffold && \
npm create tauri-app@latest ember-scaffold -- --template react-ts --manager npm --yes
```
Expected: a `/tmp/ember-scaffold` folder containing `src/`, `src-tauri/`, `package.json`, `vite.config.ts`, etc.

- [ ] **Step 2: Move scaffold files into the repo root (preserving our docs/scripts/git)**

Run:
```bash
cd /tmp/ember-scaffold && \
cp -R src src-tauri index.html package.json vite.config.ts tsconfig.json tsconfig.node.json /Users/makar/dev/ownmail/ && \
cd /Users/makar/dev/ownmail && rm -rf /tmp/ember-scaffold
```
Expected: `/Users/makar/dev/ownmail/src-tauri/` and `src/` now exist.

- [ ] **Step 3: Install dependencies**

Run: `cd /Users/makar/dev/ownmail && npm install`
Expected: `node_modules/` created, no errors.

- [ ] **Step 4: Verify the app builds and launches**

Run: `cd /Users/makar/dev/ownmail && npm run tauri dev`
Expected: a desktop window opens showing the default Tauri+React page. (First Rust build is slow.) Close the window to stop.

- [ ] **Step 5: Commit**

```bash
cd /Users/makar/dev/ownmail
git add -A
git commit -m "chore: scaffold Tauri 2 + React-TS app"
```

---

### Task 2: Rust dependencies, error type, and app entry

**Files:**
- Modify (overwrite): `src-tauri/Cargo.toml`
- Create: `src-tauri/src/error.rs`
- Modify (overwrite): `src-tauri/src/lib.rs`, `src-tauri/src/main.rs`

- [ ] **Step 1: Overwrite `src-tauri/Cargo.toml`**

```toml
[package]
name = "ember"
version = "0.1.0"
description = "Ember — a local Gmail client"
edition = "2021"

[lib]
name = "ember_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json"] }
oauth2 = "5"
keyring = { version = "3", features = ["apple-native"] }
url = "2"
open = "5"
thiserror = "2"
dotenvy = "0.15"

[dev-dependencies]
wiremock = "0.6"
```

- [ ] **Step 2: Create `src-tauri/src/error.rs`**

```rust
use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("auth error: {0}")]
    Auth(String),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("keychain error: {0}")]
    Keychain(#[from] keyring::Error),
    #[error("config error: {0}")]
    Config(String),
    #[error("{0}")]
    Other(String),
}

// Tauri commands must return a serializable error; surface it as a string.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
```

- [ ] **Step 3: Overwrite `src-tauri/src/lib.rs` (minimal entry for now)**

```rust
mod error;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load src-tauri/.env in dev so Google client id/secret are available.
    let _ = dotenvy::from_path(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env"),
    );
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 4: Ensure `src-tauri/src/main.rs` calls the lib**

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ember_lib::run();
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cd /Users/makar/dev/ownmail/src-tauri && cargo build`
Expected: builds successfully (warnings about unused `error` module are fine).

- [ ] **Step 6: Commit**

```bash
cd /Users/makar/dev/ownmail
git add -A
git commit -m "feat(backend): add deps, AppError, and app entry"
```

---

### Task 3: Token model + Keychain storage

**Files:**
- Create: `src-tauri/src/auth/tokens.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod auth;` and `auth/mod.rs` declaring `tokens`)
- Create: `src-tauri/src/auth/mod.rs` (temporary: just declares `tokens`; replaced in Task 6)

- [ ] **Step 1: Write the failing unit tests inside `tokens.rs`**

Create `src-tauri/src/auth/tokens.rs`:
```rust
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

const KEYCHAIN_SERVICE: &str = "dev.ember.oauth";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredToken {
    pub email: String,
    pub access_token: String,
    pub refresh_token: String,
    /// Unix epoch seconds when the access token expires.
    pub expires_at: u64,
}

impl StoredToken {
    /// True if the access token is expired or within `skew_secs` of expiring.
    pub fn is_expired(&self, now_secs: u64, skew_secs: u64) -> bool {
        now_secs + skew_secs >= self.expires_at
    }
}

pub fn save_token(account: &str, token: &StoredToken) -> Result<()> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account)?;
    let json = serde_json::to_string(token).map_err(|e| AppError::Other(e.to_string()))?;
    entry.set_password(&json)?;
    Ok(())
}

pub fn load_token(account: &str) -> Result<Option<StoredToken>> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account)?;
    match entry.get_password() {
        Ok(json) => Ok(Some(
            serde_json::from_str(&json).map_err(|e| AppError::Other(e.to_string()))?,
        )),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::StoredToken;

    fn tok(expires_at: u64) -> StoredToken {
        StoredToken {
            email: "a@b.com".into(),
            access_token: "x".into(),
            refresh_token: "r".into(),
            expires_at,
        }
    }

    #[test]
    fn expired_when_within_skew() {
        let t = tok(1000);
        assert!(t.is_expired(950, 60)); // 950 + 60 >= 1000
        assert!(!t.is_expired(900, 60)); // 900 + 60 < 1000
    }

    #[test]
    fn serde_round_trips() {
        let t = tok(1234);
        let json = serde_json::to_string(&t).unwrap();
        let back: StoredToken = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }
}
```

Create `src-tauri/src/auth/mod.rs`:
```rust
pub mod tokens;
```

Add to the top of `src-tauri/src/lib.rs` (after `mod error;`):
```rust
pub mod auth;
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cd /Users/makar/dev/ownmail/src-tauri && cargo test tokens`
Expected: `expired_when_within_skew` and `serde_round_trips` PASS.

(Keychain `save_token`/`load_token` are exercised in the Task 9 manual run, not in unit tests, to avoid a Keychain prompt in headless test runs.)

- [ ] **Step 3: Commit**

```bash
cd /Users/makar/dev/ownmail
git add -A
git commit -m "feat(auth): token model with expiry logic and Keychain storage"
```

---

### Task 4: Loopback redirect listener

**Files:**
- Create: `src-tauri/src/auth/loopback.rs`
- Modify: `src-tauri/src/auth/mod.rs` (add `pub mod loopback;`)

- [ ] **Step 1: Write `loopback.rs` with its failing unit tests**

```rust
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

use crate::error::{AppError, Result};

/// A bound loopback listener plus the redirect URI Google should call back.
pub struct Loopback {
    listener: TcpListener,
    pub redirect_uri: String,
}

impl Loopback {
    pub fn bind() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| AppError::Auth(format!("bind failed: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| AppError::Auth(e.to_string()))?
            .port();
        Ok(Self {
            listener,
            redirect_uri: format!("http://127.0.0.1:{port}"),
        })
    }

    /// Block until Google redirects back, then return (code, state).
    pub fn wait_for_code(self) -> Result<(String, String)> {
        let (mut stream, _) = self
            .listener
            .accept()
            .map_err(|e| AppError::Auth(e.to_string()))?;
        let mut request_line = String::new();
        BufReader::new(&stream)
            .read_line(&mut request_line)
            .map_err(|e| AppError::Auth(e.to_string()))?;

        let (code, state) = parse_redirect_query(&request_line)
            .ok_or_else(|| AppError::Auth("missing code/state in redirect".into()))?;

        let body = "<html><body style=\"font-family:sans-serif;text-align:center;padding:40px\">\
            <h2>Ember is connected</h2><p>You can close this tab and return to the app.</p></body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).ok();
        Ok((code, state))
    }
}

/// Parse the request line `GET /?code=XXX&state=YYY HTTP/1.1` into (code, state).
pub fn parse_redirect_query(request_line: &str) -> Option<(String, String)> {
    let path = request_line.split_whitespace().nth(1)?; // "/?code=...&state=..."
    let query = path.split_once('?')?.1;
    let mut code = None;
    let mut state = None;
    for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            _ => {}
        }
    }
    Some((code?, state?))
}

#[cfg(test)]
mod tests {
    use super::parse_redirect_query;

    #[test]
    fn parses_code_and_state_with_percent_encoding() {
        let line = "GET /?code=4%2F0Ab&state=xyz789 HTTP/1.1";
        assert_eq!(
            parse_redirect_query(line),
            Some(("4/0Ab".to_string(), "xyz789".to_string()))
        );
    }

    #[test]
    fn returns_none_without_code() {
        let line = "GET /?state=only HTTP/1.1";
        assert_eq!(parse_redirect_query(line), None);
    }
}
```

Add to `src-tauri/src/auth/mod.rs`:
```rust
pub mod loopback;
pub mod tokens;
```

- [ ] **Step 2: Run the tests**

Run: `cd /Users/makar/dev/ownmail/src-tauri && cargo test loopback`
Expected: both `parse_redirect_query` tests PASS.

- [ ] **Step 3: Commit**

```bash
cd /Users/makar/dev/ownmail
git add -A
git commit -m "feat(auth): one-shot loopback listener and redirect query parser"
```

---

### Task 5: Gmail REST client

**Files:**
- Create: `src-tauri/src/gmail/types.rs`, `src-tauri/src/gmail/mod.rs`
- Create: `src-tauri/tests/gmail_test.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod gmail;`)

- [ ] **Step 1: Create `src-tauri/src/gmail/types.rs`**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct Profile {
    #[serde(rename = "emailAddress")]
    pub email_address: String,
    #[serde(rename = "messagesTotal", default)]
    pub messages_total: u64,
}

#[derive(Debug, Deserialize)]
pub struct MessageList {
    #[serde(default)]
    pub messages: Vec<MessageRef>,
}

#[derive(Debug, Deserialize)]
pub struct MessageRef {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct RawMessage {
    pub id: String,
    #[serde(default)]
    pub snippet: String,
    pub payload: Payload,
}

#[derive(Debug, Deserialize)]
pub struct Payload {
    #[serde(default)]
    pub headers: Vec<Header>,
}

#[derive(Debug, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// What the UI consumes for the inbox preview.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MessagePreview {
    pub id: String,
    pub from: String,
    pub subject: String,
    pub date: String,
    pub snippet: String,
}
```

- [ ] **Step 2: Write the failing wiremock tests `src-tauri/tests/gmail_test.rs`**

```rust
use ember_lib::gmail::GmailClient;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread")]
async fn get_profile_parses_email() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "emailAddress": "jordan@example.com",
            "messagesTotal": 1234
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let profile = client.get_profile().await.unwrap();
    assert_eq!(profile.email_address, "jordan@example.com");
    assert_eq!(profile.messages_total, 1234);
}

#[tokio::test(flavor = "multi_thread")]
async fn list_inbox_message_ids_collects_ids() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .and(query_param("labelIds", "INBOX"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messages": [{"id": "a1"}, {"id": "a2"}]
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let ids = client.list_inbox_message_ids(20).await.unwrap();
    assert_eq!(ids, vec!["a1".to_string(), "a2".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_message_preview_extracts_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/a1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "a1",
            "snippet": "Hello there",
            "payload": { "headers": [
                {"name": "From", "value": "Maya <maya@studio.co>"},
                {"name": "Subject", "value": "Q3 roadmap"},
                {"name": "Date", "value": "Wed, 18 Jun 2026 09:42:00 -0700"}
            ]}
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let m = client.get_message_preview("a1").await.unwrap();
    assert_eq!(m.from, "Maya <maya@studio.co>");
    assert_eq!(m.subject, "Q3 roadmap");
    assert_eq!(m.snippet, "Hello there");
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cd /Users/makar/dev/ownmail/src-tauri && cargo test --test gmail_test`
Expected: FAILS to compile — `ember_lib::gmail` / `GmailClient` not found.

- [ ] **Step 4: Create `src-tauri/src/gmail/mod.rs`**

```rust
pub mod types;

use types::{MessageList, MessagePreview, Profile, RawMessage};

use crate::error::Result;

const DEFAULT_BASE: &str = "https://gmail.googleapis.com";

pub struct GmailClient {
    base_url: String,
    access_token: String,
    http: reqwest::Client,
}

impl GmailClient {
    pub fn new(access_token: String) -> Self {
        Self {
            base_url: DEFAULT_BASE.to_string(),
            access_token,
            http: reqwest::Client::new(),
        }
    }

    /// Used by tests to point the client at a mock server.
    pub fn with_base_url(access_token: String, base_url: String) -> Self {
        Self {
            base_url,
            access_token,
            http: reqwest::Client::new(),
        }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let resp = self
            .http
            .get(url)
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json::<T>().await?)
    }

    pub async fn get_profile(&self) -> Result<Profile> {
        let url = format!("{}/gmail/v1/users/me/profile", self.base_url);
        self.get_json(&url).await
    }

    pub async fn list_inbox_message_ids(&self, max: u32) -> Result<Vec<String>> {
        let url = format!(
            "{}/gmail/v1/users/me/messages?maxResults={}&labelIds=INBOX",
            self.base_url, max
        );
        let list: MessageList = self.get_json(&url).await?;
        Ok(list.messages.into_iter().map(|m| m.id).collect())
    }

    pub async fn get_message_preview(&self, id: &str) -> Result<MessagePreview> {
        let url = format!(
            "{}/gmail/v1/users/me/messages/{}?format=metadata&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date",
            self.base_url, id
        );
        let raw: RawMessage = self.get_json(&url).await?;
        let header = |name: &str| {
            raw.payload
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case(name))
                .map(|h| h.value.clone())
                .unwrap_or_default()
        };
        Ok(MessagePreview {
            id: raw.id.clone(),
            from: header("From"),
            subject: header("Subject"),
            date: header("Date"),
            snippet: raw.snippet,
        })
    }
}
```

Add to `src-tauri/src/lib.rs` (after `pub mod auth;`):
```rust
pub mod gmail;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd /Users/makar/dev/ownmail/src-tauri && cargo test --test gmail_test`
Expected: all three tests PASS.

- [ ] **Step 6: Commit**

```bash
cd /Users/makar/dev/ownmail
git add -A
git commit -m "feat(gmail): typed Gmail client for profile, ids, and message preview"
```

---

### Task 6: OAuth flow (connect, refresh, ensure-valid)

**Files:**
- Modify (overwrite): `src-tauri/src/auth/mod.rs`
- Create: `src-tauri/.env`, `src-tauri/.env.example`

- [ ] **Step 1: Create the env files**

`src-tauri/.env.example` (committed):
```
EMBER_GOOGLE_CLIENT_ID=your-client-id.apps.googleusercontent.com
EMBER_GOOGLE_CLIENT_SECRET=your-client-secret
```

`src-tauri/.env` (gitignored — fill in the values from Prerequisites step 4):
```
EMBER_GOOGLE_CLIENT_ID=PASTE_CLIENT_ID
EMBER_GOOGLE_CLIENT_SECRET=PASTE_CLIENT_SECRET
```

- [ ] **Step 2: Overwrite `src-tauri/src/auth/mod.rs`**

```rust
pub mod loopback;
pub mod tokens;

use std::time::{SystemTime, UNIX_EPOCH};

use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge, RedirectUrl,
    RefreshToken, Scope, TokenResponse, TokenUrl,
};

use crate::auth::loopback::Loopback;
use crate::auth::tokens::{load_token, save_token, StoredToken};
use crate::error::{AppError, Result};
use crate::gmail::GmailClient;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE_GMAIL_MODIFY: &str = "https://www.googleapis.com/auth/gmail.modify";
pub const PRIMARY_ACCOUNT: &str = "primary";

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn no_redirect_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(AppError::from)
}

pub struct GoogleOAuth {
    client_id: String,
    client_secret: String,
}

impl GoogleOAuth {
    pub fn from_env() -> Result<Self> {
        let client_id = std::env::var("EMBER_GOOGLE_CLIENT_ID")
            .map_err(|_| AppError::Config("EMBER_GOOGLE_CLIENT_ID not set".into()))?;
        let client_secret = std::env::var("EMBER_GOOGLE_CLIENT_SECRET")
            .map_err(|_| AppError::Config("EMBER_GOOGLE_CLIENT_SECRET not set".into()))?;
        Ok(Self {
            client_id,
            client_secret,
        })
    }

    /// Run the full interactive loopback + PKCE flow, fetch the account email,
    /// store the token in the Keychain, and return it.
    pub async fn connect(&self) -> Result<StoredToken> {
        let loopback = Loopback::bind()?;
        let redirect_uri = loopback.redirect_uri.clone();

        let client = BasicClient::new(ClientId::new(self.client_id.clone()))
            .set_client_secret(ClientSecret::new(self.client_secret.clone()))
            .set_auth_uri(AuthUrl::new(AUTH_URL.into()).map_err(|e| AppError::Auth(e.to_string()))?)
            .set_token_uri(
                TokenUrl::new(TOKEN_URL.into()).map_err(|e| AppError::Auth(e.to_string()))?,
            )
            .set_redirect_uri(
                RedirectUrl::new(redirect_uri).map_err(|e| AppError::Auth(e.to_string()))?,
            );

        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let (auth_url, csrf) = client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new(SCOPE_GMAIL_MODIFY.into()))
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent")
            .set_pkce_challenge(pkce_challenge)
            .url();

        open::that(auth_url.to_string())
            .map_err(|e| AppError::Auth(format!("could not open browser: {e}")))?;

        // The listener blocks; run it off the async runtime.
        let (code, state) = tokio::task::spawn_blocking(move || loopback.wait_for_code())
            .await
            .map_err(|e| AppError::Auth(e.to_string()))??;

        if state != *csrf.secret() {
            return Err(AppError::Auth("CSRF state mismatch".into()));
        }

        let http = no_redirect_http_client()?;
        let token = client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(&http)
            .await
            .map_err(|e| AppError::Auth(format!("token exchange failed: {e}")))?;

        let refresh_token = token
            .refresh_token()
            .ok_or_else(|| AppError::Auth("no refresh token returned".into()))?
            .secret()
            .clone();
        let access_token = token.access_token().secret().clone();
        let expires_in = token.expires_in().map(|d| d.as_secs()).unwrap_or(3600);

        let email = GmailClient::new(access_token.clone())
            .get_profile()
            .await?
            .email_address;

        let stored = StoredToken {
            email,
            access_token,
            refresh_token,
            expires_at: now_secs() + expires_in,
        };
        save_token(PRIMARY_ACCOUNT, &stored)?;
        Ok(stored)
    }

    /// Exchange a refresh token for a fresh access token. Returns (access_token, expires_at).
    pub async fn refresh(&self, refresh_token: &str) -> Result<(String, u64)> {
        let client = BasicClient::new(ClientId::new(self.client_id.clone()))
            .set_client_secret(ClientSecret::new(self.client_secret.clone()))
            .set_auth_uri(AuthUrl::new(AUTH_URL.into()).map_err(|e| AppError::Auth(e.to_string()))?)
            .set_token_uri(
                TokenUrl::new(TOKEN_URL.into()).map_err(|e| AppError::Auth(e.to_string()))?,
            );

        let http = no_redirect_http_client()?;
        let token = client
            .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
            .request_async(&http)
            .await
            .map_err(|e| AppError::Auth(format!("refresh failed: {e}")))?;

        let access = token.access_token().secret().clone();
        let expires_in = token.expires_in().map(|d| d.as_secs()).unwrap_or(3600);
        Ok((access, now_secs() + expires_in))
    }
}

/// Load the stored token, refreshing the access token if it is expired, and return it.
pub async fn ensure_access_token() -> Result<StoredToken> {
    let mut stored =
        load_token(PRIMARY_ACCOUNT)?.ok_or_else(|| AppError::Auth("no connected account".into()))?;
    if stored.is_expired(now_secs(), 60) {
        let oauth = GoogleOAuth::from_env()?;
        let (access, expires_at) = oauth.refresh(&stored.refresh_token).await?;
        stored.access_token = access;
        stored.expires_at = expires_at;
        save_token(PRIMARY_ACCOUNT, &stored)?;
    }
    Ok(stored)
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /Users/makar/dev/ownmail/src-tauri && cargo build`
Expected: builds successfully.

- [ ] **Step 4: Commit**

```bash
cd /Users/makar/dev/ownmail
git add -A
git commit -m "feat(auth): Google OAuth loopback+PKCE flow with token refresh"
```

---

### Task 7: Tauri commands

**Files:**
- Create: `src-tauri/src/commands.rs`
- Modify (overwrite): `src-tauri/src/lib.rs`

- [ ] **Step 1: Create `src-tauri/src/commands.rs`**

```rust
use crate::auth::tokens::load_token;
use crate::auth::{ensure_access_token, GoogleOAuth, PRIMARY_ACCOUNT};
use crate::error::Result;
use crate::gmail::types::MessagePreview;
use crate::gmail::GmailClient;

/// Run the interactive Google sign-in. Returns the connected email address.
#[tauri::command]
pub async fn connect_gmail() -> Result<String> {
    let oauth = GoogleOAuth::from_env()?;
    let stored = oauth.connect().await?;
    Ok(stored.email)
}

/// The currently connected account email, if any.
#[tauri::command]
pub async fn get_connected_account() -> Result<Option<String>> {
    Ok(load_token(PRIMARY_ACCOUNT)?.map(|t| t.email))
}

/// Fetch a preview (from/subject/snippet) of the most recent inbox messages.
#[tauri::command]
pub async fn fetch_inbox_preview(max: u32) -> Result<Vec<MessagePreview>> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    let ids = client.list_inbox_message_ids(max).await?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(client.get_message_preview(&id).await?);
    }
    Ok(out)
}
```

- [ ] **Step 2: Overwrite `src-tauri/src/lib.rs` (register modules + commands)**

```rust
mod commands;
mod error;
pub mod auth;
pub mod gmail;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load src-tauri/.env in dev so Google client id/secret are available.
    let _ = dotenvy::from_path(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env"),
    );
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::connect_gmail,
            commands::get_connected_account,
            commands::fetch_inbox_preview,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /Users/makar/dev/ownmail/src-tauri && cargo build`
Expected: builds successfully.

- [ ] **Step 4: Commit**

```bash
cd /Users/makar/dev/ownmail
git add -A
git commit -m "feat(backend): expose connect/account/inbox-preview Tauri commands"
```

---

### Task 8: Minimal frontend (Connect + inbox preview)

**Files:**
- Create: `src/lib/api.ts`
- Modify (overwrite): `src/App.tsx`

- [ ] **Step 1: Create `src/lib/api.ts`**

```ts
import { invoke } from "@tauri-apps/api/core";

export interface MessagePreview {
  id: string;
  from: string;
  subject: string;
  date: string;
  snippet: string;
}

export const connectGmail = () => invoke<string>("connect_gmail");
export const getConnectedAccount = () =>
  invoke<string | null>("get_connected_account");
export const fetchInboxPreview = (max = 20) =>
  invoke<MessagePreview[]>("fetch_inbox_preview", { max });
```

- [ ] **Step 2: Overwrite `src/App.tsx`**

```tsx
import { useEffect, useState } from "react";
import {
  connectGmail,
  fetchInboxPreview,
  getConnectedAccount,
  type MessagePreview,
} from "./lib/api";

export default function App() {
  const [account, setAccount] = useState<string | null>(null);
  const [messages, setMessages] = useState<MessagePreview[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getConnectedAccount()
      .then(setAccount)
      .catch(() => {});
  }, []);

  async function handleConnect() {
    setBusy(true);
    setError(null);
    try {
      setAccount(await connectGmail());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleLoad() {
    setBusy(true);
    setError(null);
    try {
      setMessages(await fetchInboxPreview(20));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <main style={{ fontFamily: "system-ui", padding: 24, maxWidth: 720, margin: "0 auto" }}>
      <h1>Ember — M1</h1>
      {account ? (
        <p>
          Connected as <strong>{account}</strong>
        </p>
      ) : (
        <button onClick={handleConnect} disabled={busy}>
          Connect Gmail
        </button>
      )}
      {account && (
        <button onClick={handleLoad} disabled={busy} style={{ marginLeft: 8 }}>
          Load inbox preview
        </button>
      )}
      {error && <pre style={{ color: "crimson", whiteSpace: "pre-wrap" }}>{error}</pre>}
      <ul style={{ listStyle: "none", padding: 0 }}>
        {messages.map((m) => (
          <li key={m.id} style={{ borderBottom: "1px solid #eee", padding: "10px 0" }}>
            <div style={{ fontWeight: 600 }}>{m.from}</div>
            <div>{m.subject}</div>
            <div style={{ color: "#666", fontSize: 13 }}>{m.snippet}</div>
          </li>
        ))}
      </ul>
    </main>
  );
}
```

- [ ] **Step 3: Verify the frontend type-checks**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: `tsc` passes and Vite builds with no errors.

- [ ] **Step 4: Commit**

```bash
cd /Users/makar/dev/ownmail
git add -A
git commit -m "feat(ui): minimal connect + inbox preview screen"
```

---

### Task 9: End-to-end manual verification

**Files:** none (manual run).

- [ ] **Step 1: Confirm prerequisites are filled in**

Check `src-tauri/.env` has real `EMBER_GOOGLE_CLIENT_ID` and `EMBER_GOOGLE_CLIENT_SECRET`, and your Google address is a Test User on the consent screen.

- [ ] **Step 2: Run the full test suite**

Run: `cd /Users/makar/dev/ownmail/src-tauri && cargo test`
Expected: token, loopback, and gmail tests all PASS.

- [ ] **Step 3: Launch the app**

Run: `cd /Users/makar/dev/ownmail && npm run tauri dev`
Expected: the Ember M1 window opens with a "Connect Gmail" button.

- [ ] **Step 4: Connect**

Click **Connect Gmail**. Your browser opens to Google's consent screen; sign in and approve. The browser tab shows "Ember is connected"; the app now shows "Connected as <your address>".
Expected: no errors; the email is correct. (macOS may prompt once to allow Keychain access — allow it.)

- [ ] **Step 5: Load the inbox preview**

Click **Load inbox preview**.
Expected: a list of your real recent inbox messages (sender, subject, snippet) appears.

- [ ] **Step 6: Confirm token persistence + refresh**

Quit the app and relaunch (`npm run tauri dev`). It should show "Connected as <your address>" immediately (token loaded from Keychain) and "Load inbox preview" should still work (access token auto-refreshes if expired).

- [ ] **Step 7: Tag the milestone**

```bash
cd /Users/makar/dev/ownmail
git tag m1-foundation-auth
```

---

## Self-Review

**1. Spec coverage (M1 slice):** Tauri + React/TS stack ✓ (Tasks 1–8); local/no-backend, talks directly to Google ✓; OAuth 2.0 Desktop loopback ✓ (Task 6); tokens in Keychain ✓ (Task 3); `gmail.modify` scope ✓ (Task 6); test user / no verification ✓ (Prerequisites); a real Gmail fetch ✓ (Tasks 5,7,8). Out-of-M1 spec items (SQLite store, sync engine, scorer, full Ember UI, calendar, snooze/compose) are explicitly deferred to M2–M8 in the roadmap — not gaps.

**2. Placeholder scan:** No TBD/TODO. The only fill-in is the user's own Google client id/secret in `.env` (Task 6 Step 1, Prerequisites) — a real credential, not a code placeholder.

**3. Type consistency:** `StoredToken{email,access_token,refresh_token,expires_at}` defined Task 3, used Tasks 6–7. `MessagePreview{id,from,subject,date,snippet}` defined Task 5 (Rust) and mirrored Task 8 (TS). `GmailClient::new`/`with_base_url`, `get_profile`, `list_inbox_message_ids`, `get_message_preview` consistent across Tasks 5,6,7. Commands `connect_gmail`/`get_connected_account`/`fetch_inbox_preview` consistent across Tasks 7,8. `ensure_access_token`, `GoogleOAuth::{from_env,connect,refresh}`, `PRIMARY_ACCOUNT` consistent across Tasks 6,7. Frontend `invoke` arg `{ max }` matches the Rust command param `max: u32` ✓.

## Notes / risks for the executor

- **oauth2 v5 generics:** clients are built inline (not returned from a helper) to avoid the typed-endpoint generic signatures. Keep it that way.
- **Refresh token:** `access_type=offline` + `prompt=consent` are required for Google to return a refresh token; do not drop them.
- **Keychain in tests:** intentionally not unit-tested (would prompt/half-fail headless). Verified in Task 9.
- **keyring v3** needs the `apple-native` feature on macOS (set in Cargo.toml).
