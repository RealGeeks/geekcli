//! `geekcli nav|sidebars|settings …` against a mock site.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use mockito::{Matcher, Server, ServerGuard};
use predicates::prelude::*;
use serde_json::{json, Value};

const KEY: &str = "rg_live_k";

fn cmd(server: &ServerGuard, dir: &tempfile::TempDir) -> Command {
    let mut c = Command::cargo_bin("geekcli").unwrap();
    c.env_clear()
        .env(
            "SYSTEMROOT",
            std::env::var("SYSTEMROOT").unwrap_or_default(),
        )
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("GEEKCLI_SITE", "www.test.local")
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

const SIDEBAR: &str = r#"{"id":5,"name":"Area Sidebar","special":false,"used_by":["jupiter"],"items":[
{"id":50,"order":0,"type":"html","html":"<h2>Hello</h2>"},
{"id":51,"order":1,"type":"links","header":{"text":"Areas","url":null},"links":[{"url":"/jupiter/","anchor":"Jupiter"}],"columns":1}]}"#;

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
            "links": [{"anchor": "Jupiter", "url": "/jupiter/"}, {"anchor": "Coming soon"}]
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
    assert_eq!(parse(&out)["used_by"][0], "jupiter");

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
            "Jupiter=/jupiter/",
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
        .with_body(r#"{"id":23,"path":"/ballenisles/"}"#)
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
            "search": { "subdivision": ["Ballenisles"], "type": ["res", "con"] }
        })))
        .with_body(r#"{"id":23,"path":"/ballenisles/","url":"https://x/ballenisles/","sidebar":{"id":1,"name":"Default Sidebar"},"search":{"id":9,"short_id":"9","description":"d","criteria":{}}}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "area-pages",
            "update",
            "23",
            "--sidebar",
            "Default Sidebar",
            "--search-criteria",
            "subdivision=Ballenisles",
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
        .with_body(r#"{"error":{"code":"conflict","message":"Sidebar is used by 1 page(s).","fields":{"used_by":["jupiter"]}}}"#)
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
    patch.assert();
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
            "search_field_defaults": { "city": ["Jupiter"] }
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
            "city=Jupiter",
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
        .with_body(r#"{"template":"molly","styles":{"name":"default","vars":{}},"preview_url":"https://x/?DESIGN=molly&zvars=abc"}"#)
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
fn design_preview_is_rehomed_onto_the_cli_base_url() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("POST", "/api/v3/design/preview/")
        .with_body(r#"{"template":"molly","styles":{"name":"d","vars":{}},"preview_url":"https://www.live-domain.com/?DESIGN=molly&zvars=abc"}"#)
        .create();
    cmd(&server, &dir)
        .args(["design", "preview", "--template", "molly", "-q"])
        .assert()
        .success()
        .stdout(format!("{}/?DESIGN=molly&zvars=abc\n", server.url()));
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
        .match_body(Matcher::Json(json!({ "title": "Jupiter", "link": "/jupiter/", "cta": "View Homes", "image": "https://u/j.jpg" })))
        .with_status(201)
        .with_body(r#"{"id":3,"title":"Where We Work","tiles":[{"id":9,"title":"Jupiter","link":"/jupiter/","cta":"View Homes","image":"https://u/j.jpg"}]}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "featured",
            "add-tile",
            "3",
            "--title",
            "Jupiter",
            "--link",
            "/jupiter/",
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
        .with_body(r#"{"id":23,"path":"/jupiter/"}"#)
        .create();
    let area = server
        .mock("PATCH", "/api/v3/content/area-pages/23/")
        .match_body(Matcher::Json(json!({ "landscape_image_override": "https://u/j.jpg", "landscape_image_override_alt_text": "Jupiter inlet" })))
        .with_body(r#"{"id":23,"path":"/jupiter/","url":"https://x/jupiter/"}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "area-pages",
            "update",
            "23",
            "--landscape",
            "https://u/j.jpg",
            "--landscape-alt",
            "Jupiter inlet",
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
            json!({"search_header": "Jupiter Homes for Sale"}),
        ))
        .with_body(r#"{"id":1,"url":"https://x/"}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "home-page",
            "update",
            "--page-heading",
            "Jupiter Homes for Sale",
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
        .match_body(Matcher::Json(json!({"search_header": "Buying in Jupiter"})))
        .with_body(r#"{"id":7,"path":"/buying/","url":"https://x/buying/"}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "pages",
            "update",
            "7",
            "--page-heading",
            "Buying in Jupiter",
        ])
        .assert()
        .success();
    page.assert();

    cmd(&server, &dir)
        .args(["pages", "update", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("page-heading"))
        .stdout(predicate::str::contains("BIG_SEARCH_TITLE"));
}
