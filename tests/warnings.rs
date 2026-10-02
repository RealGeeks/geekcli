//! API `warnings` on successful responses: printed to stderr in every output
//! mode, kept in the JSON on stdout, and fatal only with `--fail-on-warnings`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use mockito::{Mock, Server, ServerGuard};
use serde_json::{json, Value};

const KEY: &str = "rg_live_testkey0000000000000000000000";

struct Env {
    server: ServerGuard,
    config_dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        Self {
            server: Server::new(),
            config_dir: tempfile::tempdir().unwrap(),
        }
    }

    fn cmd(&self) -> Command {
        let mut cmd = Command::cargo_bin("geekcli").unwrap();
        cmd.env_clear()
            .env(
                "SYSTEMROOT",
                std::env::var("SYSTEMROOT").unwrap_or_default(),
            )
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("GEEKCLI_SITE", "www.test.local")
            .env("GEEKCLI_API_KEY", KEY)
            .env("GEEKCLI_BASE_URL", self.server.url())
            .env("GEEKCLI_CONFIG_DIR", self.config_dir.path());
        cmd
    }

    fn patch_post(&mut self, body: &Value) -> Mock {
        self.server
            .mock("GET", "/api/v3/blog/posts/9/")
            .with_body(r#"{"id":9,"title":"Old"}"#)
            .create();
        self.server
            .mock("PATCH", "/api/v3/blog/posts/9/")
            .with_body(body.to_string())
            .create()
    }

    /// Run `posts update 9 --title New` and return (code, stdout, stderr).
    fn update(&self, extra: &[&str], env: Option<(&str, &str)>) -> (i32, String, String) {
        let mut cmd = self.cmd();
        if let Some((k, v)) = env {
            cmd.env(k, v);
        }
        let out = cmd
            .args(["posts", "update", "9", "--title", "New"])
            .args(extra)
            .output()
            .unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap(),
        )
    }
}

fn written_with_warnings() -> Value {
    json!({
        "id": 9,
        "title": "New",
        "warnings": [
            {"code": "sanitized", "field": "body", "removed": ["script", "onclick"],
             "message": "removed <script> and onclick from body"},
            {"code": "unknown_value", "field": "search", "criterion": "city",
             "value": "Miamy", "message": "unknown city 'Miamy'",
             "suggestions": ["Miami", "Miami Beach", "Miami Gardens"]}
        ]
    })
}

#[test]
fn warnings_go_to_stderr_and_stay_in_stdout_json() {
    let mut env = Env::new();
    let mock = env.patch_post(&written_with_warnings());
    let (code, stdout, stderr) = env.update(&[], None);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stderr.contains("warning: removed <script> and onclick from body\n"),
        "{stderr}"
    );
    assert!(
        stderr.contains(
            "warning: unknown city 'Miamy' (did you mean: Miami, Miami Beach, Miami Gardens?)\n"
        ),
        "{stderr}"
    );
    let body: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(body["warnings"], written_with_warnings()["warnings"]);
    mock.assert();
}

#[test]
fn no_warnings_leaves_stderr_clean() {
    for body in [
        json!({"id": 9, "title": "New"}),
        json!({"id": 9, "warnings": []}),
    ] {
        let mut env = Env::new();
        env.patch_post(&body);
        let (code, _, stderr) = env.update(&["--fail-on-warnings"], None);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stderr, "");
    }
}

#[test]
fn fail_on_warnings_exits_5_after_printing_the_result() {
    let mut env = Env::new();
    let mock = env.patch_post(&written_with_warnings());
    let (code, stdout, stderr) = env.update(&["--fail-on-warnings"], None);
    assert_eq!(code, 5, "{stderr}");
    let body: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(body["id"], 9);
    assert!(stderr.contains("warning: removed <script>"), "{stderr}");
    assert!(stderr.contains(r#""code":"warnings""#), "{stderr}");
    assert!(stderr.contains(r#""exit_code":5"#), "{stderr}");
    mock.assert();
}

#[test]
fn fail_on_warnings_env_var() {
    let mut env = Env::new();
    env.patch_post(&written_with_warnings());
    let (code, stdout, _) = env.update(&[], Some(("GEEKCLI_FAIL_ON_WARNINGS", "1")));
    assert_eq!(code, 5);
    assert_eq!(serde_json::from_str::<Value>(&stdout).unwrap()["id"], 9);

    let mut env = Env::new();
    env.patch_post(&written_with_warnings());
    let (code, _, stderr) = env.update(&[], Some(("GEEKCLI_FAIL_ON_WARNINGS", "0")));
    assert_eq!(code, 0);
    assert!(stderr.contains("warning: "), "{stderr}");
}

#[test]
fn unknown_codes_print_their_message_or_json() {
    let mut env = Env::new();
    env.patch_post(&json!({
        "id": 9,
        "warnings": [
            {"code": "something_new", "message": "a warning this CLI has never seen"},
            {"code": "bare", "field": "title"}
        ]
    }));
    let (code, _, stderr) = env.update(&[], None);
    assert_eq!(code, 0);
    assert!(
        stderr.contains("warning: a warning this CLI has never seen\n"),
        "{stderr}"
    );
    assert!(
        stderr.contains(r#"warning: {"code":"bare","field":"title"}"#),
        "{stderr}"
    );
}

#[test]
fn warnings_print_in_table_mode_and_on_reads() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/blog/posts/9/")
        .with_body(
            json!({"id": 9, "title": "T", "warnings": [{"code": "x", "message": "heads up"}]})
                .to_string(),
        )
        .create();
    let out = env
        .cmd()
        .args(["-o", "table", "posts", "get", "9"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("warning: heads up\n"));
}

#[test]
fn warning_text_cannot_send_terminal_escapes() {
    let mut env = Env::new();
    env.patch_post(&json!({
        "id": 9,
        "title": "New",
        "warnings": ["html stripped\u{1b}]0;PWNED\u{7}\u{1b}[2J"],
    }));
    let (code, stdout, stderr) = env.update(&[], None);
    assert_eq!(code, 0);
    assert!(
        !stderr.contains('\u{1b}') && !stderr.contains('\u{7}'),
        "{stderr:?}"
    );
    assert!(
        stderr.contains("warning: html stripped]0;PWNED[2J"),
        "{stderr:?}"
    );
    // the JSON on stdout keeps the original text, escaped by serde
    assert!(stdout.contains("\\u001b"), "{stdout}");
}
