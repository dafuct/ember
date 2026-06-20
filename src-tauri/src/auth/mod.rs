pub mod loopback;
pub mod tokens;

// 🦀 `use std::time::{SystemTime, UNIX_EPOCH}` imports two items from the standard
//    library's `time` module.  `SystemTime::now()` returns the current wall-clock
//    time; `UNIX_EPOCH` is the constant reference point (1970-01-01 00:00 UTC).
//    `duration_since(UNIX_EPOCH)` subtracts the epoch from now, giving elapsed
//    time as a `Duration` — then `.as_secs()` converts that to a plain `u64`.
use std::time::{SystemTime, UNIX_EPOCH};

use oauth2::basic::BasicClient;
// 🦀 `use oauth2::TokenResponse;` imports the *trait* that defines helper methods
//    `.access_token()`, `.refresh_token()`, and `.expires_in()` on token objects.
//    Traits in Rust are like interfaces: the methods only resolve at call sites
//    where the trait is in scope.  Without this import the compiler would error
//    with "method not found" even though the type implements the trait.
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
// 🦀 A second OAuth scope. Adding it here means the next `connect()` requests BOTH scopes;
//    because connect() always sends `prompt=consent`, Google re-prompts and grants the new
//    scope — no migration needed for a user who reconnects.
const SCOPE_CALENDAR_READONLY: &str = "https://www.googleapis.com/auth/calendar.readonly";
const SCOPE_CALENDAR_EVENTS: &str = "https://www.googleapis.com/auth/calendar.events";
pub const PRIMARY_ACCOUNT: &str = "primary";

// 🦀 `SystemTime::now()` returns the current wall-clock time.
//    `.duration_since(UNIX_EPOCH)` returns `Result<Duration, SystemTimeError>` —
//    it can fail only if the system clock is set before 1970 (highly unlikely).
//    `.map(|d| d.as_secs())` transforms the `Ok(Duration)` into `Ok(u64)` via a
//    closure; if it's `Err`, the `.unwrap_or(0)` provides a safe fallback of 0.
//    Returning a tuple `(String, u64)` from `refresh()` packages two related
//    values together without needing a named struct — Rust tuples are lightweight
//    anonymous product types, accessed by index (`.0`, `.1`).
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
        // 🦀 `std::env::var("KEY")` returns `Result<String, VarError>` — `Ok(value)`
        //    if the environment variable is set, or `Err(VarError::NotPresent)` if not.
        //    `.map_err(|_| AppError::Config(...))` transforms the error type: the closure
        //    receives the `VarError` (we ignore it with `_`) and returns a domain-specific
        //    `AppError::Config`.  Then `?` propagates early if still `Err`.
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

        // 🦀 The `oauth2` crate uses a *builder pattern*: `BasicClient::new(...)`
        //    returns a partially-configured value, and each `.set_*()` call returns
        //    a new (or mutated) value with that field set.  The chain ends when you
        //    call `.authorize_url(...)` or `.exchange_code(...)`.  This enforces at
        //    the type level that required fields (auth_uri, token_uri) are set before
        //    use — missing them would be a compile error.
        //
        // 🦀 PKCE (Proof Key for Code Exchange): a security extension for OAuth 2.0
        //    that prevents authorization-code interception attacks.  The client generates
        //    a random `code_verifier`, sends a SHA-256 hash of it (`code_challenge`) with
        //    the auth request, then sends the original `code_verifier` when exchanging the
        //    code — the server verifies they match, so a stolen code is useless without
        //    the verifier.
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
            .add_scope(Scope::new(SCOPE_CALENDAR_READONLY.into()))
            .add_scope(Scope::new(SCOPE_CALENDAR_EVENTS.into()))
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent")
            .set_pkce_challenge(pkce_challenge)
            .url();

        open::that(auth_url.to_string())
            .map_err(|e| AppError::Auth(format!("could not open browser: {e}")))?;

        // 🦀 `tokio::task::spawn_blocking` offloads a *blocking* call onto a dedicated
        //    thread pool so it does not starve the async runtime's thread(s).  The async
        //    runtime uses a small number of threads for polling futures; a blocking
        //    `TcpListener::accept()` would occupy one of those threads indefinitely,
        //    preventing other tasks from running.  `spawn_blocking` solves this by
        //    running the closure on a separate blocking-safe thread.
        //
        // 🦀 The `move` keyword on the closure *moves* captured variables (here `loopback`)
        //    into the closure's environment.  Without `move`, the closure would borrow
        //    `loopback` by reference, but references cannot be sent across thread boundaries
        //    (`'static` requirement) — `move` transfers ownership instead.
        let (code, state) = tokio::task::spawn_blocking(move || loopback.wait_for_code())
            .await
            .map_err(|e| AppError::Auth(e.to_string()))??;

        // 🦀 `*csrf.secret()` — `csrf` is a `CsrfToken` (a wrapper/smart pointer).
        //    `.secret()` returns a `&String` (a reference to the inner value).
        //    The `*` dereferences it to get a `String` for comparison with `state`
        //    (which is also a `String`).  We compare the state parameter Google echoed
        //    back with the one we generated to detect CSRF: a malicious redirect would
        //    carry a different state value that wouldn't match.
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

        // 🦀 `use oauth2::TokenResponse` (imported above) enables these methods:
        //    `.access_token()` → `&AccessToken`; `.secret()` unwraps to `&String`.
        //    `.refresh_token()` → `Option<&RefreshToken>` (not always returned).
        //    `.expires_in()` → `Option<Duration>` (seconds until expiry).
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
    // 🦀 `let mut stored` — the `mut` keyword makes the binding *mutable*, allowing
    //    us to reassign fields on `stored` later (access_token, expires_at) when we
    //    refresh.  Without `mut` Rust would reject any field assignment as a
    //    compile error: "cannot assign to `stored.access_token`, as `stored` is not
    //    declared as mutable."
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
