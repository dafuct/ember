use serde::{Serialize, Serializer};

// 🦀 An `enum` in Rust defines a type that can be one of several named variants.
//    Unlike C enums, each Rust variant can carry data (here: a String or a
//    wrapped error type).  This makes enums the idiomatic way to represent
//    "one of these possible error kinds."
//
// 🦀 `#[derive(Debug, thiserror::Error)]` are *derive macros* — they
//    automatically generate trait implementations.  `Debug` lets you print the
//    value with `{:?}`.  `thiserror::Error` generates the `std::error::Error`
//    impl and uses the `#[error("...")]` attributes below to build the
//    human-readable `Display` message for each variant.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("auth error: {0}")]
    Auth(String),

    // 🦀 `#[from] reqwest::Error` tells thiserror to generate a
    //    `From<reqwest::Error> for AppError` implementation automatically.
    //    That means `?` on a `reqwest::Result` inside a function that returns
    //    `Result<_, AppError>` will silently wrap the error in `AppError::Http`.
    //    No manual `map_err` needed.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    // 🦀 Same `#[from]` pattern for keyring errors.
    #[error("keychain error: {0}")]
    Keychain(#[from] keyring::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("{0}")]
    Other(String),
}

// 🦀 Tauri commands must return types that implement `Serialize` (so they can be
//    sent back to the JavaScript frontend as JSON).  The standard library's
//    `std::error::Error` trait does NOT require `Serialize`, so we implement it
//    manually here.  We simply serialize the error as its `Display` string —
//    that's what the frontend will receive when a command returns `Err(...)`.
impl Serialize for AppError {
    // 🦀 We write the fully-qualified `std::result::Result` here because our
    //    own `Result<T>` type alias is in scope and would otherwise shadow it,
    //    causing the return type to resolve to `Result<S::Ok, AppError>` — which
    //    does not match the `Serialize` trait's expected `Result<S::Ok, S::Error>`.
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

// 🦀 A *type alias* — `Result<T>` here is shorthand for
//    `std::result::Result<T, AppError>`.  Defining this at module level means
//    every function in the crate can write `Result<MyType>` instead of spelling
//    out the full `std::result::Result<MyType, AppError>` every time.
pub type Result<T> = std::result::Result<T, AppError>;
