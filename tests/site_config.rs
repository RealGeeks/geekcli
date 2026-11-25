//! `geekcli nav|sidebars|settings …` against a mock site.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use assert_cmd::Command;
use mockito::{Matcher, Server, ServerGuard};
use predicates::prelude::*;
use serde_json::{json, Value};

const KEY: &str = "rg_live_k";

fn cmd(server: &ServerGuard, dir: &tempfile::TempDir) -> Command {
    let mut c = common::geekcli();
    c.env("GEEKCLI_SITE", "www.test.local")
        .env("GEEKCLI_API_KEY", KEY)
        .env("GEEKCLI_BASE_URL", server.url())
        .env("GEEKCLI_CONFIG_DIR", dir.path());
    c
}

fn parse(out: &[u8]) -> Value {
    serde_json::from_slice(out)
        .unwrap_or_else(|e| panic!("not JSON: {e}\n{}", String::from_utf8_lossy(out)))
}

const BARS: &str = r#"{"results":[{"id":1,"type":"top_primary","label":"Primary Top Navigation Bar","links":[
{"id":10,"type":"custom","url":"/","anchor_text":"Home","nofollow":false,"order":0},
{"id":11,"type":"custom","url":"/buying/","anchor_text":"Buying","nofollow":false,"order":1},
{"id":12,"type":"contact","url":null,"anchor_text":null,"nofollow":false,"order":2}]}]}"#;

#[test]
fn nav_get_by_position_and_add_link() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/navigation-bars/")
        .with_body(BARS)
        .expect_at_least(1)
        .create();
    let post = server
        .mock("POST", "/api/v3/content/navigation-bars/1/links/")
        .match_body(Matcher::Json(
            json!({ "type": "custom", "url": "/blog/", "anchor_text": "Blog", "nofollow": false }),
        ))
        .with_status(201)
        .with_body(r#"{"id":1,"type":"top_primary","label":"x","links":[{"id":13}]}"#)
        .create();

    let out = cmd(&server, &dir)
        .args(["nav", "get", "top-primary"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["links"][1]["anchor_text"], "Buying");

    cmd(&server, &dir)
        .args([
            "nav",
            "add",
            "top_primary",
            "--url",
            "/blog/",
            "--text",
            "Blog",
        ])
        .assert()
        .success();
    post.assert();
}

#[test]
fn nav_add_at_position_and_move_use_order() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/navigation-bars/1/")
        .with_body(
            r#"{"id":1,"type":"top_primary","label":"x","links":[
        {"id":10,"type":"custom","url":"/","anchor_text":"Home","nofollow":false,"order":0},
        {"id":12,"type":"contact","url":null,"anchor_text":null,"nofollow":false,"order":1}]}"#,
        )
        .expect_at_least(3)
        .create();
    let post_at = server
        .mock("POST", "/api/v3/content/navigation-bars/1/links/")
        .match_body(Matcher::Json(json!({
            "type": "custom", "url": "/selling/", "anchor_text": "Selling", "nofollow": true, "order": 1
        })))
        .with_status(201)
        .with_body(r#"{"id":1,"type":"top_primary","label":"x","links":[]}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "nav",
            "add",
            "1",
            "--url",
            "/selling/",
            "--text",
            "Selling",
            "--nofollow",
            "--at",
            "1",
        ])
        .assert()
        .success();
    post_at.assert();

    // move by id, then by text (case-insensitive); each is one PATCH with order
    let patch_move = server
        .mock("PATCH", "/api/v3/content/navigation-bars/1/links/12/")
        .match_body(Matcher::Json(json!({ "order": 0 })))
        .with_body(r#"{"id":12,"order":0}"#)
        .create();
    cmd(&server, &dir)
        .args(["nav", "move", "1", "12", "--to", "0"])
        .assert()
        .success();
    patch_move.assert();

    let patch_by_text = server
        .mock("PATCH", "/api/v3/content/navigation-bars/1/links/10/")
        .match_body(Matcher::Json(json!({ "order": 1 })))
        .with_body(r#"{"id":10,"order":1}"#)
        .create();
    cmd(&server, &dir)
        .args(["nav", "move", "1", "home", "--to", "1"])
        .assert()
        .success();
    patch_by_text.assert();
}

#[test]
fn nav_update_and_remove_link() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/navigation-bars/1/")
        .with_body(r#"{"id":1,"type":"top_primary","label":"x","links":[{"id":10,"type":"custom","url":"/","anchor_text":"Home","nofollow":false,"order":0}]}"#)
        .expect(2)
        .create();
    let patch = server
        .mock("PATCH", "/api/v3/content/navigation-bars/1/links/10/")
        .match_body(Matcher::Json(
            json!({ "anchor_text": "Start", "nofollow": true }),
        ))
        .with_body(r#"{"id":10,"anchor_text":"Start"}"#)
        .create();
    let del = server
        .mock("DELETE", "/api/v3/content/navigation-bars/1/links/10/")
        .with_status(204)
        .create();
    cmd(&server, &dir)
        .args([
            "nav",
            "update",
            "1",
            "10",
            "--text",
            "Start",
            "--nofollow",
            "true",
        ])
        .assert()
        .success();
    cmd(&server, &dir)
        .args(["nav", "remove", "1", "Home"])
        .assert()
        .success();
    patch.assert();
    del.assert();
}

#[test]
fn nav_unknown_bar_exits_4() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/navigation-bars/")
        .with_body(BARS)
        .create();
    cmd(&server, &dir)
        .args(["nav", "get", "sidebar_left"])
        .assert()
        .code(4)
        .stderr(predicate::str::contains("top_primary"));
}

const SIDEBAR: &str = r#"{"id":5,"name":"Area Sidebar","special":false,"used_by":["riverside"],"items":[
{"id":50,"order":0,"type":"html","html":"<h2>Hello</h2>"},
{"id":51,"order":1,"type":"links","header":{"text":"Areas","url":null},"links":[{"url":"/riverside/","anchor":"Riverside"}],"columns":1}]}"#;

#[test]
fn sidebars_get_by_name_and_add_links_item() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/sidebars/")
        .match_query(Matcher::UrlEncoded("name".into(), "Area Sidebar".into()))
        .with_body(r#"{"results":[{"id":5,"name":"Area Sidebar","special":false,"items":[]}]}"#)
        .create();
    server
        .mock("GET", "/api/v3/content/sidebars/5/")
        .with_body(SIDEBAR)
        .expect_at_least(1)
        .create();
    let post = server
        .mock("POST", "/api/v3/content/sidebars/5/items/")
        .match_body(Matcher::Json(json!({
            "type": "links", "columns": 2,
            "header": {"text": "Featured", "url": "/areas/"},
            "links": [{"anchor": "Riverside", "url": "/riverside/"}, {"anchor": "Coming soon"}]
        })))
        .with_status(201)
        .with_body(SIDEBAR)
        .create();

    let out = cmd(&server, &dir)
        .args(["sidebars", "get", "Area Sidebar"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["used_by"][0], "riverside");

    cmd(&server, &dir)
        .args([
            "sidebars",
            "add-links",
            "5",
            "--header",
            "Featured",
            "--header-url",
            "/areas/",
            "--link",
            "Riverside=/riverside/",
            "--link",
            "Coming soon",
            "--columns",
            "2",
        ])
        .assert()
        .success();
    post.assert();
}

#[test]
fn sidebars_add_html_from_markdown_at_position() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let md = dir.path().join("card.md");
    std::fs::write(&md, "## Call me\n\n**561-555-0100**\n").unwrap();
    server
        .mock("GET", "/api/v3/content/sidebars/5/")
        .with_body(SIDEBAR)
        .create();
    let post = server
        .mock("POST", "/api/v3/content/sidebars/5/items/")
        .match_body(Matcher::Json(json!({
            "type": "html", "html": "<h2>Call me</h2>\n<p><strong>561-555-0100</strong></p>", "order": 0
        })))
        .with_status(201)
        .with_body(SIDEBAR)
        .create();
    cmd(&server, &dir)
        .args(["sidebars", "add-html", "5", "--at", "0"])
        .arg("--html-file")
        .arg(&md)
        .assert()
        .success();
    post.assert();

    let patch = server
        .mock("PATCH", "/api/v3/content/sidebars/5/items/51/")
        .match_body(Matcher::Json(json!({ "order": 0 })))
        .with_body(r#"{"id":51,"order":0}"#)
        .create();
    cmd(&server, &dir)
        .args(["sidebars", "move-item", "5", "51", "--to", "0"])
        .assert()
        .success();
    patch.assert();
}

#[test]
fn pages_update_attaches_sidebar_by_name_and_search_by_criteria() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/area-pages/23/")
        .with_body(r#"{"id":23,"path":"/lakeside/"}"#)
        .create();
    server
        .mock("GET", "/api/v3/content/sidebars/")
        .match_query(Matcher::UrlEncoded("name".into(), "Default Sidebar".into()))
        .with_body(r#"{"results":[{"id":1,"name":"Default Sidebar","special":true,"items":[]}]}"#)
        .create();
    server
        .mock("GET", "/api/v3/content/sidebars/1/")
        .with_body(r#"{"id":1,"name":"Default Sidebar","special":true,"items":[],"used_by":[]}"#)
        .create();
    let patch = server
        .mock("PATCH", "/api/v3/content/area-pages/23/")
        .match_body(Matcher::Json(json!({
            "sidebar": 1,
            "search": { "subdivision": ["Lakeside"], "type": ["res", "con"] }
        })))
        .with_body(r#"{"id":23,"path":"/lakeside/","url":"https://x/lakeside/","sidebar":{"id":1,"name":"Default Sidebar"},"search":{"id":9,"short_id":"9","description":"d","criteria":{}}}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "area-pages",
            "update",
            "23",
            "--sidebar",
            "Default Sidebar",
            "--search-criteria",
            "subdivision=Lakeside",
            "--search-criteria",
            "type=res",
            "--search-criteria",
            "type=con",
        ])
        .assert()
        .success();
    patch.assert();

    let detach = server
        .mock("PATCH", "/api/v3/content/area-pages/23/")
        .match_body(Matcher::Json(json!({ "sidebar": null, "search": "a" })))
        .with_body(r#"{"id":23}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "area-pages",
            "update",
            "23",
            "--sidebar",
            "null",
            "--search",
            "a",
        ])
        .assert()
        .success();
    detach.assert();
}

#[test]
fn sidebars_update_item_keeps_header_text_when_only_url_changes() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/sidebars/5/")
        .with_body(SIDEBAR)
        .create();
    server
        .mock("GET", "/api/v3/content/sidebars/5/items/51/")
        .with_body(r#"{"id":51,"type":"links","header":{"text":"Areas","url":null},"links":[],"columns":1}"#)
        .create();
    let patch = server
        .mock("PATCH", "/api/v3/content/sidebars/5/items/51/")
        .match_body(Matcher::Json(
            json!({ "type": "links", "header": {"text": "Areas", "url": "/areas/"}, "columns": 2 }),
        ))
        .with_body(r#"{"id":51}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "sidebars",
            "update-item",
            "5",
            "51",
            "--header-url",
            "/areas/",
            "--columns",
            "2",
        ])
        .assert()
        .success();
    patch.assert();
}

#[test]
fn sidebars_delete_in_use_exits_6_then_force() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/sidebars/5/")
        .with_body(SIDEBAR)
        .expect(2)
        .create();
    server
        .mock("DELETE", "/api/v3/content/sidebars/5/")
        .with_status(409)
        .with_body(r#"{"error":{"code":"conflict","message":"Sidebar is used by 1 page(s).","fields":{"used_by":["riverside"]}}}"#)
        .create();
    server
        .mock("DELETE", "/api/v3/content/sidebars/5/")
        .match_query(Matcher::UrlEncoded("force".into(), "true".into()))
        .with_status(204)
        .create();
    cmd(&server, &dir)
        .args(["sidebars", "delete", "5"])
        .assert()
        .code(6);
    cmd(&server, &dir)
        .args(["sidebars", "delete", "5", "--force"])
        .assert()
        .success();
}

const SETTINGS: &str = r#"{"results":[
{"name":"EMAIL_FROM_NAME","label":"Email From Name","group":"Common Settings","description":"From name","type":"string","value":"Coastal Realty Group","overridden":true,"inherited_value":null,"required":true,"depends":{}},
{"name":"LEAD_CAPTURE_ON_PROPERTY","label":"Lead Capture","group":"Advanced Site Settings","description":"x","type":"choice","choices":[0,1,2],"value":1,"overridden":false,"inherited_value":1,"required":false,"depends":{}}]}"#;

#[test]
fn settings_list_filters_and_get() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/settings/")
        .with_body(SETTINGS)
        .expect_at_least(1)
        .create();
    server
        .mock("GET", "/api/v3/settings/EMAIL_FROM_NAME/")
        .with_body(r#"{"name":"EMAIL_FROM_NAME","type":"string","value":"Coastal Realty Group"}"#)
        .create();

    let out = cmd(&server, &dir)
        .args(["settings", "list", "--group", "advanced"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc = parse(&out);
    assert_eq!(doc["results"].as_array().unwrap().len(), 1);
    assert_eq!(doc["results"][0]["name"], "LEAD_CAPTURE_ON_PROPERTY");

    let out = cmd(&server, &dir)
        .args(["settings", "list", "--overridden"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["results"][0]["name"], "EMAIL_FROM_NAME");

    let out = cmd(&server, &dir)
        .args(["settings", "get", "email_from_name"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["value"], "Coastal Realty Group");

    let out = cmd(&server, &dir)
        .args(["settings", "groups"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["results"].as_array().unwrap().len(), 2);

    let out = cmd(&server, &dir)
        .args(["settings", "list", "-s", "capture"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        parse(&out)["results"][0]["name"],
        "LEAD_CAPTURE_ON_PROPERTY"
    );
}

#[test]
fn settings_set_coerces_types_and_clear_sends_null() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/settings/LEAD_CAPTURE_ON_PROPERTY/")
        .with_body(r#"{"name":"LEAD_CAPTURE_ON_PROPERTY","type":"choice","choices":[0,1,2]}"#)
        .create();
    server
        .mock("GET", "/api/v3/settings/EMAIL_FROM_NAME/")
        .with_body(r#"{"name":"EMAIL_FROM_NAME","type":"string"}"#)
        .create();
    let patch = server
        .mock("PATCH", "/api/v3/settings/")
        .match_body(Matcher::Json(json!({ "LEAD_CAPTURE_ON_PROPERTY": 2, "EMAIL_FROM_NAME": "Jordan Avery", "GA4_MEASUREMENT_ID": ["G-1"] })))
        .with_body(r#"{"results":[{"name":"LEAD_CAPTURE_ON_PROPERTY","value":2},{"name":"EMAIL_FROM_NAME","value":"Jordan Avery"},{"name":"GA4_MEASUREMENT_ID","value":["G-1"]}]}"#)
        .create();
    let out = cmd(&server, &dir)
        .args([
            "settings",
            "set",
            "lead_capture_on_property=2",
            "EMAIL_FROM_NAME=Jordan Avery",
            "--data",
            r#"{"GA4_MEASUREMENT_ID": ["G-1"]}"#,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["results"].as_array().unwrap().len(), 3);
    patch.assert();

    let clear = server
        .mock("PATCH", "/api/v3/settings/")
        .match_body(Matcher::Json(json!({ "EMAIL_FROM_NAME": null })))
        .with_body(r#"{"results":[{"name":"EMAIL_FROM_NAME","value":null}]}"#)
        .create();
    cmd(&server, &dir)
        .args(["settings", "clear", "email_from_name"])
        .assert()
        .success();
    clear.assert();
}

#[test]
fn settings_set_rejects_bad_choice_before_sending() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/settings/LEAD_CAPTURE_ON_PROPERTY/")
        .with_body(r#"{"name":"LEAD_CAPTURE_ON_PROPERTY","type":"choice","choices":[0,1,2]}"#)
        .create();
    let patch = server.mock("PATCH", "/api/v3/settings/").expect(0).create();
    cmd(&server, &dir)
        .args(["settings", "set", "LEAD_CAPTURE_ON_PROPERTY=9"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("one of: 0, 1, 2"));
    cmd(&server, &dir)
        .args(["settings", "set", "NO_EQUALS"])
        .assert()
        .code(2);
    cmd(&server, &dir)
        .args(["settings", "set"])
        .assert()
        .code(2);
    cmd(&server, &dir)
        .args(["settings", "set", "--data", "[]"])
        .assert()
        .code(2);
    patch.assert();

    // --data @file and --data - send raw JSON objects
    let file = dir.path().join("settings.json");
    std::fs::write(&file, r#"{"tagline":"River homes"}"#).unwrap();
    let patch_file = server
        .mock("PATCH", "/api/v3/settings/")
        .match_body(Matcher::Json(json!({ "TAGLINE": "River homes" })))
        .with_body(r#"{"results":[{"name":"TAGLINE","value":"River homes"}]}"#)
        .expect(2)
        .create();
    cmd(&server, &dir)
        .args(["settings", "set", "--data"])
        .arg(format!("@{}", file.display()))
        .assert()
        .success();
    cmd(&server, &dir)
        .args(["settings", "set", "--data", "-"])
        .write_stdin(r#"{"tagline":"River homes"}"#)
        .assert()
        .success();
    patch_file.assert();
}

#[test]
fn footers_update_by_name_and_page_attach() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/footers/")
        .with_body(r#"{"results":[{"id":1,"name":"Default Footer","default":true,"content":"<p>old</p>"}]}"#)
        .expect_at_least(1)
        .create();
    server
        .mock("GET", "/api/v3/content/footers/1/")
        .with_body(r#"{"id":1,"name":"Default Footer","default":true,"content":"<p>old</p>","used_by":["buying"]}"#)
        .expect_at_least(1)
        .create();
    let patch = server
        .mock("PATCH", "/api/v3/content/footers/1/")
        .match_body(Matcher::Json(json!({ "content": "<p><strong>Jordan</strong></p>" })))
        .with_body(r#"{"id":1,"name":"Default Footer","default":true,"content":"<p><strong>Jordan</strong></p>","used_by":[]}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "footers",
            "update",
            "default footer",
            "--markdown",
            "--content",
            "**Jordan**",
        ])
        .assert()
        .success();
    patch.assert();

    server
        .mock("GET", "/api/v3/content/pages/7/")
        .with_body(r#"{"id":7,"path":"/buying/"}"#)
        .create();
    let page = server
        .mock("PATCH", "/api/v3/content/pages/7/")
        .match_body(Matcher::Json(json!({
            "footer": 1,
            "search_form_type": "typeahead",
            "search_field_defaults": { "city": ["Riverside"] }
        })))
        .with_body(r#"{"id":7,"path":"/buying/","url":"https://x/buying/"}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "pages",
            "update",
            "7",
            "--footer",
            "Default Footer",
            "--search-form-type",
            "typeahead",
            "--search-field-defaults-criteria",
            "city=Riverside",
        ])
        .assert()
        .success();
    page.assert();
}

#[test]
fn home_page_display_options() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let patch = server
        .mock("PATCH", "/api/v3/content/home-page/")
        .match_body(Matcher::Json(json!({
            "property_display_type": "grid",
            "search_form_tabs": false,
            "search_image": null,
            "number_of_properties": 24,
            "search_field_defaults": "4yt"
        })))
        .with_body(r#"{"id":1,"url":"https://x/"}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "home-page",
            "update",
            "--property-display-type",
            "grid",
            "--search-form-tabs",
            "false",
            "--search-image",
            "null",
            "--number-of-properties",
            "24",
            "--search-field-defaults",
            "4yt",
        ])
        .assert()
        .success();
    patch.assert();
}

#[test]
fn home_page_featured_agents() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let lookup = server
        .mock("GET", "/api/v3/content/pages/")
        .match_query(Matcher::UrlEncoded("path".into(), "/dana/".into()))
        .with_body(r#"{"results":[{"id":90,"path":"/dana/"}]}"#)
        .create();
    let set = server
        .mock("PATCH", "/api/v3/content/home-page/")
        .match_body(Matcher::Json(json!({ "featured_agents": [86, 90] })))
        .with_body(r#"{"id":1,"url":"https://x/","featured_agents":[86,90]}"#)
        .create();
    cmd(&server, &dir)
        .args(["home-page", "update", "--featured-agents", "86, /dana/"])
        .assert()
        .success();
    lookup.assert();
    set.assert();

    let clear = server
        .mock("PATCH", "/api/v3/content/home-page/")
        .match_body(Matcher::Json(json!({ "featured_agents": [] })))
        .with_body(r#"{"id":1,"url":"https://x/","featured_agents":[]}"#)
        .create();
    cmd(&server, &dir)
        .args(["home-page", "update", "--featured-agents", "null"])
        .assert()
        .success();
    clear.assert();
}

#[test]
fn design_set_and_preview() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let patch = server
        .mock("PATCH", "/api/v3/design/")
        .match_body(Matcher::Json(json!({ "template": "anna-modern", "variation": "coastal", "vars": { "palette-brand-color": "#0066A7" } })))
        .with_body(r##"{"template":"anna-modern","template_family":"templates4","styles":{"name":"coastal","vars":{"palette-brand-color":"#0066A7"}},"previous_template":"molly","consistent":true}"##)
        .create();
    let out = cmd(&server, &dir)
        .args([
            "design",
            "set",
            "--template",
            "anna-modern",
            "--variation",
            "coastal",
            "--var",
            "palette-brand-color=#0066A7",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["styles"]["name"], "coastal");
    patch.assert();

    let preview = server
        .mock("POST", "/api/v3/design/preview/")
        .match_body(Matcher::Json(json!({ "template": "molly" })))
        .with_body(r#"{"template":"molly","styles":{"name":"default","vars":{}},"preview_url":"https://www.live-domain.com/?DESIGN=molly&zvars=abc"}"#)
        .create();
    cmd(&server, &dir)
        .args(["design", "preview", "--template", "molly", "-q"])
        .assert()
        .success()
        .stdout(format!("{}/?DESIGN=molly&zvars=abc\n", server.url()));
    preview.assert();

    cmd(&server, &dir).args(["design", "set"]).assert().code(2);
}

#[test]
fn pages_create_agent_detail_with_areas() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let post = server
        .mock("POST", "/api/v3/content/pages/")
        .match_body(Matcher::Json(json!({
            "slug": "dana-whitfield", "anchor_text": "Dana Whitfield", "template": "Agent Detail Page",
            "extra_content": { "Agent_Name": "Dana Whitfield", "Agent_Photo": "https://u/x.jpg", "Fax_Number": null }
        })))
        .with_status(201)
        .with_body(r#"{"id":9,"path":"/dana-whitfield/","url":"https://x/dana-whitfield/"}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "pages",
            "create",
            "--slug",
            "dana-whitfield",
            "--anchor-text",
            "Dana Whitfield",
            "--template",
            "Agent Detail Page",
            "--area",
            "Agent Name=Dana Whitfield",
            "--area",
            "Agent_Photo=https://u/x.jpg",
            "--area",
            "Fax Number=null",
        ])
        .assert()
        .success();
    post.assert();
    server
        .mock("GET", "/api/v3/content/pages/9/")
        .with_body(r#"{"id":9,"path":"/dana-whitfield/"}"#)
        .create();
    cmd(&server, &dir)
        .args(["pages", "update", "9", "--area", "nonsense"])
        .assert()
        .code(2);
}

#[test]
fn agent_pages_create_with_agent_id_and_list_agents() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let post = server
        .mock("POST", "/api/v3/content/agent-pages/")
        .match_body(Matcher::Json(json!({
            "slug": "riley", "anchor_text": "Riley", "template": "Agent Detail Page", "agent_id": "42",
            "extra_content": { "Agent_Name": "Riley" }
        })))
        .with_status(201)
        .with_body(r#"{"id":5,"path":"/riley/","url":"https://x/riley/","agent_id":"42","agent_name":"Riley P"}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "agent-pages",
            "create",
            "--slug",
            "riley",
            "--anchor-text",
            "Riley",
            "--template",
            "Agent Detail Page",
            "--agent-id",
            "42",
            "--area",
            "Agent Name=Riley",
        ])
        .assert()
        .success();
    post.assert();

    server
        .mock("GET", "/api/v3/content/agents/")
        .with_body(r#"{"results":[{"id":"42","name":"Riley P"}]}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["agents"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["results"][0]["name"], "Riley P");

    server
        .mock("GET", "/api/v3/content/agent-pages/5/")
        .with_body(r#"{"id":5,"path":"/riley/"}"#)
        .create();
    let del = server
        .mock("DELETE", "/api/v3/content/agent-pages/5/")
        .with_status(204)
        .create();
    cmd(&server, &dir)
        .args(["agent-pages", "delete", "5"])
        .assert()
        .success();
    del.assert();
}

#[test]
fn revisions_list_preview_and_revert() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/home-page/revisions/")
        .with_body(r#"{"results":[{"id":325,"at":"2026-09-03T02:20:39+00:00","by":"API key x","action":"changed","changed_fields":["content"],"revertible":true},{"id":1,"at":"2020-01-01T00:00:00+00:00","by":"admin","action":"created","changed_fields":[],"revertible":false}]}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["home-page", "revisions", "--limit", "1", "-q"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(String::from_utf8_lossy(&out), "325\n");

    server
        .mock("GET", "/api/v3/content/home-page/revisions/325/")
        .with_body(r#"{"id":325,"action":"changed","by":"API key x","at":"t","revertible":true,"preview":{"content":{"now":"<p>new</p>","restored":"<p>old</p>"}}}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["home-page", "revision", "325"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["preview"]["content"]["restored"], "<p>old</p>");

    server
        .mock("GET", "/api/v3/content/pages/7/")
        .with_body(r#"{"id":7,"path":"/buying/"}"#)
        .create();
    let revert = server
        .mock("POST", "/api/v3/content/pages/7/revisions/300/revert/")
        .with_body(r#"{"id":7,"path":"/buying/","title":"restored"}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["pages", "revert", "7", "300", "-y"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["title"], "restored");
    revert.assert();
}

#[test]
fn featured_group_tiles_and_home_page_attach() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let post = server
        .mock("POST", "/api/v3/content/featured-pages/")
        .match_body(Matcher::Json(
            json!({ "title": "Where We Work", "blurb": "Six areas" }),
        ))
        .with_status(201)
        .with_body(r#"{"id":3,"title":"Where We Work","blurb":"Six areas","tiles":[]}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "featured",
            "create",
            "--title",
            "Where We Work",
            "--blurb",
            "Six areas",
        ])
        .assert()
        .success();
    post.assert();

    server
        .mock("GET", "/api/v3/content/featured-pages/3/")
        .with_body(r#"{"id":3,"title":"Where We Work","tiles":[]}"#)
        .create();
    let tile = server
        .mock("POST", "/api/v3/content/featured-pages/3/tiles/")
        .match_body(Matcher::Json(json!({ "title": "Riverside", "link": "/riverside/", "cta": "View Homes", "image": "https://u/j.jpg" })))
        .with_status(201)
        .with_body(r#"{"id":3,"title":"Where We Work","tiles":[{"id":9,"title":"Riverside","link":"/riverside/","cta":"View Homes","image":"https://u/j.jpg"}]}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "featured",
            "add-tile",
            "3",
            "--title",
            "Riverside",
            "--link",
            "/riverside/",
            "--image",
            "https://u/j.jpg",
        ])
        .assert()
        .success();
    tile.assert();

    server
        .mock("GET", "/api/v3/content/featured-pages/")
        .with_body(r#"{"results":[{"id":3,"title":"Where We Work","tiles":[]}]}"#)
        .create();
    let home = server
        .mock("PATCH", "/api/v3/content/home-page/")
        .match_body(Matcher::Json(
            json!({ "tile_group": 3, "landscape_image_override": "none" }),
        ))
        .with_body(r#"{"id":1,"url":"https://x/","tile_group":{"id":3,"title":"Where We Work"}}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "home-page",
            "update",
            "--tile-group",
            "where we work",
            "--landscape",
            "none",
        ])
        .assert()
        .success();
    home.assert();

    server
        .mock("GET", "/api/v3/content/area-pages/23/")
        .with_body(r#"{"id":23,"path":"/riverside/"}"#)
        .create();
    let area = server
        .mock("PATCH", "/api/v3/content/area-pages/23/")
        .match_body(Matcher::Json(json!({ "landscape_image_override": "https://u/j.jpg", "landscape_image_override_alt_text": "Riverside harbor" })))
        .with_body(r#"{"id":23,"path":"/riverside/","url":"https://x/riverside/"}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "area-pages",
            "update",
            "23",
            "--landscape",
            "https://u/j.jpg",
            "--landscape-alt",
            "Riverside harbor",
        ])
        .assert()
        .success();
    area.assert();
}

#[test]
fn page_heading_is_an_alias_for_search_header() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let home = server
        .mock("PATCH", "/api/v3/content/home-page/")
        .match_body(Matcher::Json(
            json!({"search_header": "Riverside Homes for Sale"}),
        ))
        .with_body(r#"{"id":1,"url":"https://x/"}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "home-page",
            "update",
            "--page-heading",
            "Riverside Homes for Sale",
        ])
        .assert()
        .success();
    home.assert();

    server
        .mock("GET", "/api/v3/content/pages/7/")
        .with_body(r#"{"id":7,"path":"/buying/"}"#)
        .create();
    let page = server
        .mock("PATCH", "/api/v3/content/pages/7/")
        .match_body(Matcher::Json(
            json!({"search_header": "Buying in Riverside"}),
        ))
        .with_body(r#"{"id":7,"path":"/buying/","url":"https://x/buying/"}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "pages",
            "update",
            "7",
            "--page-heading",
            "Buying in Riverside",
        ])
        .assert()
        .success();
    page.assert();
}

#[test]
fn banners_create_update_by_name_and_page_attach() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let post = server
        .mock("POST", "/api/v3/content/banners/")
        .match_body(Matcher::Json(json!({
            "name": "Open house",
            "message": "Open house Saturday",
            "url": "/open-house/",
            "call_to_action": "See details"
        })))
        .with_status(201)
        .with_body(r#"{"id":4,"name":"Open house","used_by":[],"used_by_count":0}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "banners",
            "create",
            "--name",
            "Open house",
            "--message",
            "Open house Saturday",
            "--url",
            "/open-house/",
            "--cta",
            "See details",
        ])
        .assert()
        .success();
    post.assert();
    cmd(&server, &dir)
        .args(["banners", "create", "--name", "No button"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--url is required"));

    server
        .mock("GET", "/api/v3/content/banners/")
        .with_body(r#"{"results":[{"id":4,"name":"Open house"}]}"#)
        .expect_at_least(1)
        .create();
    server
        .mock("GET", "/api/v3/content/banners/4/")
        .with_body(r#"{"id":4,"name":"Open house","used_by":["buying"],"used_by_count":1}"#)
        .expect_at_least(1)
        .create();
    let patch = server
        .mock("PATCH", "/api/v3/content/banners/4/")
        .match_body(Matcher::Json(json!({ "message": "Open house Sunday" })))
        .with_body(r#"{"id":4,"name":"Open house","message":"Open house Sunday"}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "banners",
            "update",
            "open house",
            "--message",
            "Open house Sunday",
        ])
        .assert()
        .success();
    patch.assert();

    let attach = server
        .mock("PATCH", "/api/v3/content/pages/7/")
        .match_body(Matcher::Json(json!({ "banner": 4 })))
        .with_body(r#"{"id":7,"path":"/buying/","url":"https://x/buying/"}"#)
        .create();
    cmd(&server, &dir)
        .args(["pages", "update", "7", "--banner", "Open house"])
        .assert()
        .success();
    attach.assert();
    let detach = server
        .mock("PATCH", "/api/v3/content/home-page/")
        .match_body(Matcher::Json(json!({ "banner": null })))
        .with_body(r#"{"id":1,"url":"https://x/"}"#)
        .create();
    cmd(&server, &dir)
        .args(["home-page", "update", "--banner", "null"])
        .assert()
        .success();
    detach.assert();

    let delete = server
        .mock("DELETE", "/api/v3/content/banners/4/")
        .match_query(Matcher::UrlEncoded("force".into(), "true".into()))
        .with_status(204)
        .create();
    cmd(&server, &dir)
        .args(["banners", "delete", "4", "--force", "--yes"])
        .assert()
        .success();
    delete.assert();
}

#[test]
fn market_reports_create_update_and_require_a_search() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    cmd(&server, &dir)
        .args([
            "market-reports",
            "create",
            "--slug",
            "riverside-market",
            "--anchor-text",
            "Riverside Market",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--search-criteria or --search"));

    let post = server
        .mock("POST", "/api/v3/content/market-report-pages/")
        .match_body(Matcher::Json(json!({
            "slug": "riverside-market",
            "anchor_text": "Riverside Market",
            "search": { "city": ["Riverside"] },
            "header": "Riverside this month"
        })))
        .with_status(201)
        .with_body(r#"{"id":9,"path":"/riverside-market/","sold_within":6}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "market-reports",
            "create",
            "--slug",
            "/riverside-market/",
            "--anchor-text",
            "Riverside Market",
            "--search-criteria",
            "city=Riverside",
            "--header",
            "Riverside this month",
        ])
        .assert()
        .success();
    post.assert();

    let lookup = server
        .mock("GET", "/api/v3/content/market-report-pages/")
        .match_query(Matcher::UrlEncoded(
            "path".into(),
            "/riverside-market/".into(),
        ))
        .with_body(r#"{"results":[{"id":9,"path":"/riverside-market/"}]}"#)
        .create();
    let patch = server
        .mock("PATCH", "/api/v3/content/market-report-pages/9/")
        .match_body(Matcher::Json(json!({
            "sold_within": 12,
            "number_of_properties": 10
        })))
        .with_body(r#"{"id":9,"path":"/riverside-market/","sold_within":12}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "market-reports",
            "update",
            "/riverside-market/",
            "--sold-within",
            "12",
            "--number-of-properties",
            "10",
        ])
        .assert()
        .success();
    lookup.assert();
    patch.assert();
}

#[test]
fn valuation_pages_create_update_and_explain_seller_leads_off() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let post = server
        .mock("POST", "/api/v3/content/valuation-pages/")
        .match_body(Matcher::Json(json!({
            "slug": "riverside-home-value",
            "anchor_text": "Riverside Home Value",
            "search_headline": "<h3>What is your Riverside home worth?</h3>",
            "search_input_placeholder": "e.g. 4120 Main St",
            "landscape_image_override": "none"
        })))
        .with_status(201)
        .with_body(r#"{"id":5,"path":"/riverside-home-value/"}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "valuation-pages",
            "create",
            "--slug",
            "/riverside-home-value/",
            "--anchor-text",
            "Riverside Home Value",
            "--headline",
            "<h3>What is your Riverside home worth?</h3>",
            "--placeholder",
            "e.g. 4120 Main St",
            "--landscape",
            "hide",
        ])
        .assert()
        .success();
    post.assert();

    let lookup = server
        .mock("GET", "/api/v3/content/valuation-pages/")
        .match_query(Matcher::UrlEncoded(
            "path".into(),
            "/riverside-home-value/".into(),
        ))
        .with_body(r#"{"results":[{"id":5,"path":"/riverside-home-value/"}]}"#)
        .create();
    let patch = server
        .mock("PATCH", "/api/v3/content/valuation-pages/5/")
        .match_body(Matcher::Json(json!({ "search_input_placeholder": "" })))
        .with_status(409)
        .with_body(
            r#"{"error":{"code":"valuation_disabled","message":"Seller Leads is turned off on this site"}}"#,
        )
        .create();
    cmd(&server, &dir)
        .args([
            "valuation-pages",
            "update",
            "/riverside-home-value/",
            "--search-input-placeholder",
            "",
        ])
        .assert()
        .code(6)
        .stderr(predicates::str::contains("/admin/valuation/"));
    lookup.assert();
    patch.assert();
}

#[test]
fn sidebars_list_table_create_rename_item_update_remove_and_set_items() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/sidebars/")
        .match_query(Matcher::UrlEncoded("q".into(), "area".into()))
        .with_body(r#"{"results":[{"id":5,"name":"Area Sidebar","special":true,"items":[{"id":50},{"id":51}],"used_by":["a"]}]}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["sidebars", "list", "-s", "area"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["results"][0]["items_count"], 2);

    let rich_sidebar = json!({
        "id": 5,
        "name": "Area Sidebar",
        "special": true,
        "used_by": ["p1", "p2", "p3", "p4", "p5", "p6"],
        "used_by_count": 8,
        "items": [
            {"id": 50, "order": 0, "type": "html", "html": format!("<p>{}</p>", "x".repeat(100))},
            {"id": 51, "order": 1, "type": "links", "header": null, "links": [{"anchor": "A", "url": "/a/"}]}
        ]
    })
    .to_string();
    server
        .mock("GET", "/api/v3/content/sidebars/5/")
        .with_body(&rich_sidebar)
        .expect_at_least(1)
        .create();
    cmd(&server, &dir)
        .args(["-o", "table", "sidebars", "get", "5"])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("built-in").and(predicate::str::contains(
                "8 page(s): p1, p2, p3, p4, p5, +3 more",
            )),
        )
        .stdout(predicate::str::contains("(no header): 1 link(s)"));

    let create = server
        .mock("POST", "/api/v3/content/sidebars/")
        .match_body(Matcher::Json(json!({
            "name": "Blog",
            "items": [{"type": "html", "html": "<p>Hi</p>"}]
        })))
        .with_status(201)
        .with_body(r#"{"id":6,"name":"Blog","items":[{"id":60}]}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "sidebars",
            "create",
            "--name",
            "Blog",
            "--data",
            r#"[{"type":"html","html":"<p>Hi</p>"}]"#,
        ])
        .assert()
        .success();
    create.assert();

    let rename = server
        .mock("PATCH", "/api/v3/content/sidebars/5/")
        .match_body(Matcher::Json(json!({ "name": "Renamed" })))
        .with_body(r#"{"id":5,"name":"Renamed","items":[]}"#)
        .create();
    cmd(&server, &dir)
        .args(["sidebars", "rename", "5", "--name", "Renamed"])
        .assert()
        .success();
    rename.assert();

    server
        .mock("GET", "/api/v3/content/sidebars/5/items/51/")
        .with_body(r#"{"id":51,"type":"links","header":null,"links":[]}"#)
        .expect(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["sidebars", "item", "5", "51"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["id"], 51);

    // validation errors on add-html / update-item / set-items
    cmd(&server, &dir)
        .args(["sidebars", "add-html", "5"])
        .assert()
        .code(2);
    cmd(&server, &dir)
        .args(["sidebars", "update-item", "5", "51"])
        .assert()
        .code(2);
    cmd(&server, &dir)
        .args(["sidebars", "update-item", "5", "51", "--header-url", "/x/"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--header-url needs a header"));

    let patch_item = server
        .mock("PATCH", "/api/v3/content/sidebars/5/items/51/")
        .match_body(Matcher::Json(json!({
            "type": "links",
            "header": null,
            "links": [{"anchor": "Home", "url": "/"}]
        })))
        .with_body(r#"{"id":51}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "sidebars",
            "update-item",
            "5",
            "51",
            "--no-header",
            "--link",
            "Home=/",
        ])
        .assert()
        .success();
    patch_item.assert();

    let del_item = server
        .mock("DELETE", "/api/v3/content/sidebars/5/items/51/")
        .with_status(204)
        .create();
    let out = cmd(&server, &dir)
        .args(["sidebars", "remove-item", "5", "51"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["deleted"], true);
    del_item.assert();

    let items_file = dir.path().join("items.json");
    std::fs::write(
        &items_file,
        r#"{"items":[{"type":"html","html":"<p>A</p>"}]}"#,
    )
    .unwrap();
    let put_items = server
        .mock("PUT", "/api/v3/content/sidebars/5/items/")
        .match_body(Matcher::Json(
            json!({ "items": [{"type": "html", "html": "<p>A</p>"}] }),
        ))
        .with_body(&rich_sidebar)
        .create();
    cmd(&server, &dir)
        .args(["sidebars", "set-items", "5", "--data"])
        .arg(format!("@{}", items_file.display()))
        .assert()
        .success();
    put_items.assert();

    cmd(&server, &dir)
        .args(["sidebars", "set-items", "5", "--data", "{}"])
        .assert()
        .code(2);
}

#[test]
fn featured_list_get_update_tiles_and_delete() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/featured-pages/")
        .with_body(r#"{"results":[{"id":3,"title":"Areas","blurb":"Top","tiles":[{"id":9,"title":"Riverside","link":"/riverside/","cta":"Go","image":null}]}]}"#)
        .expect_at_least(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["featured", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["results"][0]["tiles_count"], 1);

    let group = r#"{"id":3,"title":"Areas","blurb":"Top","tiles":[{"id":9,"title":"Riverside","link":"/riverside/","cta":"Go","image":null}]}"#;
    server
        .mock("GET", "/api/v3/content/featured-pages/3/")
        .with_body(group)
        .expect_at_least(1)
        .create();
    cmd(&server, &dir)
        .args(["featured", "get", "3", "-q"])
        .assert()
        .success()
        .stdout("3\n");
    cmd(&server, &dir)
        .args(["-o", "table", "featured", "get", "3"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Areas (id 3)"))
        .stdout(predicate::str::contains("Riverside"));
    cmd(&server, &dir)
        .args(["featured", "get", "Missing"])
        .assert()
        .code(4);

    let create = server
        .mock("POST", "/api/v3/content/featured-pages/")
        .match_body(Matcher::Json(json!({
            "title": "New Group",
            "tiles": [{"title": "Downtown", "link": "/downtown/"}]
        })))
        .with_status(201)
        .with_body(group)
        .create();
    cmd(&server, &dir)
        .args([
            "featured",
            "create",
            "--title",
            "New Group",
            "--data",
            r#"[{"title":"Downtown","link":"/downtown/"}]"#,
        ])
        .assert()
        .success();
    create.assert();

    cmd(&server, &dir)
        .args(["featured", "update", "3"])
        .assert()
        .code(2);
    let update = server
        .mock("PATCH", "/api/v3/content/featured-pages/3/")
        .match_body(Matcher::Json(json!({ "title": "Updated", "blurb": "Sub" })))
        .with_body(group)
        .create();
    cmd(&server, &dir)
        .args([
            "featured", "update", "3", "--title", "Updated", "--blurb", "Sub",
        ])
        .assert()
        .success();
    update.assert();

    cmd(&server, &dir)
        .args(["featured", "update-tile", "3", "9"])
        .assert()
        .code(2);
    let update_tile = server
        .mock("PATCH", "/api/v3/content/featured-pages/3/tiles/9/")
        .match_body(Matcher::Json(json!({ "title": "Bay", "image": null })))
        .with_body(r#"{"id":9,"title":"Bay","link":"/riverside/","cta":"Go","image":null}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "featured",
            "update-tile",
            "3",
            "9",
            "--title",
            "Bay",
            "--image",
            "null",
        ])
        .assert()
        .success();
    update_tile.assert();

    let remove_tile = server
        .mock("DELETE", "/api/v3/content/featured-pages/3/tiles/9/")
        .with_status(204)
        .create();
    let out = cmd(&server, &dir)
        .args(["featured", "remove-tile", "3", "9"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["deleted"], true);
    remove_tile.assert();

    let tiles_file = dir.path().join("tiles.json");
    std::fs::write(&tiles_file, r#"{"tiles":[{"title":"A","link":"/a/"}]}"#).unwrap();
    let set_tiles = server
        .mock("PUT", "/api/v3/content/featured-pages/3/tiles/")
        .match_body(Matcher::Json(
            json!({ "tiles": [{"title": "A", "link": "/a/"}] }),
        ))
        .with_body(group)
        .create();
    cmd(&server, &dir)
        .args(["featured", "set-tiles", "3", "--data"])
        .arg(format!("@{}", tiles_file.display()))
        .assert()
        .success();
    set_tiles.assert();
    cmd(&server, &dir)
        .args(["featured", "set-tiles", "3", "--data", "{}"])
        .assert()
        .code(2);

    let delete = server
        .mock("DELETE", "/api/v3/content/featured-pages/3/")
        .match_query(Matcher::UrlEncoded("force".into(), "true".into()))
        .with_status(204)
        .create();
    let out = cmd(&server, &dir)
        .args(["featured", "delete", "3", "--force", "-y"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["deleted"], true);
    delete.assert();
}

#[test]
fn design_get_templates_variation_preview_and_snapshot_guard() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let current = r##"{"template":"anna-modern","styles":{"name":"coastal","vars":{"palette-brand-color":"#0066A7"}}}"##;
    server
        .mock("GET", "/api/v3/design/")
        .with_body(current)
        .expect(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["design", "get"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["template"], "anna-modern");
    cmd(&server, &dir)
        .args(["-o", "table", "design", "get"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("coastal")
                .and(predicate::str::contains("palette-brand-color")),
        );

    server
        .mock("GET", "/api/v3/design/templates/")
        .with_body(
            r#"{"results":[
            {"name":"anna-modern","variations":["a","b","c","d","e","f"]},
            {"name":"molly","variations":["classic","dark"]}
        ]}"#,
        )
        .create();
    let out = cmd(&server, &dir)
        .args(["design", "templates"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let templates = parse(&out);
    assert_eq!(templates["results"][0]["variations_count"], 6);
    assert_eq!(
        templates["results"][0]["variations_sample"],
        "a, b, c, d, +2 more"
    );
    assert_eq!(
        templates["results"][1]["variations_sample"],
        "classic, dark"
    );

    server
        .mock(
            "GET",
            "/api/v3/design/templates/anna-modern/variations/coastal/",
        )
        .with_body(r##"{"template":"anna-modern","name":"coastal","vars":{"brand":"#0066A7"}}"##)
        .expect(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["design", "variation", "anna-modern", "coastal"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["vars"]["brand"], "#0066A7");
    cmd(&server, &dir)
        .args([
            "-o",
            "table",
            "design",
            "variation",
            "anna-modern",
            "coastal",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("anna-modern / coastal"))
        .stdout(predicate::str::contains("#0066A7"));

    server
        .mock("POST", "/api/v3/design/preview/")
        .with_body(r#"{"template":"molly","styles":{"name":"classic","vars":{}},"preview_url":"https://live.example/?DESIGN=molly"}"#)
        .expect(3)
        .create();
    let out = cmd(&server, &dir)
        .args(["design", "preview", "--template", "molly"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        parse(&out)["preview_url"],
        format!("{}/?DESIGN=molly", server.url())
    );
    cmd(&server, &dir)
        .args(["-o", "table", "design", "preview", "--template", "molly"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Preview of molly / classic"))
        .stdout(predicate::str::contains("/?DESIGN=molly"));
    cmd(&server, &dir)
        .args([
            "design",
            "preview",
            "--template",
            "molly",
            "--variation",
            "classic",
            "--snapshot",
            "out.png",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("signed in to the site's admin"));
}

#[test]
fn nav_list_table_contact_set_clear_and_link_errors() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/navigation-bars/")
        .with_body(BARS)
        .create();
    let out = cmd(&server, &dir)
        .args(["nav", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["results"][0]["links_count"], 3);

    let dup_bar = r#"{"id":1,"type":"top_primary","label":"Primary","links":[
        {"id":10,"type":"custom","url":"/a/","anchor_text":"Same","nofollow":false,"order":0},
        {"id":11,"type":"custom","url":"/b/","anchor_text":"Same","nofollow":false,"order":1}
    ]}"#;
    server
        .mock("GET", "/api/v3/content/navigation-bars/1/")
        .with_body(dup_bar)
        .expect_at_least(1)
        .create();
    cmd(&server, &dir)
        .args(["-o", "table", "nav", "get", "1"])
        .assert()
        .success()
        .stderr(predicate::str::contains(r#"Primary (id 1, "top_primary")"#))
        .stdout(predicate::str::contains("/a/"));
    cmd(&server, &dir)
        .args(["nav", "get", "1", "-q"])
        .assert()
        .success()
        .stdout("10\n11\n");

    let add_contact = server
        .mock("POST", "/api/v3/content/navigation-bars/1/links/")
        .match_body(Matcher::Json(
            json!({ "type": "contact", "nofollow": false }),
        ))
        .with_status(201)
        .with_body(dup_bar)
        .create();
    cmd(&server, &dir)
        .args(["nav", "add", "1", "--contact"])
        .assert()
        .success();
    add_contact.assert();

    cmd(&server, &dir)
        .args(["nav", "add", "1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--url and --text are required"));
    cmd(&server, &dir)
        .args(["nav", "update", "1", "10"])
        .assert()
        .code(2);
    cmd(&server, &dir)
        .args(["nav", "update", "1", "999", "--text", "X"])
        .assert()
        .code(4);
    cmd(&server, &dir)
        .args(["nav", "update", "1", "Missing", "--text", "X"])
        .assert()
        .code(4);
    cmd(&server, &dir)
        .args(["nav", "update", "1", "Same", "--text", "X"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("matches 2 links"));

    // exact URL disambiguates links sharing anchor text
    let patch_by_url = server
        .mock("PATCH", "/api/v3/content/navigation-bars/1/links/11/")
        .match_body(Matcher::Json(json!({ "anchor_text": "Second" })))
        .with_body(r#"{"id":11,"anchor_text":"Second"}"#)
        .create();
    cmd(&server, &dir)
        .args(["nav", "update", "1", "/b/", "--text", "Second"])
        .assert()
        .success();
    patch_by_url.assert();

    let links_file = dir.path().join("links.json");
    std::fs::write(
        &links_file,
        r#"{"links":[{"type":"custom","url":"/","anchor_text":"Home"}]}"#,
    )
    .unwrap();
    let set = server
        .mock("PUT", "/api/v3/content/navigation-bars/1/links/")
        .match_body(Matcher::Json(
            json!({ "links": [{"type": "custom", "url": "/", "anchor_text": "Home"}] }),
        ))
        .with_body(dup_bar)
        .create();
    cmd(&server, &dir)
        .args(["nav", "set", "1", "--data"])
        .arg(format!("@{}", links_file.display()))
        .assert()
        .success();
    set.assert();
    cmd(&server, &dir)
        .args(["nav", "set", "1", "--data", "{}"])
        .assert()
        .code(2);

    let clear = server
        .mock("PUT", "/api/v3/content/navigation-bars/1/links/")
        .match_body(Matcher::Json(json!({ "links": [] })))
        .with_body(r#"{"id":1,"type":"top_primary","label":"Primary","links":[]}"#)
        .create();
    cmd(&server, &dir)
        .args(["nav", "clear", "1", "-y"])
        .assert()
        .success();
    clear.assert();
}

#[test]
fn footers_and_banners_crud_and_errors() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let footer = r#"{"id":2,"name":"Team Footer","default":false,"content":"<p>Team</p>","used_by":[],"used_by_count":0}"#;
    server
        .mock("GET", "/api/v3/content/footers/")
        .with_body(format!(r#"{{"results":[{footer}]}}"#))
        .expect_at_least(2)
        .create();
    server
        .mock("GET", "/api/v3/content/footers/2/")
        .with_body(footer)
        .expect_at_least(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["footers", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["results"][0]["id"], 2);
    cmd(&server, &dir)
        .args(["footers", "get", "2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Team Footer"));
    cmd(&server, &dir)
        .args(["-o", "table", "footers", "get", "2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("<p>Team</p>"));
    cmd(&server, &dir)
        .args(["footers", "get", "No Such Footer"])
        .assert()
        .code(4);

    let create = server
        .mock("POST", "/api/v3/content/footers/")
        .match_body(Matcher::Json(json!({ "content": "<p>New</p>" })))
        .with_status(201)
        .with_body(footer)
        .create();
    cmd(&server, &dir)
        .args(["footers", "create", "--content", "<p>New</p>"])
        .assert()
        .success();
    create.assert();
    cmd(&server, &dir)
        .args(["footers", "create"])
        .assert()
        .code(2);

    server
        .mock("GET", "/api/v3/content/footers/2/revisions/10/")
        .with_body(r#"{"id":10,"action":"changed","revertible":true,"preview":{}}"#)
        .create();
    cmd(&server, &dir)
        .args(["footers", "revision", "2", "10"])
        .assert()
        .success();

    let del = server
        .mock("DELETE", "/api/v3/content/footers/2/")
        .match_query(Matcher::UrlEncoded("force".into(), "true".into()))
        .with_status(204)
        .create();
    let out = cmd(&server, &dir)
        .args(["footers", "delete", "2", "--force", "-y"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["deleted"], true);
    del.assert();

    // banners list, get, missing name, empty update
    server
        .mock("GET", "/api/v3/content/banners/")
        .with_body(r#"{"results":[{"id":4,"name":"Open house","message":"Sat","call_to_action":"Go","url":"/open/"}]}"#)
        .expect(2)
        .create();
    server
        .mock("GET", "/api/v3/content/banners/4/")
        .with_body(r#"{"id":4,"name":"Open house","used_by":[]}"#)
        .expect(2)
        .create();
    let out = cmd(&server, &dir)
        .args(["banners", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["results"][0]["id"], 4);
    cmd(&server, &dir)
        .args(["banners", "get", "4"])
        .assert()
        .success();
    cmd(&server, &dir)
        .args(["banners", "get", "Missing"])
        .assert()
        .code(4);
    cmd(&server, &dir)
        .args(["banners", "update", "4"])
        .assert()
        .code(2);
}

#[test]
fn agent_market_and_valuation_pages_full_subcommands() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();

    // agent-pages: list, get, update (--agent-id null, --replace), revisions/revision/revert
    server
        .mock("GET", "/api/v3/content/agent-pages/")
        .match_query(Matcher::UrlEncoded(
            "template".into(),
            "Agent Detail Page".into(),
        ))
        .with_body(r#"{"results":[{"id":5,"path":"/riley/","agent_id":"42"}],"pagination":{}}"#)
        .create();
    server
        .mock("GET", "/api/v3/content/agent-pages/5/")
        .with_body(r#"{"id":5,"path":"/riley/","agent_id":"42"}"#)
        .create();
    let agent_put = server
        .mock("PUT", "/api/v3/content/agent-pages/5/")
        .match_body(Matcher::Json(json!({ "agent_id": null, "title": "Riley" })))
        .with_body(r#"{"id":5,"path":"/riley/","agent_id":null}"#)
        .create();
    server
        .mock("GET", "/api/v3/content/agent-pages/5/revisions/")
        .with_body(r#"{"results":[{"id":1,"action":"changed","revertible":true}]}"#)
        .create();
    server
        .mock("GET", "/api/v3/content/agent-pages/5/revisions/1/")
        .with_body(r#"{"id":1,"action":"changed","preview":{}}"#)
        .create();
    server
        .mock("POST", "/api/v3/content/agent-pages/5/revisions/1/revert/")
        .with_body(r#"{"id":5}"#)
        .create();

    cmd(&server, &dir)
        .args(["agent-pages", "list", "--template", "Agent Detail Page"])
        .assert()
        .success();
    cmd(&server, &dir)
        .args(["agent-pages", "get", "5"])
        .assert()
        .success();
    cmd(&server, &dir)
        .args([
            "agent-pages",
            "update",
            "5",
            "--agent-id",
            "null",
            "--title",
            "Riley",
            "--replace",
        ])
        .assert()
        .success();
    agent_put.assert();
    cmd(&server, &dir)
        .args(["agent-pages", "revisions", "5"])
        .assert()
        .success();
    cmd(&server, &dir)
        .args(["agent-pages", "revision", "5", "1"])
        .assert()
        .success();
    cmd(&server, &dir)
        .args(["-y", "agent-pages", "revert", "5", "1"])
        .assert()
        .success();

    // market-reports: list, get, create (parent/content/search/defaults/footer), delete, revisions/revision/revert
    server
        .mock("GET", "/api/v3/content/market-report-pages/")
        .with_body(r#"{"results":[{"id":9,"path":"/mr/"}],"pagination":{}}"#)
        .create();
    server
        .mock("GET", "/api/v3/content/market-report-pages/9/")
        .with_body(r#"{"id":9,"path":"/mr/"}"#)
        .expect(2)
        .create();
    server
        .mock("GET", "/api/v3/content/footers/")
        .with_body(r#"{"results":[{"id":2,"name":"Report Footer"}]}"#)
        .create();
    server
        .mock("GET", "/api/v3/content/footers/2/")
        .with_body(r#"{"id":2,"name":"Report Footer"}"#)
        .create();
    let mr_post = server
        .mock("POST", "/api/v3/content/market-report-pages/")
        .match_body(Matcher::Json(json!({
            "slug": "sub-mr",
            "anchor_text": "Sub MR",
            "parent": 9,
            "content": "<p>Intro</p>",
            "search": 42,
            "search_field_defaults": { "city": ["Miami"] },
            "footer": 2
        })))
        .with_status(201)
        .with_body(r#"{"id":10,"path":"/mr/sub-mr/"}"#)
        .create();
    let mr_del = server
        .mock("DELETE", "/api/v3/content/market-report-pages/9/")
        .match_query(Matcher::UrlEncoded("orphan_children".into(), "true".into()))
        .with_status(204)
        .create();
    server
        .mock("GET", "/api/v3/content/market-report-pages/9/revisions/")
        .with_body(r#"{"results":[{"id":2}]}"#)
        .create();
    server
        .mock("GET", "/api/v3/content/market-report-pages/9/revisions/2/")
        .with_body(r#"{"id":2,"preview":{}}"#)
        .create();
    server
        .mock(
            "POST",
            "/api/v3/content/market-report-pages/9/revisions/2/revert/",
        )
        .with_body(r#"{"id":9}"#)
        .create();

    cmd(&server, &dir)
        .args(["market-reports", "list"])
        .assert()
        .success();
    cmd(&server, &dir)
        .args(["market-reports", "get", "9"])
        .assert()
        .success();
    cmd(&server, &dir)
        .args([
            "market-reports",
            "create",
            "--slug",
            "sub-mr",
            "--anchor-text",
            "Sub MR",
            "--parent",
            "9",
            "--content",
            "<p>Intro</p>",
            "--search",
            "42",
            "--search-field-defaults-criteria",
            "city=Miami",
            "--footer",
            "Report Footer",
        ])
        .assert()
        .success();
    mr_post.assert();
    cmd(&server, &dir)
        .args(["market-reports", "delete", "9", "--orphan-children", "-y"])
        .assert()
        .success();
    mr_del.assert();
    cmd(&server, &dir)
        .args(["market-reports", "revisions", "9"])
        .assert()
        .success();
    cmd(&server, &dir)
        .args(["market-reports", "revision", "9", "2"])
        .assert()
        .success();
    cmd(&server, &dir)
        .args(["-y", "market-reports", "revert", "9", "2"])
        .assert()
        .success();

    // valuation-pages: list, get, parent/landscape variants, delete, revisions/revision/revert
    server
        .mock("GET", "/api/v3/content/valuation-pages/")
        .with_body(r#"{"results":[{"id":5,"path":"/val/"}],"pagination":{}}"#)
        .create();
    server
        .mock("GET", "/api/v3/content/valuation-pages/5/")
        .with_body(r#"{"id":5,"path":"/val/"}"#)
        .expect(2)
        .create();
    let val_patch = server
        .mock("PATCH", "/api/v3/content/valuation-pages/5/")
        .match_body(Matcher::Json(json!({
            "parent": null,
            "landscape_image_override": null
        })))
        .with_body(r#"{"id":5}"#)
        .create();
    let val_url = server
        .mock("PATCH", "/api/v3/content/valuation-pages/5/")
        .match_body(Matcher::Json(
            json!({ "landscape_image_override": "https://u/bg.jpg" }),
        ))
        .with_body(r#"{"id":5}"#)
        .create();
    let val_del = server
        .mock("DELETE", "/api/v3/content/valuation-pages/5/")
        .match_query(Matcher::UrlEncoded("orphan_children".into(), "true".into()))
        .with_status(204)
        .create();
    server
        .mock("GET", "/api/v3/content/valuation-pages/5/revisions/")
        .with_body(r#"{"results":[{"id":3}]}"#)
        .create();
    server
        .mock("GET", "/api/v3/content/valuation-pages/5/revisions/3/")
        .with_body(r#"{"id":3,"preview":{}}"#)
        .create();
    server
        .mock(
            "POST",
            "/api/v3/content/valuation-pages/5/revisions/3/revert/",
        )
        .with_body(r#"{"id":5}"#)
        .create();

    cmd(&server, &dir)
        .args(["valuation-pages", "list"])
        .assert()
        .success();
    cmd(&server, &dir)
        .args(["valuation-pages", "get", "5"])
        .assert()
        .success();
    cmd(&server, &dir)
        .args([
            "valuation-pages",
            "update",
            "5",
            "--parent",
            "null",
            "--landscape",
            "default",
        ])
        .assert()
        .success();
    val_patch.assert();
    cmd(&server, &dir)
        .args([
            "valuation-pages",
            "update",
            "5",
            "--landscape",
            "https://u/bg.jpg",
        ])
        .assert()
        .success();
    val_url.assert();
    cmd(&server, &dir)
        .args(["valuation-pages", "delete", "5", "--orphan-children", "-y"])
        .assert()
        .success();
    val_del.assert();
    cmd(&server, &dir)
        .args(["valuation-pages", "revisions", "5"])
        .assert()
        .success();
    cmd(&server, &dir)
        .args(["valuation-pages", "revision", "5", "3"])
        .assert()
        .success();
    cmd(&server, &dir)
        .args(["-y", "valuation-pages", "revert", "5", "3"])
        .assert()
        .success();
}

#[test]
fn home_page_sections_items_update_variants_and_revert() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/home-page/sections/")
        .with_body(
            r#"{"sections":["press"],"empty_sections":{"press":"No logos yet"},"results":[]}"#,
        )
        .create();
    cmd(&server, &dir)
        .args(["home-page", "sections"])
        .assert()
        .success()
        .stderr(predicate::str::contains("press module is switched on"));

    server
        .mock("GET", "/api/v3/content/home-page/sections/tile/")
        .with_body(r#"{"kind":"tile","items":[]}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["home-page", "items", "get", "tile"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["kind"], "tile");

    let items_file = dir.path().join("tiles.json");
    std::fs::write(&items_file, r#"[{"title":"Buy"}]"#).unwrap();
    let put = server
        .mock("PUT", "/api/v3/content/home-page/sections/tile/")
        .match_body(Matcher::Json(json!({ "items": [{"title": "Buy"}] })))
        .with_body(r#"{"kind":"tile","items":[{"title":"Buy"}]}"#)
        .create();
    cmd(&server, &dir)
        .args(["home-page", "items", "set", "tile", "--data"])
        .arg(format!("@{}", items_file.display()))
        .assert()
        .success();
    put.assert();
    cmd(&server, &dir)
        .args(["home-page", "items", "set", "tile", "--data", "{}"])
        .assert()
        .code(2);

    let patch = server
        .mock("PATCH", "/api/v3/content/home-page/")
        .match_body(Matcher::Json(json!({
            "content": "<p>Welcome</p>",
            "property_display_type": null,
            "search_image": "https://u/logo.png",
            "tile_group": 3
        })))
        .with_body(r#"{"id":1,"url":"https://x/"}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "home-page",
            "update",
            "--content",
            "<p>Welcome</p>",
            "--property-display-type",
            "null",
            "--search-image",
            "https://u/logo.png",
            "--tile-group",
            "3",
        ])
        .assert()
        .success();
    patch.assert();
    cmd(&server, &dir)
        .args(["home-page", "update"])
        .assert()
        .code(2);

    let revert = server
        .mock("POST", "/api/v3/content/home-page/revisions/12/revert/")
        .with_body(r#"{"id":1}"#)
        .create();
    cmd(&server, &dir)
        .args(["-y", "home-page", "revert", "12"])
        .assert()
        .success();
    revert.assert();
}
