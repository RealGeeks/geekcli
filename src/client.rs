//! A thin blocking HTTP client for `/api/v3/`. It knows the bearer header,
//! the JSON conventions, the error envelope, pagination and 429 retries,
//! and nothing about individual resources.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use reqwest::blocking::{Client as HttpClient, RequestBuilder, Response};
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, RETRY_AFTER, USER_AGENT,
};
use reqwest::redirect::Policy;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

use crate::config::Target;
use crate::error::{ApiError, Error, Result};

pub const USER_AGENT_VALUE: &str = concat!("geekcli/", env!("CARGO_PKG_VERSION"));
const MAX_RETRY_WAIT: u64 = 30;

pub type Query = Vec<(String, String)>;

#[derive(Debug, Clone)]
pub struct Client {
    http: HttpClient,
    /// Used for the browser-login exchange.  A 301/302 normally changes a
    /// POST into a GET, so that flow follows redirects itself and preserves
    /// the method and JSON body.
    no_redirect_http: HttpClient,
    root: String,
    site: String,
    api_key: Option<String>,
    max_retries: u32,
    verbose: bool,
    /// `warning:` lines already printed this run, shared by clones. Used to
    /// skip repeats (every page of a list can carry the same warning) and to
    /// tell `--fail-on-warnings` whether there were any.
    warnings: Arc<Mutex<Vec<String>>>,
}

/// A successful response, decoded.
#[derive(Debug, Clone)]
pub struct ApiResponse {
    pub status: u16,
    pub body: Value,
    pub location: Option<String>,
    /// Response headers, names lower-cased.
    pub headers: HashMap<String, String>,
}

impl ApiResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

impl Client {
    pub fn new(target: &Target, max_retries: u32, verbose: bool) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
        let http = HttpClient::builder()
            .default_headers(headers.clone())
            .timeout(Duration::from_mins(1))
            .build()?;
        let no_redirect_http = HttpClient::builder()
            .default_headers(headers)
            .timeout(Duration::from_mins(1))
            .redirect(Policy::none())
            .build()?;
        Ok(Self {
            http,
            no_redirect_http,
            root: target.api_root(),
            site: target.base_url.trim_end_matches('/').to_string(),
            api_key: target.api_key.clone(),
            max_retries,
            verbose,
            warnings: Arc::default(),
        })
    }

    /// How many distinct API warnings this client has printed.
    pub fn warning_count(&self) -> usize {
        self.warnings.lock().map_or(0, |seen| seen.len())
    }

    /// Decode a response and print any `warnings` it carries to stderr.
    /// Every request goes through here, so every command reports them.
    fn finish(&self, response: Response) -> Result<ApiResponse> {
        let decoded = decode(response)?;
        self.report_warnings(&decoded.body);
        Ok(decoded)
    }

    /// Print `warning: <message>` for each entry of a successful response's
    /// `warnings` array. This is stderr in every output mode: agents run in
    /// JSON mode and need it most. The body itself is left untouched, so the
    /// warnings stay in the JSON on stdout.
    fn report_warnings(&self, body: &Value) {
        let lines = warning_lines(body);
        if lines.is_empty() {
            return;
        }
        let Ok(mut seen) = self.warnings.lock() else {
            return;
        };
        for line in lines {
            if !seen.contains(&line) {
                eprintln!("warning: {line}");
                seen.push(line);
            }
        }
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    pub fn has_key(&self) -> bool {
        self.api_key.is_some()
    }

    /// The site origin, for endpoints outside `/api/v3/`.
    pub fn site_url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            return path.to_string();
        }
        format!("{}/{}", self.site, path.trim_start_matches('/'))
    }

    /// GET a site-level (unauthenticated) endpoint such as the legacy
    /// `/api/v2/search/`.
    pub fn site_get(&self, path: &str, query: &Query) -> Result<ApiResponse> {
        self.request(&Method::GET, &self.site_url(path), query, None, false)
    }

    /// POST to a site-level endpoint with criteria in the query string.
    pub fn site_post(&self, path: &str, query: &Query) -> Result<ApiResponse> {
        self.request(&Method::POST, &self.site_url(path), query, None, false)
    }

    /// POST a multipart form (file uploads) to an `/api/v3/` path.
    pub fn post_multipart(
        &self,
        path: &str,
        form: reqwest::blocking::multipart::Form,
    ) -> Result<ApiResponse> {
        let Some(key) = &self.api_key else {
            return Err(Error::NotLoggedIn(
                "no API key for this site. Run `geekcli auth login --site <domain>` or set GEEKCLI_API_KEY".into(),
            ));
        };
        let url = self.url(path);
        if self.verbose {
            eprintln!("> POST {url} (multipart)");
        }
        let response = self
            .http
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {key}"))
            .multipart(form)
            .send()?;
        if self.verbose {
            eprintln!("< {}", response.status());
        }
        self.finish(response)
    }

    /// Resolve a path relative to `/api/v3/`. Absolute `/api/v3/...` paths
    /// (as returned in pagination links) are accepted too.
    pub fn url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            return path.to_string();
        }
        if let Some(rest) = path.strip_prefix("/api/v3/") {
            return format!("{}{}", self.root, rest);
        }
        format!("{}{}", self.root, path.trim_start_matches('/'))
    }

    pub fn get(&self, path: &str, query: &Query) -> Result<ApiResponse> {
        self.request(&Method::GET, path, query, None, true)
    }

    pub fn post(&self, path: &str, body: &Value) -> Result<ApiResponse> {
        self.request(&Method::POST, path, &Query::new(), Some(body), true)
    }

    /// POST without authentication while preserving the method and body over
    /// canonical-host redirects. This is needed by browser login: many sites
    /// redirect `example.com` to `www.example.com` with a 301, and ordinary
    /// HTTP clients turn that POST into a GET.
    pub fn anonymous_post_preserving_redirects(
        &self,
        path: &str,
        body: &Value,
    ) -> Result<ApiResponse> {
        const MAX_REDIRECTS: usize = 5;
        let mut url = self.url(path);

        for _ in 0..=MAX_REDIRECTS {
            if self.verbose {
                eprintln!("> POST {url}");
            }
            let response = self
                .build_with(
                    &self.no_redirect_http,
                    &Method::POST,
                    &url,
                    &Query::new(),
                    Some(body),
                    false,
                )
                .send()?;
            let status = response.status();
            if self.verbose {
                eprintln!("< {status}");
            }
            if !status.is_redirection() {
                return self.finish(response);
            }

            let Some(location) = response.headers().get("location") else {
                return self.finish(response);
            };
            let location = location.to_str().map_err(|_| {
                Error::Other("login redirect had an invalid Location header".into())
            })?;
            let next = response
                .url()
                .join(location)
                .map_err(|e| Error::Other(format!("login redirect had an invalid URL: {e}")))?;
            if !is_canonical_redirect(response.url(), &next) {
                return Err(Error::Other(format!(
                    "the site redirected login to {next}, which is not the same site; pass the site's canonical domain to --site"
                )));
            }
            if self.verbose {
                eprintln!("> following login redirect with POST to {next}");
            }
            url = next.into();
        }

        Err(Error::Other(
            "too many redirects while starting browser approval; pass the site's canonical domain to --site"
                .into(),
        ))
    }

    pub fn put(&self, path: &str, body: &Value) -> Result<ApiResponse> {
        self.request(&Method::PUT, path, &Query::new(), Some(body), true)
    }

    pub fn patch(&self, path: &str, body: &Value) -> Result<ApiResponse> {
        self.request(&Method::PATCH, path, &Query::new(), Some(body), true)
    }

    pub fn delete(&self, path: &str, query: &Query) -> Result<ApiResponse> {
        self.request(&Method::DELETE, path, query, None, true)
    }

    /// Fetch every page of a list endpoint and concatenate `results`.
    pub fn get_all(&self, path: &str, query: &Query) -> Result<Vec<Value>> {
        let mut query = query.clone();
        query.retain(|(k, _)| k != "page");
        if !query.iter().any(|(k, _)| k == "page_size") {
            query.push(("page_size".into(), "100".into()));
        }
        let mut results = Vec::new();
        let mut next: Option<String> = Some(self.url(path));
        let mut first = true;
        while let Some(url) = next.take() {
            let response = if first {
                first = false;
                self.get(&url, &query)?
            } else {
                self.get(&url, &Query::new())?
            };
            if let Some(items) = response.body.get("results").and_then(Value::as_array) {
                results.extend(items.iter().cloned());
            }
            next = response
                .body
                .pointer("/pagination/next")
                .and_then(Value::as_str)
                .map(|n| self.url(n));
        }
        Ok(results)
    }

    /// Perform a request. `auth` is false only for the login exchange.
    pub fn request(
        &self,
        method: &Method,
        path: &str,
        query: &Query,
        body: Option<&Value>,
        auth: bool,
    ) -> Result<ApiResponse> {
        let url = self.url(path);
        if auth && self.api_key.is_none() {
            return Err(Error::NotLoggedIn(
                "no API key for this site. Run `geekcli auth login --site <domain>` or set GEEKCLI_API_KEY"
                    .into(),
            ));
        }
        let mut attempt = 0;
        loop {
            let request = self.build(method, &url, query, body, auth);
            if self.verbose {
                eprintln!("> {method} {url}");
            }
            let response = request.send()?;
            let status = response.status();
            if self.verbose {
                eprintln!("< {status}");
            }
            if status == StatusCode::TOO_MANY_REQUESTS && attempt < self.max_retries {
                let wait = retry_after(&response).unwrap_or(5).min(MAX_RETRY_WAIT);
                if self.verbose {
                    eprintln!("< rate limited, retrying in {wait}s");
                }
                thread::sleep(Duration::from_secs(wait));
                attempt += 1;
                continue;
            }
            return self.finish(response);
        }
    }

    fn build(
        &self,
        method: &Method,
        url: &str,
        query: &Query,
        body: Option<&Value>,
        auth: bool,
    ) -> RequestBuilder {
        self.build_with(&self.http, method, url, query, body, auth)
    }

    fn build_with(
        &self,
        http: &HttpClient,
        method: &Method,
        url: &str,
        query: &Query,
        body: Option<&Value>,
        auth: bool,
    ) -> RequestBuilder {
        let mut request = http.request(method.clone(), url).query(query);
        if auth {
            if let Some(key) = &self.api_key {
                request = request.header(AUTHORIZATION, format!("Bearer {key}"));
            }
        }
        if let Some(body) = body {
            request = request.header(CONTENT_TYPE, "application/json").json(body);
        }
        request
    }
}

/// Whether a login POST may follow a redirect from `from` to `to`. The body
/// carries a one-time code and PKCE verifier, so it only goes to the same host
/// or its `www.`/apex twin, and never from https down to http.
fn is_canonical_redirect(from: &reqwest::Url, to: &reqwest::Url) -> bool {
    let (Some(a), Some(b)) = (from.host_str(), to.host_str()) else {
        return false;
    };
    let same_site =
        a == b || a.strip_prefix("www.") == Some(b) || b.strip_prefix("www.") == Some(a);
    let same_scheme = from.scheme() == to.scheme();
    let upgrade = from.scheme() == "http" && to.scheme() == "https";
    same_site && (upgrade || (same_scheme && from.port() == to.port()))
}

fn retry_after(response: &Response) -> Option<u64> {
    response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok())
}

fn decode(response: Response) -> Result<ApiResponse> {
    let status = response.status();
    let location = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let retry = retry_after(&response);
    let headers: HashMap<String, String> = response
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|v| (k.as_str().to_ascii_lowercase(), v.to_string()))
        })
        .collect();
    let text = response.text()?;

    if status.is_success() {
        let body = if text.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text }))
        };
        return Ok(ApiResponse {
            status: status.as_u16(),
            body,
            location,
            headers,
        });
    }

    Err(Error::api(parse_error(status.as_u16(), &text, retry)))
}

/// The text of each entry in a response's `warnings` array: its `message`,
/// or the entry as JSON when it has none. `unknown_value` and
/// `ambiguous_value` get their `suggestions` appended unless the message
/// already names them all.
pub fn warning_lines(body: &Value) -> Vec<String> {
    let Some(items) = body.get("warnings").and_then(Value::as_array) else {
        return Vec::new();
    };
    items.iter().map(warning_line).collect()
}

fn warning_line(warning: &Value) -> String {
    let text = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_string);
    if let Value::String(message) = warning {
        return message.clone();
    }
    let Some(message) = warning.get("message").and_then(Value::as_str) else {
        return warning.to_string();
    };
    let code = warning.get("code").and_then(Value::as_str).unwrap_or("");
    let suggestions: Vec<String> = warning
        .get("suggestions")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(text).collect())
        .unwrap_or_default();
    if matches!(code, "unknown_value" | "ambiguous_value")
        && !suggestions.is_empty()
        && !suggestions.iter().all(|s| message.contains(s.as_str()))
    {
        return format!("{message} (did you mean: {}?)", suggestions.join(", "));
    }
    message.to_string()
}

/// Turn an error response into `ApiError`, tolerating non-JSON bodies
/// (a proxy 502, an HTML 404 from a site that has no API).
pub fn parse_error(status: u16, text: &str, retry_after: Option<u64>) -> ApiError {
    let parsed: Option<Value> = serde_json::from_str(text).ok();
    let inner = parsed.as_ref().and_then(|v| v.get("error"));
    if let Some(Value::String(text)) = inner {
        // Legacy `/api/v2/` shape: {"error": "message"}
        return ApiError {
            status,
            code: default_code(status),
            message: text.clone(),
            fields: BTreeMap::new(),
            retry_after,
        };
    }
    let code = inner
        .and_then(|e| e.get("code"))
        .and_then(Value::as_str)
        .map_or_else(|| default_code(status), str::to_string);
    let message = inner
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .map_or_else(|| default_message(status, text), str::to_string);
    let mut fields = BTreeMap::new();
    if let Some(map) = inner
        .and_then(|e| e.get("fields"))
        .and_then(Value::as_object)
    {
        for (name, errors) in map {
            let list: Vec<String> = match errors {
                Value::Array(items) => items
                    .iter()
                    .map(|i| i.as_str().map_or_else(|| i.to_string(), str::to_string))
                    .collect(),
                Value::String(s) => vec![s.clone()],
                other => vec![other.to_string()],
            };
            fields.insert(name.clone(), list);
        }
    }
    ApiError {
        status,
        code,
        message,
        fields,
        retry_after,
    }
}

fn default_code(status: u16) -> String {
    match status {
        401 => "unauthorized",
        403 => "forbidden",
        404 => "not_found",
        405 => "method_not_allowed",
        409 => "conflict",
        429 => "rate_limited",
        500..=599 => "server_error",
        _ => "http_error",
    }
    .to_string()
}

fn default_message(status: u16, text: &str) -> String {
    let snippet: String = text.trim().chars().take(200).collect();
    if snippet.is_empty() || snippet.starts_with('<') {
        format!(
            "HTTP {status} (no JSON error body; is this a Real Geeks site with the API enabled?)"
        )
    } else {
        format!("HTTP {status}: {snippet}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_redirects_stay_on_the_site() {
        let url = |s: &str| reqwest::Url::parse(s).ok();
        let allowed = |from: &str, to: &str| match (url(from), url(to)) {
            (Some(from), Some(to)) => is_canonical_redirect(&from, &to),
            _ => false,
        };
        assert!(allowed(
            "https://example.com/a",
            "https://www.example.com/a"
        ));
        assert!(allowed(
            "https://www.example.com/a",
            "https://example.com/b"
        ));
        assert!(allowed(
            "http://www.example.com/a",
            "https://www.example.com/a"
        ));
        assert!(allowed(
            "http://127.0.0.1:8000/a",
            "http://127.0.0.1:8000/b"
        ));

        assert!(!allowed(
            "https://www.example.com/a",
            "http://www.example.com/a"
        ));
        assert!(!allowed(
            "https://www.example.com/a",
            "https://evil.example.net/a"
        ));
        assert!(!allowed(
            "https://example.com/a",
            "https://www.example.com.evil.net/a"
        ));
        assert!(!allowed(
            "https://www.example.com/a",
            "https://api.example.com/a"
        ));
        assert!(!allowed(
            "http://127.0.0.1:8000/a",
            "http://127.0.0.1:9000/a"
        ));
    }

    #[test]
    fn parses_envelope() {
        let err = parse_error(
            422,
            r#"{"error":{"code":"validation_error","message":"bad","fields":{"slug":["taken"]}}}"#,
            None,
        );
        assert_eq!(err.code, "validation_error");
        assert_eq!(err.fields.get("slug"), Some(&vec!["taken".to_string()]));
        assert_eq!(err.exit_code(), crate::error::exit::VALIDATION);
    }

    #[test]
    fn warning_lines_cover_known_and_unknown_codes() {
        let body = json!({"id": 1, "warnings": [
            {"code": "sanitized", "field": "body", "removed": ["script"],
             "message": "removed <script> from body"},
            {"code": "unknown_value", "field": "search", "criterion": "city",
             "value": "Miamy", "message": "unknown city 'Miamy'",
             "suggestions": ["Miami", "Miami Beach"]},
            {"code": "ambiguous_value", "message": "did you mean Miami or Miami Beach?",
             "suggestions": ["Miami", "Miami Beach"]},
            {"code": "brand_new", "message": "something new"},
            {"code": "no_message", "field": "x"},
        ]});
        assert_eq!(
            warning_lines(&body),
            vec![
                "removed <script> from body".to_string(),
                "unknown city 'Miamy' (did you mean: Miami, Miami Beach?)".to_string(),
                "did you mean Miami or Miami Beach?".to_string(),
                "something new".to_string(),
                r#"{"code":"no_message","field":"x"}"#.to_string(),
            ]
        );
        assert!(warning_lines(&json!({"id": 1})).is_empty());
        assert!(warning_lines(&json!({"warnings": []})).is_empty());
        assert!(warning_lines(&json!([1, 2])).is_empty());
    }

    #[test]
    fn tolerates_html_bodies() {
        let err = parse_error(502, "<html>bad gateway</html>", None);
        assert_eq!(err.code, "server_error");
        assert!(err.message.contains("HTTP 502"));
    }

    #[test]
    fn resolves_paths() {
        let target = Target {
            domain: "x.com".into(),
            base_url: "https://x.com".into(),
            api_key: None,
        };
        let Ok(client) = Client::new(&target, 0, false) else {
            return;
        };
        assert_eq!(
            client.url("blog/posts/"),
            "https://x.com/api/v3/blog/posts/"
        );
        assert_eq!(
            client.url("/api/v3/blog/posts/?page=2"),
            "https://x.com/api/v3/blog/posts/?page=2"
        );
        assert_eq!(client.url("https://y.com/z"), "https://y.com/z");
    }
}
