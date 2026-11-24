//! `geekcli search-forms …` against a mock site.
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

const FORMS: &str = r#"{"source":"board","area_pages":true,"home_page_uses_forms":true,
"pages":[
 {"id":"advanced_search","name":"Advanced Search","layout":"advanced_default","required":true,"prepackaged":false,
  "rules":{"primary_widgets":["select","multiselect"]},
  "primary":[{"field":"city","widget":"multiselect"}],
  "secondary":[{"field":"res"},{"field":"list_price","range":"both"}]},
 {"id":"mls_search","name":"MLS Number Search","layout":"mls_search","required":false,"prepackaged":true,
  "rules":{},"primary":[{"field":"mls_number"}],"secondary":[]}],
"home_page_forms":[{"id":"homepage","name":"Home page search","layout":"homepage","required":false,"prepackaged":false,
  "rules":{},"primary":[{"field":"city"}],"secondary":[]}],
"fields":[
 {"key":"city","label":"City","group":"Location","kind":"choice","from_mls":true,"choices":1148},
 {"key":"street_name","label":"Street Name","group":"Location","kind":"choice","from_mls":true,"choices":51620},
 {"key":"pool","label":"Pool","group":"Other","kind":"choice","from_mls":false},
 {"key":"list_price","label":"Price","group":"Price & size","kind":"range"}],
"max_choices":3000,"dropdown_warning":14,"prepackaged_pages":[],"widgets":{}}"#;

#[test]
fn get_prints_the_document_or_a_page_table() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/search-forms/")
        .with_body(FORMS)
        .expect(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["search-forms", "get"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc = parse(&out);
    assert_eq!(doc["pages"][0]["id"], "advanced_search");
    assert_eq!(doc["max_choices"], 3000);

    let table = cmd(&server, &dir)
        .args(["-o", "table", "search-forms", "get"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let table = String::from_utf8_lossy(&table);
    assert!(table.contains("Advanced Search"), "{table}");
    assert!(table.contains("Home page search"), "{table}");
    assert!(table.contains("true"), "{table}"); // MLS Number Search is ready-made
}

#[test]
fn fields_filters_by_group_and_text() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/search-forms/")
        .with_body(FORMS)
        .create();
    let out = cmd(&server, &dir)
        .args([
            "search-forms",
            "fields",
            "--group",
            "location",
            "-s",
            "street",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let listed = parse(&out);
    let rows = listed
        .as_array()
        .or_else(|| listed["results"].as_array())
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["key"], "street_name");
    assert_eq!(rows[0]["choices"], 51620);
}

#[test]
fn update_sends_the_file_and_dry_runs() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let body = json!({"pages": [{"id": "advanced_search", "name": "Find a home",
        "primary": [{"field": "city"}], "secondary": []}]});
    let file = dir.path().join("forms.json");
    std::fs::write(&file, body.to_string()).unwrap();

    let dry = server
        .mock("PUT", "/api/v3/search-forms/")
        .match_query(Matcher::UrlEncoded("dry_run".into(), "1".into()))
        .match_body(Matcher::Json(body.clone()))
        .with_body(r#"{"source":"board","pages":[],"home_page_forms":[],"dry_run":true,
"warnings":[{"code":"long_form","field":"pages[0]","message":"\"Advanced Search\" has 17 drop-downs."}]}"#)
        .create();
    cmd(&server, &dir)
        .args(["search-forms", "update", "--dry-run", "--data"])
        .arg(format!("@{}", file.display()))
        .assert()
        .success()
        .stderr(
            predicate::str::contains("warning:").and(predicate::str::contains("17 drop-downs")),
        );
    dry.assert();

    let save = server
        .mock("PUT", "/api/v3/search-forms/")
        .match_query(Matcher::Missing)
        .match_body(Matcher::Json(body))
        .with_body(r#"{"source":"site","pages":[{"id":"advanced_search","name":"Find a home"}],"home_page_forms":[]}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["search-forms", "update", "--data"])
        .arg(format!("@{}", file.display()))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    save.assert();
    assert_eq!(parse(&out)["source"], "site");
}

#[test]
fn update_refuses_bad_json_and_hidden_files() {
    let server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    cmd(&server, &dir)
        .args(["search-forms", "update", "--data", "{not json"])
        .assert()
        .code(2);
    cmd(&server, &dir)
        .args(["search-forms", "update", "--data", "[]"])
        .assert()
        .code(2);
    let hidden = dir.path().join(".forms.json");
    std::fs::write(&hidden, "{}").unwrap();
    cmd(&server, &dir)
        .args(["search-forms", "update", "--data"])
        .arg(format!("@{}", hidden.display()))
        .assert()
        .failure();
}

#[test]
fn rule_violations_come_back_as_validation_errors() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("PUT", "/api/v3/search-forms/")
        .with_status(422)
        .with_body(r#"{"error":{"code":"validation_error","message":"One or more fields are invalid.",
"fields":{"pages[0].primary[1]":["Not shown on this page: this page's Location column only shows drop-downs and lists."]}}}"#)
        .create();
    cmd(&server, &dir)
        .args(["search-forms", "update", "--data", r#"{"pages":[]}"#])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("pages[0].primary[1]"));
}

#[test]
fn reset_and_revisions() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let reset = server
        .mock("DELETE", "/api/v3/search-forms/")
        .with_body(r#"{"source":"board","pages":[],"home_page_forms":[]}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["--yes", "search-forms", "reset"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    reset.assert();
    assert_eq!(parse(&out)["source"], "board");

    server
        .mock("GET", "/api/v3/search-forms/revisions/")
        .with_body(r#"{"results":[{"id":24,"at":"t","by":{"name":"Riley Chen","api_key":null,"locutus_id":null},"action":"changed","message":"Changed the search forms","changed_fields":["search_forms"],"revertible":true}]}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["search-forms", "revisions", "--limit", "1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["results"][0]["id"], 24);

    server
        .mock("GET", "/api/v3/search-forms/revisions/24/")
        .with_body(r#"{"id":24,"message":"Changed the search forms","preview":{"now":{"source":"site","pages":["Find a home"]},"after_revert":{"source":"board","pages":["Advanced Search"]}}}"#)
        .create();
    let table = cmd(&server, &dir)
        .args(["-o", "table", "search-forms", "revision", "24"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let table = String::from_utf8_lossy(&table);
    assert!(table.contains("board forms: Advanced Search"), "{table}");

    let revert = server
        .mock("POST", "/api/v3/search-forms/revisions/24/revert/")
        .with_body(r#"{"source":"board","pages":[],"home_page_forms":[]}"#)
        .create();
    cmd(&server, &dir)
        .args(["--yes", "search-forms", "revert", "24"])
        .assert()
        .success();
    revert.assert();
}
