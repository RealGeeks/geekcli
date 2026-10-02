//! `geekcli files …` against a mock site.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use mockito::{Matcher, Server, ServerGuard};
use predicates::prelude::*;
use serde_json::{json, Value};

fn cmd(server: &ServerGuard, dir: &tempfile::TempDir) -> Command {
    let mut c = Command::cargo_bin("geekcli").unwrap();
    c.env_clear()
        .env(
            "SYSTEMROOT",
            std::env::var("SYSTEMROOT").unwrap_or_default(),
        )
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("GEEKCLI_SITE", "www.test.local")
        .env("GEEKCLI_API_KEY", "rg_live_k")
        .env("GEEKCLI_BASE_URL", server.url())
        .env("GEEKCLI_CONFIG_DIR", dir.path());
    c
}

fn parse(out: &[u8]) -> Value {
    serde_json::from_slice(out)
        .unwrap_or_else(|e| panic!("not JSON: {e}\n{}", String::from_utf8_lossy(out)))
}

const LOGO: &str = r#"{"name":"logo.png","path":"images/logo.png","type":"file","url":"https://u.realgeeks.media/example/images/logo.png","size":12345,"content_type":"image/png","last_modified":"2026-09-02T18:00:00+00:00","dimensions":{"width":400,"height":200}}"#;

#[test]
fn list_follows_cursor_with_all() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    // the more specific (cursor) mock first: mockito serves the first mock that matches
    server
        .mock("GET", "/api/v3/files/")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("path".into(), "images".into()),
            Matcher::UrlEncoded("cursor".into(), "c1".into()),
        ]))
        .with_body(format!(
            r#"{{"path":"images","results":[{LOGO}],"next_cursor":null}}"#
        ))
        .create();
    server
        .mock("GET", "/api/v3/files/")
        .match_query(Matcher::UrlEncoded("path".into(), "images".into()))
        .with_body(r#"{"path":"images","results":[{"name":"2026","path":"images/2026","type":"folder"}],"next_cursor":"c1"}"#)
        .create();

    let out = cmd(&server, &dir)
        .args(["files", "list", "/images/", "--all", "-q"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        String::from_utf8_lossy(&out),
        "images/2026\nimages/logo.png\n"
    );
}

#[test]
fn get_and_url_encode_spaces() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/files/images/my%20logo.png/")
        .with_body(LOGO)
        .expect(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["files", "get", "images/my logo.png"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["size"], 12345);
    cmd(&server, &dir)
        .args(["files", "url", "images/my logo.png"])
        .assert()
        .success()
        .stdout("https://u.realgeeks.media/example/images/logo.png\n");
}

#[test]
fn upload_sends_multipart_with_detected_type() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("hero.png");
    std::fs::write(&local, b"\x89PNG fake").unwrap();
    let mock = server
        .mock("POST", "/api/v3/files/upload/")
        .match_header("authorization", "Bearer rg_live_k")
        .match_header(
            "content-type",
            Matcher::Regex("multipart/form-data; boundary=.*".into()),
        )
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(r#"name="file"; filename="hero.png""#.into()),
            Matcher::Regex("Content-Type: image/png".into()),
            Matcher::Regex(r#"name="path"\r\n\r\nimages"#.into()),
            Matcher::Regex(r#"name="overwrite"\r\n\r\ntrue"#.into()),
        ]))
        .with_status(201)
        .with_body(LOGO)
        .create();
    cmd(&server, &dir)
        .args(["files", "upload", "--to", "images/", "--overwrite", "-q"])
        .arg(&local)
        .assert()
        .success()
        .stdout("https://u.realgeeks.media/example/images/logo.png\n");
    mock.assert();
}

#[test]
fn mkdir_splits_folder_and_name() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let mock = server
        .mock("POST", "/api/v3/files/folders/")
        .match_body(Matcher::Json(json!({ "path": "images", "name": "2026" })))
        .with_status(201)
        .with_body(r#"{"name":"2026","path":"images/2026","type":"folder"}"#)
        .create();
    cmd(&server, &dir)
        .args(["files", "mkdir", "images/2026"])
        .assert()
        .success();
    mock.assert();
}

#[test]
fn move_and_delete() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let mv = server
        .mock("POST", "/api/v3/files/move/")
        .match_body(Matcher::Json(
            json!({ "from": "logo.png", "to": "images/logo.png", "overwrite": false }),
        ))
        .with_body(LOGO)
        .create();
    let del = server
        .mock("DELETE", "/api/v3/files/images/logo.png/")
        .with_status(204)
        .create();
    cmd(&server, &dir)
        .args(["files", "move", "/logo.png", "images/logo.png"])
        .assert()
        .success();
    let out = cmd(&server, &dir)
        .args(["files", "delete", "images/logo.png"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["deleted"], true);
    mv.assert();
    del.assert();
}

#[test]
fn upload_conflict_exits_6() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("logo.png");
    std::fs::write(&local, b"x").unwrap();
    server
        .mock("POST", "/api/v3/files/upload/")
        .with_status(409)
        .with_body(r#"{"error":{"code":"conflict","message":"A file named logo.png already exists there.","fields":{"name":["Already exists"]}}}"#)
        .create();
    cmd(&server, &dir)
        .args(["files", "upload"])
        .arg(&local)
        .assert()
        .code(6)
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn upload_from_url_asks_the_site_to_fetch() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let upload = server
        .mock("POST", "/api/v3/files/upload/")
        .match_header("content-type", "application/json")
        .match_body(Matcher::Json(json!({
            "path": "images/2026",
            "name": "logo.png",
            "content_url": "https://u.realgeeks.media/example/images/logo.png?v=2",
        })))
        .with_status(201)
        .with_body(LOGO)
        .create();

    cmd(&server, &dir)
        .args([
            "-q",
            "files",
            "upload",
            "--from-url",
            "https://u.realgeeks.media/example/images/logo.png?v=2",
            "--to",
            "images/2026",
        ])
        .assert()
        .success()
        .stdout("https://u.realgeeks.media/example/images/logo.png\n");
    upload.assert();
}

#[test]
fn upload_from_url_without_a_file_name_needs_name() {
    let server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    cmd(&server, &dir)
        .args([
            "files",
            "upload",
            "--from-url",
            "https://files.oaiusercontent.com/file-abc123",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--name"));
}

#[test]
fn upload_rejects_a_local_file_with_from_url() {
    let server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    cmd(&server, &dir)
        .args([
            "files",
            "upload",
            "hero.jpg",
            "--from-url",
            "https://u.realgeeks.media/a/b.png",
        ])
        .assert()
        .code(2);
}
