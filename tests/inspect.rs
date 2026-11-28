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
<svg><text>Logo</text></svg>
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

#[test]
fn inspect_js_returns_objects_arrays_and_null_and_fails_on_exceptions() {
    if find_browser(None).is_err() {
        eprintln!("skipping: no Chrome installed");
        return;
    }
    let mut server = Server::new();
    server
        .mock("GET", "/")
        .with_header("content-type", "text/html")
        .with_body(PAGE)
        .expect_at_least(5)
        .create();
    let dir = tempfile::tempdir().unwrap();

    let obj = command(&server, &dir)
        .args(["inspect", "/", "--js", "({a: 1})", "--wait", "10"])
        .assert()
        .success();
    let obj: Value = serde_json::from_slice(&obj.get_output().stdout).unwrap();
    assert_eq!(obj["value"], serde_json::json!({"a": 1}));

    let arr = command(&server, &dir)
        .args(["inspect", "/", "--js", "[1, 2]", "--wait", "10"])
        .assert()
        .success();
    let arr: Value = serde_json::from_slice(&arr.get_output().stdout).unwrap();
    assert_eq!(arr["value"], serde_json::json!([1, 2]));

    let null = command(&server, &dir)
        .args([
            "inspect",
            "/",
            "--js",
            "document.querySelector('.missing')",
            "--wait",
            "10",
        ])
        .assert()
        .success();
    let null: Value = serde_json::from_slice(&null.get_output().stdout).unwrap();
    assert_eq!(null["value"], Value::Null);

    command(&server, &dir)
        .args(["inspect", "/", "--assert", "throw true", "--wait", "10"])
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("JavaScript error")
                .and(predicate::str::contains("assertion failed").not()),
        );

    command(&server, &dir)
        .args(["inspect", "/", "--count", "div[", "--wait", "10"])
        .assert()
        .code(1)
        // Same wording as --exists/--visible/--text for a bad selector.
        .stderr(
            predicate::str::contains("browser query failed")
                .and(predicate::str::contains("JavaScript error").not()),
        );
}

#[test]
fn inspect_text_reads_svg_elements_without_panicking() {
    if find_browser(None).is_err() {
        eprintln!("skipping: no Chrome installed");
        return;
    }
    let mut server = Server::new();
    server
        .mock("GET", "/")
        .with_header("content-type", "text/html")
        .with_body(PAGE)
        .expect_at_least(1)
        .create();
    let dir = tempfile::tempdir().unwrap();

    let svg = command(&server, &dir)
        .args(["inspect", "/", "--text", "svg", "--wait", "10"])
        .assert()
        .success();
    let svg: Value = serde_json::from_slice(&svg.get_output().stdout).unwrap();
    assert_eq!(svg["value"], "Logo");
}

#[test]
fn inspect_exists_and_visible_fail_on_invalid_selectors() {
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

    let missing_exists = command(&server, &dir)
        .args(["inspect", "/", "--exists", ".missing", "--wait", "10"])
        .assert()
        .success();
    let missing_exists: Value =
        serde_json::from_slice(&missing_exists.get_output().stdout).unwrap();
    assert_eq!(missing_exists["value"], false);

    let missing_visible = command(&server, &dir)
        .args(["inspect", "/", "--visible", ".missing", "--wait", "10"])
        .assert()
        .success();
    let missing_visible: Value =
        serde_json::from_slice(&missing_visible.get_output().stdout).unwrap();
    assert_eq!(missing_visible["value"], false);

    command(&server, &dir)
        .args(["inspect", "/", "--exists", "div[", "--wait", "10"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("browser query failed"));

    command(&server, &dir)
        .args(["inspect", "/", "--visible", "div[", "--wait", "10"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("browser query failed"));
}
