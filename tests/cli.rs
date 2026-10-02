//! End-to-end tests: run the `geekcli` binary against a mock `/api/v3/`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use mockito::{Matcher, Server, ServerGuard};
use predicates::prelude::*;
use serde_json::{json, Value};

const KEY: &str = "rg_live_testkey0000000000000000000000";

struct Env {
    server: ServerGuard,
    config_dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        let server = Server::new();
        let config_dir = tempfile::tempdir().unwrap();
        Self { server, config_dir }
    }

    fn cmd(&self) -> Command {
        let mut cmd = Command::cargo_bin("geekcli").unwrap();
        cmd.env_clear()
            .env(
                "SYSTEMROOT",
                std::env::var("SYSTEMROOT").unwrap_or_default(),
            )
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("GEEKCLI_SITE", "www.test.local")
            .env("GEEKCLI_API_KEY", KEY)
            .env("GEEKCLI_BASE_URL", self.server.url())
            .env("GEEKCLI_CONFIG_DIR", self.config_dir.path());
        cmd
    }
}

fn parse(out: &[u8]) -> Value {
    serde_json::from_slice(out)
        .unwrap_or_else(|e| panic!("not JSON: {e}\n{}", String::from_utf8_lossy(out)))
}

#[test]
fn me_sends_bearer_and_prints_json() {
    let mut env = Env::new();
    let mock = env
        .server
        .mock("GET", "/api/v3/me/")
        .match_header("authorization", format!("Bearer {KEY}").as_str())
        .match_header("accept", "application/json")
        .with_status(200)
        .with_body(r#"{"api_version":"v3","site":{"domain":"www.test.local"}}"#)
        .create();

    let out = env
        .cmd()
        .arg("me")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body = parse(&out);
    assert_eq!(body["site"]["domain"], "www.test.local");
    mock.assert();
}

#[test]
fn missing_key_exits_3_with_json_error() {
    let env = Env::new();
    env.cmd()
        .env_remove("GEEKCLI_API_KEY")
        .arg("posts")
        .arg("list")
        .assert()
        .code(3)
        .stderr(predicate::str::contains(r#""code":"not_logged_in""#));
}

#[test]
fn invalid_key_exits_3() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/me/")
        .with_status(401)
        .with_body(r#"{"error":{"code":"invalid_token","message":"nope"}}"#)
        .create();
    env.cmd()
        .arg("me")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("invalid_token"));
}

#[test]
fn expired_key_exits_3_with_a_login_hint() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/me/")
        .with_status(401)
        .with_body(r#"{"error":{"code":"token_expired","message":"API key has expired"}}"#)
        .create();
    env.cmd()
        .arg("me")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("\"hint\":\"the key has expired"))
        .stderr(predicate::str::contains("geekcli auth login"));
}

#[test]
fn posts_list_passes_filters() {
    let mut env = Env::new();
    let mock = env
        .server
        .mock("GET", "/api/v3/blog/posts/")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "published".into()),
            Matcher::UrlEncoded("q".into(), "market".into()),
            Matcher::UrlEncoded("include_body".into(), "false".into()),
            Matcher::UrlEncoded("ordering".into(), "-updated_at".into()),
        ]))
        .with_body(r#"{"results":[{"id":1,"title":"A"}],"pagination":{"page":1,"total":1}}"#)
        .create();

    let out = env
        .cmd()
        .args([
            "posts",
            "list",
            "--state",
            "published",
            "--search",
            "market",
            "--no-body",
            "--ordering",
            "-updated_at",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body = parse(&out);
    assert_eq!(body["results"][0]["id"], 1);
    assert_eq!(body["pagination"]["total"], 1);
    mock.assert();
}

#[test]
fn list_all_follows_pagination() {
    let mut env = Env::new();
    let base = env.server.url();
    env.server
        .mock("GET", "/api/v3/blog/categories/")
        .match_query(Matcher::UrlEncoded("page_size".into(), "100".into()))
        .with_body(format!(
            r#"{{"results":[{{"id":1}}],"pagination":{{"next":"{base}/api/v3/blog/categories/?page=2&page_size=100"}}}}"#
        ))
        .create();
    env.server
        .mock("GET", "/api/v3/blog/categories/")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "2".into()),
            Matcher::UrlEncoded("page_size".into(), "100".into()),
        ]))
        .with_body(r#"{"results":[{"id":2}],"pagination":{"next":null}}"#)
        .create();

    let out = env
        .cmd()
        .args(["categories", "list", "--all", "-q"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(String::from_utf8_lossy(&out), "1\n2\n");
}

#[test]
fn posts_create_defaults_to_draft_and_converts_markdown() {
    let mut env = Env::new();
    let md = env.config_dir.path().join("post.md");
    std::fs::write(
        &md,
        "Intro paragraph.\n\n<!--read more-->\n\n## Details\n\nMore *here*.\n",
    )
    .unwrap();

    let mock = env
        .server
        .mock("POST", "/api/v3/blog/posts/")
        .match_header("content-type", "application/json")
        .match_body(Matcher::Json(json!({
            "title": "Hello",
            "slug": "hello",
            "status": "draft",
            "categories": ["news", 7],
            "body": "<p>Intro paragraph.</p>\n<!--read more-->\n<h2>Details</h2>\n<p>More <em>here</em>.</p>"
        })))
        .with_status(201)
        .with_header("location", "/api/v3/blog/posts/9/")
        .with_body(r#"{"id":9,"title":"Hello","slug":"hello","status":"draft","state":"draft","url":"https://www.test.local/blog/hello/"}"#)
        .create();

    let out = env
        .cmd()
        .args([
            "posts",
            "create",
            "--title",
            "Hello",
            "--slug",
            "hello",
            "--category",
            "news,7",
        ])
        .arg("--body-file")
        .arg(&md)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["id"], 9);
    mock.assert();
}

#[test]
fn posts_create_requires_body() {
    let env = Env::new();
    env.cmd()
        .args(["posts", "create", "--title", "x", "--slug", "x"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "--content (or --content-file) is required",
        ));
}

#[test]
fn validation_error_exits_5_with_fields() {
    let mut env = Env::new();
    env.server
        .mock("POST", "/api/v3/blog/posts/")
        .with_status(422)
        .with_body(r#"{"error":{"code":"validation_error","message":"One or more fields are invalid.","fields":{"slug":["Post with this slug already exists."]}}}"#)
        .create();

    let assert = env
        .cmd()
        .args([
            "posts", "create", "--title", "x", "--slug", "x", "--body", "<p>x</p>",
        ])
        .assert()
        .code(5);
    let err: Value = serde_json::from_slice(&assert.get_output().stderr).unwrap();
    assert_eq!(err["error"]["code"], "validation_error");
    assert_eq!(
        err["error"]["fields"]["slug"][0],
        "Post with this slug already exists."
    );
    assert_eq!(err["error"]["exit_code"], 5);
}

#[test]
fn posts_update_by_slug_patches_only_given_fields() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/blog/posts/")
        .match_query(Matcher::UrlEncoded("slug".into(), "hello".into()))
        .with_body(r#"{"results":[{"id":9,"slug":"hello","categories":[{"id":1,"slug":"news"}]}],"pagination":{}}"#)
        .create();
    let patch = env
        .server
        .mock("PATCH", "/api/v3/blog/posts/9/")
        .match_body(Matcher::Json(
            json!({ "title": "New", "categories": [1, "buyers"] }),
        ))
        .with_body(r#"{"id":9,"title":"New"}"#)
        .create();

    env.cmd()
        .args([
            "posts",
            "update",
            "hello",
            "--title",
            "New",
            "--add-category",
            "buyers",
        ])
        .assert()
        .success();
    patch.assert();
}

#[test]
fn posts_update_remove_category_by_slug() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/blog/posts/9/")
        .with_body(r#"{"id":9,"categories":[{"id":1,"slug":"news"},{"id":2,"slug":"buyers"}]}"#)
        .create();
    let patch = env
        .server
        .mock("PATCH", "/api/v3/blog/posts/9/")
        .match_body(Matcher::Json(json!({ "categories": [2] })))
        .with_body(r#"{"id":9}"#)
        .create();
    env.cmd()
        .args(["posts", "update", "9", "--remove-category", "news"])
        .assert()
        .success();
    patch.assert();
}

#[test]
fn posts_publish_sets_status_and_date() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/blog/posts/9/")
        .with_body(r#"{"id":9}"#)
        .create();
    let patch = env
        .server
        .mock("PATCH", "/api/v3/blog/posts/9/")
        .match_body(Matcher::Json(
            json!({ "status": "published", "publish": "2026-10-01T09:00:00-05:00" }),
        ))
        .with_body(r#"{"id":9,"state":"scheduled"}"#)
        .create();
    env.cmd()
        .args(["posts", "publish", "9", "--at", "2026-10-01T09:00:00-05:00"])
        .assert()
        .success();
    patch.assert();
}

#[test]
fn ambiguous_slug_is_a_usage_error() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/content/pages/")
        .match_query(Matcher::UrlEncoded("slug".into(), "about".into()))
        .with_body(r#"{"results":[{"id":1},{"id":2}],"pagination":{}}"#)
        .create();
    env.cmd()
        .args(["pages", "get", "about"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("more than one page"));
}

#[test]
fn unknown_slug_exits_4() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/blog/posts/")
        .match_query(Matcher::UrlEncoded("slug".into(), "nope".into()))
        .with_body(r#"{"results":[],"pagination":{}}"#)
        .create();
    env.cmd().args(["posts", "get", "nope"]).assert().code(4);
}

#[test]
fn pages_create_resolves_parent_path() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/content/pages/")
        .match_query(Matcher::UrlEncoded("path".into(), "/resources/".into()))
        .with_body(r#"{"results":[{"id":4,"path":"/resources/"}],"pagination":{}}"#)
        .create();
    let post = env
        .server
        .mock("POST", "/api/v3/content/pages/")
        .match_body(Matcher::Json(json!({
            "slug": "sellers",
            "anchor_text": "Sellers",
            "parent": 4,
            "template": "Page without Search",
            "content": "<p>Hi</p>",
            "meta_keywords": "a, b"
        })))
        .with_status(201)
        .with_body(r#"{"id":5,"path":"/resources/sellers/","url":"https://www.test.local/resources/sellers/"}"#)
        .create();

    env.cmd()
        .args([
            "pages",
            "create",
            "--slug",
            "sellers",
            "--anchor-text",
            "Sellers",
            "--parent",
            "/resources/",
            "--template",
            "Page without Search",
            "--content",
            "<p>Hi</p>",
            "--data",
            r#"{"meta_keywords":"a, b","slug":"ignored"}"#,
        ])
        .assert()
        .success();
    post.assert();
}

#[test]
fn pages_delete_conflict_exits_6() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/content/pages/4/")
        .with_body(r#"{"id":4,"path":"/resources/"}"#)
        .create();
    env.server
        .mock("DELETE", "/api/v3/content/pages/4/")
        .with_status(409)
        .with_body(r#"{"error":{"code":"conflict","message":"Page has 1 child page(s).","fields":{"children":["1 child page(s)"]}}}"#)
        .create();
    env.cmd()
        .args(["pages", "delete", "4"])
        .assert()
        .code(6)
        .stderr(predicate::str::contains("child page"));
}

#[test]
fn pages_delete_with_orphan_flag() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/content/pages/4/")
        .with_body(r#"{"id":4,"path":"/resources/"}"#)
        .create();
    let del = env
        .server
        .mock("DELETE", "/api/v3/content/pages/4/")
        .match_query(Matcher::UrlEncoded("orphan_children".into(), "true".into()))
        .with_status(204)
        .create();
    let out = env
        .cmd()
        .args(["pages", "delete", "4", "--orphan-children"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out), json!({ "deleted": true, "id": 4 }));
    del.assert();
}

#[test]
fn rate_limit_is_retried() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/content/templates/")
        .with_status(429)
        .with_header("retry-after", "0")
        .with_body(r#"{"error":{"code":"rate_limited","message":"slow down"}}"#)
        .expect(1)
        .create();
    // mockito serves mocks in creation order until each is exhausted
    env.server
        .mock("GET", "/api/v3/content/templates/")
        .with_body(
            r#"{"results":[{"name":"About Page","description":"","extra_content_areas":[]}]}"#,
        )
        .create();
    // the notice goes to stderr without -v; stdout stays the JSON result
    let output = env
        .cmd()
        .args(["templates", "list"])
        .assert()
        .success()
        .stderr("rate limited; retrying in 0s (attempt 1 of 3)\n")
        .get_output()
        .clone();
    assert_eq!(parse(&output.stdout)["results"][0]["name"], "About Page");
}

#[test]
fn retry_after_http_date_in_the_past_retries_at_once() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/content/templates/")
        .with_status(429)
        .with_header("retry-after", "Wed, 21 Oct 2015 07:28:00 GMT")
        .with_body(r#"{"error":{"code":"rate_limited","message":"slow down"}}"#)
        .expect(1)
        .create();
    env.server
        .mock("GET", "/api/v3/content/templates/")
        .with_body(r#"{"results":[]}"#)
        .create();
    env.cmd()
        .args(["templates", "list", "--max-retries", "2"])
        .assert()
        .success()
        .stderr("rate limited; retrying in 0s (attempt 1 of 2)\n");
}

#[test]
fn low_rate_limit_remaining_warns_once() {
    let mut env = Env::new();
    let list = r#"{"results":[{"name":"About Page","description":"","extra_content_areas":[]}]}"#;
    env.server
        .mock("GET", "/api/v3/content/templates/")
        .with_header("x-ratelimit-limit", "600")
        .with_header("x-ratelimit-remaining", "12")
        .with_header("x-ratelimit-reset", "900")
        .with_body(list)
        .create();
    let output = env
        .cmd()
        .args(["templates", "list"])
        .assert()
        .success()
        .stderr(
            "warning: rate limit nearly used: 12 of 600 requests left in this window (resets in 900s)\n",
        )
        .get_output()
        .clone();
    assert_eq!(parse(&output.stdout)["results"][0]["name"], "About Page");
}

#[test]
fn rate_limit_headers_with_room_say_nothing() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/content/templates/")
        .with_header("x-ratelimit-limit", "600")
        .with_header("x-ratelimit-remaining", "60")
        .with_body(r#"{"results":[]}"#)
        .create();
    env.cmd()
        .args(["templates", "list"])
        .assert()
        .success()
        .stderr("");
}

#[test]
fn rate_limit_exhausted_exits_7() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/content/home-page/")
        .with_status(429)
        .with_header("retry-after", "0")
        .with_body(r#"{"error":{"code":"rate_limited","message":"slow down"}}"#)
        .expect(1)
        .create();
    env.cmd()
        .args(["home-page", "get", "--max-retries", "0"])
        .assert()
        .code(7)
        .stderr(predicate::str::contains("rate_limited"));
}

#[test]
fn raw_api_call() {
    let mut env = Env::new();
    let mock = env
        .server
        .mock("PATCH", "/api/v3/blog/posts/3/")
        .match_query(Matcher::UrlEncoded("x".into(), "1".into()))
        .match_body(Matcher::Json(json!({ "title": "t" })))
        .with_body(r#"{"id":3}"#)
        .create();
    env.cmd()
        .args([
            "api",
            "patch",
            "blog/posts/3/",
            "-p",
            "x=1",
            "-d",
            r#"{"title":"t"}"#,
        ])
        .assert()
        .success();
    mock.assert();
}

#[test]
fn login_with_api_key_verifies_and_stores() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/me/")
        .match_header("authorization", "Bearer rg_live_new")
        .with_body(r#"{"api_key":{"name":"CLI","scopes":["blog:read","blog:write"],"expires_at":null},"site":{"domain":"www.test.local"}}"#)
        .create();

    env.cmd()
        .env_remove("GEEKCLI_API_KEY")
        .args(["auth", "login", "--api-key", "rg_live_new"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""default": true"#));

    let config = std::fs::read_to_string(env.config_dir.path().join("config.toml")).unwrap();
    assert!(
        config.contains("default_site = \"www.test.local\""),
        "{config}"
    );
    assert!(config.contains("api_key = \"rg_live_new\""), "{config}");
    assert!(config.contains("blog:write"), "{config}");

    // and the stored key is used when no env key is present
    env.server
        .mock("GET", "/api/v3/blog/categories/")
        .match_header("authorization", "Bearer rg_live_new")
        .with_body(r#"{"results":[],"pagination":{}}"#)
        .create();
    env.cmd()
        .env_remove("GEEKCLI_API_KEY")
        .args(["categories", "list"])
        .assert()
        .success();

    let out = env
        .cmd()
        .env_remove("GEEKCLI_API_KEY")
        .args(["auth", "sites"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let sites = parse(&out);
    assert_eq!(sites["results"][0]["site"], "www.test.local");
    assert_eq!(sites["results"][0]["prefix"], "rg_live_new…");

    env.cmd()
        .env_remove("GEEKCLI_API_KEY")
        .args(["auth", "logout"])
        .assert()
        .success();
    env.cmd()
        .env_remove("GEEKCLI_API_KEY")
        .args(["categories", "list"])
        .assert()
        .code(3);
}

#[test]
fn login_rejects_bad_key_without_storing() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/me/")
        .with_status(401)
        .with_body(r#"{"error":{"code":"invalid_token","message":"bad"}}"#)
        .create();
    env.cmd()
        .args(["auth", "login", "--api-key", "rg_live_bad"])
        .assert()
        .code(3);
    assert!(!env.config_dir.path().join("config.toml").exists());
}

#[test]
fn guide_and_help_are_available() {
    let env = Env::new();
    env.cmd()
        .arg("guide")
        .assert()
        .success()
        .stdout(predicate::str::contains("Exit codes"));
    env.cmd()
        .args(["posts", "create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--body-file"));
    env.cmd().args(["completions", "zsh"]).assert().success();
}

#[test]
fn guide_topics_and_sections() {
    let env = Env::new();
    let out = env
        .cmd()
        .args(["guide", "--list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list = String::from_utf8_lossy(&out);
    assert!(list.contains("Writing content that renders"), "{list}");
    assert!(list.contains("Rebranding a site"), "{list}");

    let out = env
        .cmd()
        .args(["guide", "html"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let html = String::from_utf8_lossy(&out);
    assert!(html.starts_with("## "), "{html}");
    assert!(html.contains("Windows-1252"), "{html}");
    assert!(
        !html.contains("Rebranding a site"),
        "guide html printed more than one section"
    );

    env.cmd()
        .args(["guide", "zzz"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no guide section matches"));

    env.cmd()
        .args(["posts", "create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Windows-1252"));
}

/// The inline-style list in `posts create --help` matches `guide html`.
#[test]
fn posts_create_help_lists_the_guide_inline_styles() {
    let env = Env::new();
    let squash = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let guide = squash(
        &env.cmd()
            .args(["guide", "html"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    let help = squash(
        &env.cmd()
            .args(["posts", "create", "--help"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    let start = guide
        .find("keeps only: ")
        .expect("style list in guide html")
        + 12;
    let list = &guide[start..start + guide[start..].find('.').unwrap()];
    let properties: Vec<&str> = list.split(", ").collect();
    assert!(properties.len() > 15, "{list}");
    for property in properties {
        assert!(
            help.contains(property),
            "posts create --help does not list {property}"
        );
    }
}

#[test]
fn blog_home_page_get_and_update() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/blog/home-page/")
        .with_body(r#"{"id":1,"title":"Old","meta_description":"x","meta_keywords":"","content":"<h2>Old</h2>","path":"/blog/","url":"https://x/blog/"}"#)
        .create();
    let out = env
        .cmd()
        .args(["blog", "get"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["title"], "Old");
    let patch = env
        .server
        .mock("PATCH", "/api/v3/blog/home-page/")
        .match_body(Matcher::Json(
            json!({ "title": "New", "content": "<h2>New heading</h2>" }),
        ))
        .with_status(201)
        .with_body(
            r#"{"id":1,"title":"New","content":"<h2>New heading</h2>","url":"https://x/blog/"}"#,
        )
        .create();
    env.cmd()
        .args([
            "blog",
            "update",
            "--title",
            "New",
            "--content",
            "<h2>New heading</h2>",
        ])
        .assert()
        .success();
    patch.assert();
    env.cmd().args(["blog", "update"]).assert().code(2);
}

const REVISION: &str = r#"{"id":512,"at":"2026-09-30T14:00:00+00:00","by":{"name":"Jordan Avery","api_key":"cli"},"action":"changed","message":"","changed_fields":["body","status"],"revertible":true}"#;

#[test]
fn posts_revisions_lists_and_limits() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/blog/posts/")
        .match_query(Matcher::UrlEncoded("slug".into(), "hello".into()))
        .with_body(r#"{"results":[{"id":9,"slug":"hello"}],"pagination":{}}"#)
        .create();
    let list = env
        .server
        .mock("GET", "/api/v3/blog/posts/9/revisions/")
        .with_body(format!(r#"{{"results":[{REVISION},{REVISION}]}}"#))
        .create();

    let out = env
        .cmd()
        .args(["posts", "revisions", "hello", "--limit", "1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows = parse(&out)["results"].clone();
    assert_eq!(rows.as_array().map(Vec::len), Some(1), "{rows}");
    assert_eq!(rows[0]["changed_fields"][1], "status");
    list.assert();
}

#[test]
fn posts_revision_shows_the_preview() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/blog/posts/9/")
        .with_body(r#"{"id":9,"slug":"hello"}"#)
        .create();
    let preview = env
        .server
        .mock("GET", "/api/v3/blog/posts/9/revisions/512/")
        .with_body(r#"{"id":512,"action":"changed","revertible":true,"preview":{"status":{"now":"published","after_revert":"draft"}}}"#)
        .create();

    let out = env
        .cmd()
        .args(["posts", "revision", "9", "512"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["preview"]["status"]["after_revert"], "draft");
    preview.assert();
}

#[test]
fn area_pages_revert_posts_to_the_revision() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/content/area-pages/23/")
        .with_body(r#"{"id":23,"path":"/riverside/"}"#)
        .create();
    let revert = env
        .server
        .mock(
            "POST",
            "/api/v3/content/area-pages/23/revisions/402/revert/",
        )
        .with_body(r#"{"id":23,"path":"/riverside/","area_name":"Riverside"}"#)
        .create();

    let out = env
        .cmd()
        .args(["-y", "area-pages", "revert", "23", "402"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["area_name"], "Riverside");
    revert.assert();
}

#[test]
fn footers_revisions_and_a_creation_revert_conflict() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/content/footers/")
        .with_body(
            r#"{"results":[{"id":1,"name":"Default Footer","default":true,"content":"<p>x</p>"}]}"#,
        )
        .create();
    env.server
        .mock("GET", "/api/v3/content/footers/1/")
        .with_body(r#"{"id":1,"name":"Default Footer","default":true,"content":"<p>x</p>","used_by":[],"used_by_count":0}"#)
        .create();
    let list = env
        .server
        .mock("GET", "/api/v3/content/footers/1/revisions/")
        .with_body(format!(r#"{{"results":[{REVISION}]}}"#))
        .create();
    env.server
        .mock("POST", "/api/v3/content/footers/1/revisions/500/revert/")
        .with_status(409)
        .with_body(r#"{"error":{"code":"conflict","message":"Revision 500 cannot be reverted (nothing to undo)"}}"#)
        .create();

    env.cmd()
        .args(["footers", "revisions", "Default Footer"])
        .assert()
        .success()
        .stdout(predicate::str::contains("512"));
    list.assert();

    env.cmd()
        .args(["-y", "footers", "revert", "1", "500"])
        .assert()
        .code(6)
        .stderr(predicate::str::contains("nothing to undo"));
}

#[test]
fn pages_list_no_longer_sends_include_content() {
    let mut env = Env::new();
    let list = env
        .server
        .mock("GET", "/api/v3/content/pages/")
        .match_query(Matcher::Missing)
        .with_body(r#"{"results":[{"id":1,"path":"/a/"}],"pagination":{}}"#)
        .create();

    // --no-content is hidden and ignored: lists never carry content now
    env.cmd()
        .args(["pages", "list", "--no-content"])
        .assert()
        .success();
    list.assert();
}

#[test]
fn a_redirect_to_another_origin_is_not_followed_with_the_key() {
    let mut env = Env::new();
    let mut elsewhere = Server::new();
    // another origin that would bounce the request on to itself, where reqwest
    // would otherwise put the Authorization header back
    let step1 = elsewhere
        .mock("GET", "/step1")
        .with_status(302)
        .with_header("location", "/step2")
        .expect(0)
        .create();
    let step2 = elsewhere.mock("GET", "/step2").expect(0).create();
    env.server
        .mock("GET", "/api/v3/blog/posts/7/")
        .with_status(301)
        .with_header("location", &format!("{}/step1", elsewhere.url()))
        .create();

    env.cmd()
        .args(["posts", "get", "7"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("another origin"));
    step1.assert();
    step2.assert();
}

#[test]
fn same_origin_redirects_are_still_followed() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/blog/posts/7")
        .with_status(301)
        .with_header("location", "/api/v3/blog/posts/7/")
        .create();
    let detail = env
        .server
        .mock("GET", "/api/v3/blog/posts/7/")
        .match_header("authorization", format!("Bearer {KEY}").as_str())
        .with_body(r#"{"id":7,"slug":"seven"}"#)
        .create();

    env.cmd()
        .args(["api", "GET", "blog/posts/7"])
        .assert()
        .success();
    detail.assert();
}

#[test]
fn the_api_command_refuses_urls_on_other_origins() {
    let env = Env::new();
    let mut elsewhere = Server::new();
    let leak = elsewhere.mock("GET", "/collect").expect(0).create();

    env.cmd()
        .args(["api", "GET", &format!("{}/collect", elsewhere.url())])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("refusing to send"));
    leak.assert();
}

#[test]
fn pagination_never_follows_next_to_another_origin() {
    let mut env = Env::new();
    let mut elsewhere = Server::new();
    let leak = elsewhere.mock("GET", Matcher::Any).expect(0).create();
    env.server
        .mock("GET", "/api/v3/blog/posts/")
        .match_query(Matcher::Any)
        .with_body(format!(
            r#"{{"results":[{{"id":1}}],"pagination":{{"next":"{}/api/v3/blog/posts/?page=2"}}}}"#,
            elsewhere.url()
        ))
        .create();

    env.cmd()
        .args(["posts", "list", "--all"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("refusing to send"));
    leak.assert();
}

#[test]
fn pagination_stops_when_next_repeats() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/blog/posts/")
        .match_query(Matcher::Any)
        .with_body(r#"{"results":[{"id":1}],"pagination":{"next":"/api/v3/blog/posts/?page=2"}}"#)
        .expect_at_most(3)
        .create();

    env.cmd()
        .args(["posts", "list", "--all"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("repeat"));
}

#[test]
fn area_pages_get_by_slug_fetches_the_detail_with_content() {
    let mut env = Env::new();
    let list = env
        .server
        .mock("GET", "/api/v3/content/area-pages/")
        .match_query(Matcher::UrlEncoded("slug".into(), "downtown".into()))
        .with_body(
            r#"{"results":[{"id":31,"slug":"downtown","path":"/downtown/"}],"pagination":{}}"#,
        )
        .expect(1)
        .create();
    let detail = env
        .server
        .mock("GET", "/api/v3/content/area-pages/31/")
        .with_body(
            r#"{"id":31,"slug":"downtown","path":"/downtown/","content":"<p>Lofts and parks</p>"}"#,
        )
        .expect(1)
        .create();

    let out = env
        .cmd()
        .args(["area-pages", "get", "downtown"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["content"], "<p>Lofts and parks</p>");
    list.assert();
    detail.assert();
}

#[test]
fn pages_get_by_path_fetches_the_detail_with_content() {
    let mut env = Env::new();
    let list = env
        .server
        .mock("GET", "/api/v3/content/pages/")
        .match_query(Matcher::UrlEncoded(
            "path".into(),
            "/resources/buyers/".into(),
        ))
        .with_body(r#"{"results":[{"id":7,"path":"/resources/buyers/"}],"pagination":{}}"#)
        .expect(1)
        .create();
    let detail = env
        .server
        .mock("GET", "/api/v3/content/pages/7/")
        .with_body(r#"{"id":7,"path":"/resources/buyers/","content":"<p>Buyer guide</p>"}"#)
        .expect(1)
        .create();

    let out = env
        .cmd()
        .args(["pages", "get", "/resources/buyers/"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["content"], "<p>Buyer guide</p>");
    list.assert();
    detail.assert();
}

#[test]
fn pages_update_by_slug_skips_the_detail_fetch() {
    let mut env = Env::new();
    let list = env
        .server
        .mock("GET", "/api/v3/content/pages/")
        .match_query(Matcher::UrlEncoded("slug".into(), "about".into()))
        .with_body(r#"{"results":[{"id":5,"slug":"about"}],"pagination":{}}"#)
        .expect(1)
        .create();
    let detail = env
        .server
        .mock("GET", "/api/v3/content/pages/5/")
        .with_body(r#"{"id":5}"#)
        .expect(0)
        .create();
    let patch = env
        .server
        .mock("PATCH", "/api/v3/content/pages/5/")
        .match_body(Matcher::Json(json!({ "title": "About us" })))
        .with_body(r#"{"id":5,"title":"About us"}"#)
        .expect(1)
        .create();

    env.cmd()
        .args(["pages", "update", "about", "--title", "About us"])
        .assert()
        .success();
    list.assert();
    detail.assert();
    patch.assert();
}

#[test]
fn pages_update_by_id_sends_only_the_patch() {
    let mut env = Env::new();
    let detail = env
        .server
        .mock("GET", "/api/v3/content/pages/5/")
        .with_body(r#"{"id":5}"#)
        .expect(0)
        .create();
    let patch = env
        .server
        .mock("PATCH", "/api/v3/content/pages/5/")
        .match_body(Matcher::Json(json!({ "title": "About us" })))
        .with_body(r#"{"id":5,"title":"About us"}"#)
        .expect(1)
        .create();

    env.cmd()
        .args(["pages", "update", "5", "--title", "About us"])
        .assert()
        .success();
    detail.assert();
    patch.assert();
}

#[test]
fn area_pages_create_without_search_is_a_usage_error() {
    let mut env = Env::new();
    // Any request at all would hit one of these; neither may be called.
    let calls: Vec<_> = ["GET", "POST"]
        .into_iter()
        .map(|method| {
            env.server
                .mock(method, Matcher::Any)
                .with_status(500)
                .expect(0)
                .create()
        })
        .collect();
    let out = env
        .cmd()
        .args([
            "area-pages",
            "create",
            "--slug",
            "downtown",
            "--anchor-text",
            "Downtown",
            "--area-name",
            "Downtown",
        ])
        .assert()
        .code(2)
        .get_output()
        .stderr
        .clone();
    let err = parse(&out);
    assert_eq!(err["error"]["code"], "usage");
    let message = err["error"]["message"].as_str().unwrap();
    assert!(message.contains("--no-search"), "{message}");
    assert!(message.contains("--area-name is only a label"), "{message}");

    // `--search null` is no search either.
    env.cmd()
        .args([
            "area-pages",
            "create",
            "--slug",
            "downtown",
            "--anchor-text",
            "Downtown",
            "--area-name",
            "Downtown",
            "--search",
            "null",
        ])
        .assert()
        .code(2);
    for mock in calls {
        mock.assert();
    }
}

#[test]
fn area_pages_create_with_no_search_sends_the_request() {
    let mut env = Env::new();
    let post = env
        .server
        .mock("POST", "/api/v3/content/area-pages/")
        .match_body(Matcher::Json(json!({
            "slug": "downtown",
            "anchor_text": "Downtown",
            "area_name": "Downtown"
        })))
        .with_status(201)
        .with_body(r#"{"id":7,"path":"/downtown/","url":"https://www.test.local/downtown/"}"#)
        .create();
    env.cmd()
        .args([
            "area-pages",
            "create",
            "--slug",
            "downtown",
            "--anchor-text",
            "Downtown",
            "--area-name",
            "Downtown",
            "--no-search",
        ])
        .assert()
        .success();
    post.assert();
}

#[test]
fn area_pages_create_sends_search_criteria() {
    let mut env = Env::new();
    let post = env
        .server
        .mock("POST", "/api/v3/content/area-pages/")
        .match_body(Matcher::Json(json!({
            "slug": "downtown",
            "anchor_text": "Downtown",
            "area_name": "Downtown",
            "search": { "subdivision": ["Downtown"] }
        })))
        .with_status(201)
        .with_body(r#"{"id":7,"path":"/downtown/","url":"https://www.test.local/downtown/"}"#)
        .create();
    env.cmd()
        .args([
            "area-pages",
            "create",
            "--slug",
            "downtown",
            "--anchor-text",
            "Downtown",
            "--area-name",
            "Downtown",
            "--search-criteria",
            "subdivision=Downtown",
        ])
        .assert()
        .success();
    post.assert();
    // --no-search alongside a search is contradictory.
    env.cmd()
        .args([
            "area-pages",
            "create",
            "--slug",
            "downtown",
            "--anchor-text",
            "Downtown",
            "--area-name",
            "Downtown",
            "--search-criteria",
            "subdivision=Downtown",
            "--no-search",
        ])
        .assert()
        .code(2);
}

#[test]
fn area_pages_help_explains_area_name_and_publishing() {
    let env = Env::new();
    for sub in ["create", "update"] {
        env.cmd()
            .args(["area-pages", sub, "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("--area-name is display text only"))
            .stdout(predicate::str::contains("subdivision"))
            .stdout(predicate::str::contains("search check"));
    }
    env.cmd()
        .args(["area-pages", "create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("public as soon as it is created"))
        .stdout(predicate::str::contains("--no-search"));
}
