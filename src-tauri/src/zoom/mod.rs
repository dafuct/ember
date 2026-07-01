pub mod client;

pub use client::{ZoomAccount, ZoomClient, ZoomMeeting};

use std::time::{SystemTime, UNIX_EPOCH};

use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge, RedirectUrl,
    RefreshToken, Scope, TokenResponse, TokenUrl,
};

use crate::auth::loopback::Loopback;
use crate::auth::pick_credentials;
use crate::auth::tokens::{load_token, load_zoom_credentials, save_token, StoredToken};
use crate::error::{AppError, Result};

const AUTH_URL: &str = "https://zoom.us/oauth/authorize";
const TOKEN_URL: &str = "https://zoom.us/oauth/token";
// The owner's Zoom OAuth app must grant this scope. Classic apps use "meeting:write";
// granular-scope apps use "meeting:write:meeting". Set to match the registered app.
const SCOPE_MEETING_WRITE: &str = "meeting:write";
pub const ZOOM_ACCOUNT: &str = "zoom";

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn no_redirect_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(AppError::from)
}

fn pair_from_env() -> Option<(String, String)> {
    Some((
        std::env::var("EMBER_ZOOM_CLIENT_ID").ok()?,
        std::env::var("EMBER_ZOOM_CLIENT_SECRET").ok()?,
    ))
}

fn baked_pair() -> Option<(String, String)> {
    Some((
        option_env!("EMBER_ZOOM_CLIENT_ID")?.to_string(),
        option_env!("EMBER_ZOOM_CLIENT_SECRET")?.to_string(),
    ))
}

pub struct ZoomOAuth {
    client_id: String,
    client_secret: String,
}

impl ZoomOAuth {
    pub fn resolve() -> Result<Self> {
        let (pair, _) = pick_credentials(load_zoom_credentials()?, pair_from_env(), baked_pair());
        match pair {
            Some((client_id, client_secret)) => Ok(Self { client_id, client_secret }),
            None => Err(AppError::Config("no Zoom credentials configured".into())),
        }
    }

    pub fn credentials_source() -> Result<&'static str> {
        Ok(pick_credentials(load_zoom_credentials()?, pair_from_env(), baked_pair()).1)
    }

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
            .add_scope(Scope::new(SCOPE_MEETING_WRITE.into()))
            .set_pkce_challenge(pkce_challenge)
            .url();

        open::that(auth_url.to_string())
            .map_err(|e| AppError::Auth(format!("could not open browser: {e}")))?;

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
            .map_err(|e| AppError::Auth(format!("Zoom token exchange failed: {e}")))?;

        let refresh_token = token
            .refresh_token()
            .ok_or_else(|| AppError::Auth("no Zoom refresh token returned".into()))?
            .secret()
            .clone();
        let access_token = token.access_token().secret().clone();
        let expires_in = token.expires_in().map(|d| d.as_secs()).unwrap_or(3600);

        let account = ZoomClient::new(access_token.clone()).get_me().await?;
        let stored = StoredToken {
            email: account.email,
            access_token,
            refresh_token,
            expires_at: now_secs() + expires_in,
        };
        save_token(ZOOM_ACCOUNT, &stored)?;
        Ok(stored)
    }

    /// Returns (access, NEW refresh, expires_at). Zoom rotates the refresh token.
    pub async fn refresh(&self, refresh_token: &str) -> Result<(String, String, u64)> {
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
            .map_err(|e| AppError::Auth(format!("Zoom refresh failed: {e}")))?;

        let access = token.access_token().secret().clone();
        let new_refresh = token
            .refresh_token()
            .map(|r| r.secret().clone())
            .unwrap_or_else(|| refresh_token.to_string());
        let expires_in = token.expires_in().map(|d| d.as_secs()).unwrap_or(3600);
        Ok((access, new_refresh, now_secs() + expires_in))
    }
}

pub async fn ensure_zoom_token() -> Result<StoredToken> {
    let mut stored = load_token(ZOOM_ACCOUNT)?
        .ok_or_else(|| AppError::Auth("Zoom is not connected — connect it in Settings.".into()))?;
    if stored.is_expired(now_secs(), 60) {
        let oauth = ZoomOAuth::resolve()?;
        let (access, new_refresh, expires_at) = oauth.refresh(&stored.refresh_token).await?;
        stored.access_token = access;
        stored.refresh_token = new_refresh; // rotation — persist the new one
        stored.expires_at = expires_at;
        save_token(ZOOM_ACCOUNT, &stored)?;
    }
    Ok(stored)
}

#[cfg(test)]
mod tests {
    use crate::auth::pick_credentials;

    fn p(a: &str, b: &str) -> Option<(String, String)> {
        Some((a.to_string(), b.to_string()))
    }

    #[test]
    fn zoom_credentials_prefer_stored_then_env_then_baked() {
        assert_eq!(pick_credentials(p("s", "s"), p("e", "e"), p("b", "b")).1, "stored");
        assert_eq!(pick_credentials(None, p("e", "e"), p("b", "b")).1, "env");
        assert_eq!(pick_credentials(None, None, p("b", "b")).1, "baked");
        assert_eq!(pick_credentials(None, None, None).1, "none");
    }
}
