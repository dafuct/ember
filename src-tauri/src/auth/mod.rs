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
const SCOPE_GMAIL_FULL: &str = "https://mail.google.com/";
const SCOPE_CALENDAR_READONLY: &str = "https://www.googleapis.com/auth/calendar.readonly";
const SCOPE_CALENDAR_EVENTS: &str = "https://www.googleapis.com/auth/calendar.events";
const SCOPE_DIRECTORY_READONLY: &str = "https://www.googleapis.com/auth/directory.readonly";
const SCOPE_CONTACTS_READONLY: &str = "https://www.googleapis.com/auth/contacts.readonly";
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

pub(crate) fn pick_credentials(
    stored: Option<(String, String)>,
    env: Option<(String, String)>,
    baked: Option<(String, String)>,
) -> (Option<(String, String)>, &'static str) {
    for (pair, label) in [(stored, "stored"), (env, "env"), (baked, "baked")] {
        if let Some((id, secret)) = pair {
            if !id.is_empty() && !secret.is_empty() {
                return (Some((id, secret)), label);
            }
        }
    }
    (None, "none")
}

fn pair_from_env() -> Option<(String, String)> {
    Some((
        std::env::var("EMBER_GOOGLE_CLIENT_ID").ok()?,
        std::env::var("EMBER_GOOGLE_CLIENT_SECRET").ok()?,
    ))
}

fn baked_pair() -> Option<(String, String)> {
    Some((
        option_env!("EMBER_GOOGLE_CLIENT_ID")?.to_string(),
        option_env!("EMBER_GOOGLE_CLIENT_SECRET")?.to_string(),
    ))
}

impl GoogleOAuth {
    pub fn resolve() -> Result<Self> {
        let (pair, _) = pick_credentials(
            crate::auth::tokens::load_credentials()?,
            pair_from_env(),
            baked_pair(),
        );
        match pair {
            Some((client_id, client_secret)) => Ok(Self {
                client_id,
                client_secret,
            }),
            None => Err(AppError::Config("no Google credentials configured".into())),
        }
    }

    pub fn credentials_source() -> Result<&'static str> {
        Ok(pick_credentials(
            crate::auth::tokens::load_credentials()?,
            pair_from_env(),
            baked_pair(),
        )
        .1)
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
            .add_scope(Scope::new(SCOPE_GMAIL_FULL.into()))
            .add_scope(Scope::new(SCOPE_CALENDAR_READONLY.into()))
            .add_scope(Scope::new(SCOPE_CALENDAR_EVENTS.into()))
            .add_scope(Scope::new(SCOPE_DIRECTORY_READONLY.into()))
            .add_scope(Scope::new(SCOPE_CONTACTS_READONLY.into()))
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent")
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
        save_token(&stored.email, &stored)?;
        Ok(stored)
    }

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

pub async fn ensure_token_for(account: &str) -> Result<StoredToken> {
    let mut stored = load_token(account)?
        .ok_or_else(|| AppError::Auth(format!("no token for account {account}")))?;
    if stored.is_expired(now_secs(), 60) {
        let oauth = GoogleOAuth::resolve()?;
        let (access, expires_at) = oauth.refresh(&stored.refresh_token).await?;
        stored.access_token = access;
        stored.expires_at = expires_at;
        save_token(account, &stored)?;
    }
    Ok(stored)
}

#[cfg(test)]
mod tests {
    use super::pick_credentials;

    fn p(a: &str, b: &str) -> Option<(String, String)> {
        Some((a.to_string(), b.to_string()))
    }

    #[test]
    fn picks_in_precedence_order_stored_env_baked() {
        assert_eq!(
            pick_credentials(p("sid", "ssec"), p("eid", "esec"), p("bid", "bsec")),
            (p("sid", "ssec"), "stored")
        );
        assert_eq!(
            pick_credentials(None, p("eid", "esec"), p("bid", "bsec")),
            (p("eid", "esec"), "env")
        );
        assert_eq!(
            pick_credentials(None, None, p("bid", "bsec")),
            (p("bid", "bsec"), "baked")
        );
        assert_eq!(pick_credentials(None, None, None), (None, "none"));
        assert_eq!(
            pick_credentials(p("", ""), p("eid", "esec"), None),
            (p("eid", "esec"), "env")
        );
    }
}
