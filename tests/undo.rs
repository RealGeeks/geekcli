//! Undo beyond pages: `revisions` / `revision` / `revert` on sidebars,
//! navigation bars, featured groups, banners, settings and design, and
//! `files versions|deleted|restore`, against a mock site.
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

fn stdout(assert: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assert.get_output().stdout).into_owned()
}

const REVISIONS: &str = r#"{"results":[
{"id":712,"at":"2026-10-07T15:00:00+00:00","by":{"name":"Jordan Avery","api_key":"cli","locutus_id":null},"action":"changed","message":"","changed_fields":["items"],"revertible":true},
{"id":640,"at":"2026-09-01T09:00:00+00:00","by":{"name":"Riley Chen","api_key":null,"locutus_id":null},"action":"created","message":"","changed_fields":[],"revertible":false}]}"#;

const SIDEBAR: &str = r#"{"id":5,"name":"Area Sidebar","special":false,"items":[{"id":51,"order":0,"type":"links","header":{"text":"Featured Areas","url":"/areas/"},"links":[{"url":"/riverside/","anchor":"Riverside"}],"columns":1}],"used_by":[]}"#;

#[test]
fn sidebars_revisions_revision_and_revert_by_name() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/sidebars/")
        .match_query(Matcher::UrlEncoded("name".into(), "Area Sidebar".into()))
        .with_body(r#"{"results":[{"id":5,"name":"Area Sidebar"}]}"#)
        .expect_at_least(1)
        .create();
    server
        .mock("GET", "/api/v3/content/sidebars/5/")
        .with_body(SIDEBAR)
        .expect_at_least(1)
        .create();
    server
        .mock("GET", "/api/v3/content/sidebars/5/revisions/")
        .with_body(REVISIONS)
        .expect(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["sidebars", "revisions", "Area Sidebar", "--limit", "1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let listed = parse(&out);
    assert_eq!(listed["results"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["results"][0]["id"], 712);
    assert_eq!(listed["results"][0]["by"]["name"], "Jordan Avery");
    cmd(&server, &dir)
        .args(["sidebars", "revisions", "5", "-q"])
        .assert()
        .success()
        .stdout("712\n640\n");

    // the preview of a list field is the list, as the sidebar shows it
    server
        .mock("GET", "/api/v3/content/sidebars/5/revisions/712/")
        .with_body(r#"{"id":712,"at":"2026-10-07T15:00:00+00:00","by":{"name":"Jordan Avery","api_key":"cli","locutus_id":null},"action":"changed","changed_fields":["items"],"revertible":true,"preview":{"items":{"now":[],"after_revert":[{"id":51,"order":0,"type":"links","header":{"text":"Featured Areas","url":"/areas/"},"links":[{"url":"/riverside/","anchor":"Riverside"}],"columns":1},{"id":52,"order":1,"type":"html","html":"<p>Call us</p>"}]}}}"#)
        .expect(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["sidebars", "revision", "5", "712"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        parse(&out)["preview"]["items"]["after_revert"][1]["html"],
        "<p>Call us</p>"
    );
    let table = cmd(&server, &dir)
        .args(["-o", "table", "sidebars", "revision", "5", "712"])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("Revision 712").and(predicate::str::contains("Jordan Avery")),
        );
    let table = stdout(&table);
    assert!(table.contains("(empty)"), "{table}");
    assert!(
        table.contains("2 entries: Featured Areas, <p>Call us</p>"),
        "{table}"
    );
    assert!(!table.contains("\"order\""), "{table}");

    let revert = server
        .mock("POST", "/api/v3/content/sidebars/5/revisions/712/revert/")
        .match_body(Matcher::Json(json!({})))
        .with_body(SIDEBAR)
        .create();
    let out = cmd(&server, &dir)
        .args(["-y", "sidebars", "revert", "Area Sidebar", "712"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["items"][0]["id"], 51);
    revert.assert();
}

const BARS: &str = r#"{"results":[{"id":1,"type":"top_primary","label":"Primary Top Navigation Bar","links":[
{"id":10,"type":"custom","url":"/","anchor_text":"Home","nofollow":false,"order":0}]}]}"#;

#[test]
fn nav_revisions_revision_and_revert_by_position() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/navigation-bars/")
        .with_body(BARS)
        .expect_at_least(2)
        .create();
    server
        .mock("GET", "/api/v3/content/navigation-bars/1/")
        .with_body(
            r#"{"id":1,"type":"top_primary","label":"Primary Top Navigation Bar","links":[]}"#,
        )
        .create();
    server
        .mock("GET", "/api/v3/content/navigation-bars/1/revisions/")
        .with_body(REVISIONS)
        .create();
    let table = cmd(&server, &dir)
        .args(["-o", "table", "nav", "revisions", "top-primary"])
        .assert()
        .success();
    let table = stdout(&table);
    assert!(table.contains("712") && table.contains("640"), "{table}");
    assert!(table.contains("Jordan Avery"), "{table}");
    assert!(table.contains("Riley Chen"), "{table}");

    server
        .mock("GET", "/api/v3/content/navigation-bars/1/revisions/712/")
        .with_body(r#"{"id":712,"action":"changed","at":"t","by":{"name":"Jordan Avery","api_key":"cli"},"revertible":true,"preview":{"links":{"now":[{"id":10,"type":"custom","url":"/","anchor_text":"Home","nofollow":false,"order":0}],"after_revert":[{"id":10,"type":"custom","url":"/","anchor_text":"Home","nofollow":false,"order":0},{"id":11,"type":"custom","url":"/buying/","anchor_text":"Buying","nofollow":false,"order":1},{"id":12,"type":"contact","url":null,"anchor_text":null,"nofollow":false,"order":2}]}}}"#)
        .expect(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["nav", "revision", "top_primary", "712"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        parse(&out)["preview"]["links"]["after_revert"][1]["anchor_text"],
        "Buying"
    );
    let table = cmd(&server, &dir)
        .args(["-o", "table", "nav", "revision", "1", "712"])
        .assert()
        .success();
    let table = stdout(&table);
    assert!(table.contains("1 entry: Home"), "{table}");
    assert!(
        table.contains("3 entries: Home, Buying, contact"),
        "{table}"
    );

    let revert = server
        .mock(
            "POST",
            "/api/v3/content/navigation-bars/1/revisions/712/revert/",
        )
        .with_body(r#"{"id":1,"type":"top_primary","label":"Primary Top Navigation Bar","links":[{"id":10},{"id":31},{"id":32}]}"#)
        .create();
    cmd(&server, &dir)
        .args(["-y", "nav", "revert", "top_primary", "712", "-q"])
        .assert()
        .success()
        .stdout("10\n31\n32\n");
    revert.assert();
}

#[test]
fn featured_revisions_revision_and_revert_by_title() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/featured-pages/")
        .with_body(r#"{"results":[{"id":3,"title":"Where We Live","blurb":"","tiles":[]}]}"#)
        .expect_at_least(1)
        .create();
    server
        .mock("GET", "/api/v3/content/featured-pages/3/")
        .with_body(r#"{"id":3,"title":"Where We Live","blurb":"","tiles":[]}"#)
        .expect_at_least(1)
        .create();
    server
        .mock("GET", "/api/v3/content/featured-pages/3/revisions/")
        .with_body(REVISIONS)
        .create();
    cmd(&server, &dir)
        .args(["featured", "revisions", "where we live", "-q"])
        .assert()
        .success()
        .stdout("712\n640\n");

    server
        .mock("GET", "/api/v3/content/featured-pages/3/revisions/712/")
        .with_body(r#"{"id":712,"action":"changed","at":"t","by":{"name":"Jordan Avery","api_key":"cli"},"revertible":true,"preview":{"title":{"now":"Where We Live","after_revert":"Our Neighborhoods"},"tiles":{"now":[],"after_revert":[{"id":9,"title":"Riverside","link":"/riverside/","cta":"View Homes","image":null}]}}}"#)
        .expect(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["featured", "revision", "3", "712"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        parse(&out)["preview"]["tiles"]["after_revert"][0]["title"],
        "Riverside"
    );
    let table = cmd(&server, &dir)
        .args(["-o", "table", "featured", "revision", "3", "712"])
        .assert()
        .success();
    let table = stdout(&table);
    assert!(table.contains("1 entry: Riverside"), "{table}");
    assert!(table.contains("Our Neighborhoods"), "{table}");

    let revert = server
        .mock(
            "POST",
            "/api/v3/content/featured-pages/3/revisions/712/revert/",
        )
        .with_body(r#"{"id":3,"title":"Our Neighborhoods","blurb":"","tiles":[{"id":14,"title":"Riverside","link":"/riverside/","cta":"View Homes","image":null}]}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["-y", "featured", "revert", "Where We Live", "712"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    // the tile came back as a new row
    assert_eq!(parse(&out)["tiles"][0]["id"], 14);
    revert.assert();
}

#[test]
fn banners_revisions_revision_revert_and_a_name_conflict() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/banners/")
        .with_body(r#"{"results":[{"id":4,"name":"Open house"}]}"#)
        .expect_at_least(1)
        .create();
    server
        .mock("GET", "/api/v3/content/banners/4/")
        .with_body(r#"{"id":4,"name":"Open house","message":"This Sunday","call_to_action":"See details","url":"/open-house/"}"#)
        .expect_at_least(1)
        .create();
    server
        .mock("GET", "/api/v3/content/banners/4/revisions/")
        .with_body(REVISIONS)
        .create();
    let out = cmd(&server, &dir)
        .args(["banners", "revisions", "Open house"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["results"][1]["revertible"], false);

    server
        .mock("GET", "/api/v3/content/banners/4/revisions/712/")
        .with_body(r#"{"id":712,"action":"changed","revertible":true,"preview":{"message":{"now":"This Sunday","after_revert":"This Saturday"}}}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["banners", "revision", "4", "712"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        parse(&out)["preview"]["message"]["after_revert"],
        "This Saturday"
    );

    let revert = server
        .mock("POST", "/api/v3/content/banners/4/revisions/712/revert/")
        .with_body(r#"{"id":4,"name":"Open house","message":"This Saturday","call_to_action":"See details","url":"/open-house/"}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["-y", "banners", "revert", "open house", "712"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["message"], "This Saturday");
    revert.assert();

    server
        .mock("POST", "/api/v3/content/banners/4/revisions/700/revert/")
        .with_status(409)
        .with_body(r#"{"error":{"code":"conflict","message":"Revision 700 cannot be reverted: another banner is named \"Spring\""}}"#)
        .create();
    cmd(&server, &dir)
        .args(["-y", "banners", "revert", "4", "700"])
        .assert()
        .code(6)
        .stderr(predicate::str::contains("\"code\":\"conflict\""));
}

const SETTINGS_REVISIONS: &str = r#"{"results":[
{"id":9001,"at":"2026-10-07T15:00:00+00:00","by":{"name":null,"api_key":"cli","locutus_id":null},"action":"changed","changed_fields":["EMAIL_FROM_NAME","TAGLINE"],"revertible":true},
{"id":8990,"at":"2026-10-01T12:00:00+00:00","by":{"name":"Riley Chen","api_key":null,"locutus_id":null},"action":"changed","changed_fields":["HEADER_LOGO"],"revertible":true}]}"#;

#[test]
fn settings_revisions_revision_and_revert() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/settings/revisions/")
        .with_body(SETTINGS_REVISIONS)
        .expect(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["settings", "revisions", "--limit", "1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let listed = parse(&out);
    assert_eq!(listed["results"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["results"][0]["changed_fields"][1], "TAGLINE");
    // a change made through the API has no name, only the key
    let table = cmd(&server, &dir)
        .args(["-o", "table", "settings", "revisions"])
        .assert()
        .success();
    let table = stdout(&table);
    assert!(table.contains("API key cli"), "{table}");
    assert!(table.contains("Riley Chen"), "{table}");
    assert!(table.contains("EMAIL_FROM_NAME, TAGLINE"), "{table}");

    server
        .mock("GET", "/api/v3/settings/revisions/9001/")
        .with_body(r#"{"id":9001,"at":"t","by":{"name":null,"api_key":"cli","locutus_id":null},"action":"changed","changed_fields":["EMAIL_FROM_NAME","TAGLINE"],"revertible":true,"preview":{"EMAIL_FROM_NAME":{"now":"Jordan Avery","after_revert":null},"TAGLINE":{"now":"Homes by the river","after_revert":"Riverside homes"}}}"#)
        .expect(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["settings", "revision", "9001"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let detail = parse(&out);
    assert_eq!(
        detail["preview"]["EMAIL_FROM_NAME"]["after_revert"],
        Value::Null
    );
    assert_eq!(
        detail["preview"]["TAGLINE"]["after_revert"],
        "Riverside homes"
    );
    cmd(&server, &dir)
        .args(["-o", "table", "settings", "revision", "9001"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Riverside homes"))
        .stderr(
            predicate::str::contains("API key cli")
                .and(predicate::str::contains("inherited default")),
        );

    let revert = server
        .mock("POST", "/api/v3/settings/revisions/9001/revert/")
        .match_body(Matcher::Json(json!({})))
        .with_body(r#"{"results":[{"name":"EMAIL_FROM_NAME","type":"string","value":"Example Realty","overridden":false,"group":"Email"},{"name":"TAGLINE","type":"string","value":"Riverside homes","overridden":true,"group":"Common"}]}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["-y", "settings", "revert", "9001"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let reverted = parse(&out);
    assert_eq!(reverted["results"][0]["overridden"], false);
    assert_eq!(reverted["results"][1]["value"], "Riverside homes");
    revert.assert();
}

#[test]
fn settings_revert_that_no_longer_applies_is_a_conflict() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("POST", "/api/v3/settings/revisions/8990/revert/")
        .with_status(409)
        .with_body(r#"{"error":{"code":"conflict","message":"Revision 8990 cannot be reverted: Invalid settings","fields":{"HEADER_LOGO":["Not a valid file URL"]}}}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["-y", "settings", "revert", "8990"])
        .assert()
        .code(6)
        .get_output()
        .stderr
        .clone();
    let err = parse(&out);
    assert_eq!(err["error"]["code"], "conflict");
    assert_eq!(
        err["error"]["fields"]["HEADER_LOGO"][0],
        "Not a valid file URL"
    );
}

#[test]
fn design_revisions_revision_and_revert() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/design/revisions/")
        .with_body(r#"{"results":[{"id":9100,"at":"2026-10-07T16:00:00+00:00","by":{"name":null,"api_key":"cli","locutus_id":null},"action":"changed","changed_fields":["template","styles"],"revertible":true}]}"#)
        .create();
    cmd(&server, &dir)
        .args(["design", "revisions", "-q"])
        .assert()
        .success()
        .stdout("9100\n");

    server
        .mock("GET", "/api/v3/design/revisions/9100/")
        .with_body(r##"{"id":9100,"at":"t","by":{"name":null,"api_key":"cli"},"action":"changed","changed_fields":["template","styles"],"revertible":true,"preview":{"template":{"now":"anna-modern","after_revert":"anna"},"styles":{"now":{"name":"coastal","vars":{"palette-brand-color":"#0066A7"}},"after_revert":{"name":"classic","vars":{"palette-brand-color":"#333333"}}}}}"##)
        .expect(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["design", "revision", "9100"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        parse(&out)["preview"]["styles"]["after_revert"]["name"],
        "classic"
    );
    let table = cmd(&server, &dir)
        .args(["-o", "table", "design", "revision", "9100"])
        .assert()
        .success();
    let table = stdout(&table);
    assert!(
        table.contains("classic: palette-brand-color=#333333"),
        "{table}"
    );
    assert!(table.contains("anna-modern"), "{table}");

    let revert = server
        .mock("POST", "/api/v3/design/revisions/9100/revert/")
        .with_body(r##"{"template":"anna","styles":{"name":"classic","vars":{"palette-brand-color":"#333333"}},"consistent":true}"##)
        .create();
    let out = cmd(&server, &dir)
        .args(["-y", "design", "revert", "9100"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["template"], "anna");
    assert_eq!(parse(&out)["styles"]["name"], "classic");
    revert.assert();

    server
        .mock("POST", "/api/v3/design/revisions/9000/revert/")
        .with_status(409)
        .with_body(
            r#"{"error":{"code":"conflict","message":"template steve is no longer available"}}"#,
        )
        .create();
    cmd(&server, &dir)
        .args(["-y", "design", "revert", "9000"])
        .assert()
        .code(6)
        .stderr(predicate::str::contains("no longer available"));
}

const LOGO: &str = r#"{"name":"logo.png","path":"images/logo.png","type":"file","url":"https://u.realgeeks.media/example/images/logo.png","size":12345,"content_type":"image/png","last_modified":"2026-10-08T18:00:00+00:00","dimensions":{"width":400,"height":200}}"#;

#[test]
fn files_versions_lists_a_files_history() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/files/versions/")
        .match_query(Matcher::UrlEncoded("path".into(), "images/logo.png".into()))
        .with_body(r#"{"path":"images/logo.png","results":[
{"version_id":"v3","at":"2026-10-07T10:00:00+00:00","action":"deleted","size":null,"current":true,"restorable":false},
{"version_id":"v2","at":"2026-10-01T10:00:00+00:00","action":"saved","size":12345,"current":false,"restorable":true}]}"#)
        .expect(3)
        .create();
    let out = cmd(&server, &dir)
        .args(["files", "versions", "/images/logo.png"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc = parse(&out);
    assert_eq!(doc["path"], "images/logo.png");
    assert_eq!(doc["results"][1]["restorable"], true);
    cmd(&server, &dir)
        .args(["files", "versions", "images/logo.png", "-q"])
        .assert()
        .success()
        .stdout("v3\nv2\n");
    let table = cmd(&server, &dir)
        .args(["-o", "table", "files", "versions", "images/logo.png"])
        .assert()
        .success()
        .stderr(predicate::str::contains("/images/logo.png"));
    let table = stdout(&table);
    assert!(
        table.contains("deleted") && table.contains("12345"),
        "{table}"
    );
}

#[test]
fn files_deleted_follows_cursor_with_all() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    // the more specific (cursor) mock first: mockito serves the first mock that matches
    let second = server
        .mock("GET", "/api/v3/files/deleted/")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("path".into(), "images".into()),
            Matcher::UrlEncoded("cursor".into(), "c1".into()),
        ]))
        .with_body(r#"{"path":"images","results":[{"path":"images/old/hero.jpg","type":"file","deleted_at":"2026-10-06T09:00:00+00:00"}],"next_cursor":null}"#)
        .expect(2)
        .create();
    server
        .mock("GET", "/api/v3/files/deleted/")
        .match_query(Matcher::UrlEncoded("path".into(), "images".into()))
        .with_body(r#"{"path":"images","results":[{"path":"images/old","type":"folder","deleted_at":"2026-10-06T09:00:05+00:00"}],"next_cursor":"c1"}"#)
        .expect(3)
        .create();

    cmd(&server, &dir)
        .args(["files", "deleted", "/images/", "--all", "-q"])
        .assert()
        .success()
        .stdout("images/old\nimages/old/hero.jpg\n");

    // one page: the cursor is handed back for the next call
    let out = cmd(&server, &dir)
        .args(["files", "deleted", "images"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let page = parse(&out);
    assert_eq!(page["results"].as_array().map(Vec::len), Some(1));
    assert_eq!(page["next_cursor"], "c1");
    let out = cmd(&server, &dir)
        .args(["files", "deleted", "images", "--cursor", "c1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["next_cursor"], Value::Null);
    second.assert();

    cmd(&server, &dir)
        .args(["-o", "table", "files", "deleted", "images"])
        .assert()
        .success()
        .stdout(predicate::str::contains("images/old"))
        .stderr(predicate::str::contains("more entries: --cursor c1"));
}

#[test]
fn files_deleted_defaults_to_the_root() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let root = server
        .mock("GET", "/api/v3/files/deleted/")
        .match_query(Matcher::UrlEncoded("path".into(), String::new()))
        .with_body(r#"{"path":"","results":[],"next_cursor":null}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["files", "deleted"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["results"], json!([]));
    root.assert();
}

#[test]
fn files_restore_sends_the_path_and_an_optional_version() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let latest = server
        .mock("POST", "/api/v3/files/restore/")
        .match_body(Matcher::Json(json!({ "path": "images/logo.png" })))
        .with_body(format!(r#"{{"restored":[{LOGO}],"incomplete":false}}"#))
        .expect(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["files", "restore", "/images/logo.png"])
        .assert()
        .success()
        .stderr(predicate::str::contains("ncomplete").not())
        .get_output()
        .stdout
        .clone();
    let doc = parse(&out);
    assert_eq!(doc["restored"][0]["path"], "images/logo.png");
    assert_eq!(doc["incomplete"], false);
    cmd(&server, &dir)
        .args(["files", "restore", "images/logo.png", "-q"])
        .assert()
        .success()
        .stdout("https://u.realgeeks.media/example/images/logo.png\n");
    latest.assert();

    let named = server
        .mock("POST", "/api/v3/files/restore/")
        .match_body(Matcher::Json(
            json!({ "path": "images/logo.png", "version_id": "v2" }),
        ))
        .with_body(format!(r#"{{"restored":[{LOGO}],"incomplete":false}}"#))
        .create();
    let table = cmd(&server, &dir)
        .args([
            "-o",
            "table",
            "files",
            "restore",
            "images/logo.png",
            "--version",
            "v2",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Restored images/logo.png (1 entry)",
        ));
    let table = stdout(&table);
    assert!(
        table.contains("images/logo.png") && table.contains("400x200"),
        "{table}"
    );
    named.assert();
}

#[test]
fn files_restore_says_when_a_folder_was_only_partly_restored() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("POST", "/api/v3/files/restore/")
        .match_body(Matcher::Json(json!({ "path": "images/2026" })))
        .with_body(format!(
            r#"{{"restored":[{{"name":"2026","path":"images/2026","type":"folder"}},{LOGO}],"incomplete":true}}"#
        ))
        .expect(2)
        .create();
    cmd(&server, &dir)
        .args(["-o", "table", "files", "restore", "images/2026"])
        .assert()
        .success()
        .stdout(predicate::str::contains("images/2026"))
        .stderr(
            predicate::str::contains("Restored images/2026 (2 entries)")
                .and(predicate::str::contains(
                    "incomplete: only part of images/2026",
                ))
                .and(predicate::str::contains(
                    "geekcli files deleted images/2026",
                )),
        );
    // JSON keeps the flag in the document and still warns on stderr
    let assert = cmd(&server, &dir)
        .args(["files", "restore", "images/2026"])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning: incomplete"));
    assert_eq!(parse(&assert.get_output().stdout)["incomplete"], true);
}

#[test]
fn files_restore_errors_keep_their_exit_codes() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("POST", "/api/v3/files/restore/")
        .match_body(Matcher::Json(json!({ "path": "images/old.png" })))
        .with_status(409)
        .with_body(r#"{"error":{"code":"conflict","message":"images/old.png was deleted more than 90 days ago and can no longer be restored"}}"#)
        .create();
    cmd(&server, &dir)
        .args(["files", "restore", "images/old.png"])
        .assert()
        .code(6)
        .stderr(predicate::str::contains("more than 90 days ago"));

    server
        .mock("POST", "/api/v3/files/restore/")
        .match_body(Matcher::Json(
            json!({ "path": "images/logo.png", "version_id": "nope" }),
        ))
        .with_status(404)
        .with_body(
            r#"{"error":{"code":"not_found","message":"No version nope of images/logo.png"}}"#,
        )
        .create();
    cmd(&server, &dir)
        .args(["files", "restore", "images/logo.png", "--version", "nope"])
        .assert()
        .code(4);

    server
        .mock("GET", "/api/v3/files/versions/")
        .match_query(Matcher::Any)
        .with_status(503)
        .with_body(r#"{"error":{"code":"file_history_unavailable","message":"File history is not available for this site"}}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["files", "versions", "images/logo.png"])
        .assert()
        .code(1)
        .get_output()
        .stderr
        .clone();
    let err = parse(&out);
    assert_eq!(err["error"]["code"], "file_history_unavailable");
    assert_eq!(err["error"]["status"], 503);
    assert!(err["error"]["hint"]
        .as_str()
        .is_some_and(|h| h.contains("version history")));
}
