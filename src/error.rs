//! One error type for the whole CLI, mapped onto stable exit codes so an
//! agent can branch on the outcome without parsing text.

use std::collections::BTreeMap;

use serde::Serialize;

/// Exit codes. Documented in `geekcli guide` and the README; keep them stable.
pub mod exit {
    pub const OK: i32 = 0;
    pub const GENERAL: i32 = 1;
    pub const USAGE: i32 = 2;
    pub const AUTH: i32 = 3;
    pub const NOT_FOUND: i32 = 4;
    pub const VALIDATION: i32 = 5;
    pub const CONFLICT: i32 = 6;
    pub const RATE_LIMITED: i32 = 7;
    pub const NETWORK: i32 = 8;
}

/// The API's error envelope: `{"error": {"code", "message", "fields"}}`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
}

impl ApiError {
    pub fn exit_code(&self) -> i32 {
        match self.status {
            401 | 403 => exit::AUTH,
            404 => exit::NOT_FOUND,
            400 | 413 | 415 | 422 => exit::VALIDATION,
            409 => exit::CONFLICT,
            429 => exit::RATE_LIMITED,
            _ => exit::GENERAL,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Usage(String),

    #[error("{0}")]
    Config(String),

    #[error("not logged in: {0}")]
    NotLoggedIn(String),

    #[error("{code}: {message}")]
    Api {
        #[allow(dead_code)]
        status: u16,
        code: String,
        message: String,
        fields: BTreeMap<String, Vec<String>>,
        retry_after: Option<u64>,
    },

    #[error("network error: {0}")]
    Network(String),

    #[error("{0}")]
    Io(String),

    #[error("{0}")]
    Other(String),
}

impl Error {
    /// What to do about it, for the errors where the message alone is not enough.
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Error::NotLoggedIn(_) => Some("run `geekcli auth login --site <domain>`"),
            Error::Api { code, status, .. } => match (code.as_str(), *status) {
                ("token_expired", _) => Some(
                    "the key has expired (keys live at most six months); run `geekcli auth login --site <domain>` for a new one",
                ),
                ("api_disabled", _) => Some(
                    "the content API is switched on per site by Real Geeks; ask support to enable it",
                ),
                ("crm_unavailable" | "design_catalogue_unavailable" | "files_unavailable", _) => {
                    Some("a service behind the site is down; retry in a minute")
                }
                (_, 413) => Some(
                    "the request body is too large; upload files with `files upload` and keep JSON bodies small",
                ),
                ("conflict", 409) => Some(
                    "if two writes raced on the same slug or name, retry; otherwise see the message for the flag that overrides the guard",
                ),
                (_, 405) => Some(
                    "the API received the wrong HTTP method. For browser login, check the site's canonical domain (for example, use `www.` if the site redirects there) and retry",
                ),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Usage(_) => exit::USAGE,
            Error::Config(_) | Error::NotLoggedIn(_) => exit::AUTH,
            Error::Api { status, .. } => api_exit_code(*status),
            Error::Network(_) => exit::NETWORK,
            Error::Io(_) | Error::Other(_) => exit::GENERAL,
        }
    }

    /// A short machine-readable code for the JSON error output.
    pub fn code(&self) -> &str {
        match self {
            Error::Usage(_) => "usage",
            Error::Config(_) => "config",
            Error::NotLoggedIn(_) => "not_logged_in",
            Error::Api { code, .. } => code,
            Error::Network(_) => "network",
            Error::Io(_) => "io",
            Error::Other(_) => "error",
        }
    }

    pub fn api(err: ApiError) -> Self {
        Error::Api {
            status: err.status,
            code: err.code,
            message: err.message,
            fields: err.fields,
            retry_after: err.retry_after,
        }
    }

    /// The message without the code prefix that `Display` adds.
    pub fn message(&self) -> String {
        match self {
            Error::Api { message, .. } => message.clone(),
            other => other.to_string(),
        }
    }

    pub fn fields(&self) -> Option<&BTreeMap<String, Vec<String>>> {
        match self {
            Error::Api { fields, .. } if !fields.is_empty() => Some(fields),
            _ => None,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Api { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn retry_after(&self) -> Option<u64> {
        match self {
            Error::Api { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

fn api_exit_code(status: u16) -> i32 {
    ApiError {
        status,
        code: String::new(),
        message: String::new(),
        fields: BTreeMap::new(),
        retry_after: None,
    }
    .exit_code()
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Usage(format!("invalid JSON: {err}"))
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        // Strip the URL so a key in a query string never leaks into logs.
        Error::Network(err.without_url().to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    fn api(status: u16, code: &str) -> Error {
        Error::Api {
            status,
            code: code.into(),
            message: String::new(),
            fields: std::collections::BTreeMap::new(),
            retry_after: None,
        }
    }

    #[test]
    fn hints_name_the_next_step() {
        assert!(api(401, "token_expired")
            .hint()
            .is_some_and(|h| h.contains("auth login")));
        assert!(api(403, "api_disabled").hint().is_some());
        assert!(api(502, "crm_unavailable").hint().is_some());
        assert!(api(413, "payload_too_large").hint().is_some());
        assert!(api(405, "method_not_allowed")
            .hint()
            .is_some_and(|hint| hint.contains("canonical domain")));
        assert!(api(404, "not_found").hint().is_none());
        assert!(api(422, "validation_error").hint().is_none());
    }
}
