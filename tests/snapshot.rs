//! `geekcli snapshot`. The real capture runs only when a Chrome-family
//! browser is installed; without one the test is skipped, not failed.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use geekcli::commands::snapshot::find_browser;
use mockito::Server;
use predicates::prelude::*;
use serde_json::Value;

const PAGE: &str = r#"<!doctype html><html><head><meta name="viewport" content="width=device-width"><style>
body{margin:0;font:16px sans-serif} .hero{height:100vh;background:#123} .rest{height:1600px;background:#eee}
 .component{box-sizing:border-box;width:400px;height:120px;border:10px solid #c00;background:#eee}
</style></head><body><div class="hero"></div><div class="rest">below the fold</div><div class="component">component</div></body></html>"#;

#[test]
fn full_page_capture_is_taller_than_the_viewport() {
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
    let out = dir.path().join("full.png");
    let mut cmd = Command::cargo_bin("geekcli").unwrap();
    let assert = cmd
        .env_clear()
        .env(
            "SYSTEMROOT",
            std::env::var("SYSTEMROOT").unwrap_or_default(),
        )
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env("GEEKCLI_SITE", "www.test.local")
        .env("GEEKCLI_BASE_URL", server.url())
        .env("GEEKCLI_CONFIG_DIR", dir.path())
        .args([
            "snapshot", "/", "--full", "--width", "800", "--height", "600", "--wait", "200",
            "--out",
        ])
        .arg(&out)
        .assert()
        .success();
    let doc: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(doc["full"], true);
    let png = std::fs::read(&out).unwrap();
    assert_eq!(&png[1..4], b"PNG");
    // IHDR height (bytes 20..24, big endian) must exceed the 600px viewport
    let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    assert!(height > 600, "full page capture was only {height}px tall");
}

#[test]
fn selector_capture_is_the_matched_element() {
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
    let out = dir.path().join("component.png");
    let mut cmd = Command::cargo_bin("geekcli").unwrap();
    let assert = cmd
        .env_clear()
        .env(
            "SYSTEMROOT",
            std::env::var("SYSTEMROOT").unwrap_or_default(),
        )
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env("GEEKCLI_SITE", "www.test.local")
        .env("GEEKCLI_BASE_URL", server.url())
        .env("GEEKCLI_CONFIG_DIR", dir.path())
        .args([
            "snapshot",
            "/",
            "--selector",
            ".component",
            "--wait",
            "10",
            "--out",
        ])
        .arg(&out)
        .assert()
        .success();
    let doc: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(doc["selector"], ".component");
    let png = std::fs::read(&out).unwrap();
    let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    assert_eq!((width, height), (400, 120));
}

#[test]
fn missing_browser_is_a_clear_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("geekcli").unwrap();
    cmd.env_clear()
        .env(
            "SYSTEMROOT",
            std::env::var("SYSTEMROOT").unwrap_or_default(),
        )
        .env("PATH", "/nonexistent")
        .env("GEEKCLI_SITE", "www.test.local")
        .env("GEEKCLI_CONFIG_DIR", dir.path())
        .args(["snapshot", "--browser", "/nonexistent/chrome"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("browser not found"));
}
