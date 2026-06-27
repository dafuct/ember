use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

const KEYCHAIN_SERVICE: &str = "dev.ember.oauth";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredToken {
    pub email: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
}

impl StoredToken {
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

pub fn delete_token(account: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

const CLIENT_CREDS_KEY: &str = "__google_client__";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredCredentials {
    client_id: String,
    client_secret: String,
}

pub fn save_credentials(client_id: &str, client_secret: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, CLIENT_CREDS_KEY)?;
    let creds = StoredCredentials {
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
    };
    let json = serde_json::to_string(&creds).map_err(|e| AppError::Other(e.to_string()))?;
    entry.set_password(&json)?;
    Ok(())
}

pub fn load_credentials() -> Result<Option<(String, String)>> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, CLIENT_CREDS_KEY)?;
    match entry.get_password() {
        Ok(json) => {
            let c: StoredCredentials =
                serde_json::from_str(&json).map_err(|e| AppError::Other(e.to_string()))?;
            Ok(Some((c.client_id, c.client_secret)))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn delete_credentials() -> Result<()> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, CLIENT_CREDS_KEY)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
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
        assert!(t.is_expired(950, 60));
        assert!(!t.is_expired(900, 60));
    }

    #[test]
    fn serde_round_trips() {
        let t = tok(1234);
        let json = serde_json::to_string(&t).unwrap();
        let back: StoredToken = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }
}
