//! Browser-backed checks for `geekcli inspect`; skipped without Chrome.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use assert_cmd::Command;
use geekcli::commands::snapshot::find_browser;
use mockito::Server;
use predicates::prelude::*;
use serde_json::Value;

const PAGE: &str = r#"<!doctype html><html><body>
<h1>Atlanta homes</h1><a class="cta" href="/contact/">Contact us</a>
<div class="hidden" style="display:none">secret</div>
<article class="card">one</article><article class="card">two</article>
</body></html>"#;

fn command(server: &Server, dir: &tempfile::TempDir) -> Command {
    let mut cmd = common::geekcli();
    cmd.env("HOME", std::env::var("HOME").unwrap_or_default())
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
        .expect_at_least(12)
        .create();
    let dir = tempfile::tempdir().unwrap();

    let text = command(&server, &dir)
        .args(["inspect", "/", "--text", "h1", "--wait", "10"])
        .assert()
        .success();
    let text: Value = serde_json::from_slice(&text.get_output().stdout).unwrap();
    assert_eq!(text["operation"], "text");
    assert_eq!(text["value"], "Atlanta homes");

    let html = command(&server, &dir)
        .args(["inspect", "/", "--html", "h1", "--wait", "10"])
        .assert()
        .success();
    let html: Value = serde_json::from_slice(&html.get_output().stdout).unwrap();
    assert_eq!(html["operation"], "html");
    assert_eq!(html["value"], "<h1>Atlanta homes</h1>");

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

    let missing_attr = command(&server, &dir)
        .args([
            "inspect",
            "/",
            "--attr",
            "a.cta",
            "data-none",
            "--wait",
            "10",
        ])
        .assert()
        .success();
    let missing_attr: Value = serde_json::from_slice(&missing_attr.get_output().stdout).unwrap();
    assert_eq!(missing_attr["value"], Value::Null);

    let exists = command(&server, &dir)
        .args(["inspect", "/", "--exists", "h1", "--wait", "10"])
        .assert()
        .success();
    let exists: Value = serde_json::from_slice(&exists.get_output().stdout).unwrap();
    assert_eq!(exists["value"], true);

    let missing_exists = command(&server, &dir)
        .args(["inspect", "/", "--exists", ".missing", "--wait", "10"])
        .assert()
        .success();
    let missing_exists: Value =
        serde_json::from_slice(&missing_exists.get_output().stdout).unwrap();
    assert_eq!(missing_exists["value"], false);

    let visible = command(&server, &dir)
        .args(["inspect", "/", "--visible", "h1", "--wait", "10"])
        .assert()
        .success();
    let visible: Value = serde_json::from_slice(&visible.get_output().stdout).unwrap();
    assert_eq!(visible["value"], true);

    let hidden = command(&server, &dir)
        .args(["inspect", "/", "--visible", ".hidden", "--wait", "10"])
        .assert()
        .success();
    let hidden: Value = serde_json::from_slice(&hidden.get_output().stdout).unwrap();
    assert_eq!(hidden["value"], false);

    let js = command(&server, &dir)
        .args([
            "inspect",
            "/",
            "--js",
            "document.querySelectorAll('.card').length + 1",
            "--wait",
            "10",
        ])
        .assert()
        .success();
    let js: Value = serde_json::from_slice(&js.get_output().stdout).unwrap();
    assert_eq!(js["operation"], "js");
    assert_eq!(js["value"], 3);

    let ok_assert = command(&server, &dir)
        .args([
            "inspect",
            "/",
            "--assert",
            "document.querySelectorAll('.card').length === 2",
            "--mobile",
            "--wait",
            "10",
            "-o",
            "table",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Inspecting"));
    assert!(String::from_utf8_lossy(&ok_assert.get_output().stdout).contains("true"));

    command(&server, &dir)
        .args(["inspect", "/", "--text", ".missing", "--wait", "10"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no element matches selector"));

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
