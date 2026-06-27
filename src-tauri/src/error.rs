use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("auth error: {0}")]
    Auth(String),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("keychain error: {0}")]
    Keychain(#[from] keyring::Error),

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("{0}")]
    Other(String),
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
