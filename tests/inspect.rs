//! Browser-backed checks for `geekcli inspect`; skipped without Chrome.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use geekcli::commands::snapshot::find_browser;
use mockito::Server;
use predicates::prelude::*;
use serde_json::Value;

const PAGE: &str = r#"<!doctype html><html><body>
<h1>Atlanta homes</h1><a class="cta" href="/contact/">Contact us</a>
<article class="card">one</article><article class="card">two</article>
</body></html>"#;

fn command(server: &Server, dir: &tempfile::TempDir) -> Command {
    let mut cmd = Command::cargo_bin("geekcli").unwrap();
    cmd.env_clear()
        .env(
            "SYSTEMROOT",
            std::env::var("SYSTEMROOT").unwrap_or_default(),
        )
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env("GEEKCLI_SITE", "www.test.local")
        .env("GEEKCLI_BASE_URL", server.url())
        .env("GEEKCLI_CONFIG_DIR", dir.path());
    cmd
}

#[test]
fn inspect_returns_rendered_dom_values() {
    if find_browser(None).is_err() {
        eprintln!("skipping: no Chrome installed");
        return;
    }
    let mut server = Server::new();
    server
        .mock("GET", "/")
        .with_header("content-type", "text/html")
        .with_body(PAGE)
        .expect_at_least(4)
        .create();
    let dir = tempfile::tempdir().unwrap();

    let text = command(&server, &dir)
        .args(["inspect", "/", "--text", "h1", "--wait", "10"])
        .assert()
        .success();
    let text: Value = serde_json::from_slice(&text.get_output().stdout).unwrap();
    assert_eq!(text["operation"], "text");
    assert_eq!(text["value"], "Atlanta homes");

    let count = command(&server, &dir)
        .args(["inspect", "/", "--count", ".card", "--wait", "10"])
        .assert()
        .success();
    let count: Value = serde_json::from_slice(&count.get_output().stdout).unwrap();
    assert_eq!(count["value"], 2);

    let attr = command(&server, &dir)
        .args(["inspect", "/", "--attr", "a.cta", "href", "--wait", "10"])
        .assert()
        .success();
    let attr: Value = serde_json::from_slice(&attr.get_output().stdout).unwrap();
    assert_eq!(attr["value"], "/contact/");

    command(&server, &dir)
        .args([
            "inspect",
            "/",
            "--assert",
            "document.querySelectorAll('.card').length === 1",
            "--wait",
            "10",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("assertion failed"));
}
