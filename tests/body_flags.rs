//! Every command that takes a body accepts `--content`, `--body` and `--html`
//! (and their `-file` forms) and sends the same request body for each.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use mockito::{Matcher, Server, ServerGuard};
use serde_json::{json, Value};

const INLINE: [&str; 3] = ["--content", "--body", "--html"];
const FILE: [&str; 3] = ["--content-file", "--body-file", "--html-file"];
const SIDEBAR: &str = r#"{"id":5,"name":"Area Sidebar","special":false,"items":[{"id":50,"order":0,"type":"html","html":"<p>old</p>"}]}"#;

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

/// Run `prefix <flag> <value>` once per spelling, inline and from a Markdown
/// file; every run must send exactly `body` (with `"BODY"` standing for the
/// converted HTML) to `method path`.
fn check_all_spellings(
    method: &str,
    path: &str,
    body: &Value,
    prefix: &[&str],
    setup: impl Fn(&mut ServerGuard),
    response: &str,
) {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    setup(&mut server);
    let md = dir.path().join("body.md");
    std::fs::write(&md, "Hello *there*\n").unwrap();

    // the exact request body: `body` with the body field filled in
    let mut expected = body.clone();
    for (_, v) in expected.as_object_mut().unwrap().iter_mut() {
        if v == "BODY" {
            *v = json!("<p>Hello <em>there</em></p>");
        }
    }
    let mock = server
        .mock(method, path)
        .match_body(Matcher::Json(expected))
        .with_status(200)
        .with_body(response)
        .expect(INLINE.len() + FILE.len())
        .create();

    for flag in INLINE {
        cmd(&server, &dir)
            .args(prefix)
            .args([flag, "Hello *there*", "--markdown"])
            .assert()
            .success();
    }
    for flag in FILE {
        cmd(&server, &dir)
            .args(prefix)
            .arg(flag)
            .arg(&md)
            .assert()
            .success();
    }
    mock.assert();
}

#[test]
fn posts_accept_every_body_spelling() {
    check_all_spellings(
        "PATCH",
        "/api/v3/blog/posts/42/",
        &json!({ "body": "BODY" }),
        &["posts", "update", "42"],
        |s| {
            s.mock("GET", "/api/v3/blog/posts/42/")
                .with_body(r#"{"id":42,"slug":"hello","categories":[]}"#)
                .create();
        },
        r#"{"id":42}"#,
    );
}

#[test]
fn area_pages_accept_every_body_spelling() {
    check_all_spellings(
        "PATCH",
        "/api/v3/content/area-pages/23/",
        &json!({ "content": "BODY" }),
        &["area-pages", "update", "23"],
        |s| {
            s.mock("GET", "/api/v3/content/area-pages/23/")
                .with_body(r#"{"id":23,"path":"/riverside/"}"#)
                .create();
        },
        r#"{"id":23,"path":"/riverside/"}"#,
    );
}

#[test]
fn sidebars_add_html_accepts_every_body_spelling() {
    check_all_spellings(
        "POST",
        "/api/v3/content/sidebars/5/items/",
        &json!({ "type": "html", "html": "BODY" }),
        &["sidebars", "add-html", "5"],
        |s| {
            s.mock("GET", "/api/v3/content/sidebars/5/")
                .with_body(SIDEBAR)
                .create();
        },
        SIDEBAR,
    );
}

#[test]
fn sidebars_update_item_accepts_every_body_spelling() {
    check_all_spellings(
        "PATCH",
        "/api/v3/content/sidebars/5/items/50/",
        &json!({ "type": "html", "html": "BODY" }),
        &["sidebars", "update-item", "5", "50"],
        |s| {
            s.mock("GET", "/api/v3/content/sidebars/5/")
                .with_body(SIDEBAR)
                .create();
        },
        r#"{"id":50,"order":0,"type":"html","html":"<p>x</p>"}"#,
    );
}

#[test]
fn help_shows_content_and_hides_the_other_spellings() {
    let server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    // the canonical --content-file shows; the third spelling stays hidden
    for (args, hidden) in [
        (["posts", "update", "--help"], "--html-file"),
        (["area-pages", "update", "--help"], "--body-file"),
        (["sidebars", "add-html", "--help"], "--body-file"),
    ] {
        let out = cmd(&server, &dir).args(args).assert().success();
        let help = String::from_utf8_lossy(&out.get_output().stdout).into_owned();
        assert!(help.contains("--content-file"), "{args:?}: {help}");
        assert!(!help.contains(hidden), "{args:?}: {help}");
    }
}
