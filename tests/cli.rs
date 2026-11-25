//! End-to-end tests: run the `geekcli` binary against a mock `/api/v3/`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

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
        let mut cmd = common::geekcli();
        cmd.env("GEEKCLI_SITE", "www.test.local")
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
fn rate_limit_is_retried_for_seconds_and_http_dates() {
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
        .with_status(429)
        .with_header("retry-after", "Wed, 21 Oct 2015 07:28:00 GMT")
        .with_body(r#"{"error":{"code":"rate_limited","message":"slow down"}}"#)
        .expect(1)
        .create();
    env.server
        .mock("GET", "/api/v3/content/templates/")
        .with_body(
            r#"{"results":[{"name":"About Page","description":"","extra_content_areas":[]}]}"#,
        )
        .expect(1)
        .create();
    // each back-off prints a notice to stderr without -v; stdout stays the JSON result
    let output = env
        .cmd()
        .args(["templates", "list", "--max-retries", "2"])
        .assert()
        .success()
        .stderr(
            "rate limited; retrying in 0s (attempt 1 of 2)\n\
             rate limited; retrying in 0s (attempt 2 of 2)\n",
        )
        .get_output()
        .clone();
    assert_eq!(parse(&output.stdout)["results"][0]["name"], "About Page");
}

#[test]
fn low_rate_limit_remaining_warns_once_across_requests() {
    let mut env = Env::new();
    let base = env.server.url();
    // page 1 has plenty of room (60/600): no warning. Matched on the exact
    // query, so it cannot also answer the page 2 and 3 requests.
    let page1 = env
        .server
        .mock("GET", "/api/v3/blog/categories/")
        .match_query(Matcher::Exact("page_size=100".into()))
        .expect(1)
        .with_header("x-ratelimit-limit", "600")
        .with_header("x-ratelimit-remaining", "60")
        .with_body(format!(
            r#"{{"results":[{{"id":1}}],"pagination":{{"next":"{base}/api/v3/blog/categories/?page=2&page_size=100"}}}}"#
        ))
        .create();
    // page 2 drops below 10%: warns once
    env.server
        .mock("GET", "/api/v3/blog/categories/")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "2".into()),
            Matcher::UrlEncoded("page_size".into(), "100".into()),
        ]))
        .with_header("x-ratelimit-limit", "600")
        .with_header("x-ratelimit-remaining", "12")
        .with_header("x-ratelimit-reset", "900")
        .with_body(format!(
            r#"{{"results":[{{"id":2}}],"pagination":{{"next":"{base}/api/v3/blog/categories/?page=3&page_size=100"}}}}"#
        ))
        .create();
    // page 3 is still low: must not repeat the warning
    env.server
        .mock("GET", "/api/v3/blog/categories/")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "3".into()),
            Matcher::UrlEncoded("page_size".into(), "100".into()),
        ]))
        .with_header("x-ratelimit-limit", "600")
        .with_header("x-ratelimit-remaining", "11")
        .with_header("x-ratelimit-reset", "899")
        .with_body(r#"{"results":[{"id":3}],"pagination":{"next":null}}"#)
        .create();
    let output = env
        .cmd()
        .args(["categories", "list", "--all", "-q"])
        .assert()
        .success()
        .stderr(
            "warning: rate limit nearly used: 12 of 600 requests left in this window (resets in 900s)\n",
        )
        .get_output()
        .clone();
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n2\n3\n");
    page1.assert();
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
        .with_header("location", "/api/v3/blog/posts/3/")
        .with_body(r#"{"id":3}"#)
        .create();
    env.cmd()
        .args([
            "-o",
            "table",
            "api",
            "patch",
            "blog/posts/3/",
            "-p",
            "x=1",
            "-d",
            "-",
        ])
        .write_stdin(r#"{"title":"t"}"#)
        .assert()
        .success()
        .stderr(predicate::str::contains("Location: /api/v3/blog/posts/3/"));
    mock.assert();

    // invalid method and malformed --param fail as usage errors before any request
    env.cmd()
        .args(["api", "BAD METHOD", "me/"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unknown HTTP method"));
    env.cmd()
        .args(["api", "GET", "me/", "-p", "no-equals"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("key=value"));
}

#[test]
fn xdg_config_home_and_jsonl_error_contract() {
    let mut env = Env::new();
    let xdg = tempfile::tempdir().unwrap();
    let geekcli_dir = xdg.path().join("geekcli");
    std::fs::create_dir_all(&geekcli_dir).unwrap();
    std::fs::write(
        geekcli_dir.join("config.toml"),
        format!(
            "default_site = \"www.test.local\"\n\n[sites.\"www.test.local\"]\napi_key = \"rg_live_xdg\"\nbase_url = \"{}\"\n",
            env.server.url()
        ),
    )
    .unwrap();
    let me = env
        .server
        .mock("GET", "/api/v3/me/")
        .match_header("authorization", "Bearer rg_live_xdg")
        .with_body(r#"{"site":{"domain":"www.test.local"}}"#)
        .create();
    env.cmd()
        .env_remove("GEEKCLI_CONFIG_DIR")
        .env_remove("GEEKCLI_API_KEY")
        .env_remove("GEEKCLI_SITE")
        .env_remove("GEEKCLI_BASE_URL")
        .env("XDG_CONFIG_HOME", xdg.path())
        .arg("me")
        .assert()
        .success();
    me.assert();

    // -o jsonl emits a JSON error document on stderr (never panics or prints table text)
    env.server
        .mock("GET", "/api/v3/blog/posts/99/")
        .with_status(404)
        .with_body(r#"{"error":{"code":"not_found","message":"missing"}}"#)
        .create();
    let out = env
        .cmd()
        .args(["-o", "jsonl", "posts", "get", "99"])
        .assert()
        .code(4)
        .get_output()
        .stderr
        .clone();
    assert_eq!(parse(&out)["error"]["code"], "not_found");

    // same-origin redirect loop stops cleanly with network error (exit 8)
    env.server
        .mock("GET", "/api/v3/loop/")
        .with_status(302)
        .with_header("location", "/api/v3/loop/")
        .create();
    env.cmd()
        .args(["api", "GET", "loop/"])
        .assert()
        .code(8)
        .stderr(predicate::str::contains("too many redirects"));

    // non-JSON 200 body is wrapped under {"raw": ...}
    env.server
        .mock("GET", "/api/v3/plain/")
        .with_body("hello plain")
        .create();
    let out = env
        .cmd()
        .args(["api", "GET", "plain/"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["raw"], "hello plain");
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
fn completions_generates_shell_script() {
    let env = Env::new();
    env.cmd()
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#compdef geekcli"));
}

/// `--help` as (heading, flags) pairs, in the order printed.
fn help_sections(env: &Env, args: &[&str]) -> Vec<(String, Vec<String>)> {
    let out = env
        .cmd()
        .args(args)
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    for line in String::from_utf8_lossy(&out).lines() {
        if let Some(heading) = line.strip_suffix(':').filter(|l| !l.starts_with(' ')) {
            sections.push((heading.to_string(), Vec::new()));
        } else if let Some((_, flags)) = sections.last_mut() {
            let flag = line
                .split_whitespace()
                .find(|w| w.starts_with("--"))
                .filter(|_| line.starts_with("  ") && !line.starts_with("          "));
            if let Some(flag) = flag {
                flags.push(flag.trim_end_matches(',').to_string());
            }
        }
    }
    sections
}

fn assert_required_first(env: &Env, args: &[&str], required: &[&str]) {
    let sections = help_sections(env, args);
    let headings: Vec<&str> = sections.iter().map(|(h, _)| h.as_str()).collect();
    let custom: Vec<&str> = headings
        .iter()
        .copied()
        .filter(|h| !["Arguments", "Options", "Notes"].contains(h))
        .collect();
    assert_eq!(custom.first(), Some(&"Required on create"), "{headings:?}");
    assert_eq!(custom.last(), Some(&"Global options"), "{headings:?}");
    let flags = &sections
        .iter()
        .find(|(h, _)| h == "Required on create")
        .unwrap()
        .1;
    for flag in required {
        assert!(flags.iter().any(|f| f == flag), "{flag} not in {flags:?}");
    }
    assert_eq!(flags.len(), required.len(), "{flags:?}");
}

#[test]
fn help_groups_required_on_create_flags_first() {
    let env = Env::new();
    assert_required_first(
        &env,
        &["area-pages", "create"],
        &["--slug", "--anchor-text", "--area-name"],
    );
    assert_required_first(
        &env,
        &["area-pages", "update"],
        &["--slug", "--anchor-text", "--area-name"],
    );
    assert_required_first(&env, &["pages", "create"], &["--slug", "--anchor-text"]);
    assert_required_first(
        &env,
        &["agent-pages", "create"],
        &["--slug", "--anchor-text"],
    );
    assert_required_first(
        &env,
        &["posts", "create"],
        &["--title", "--slug", "--content", "--content-file"],
    );

    let sections = help_sections(&env, &["area-pages", "create"]);
    let headings: Vec<&str> = sections.iter().map(|(h, _)| h.as_str()).collect();
    for heading in ["Content", "Search & listings", "SEO", "Layout & display"] {
        assert!(
            headings.contains(&heading),
            "{heading} missing: {headings:?}"
        );
    }
}

#[test]
fn guide_topics_and_sections() {
    let env = Env::new();
    // bare `guide` prints the full document
    env.cmd().arg("guide").assert().success().stdout(
        predicate::str::contains("Writing content that renders")
            .and(predicate::str::contains("Exit codes")),
    );

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

    // multi-match topic prints sections separated by a blank line
    let out = env
        .cmd()
        .args(["guide", "page"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let multi = String::from_utf8_lossy(&out);
    // several sections, each later heading preceded by a blank line
    assert!(multi.starts_with("## "), "{multi}");
    let later = multi.matches("\n## ").count();
    assert!(later >= 1, "{multi}");
    assert_eq!(multi.matches("\n\n## ").count(), later, "{multi}");

    env.cmd()
        .args(["guide", "zzz"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no guide section matches"));
}

/// Safety notes agents rely on live in `--help` and the guide (AGENTS.md:
/// "write a lesson down everywhere an agent would meet it"); keep them there.
#[test]
fn help_and_guide_keep_their_safety_notes() {
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
    env.cmd()
        .args(["pages", "update", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("page-heading"))
        .stdout(predicate::str::contains("BIG_SEARCH_TITLE"));
    env.cmd()
        .args(["guide", "fonts"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HEADING_FONT"))
        .stdout(predicate::str::contains("BODY_FONT"));
}

/// `posts create --help` and `guide html` agree on what the sanitizer keeps.
#[test]
fn posts_create_help_agrees_with_the_guide_on_the_sanitizer() {
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
    for kept in ["<style>", "JSON-LD", "svg", "microdata", "prefix"] {
        assert!(guide.contains(kept), "guide html does not mention {kept}");
        assert!(
            help.contains(kept),
            "posts create --help does not mention {kept}"
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

/// `geekcli … | head` must not report an internal error when `head` exits
/// first. The read end is closed before geekcli writes, so every write fails.
fn run_with_closed_stdout(args: &[&str]) -> std::process::Output {
    use std::process::{Command as StdCommand, Stdio};
    let config = tempfile::tempdir().unwrap();
    let mut child = StdCommand::new(env!("CARGO_BIN_EXE_geekcli"))
        .args(args)
        .env("GEEKCLI_CONFIG_DIR", config.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    child.wait_with_output().unwrap()
}

#[test]
fn closed_stdout_exits_quietly_for_print_output() {
    // the guide is written with print!, which panics on a closed pipe
    let out = run_with_closed_stdout(&["guide"]);
    assert!(
        out.status.success(),
        "status {:?}, stderr {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn closed_stdout_exits_quietly_for_printer_output() {
    // the result printer uses writeln!, which returns the error instead
    let out = run_with_closed_stdout(&["auth", "sites", "--json"]);
    assert!(
        out.status.success(),
        "status {:?}, stderr {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_retired_cli_is_told_to_update() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/me/")
        .match_header(
            "user-agent",
            concat!("geekcli/", env!("CARGO_PKG_VERSION")),
        )
        .with_status(426)
        .with_body(
            r#"{"error":{"code":"client_too_old","message":"geekcli 0.7 is no longer supported; 1.0.0 or newer is required"}}"#,
        )
        .create();

    let out = env.cmd().arg("me").output().unwrap();

    assert_eq!(out.status.code(), Some(9));
    assert!(
        out.stdout.is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let err = parse(&out.stderr);
    assert_eq!(err["error"]["code"], "client_too_old");
    assert_eq!(err["error"]["exit_code"], 9);
    assert!(err["error"]["message"].as_str().unwrap().contains("1.0.0"));
    assert!(err["error"]["hint"]
        .as_str()
        .unwrap()
        .contains("geekcli update"));
}

const ME_WITH_LIVE_DOMAIN: &str = r#"{"api_key":{"name":"CLI","scopes":["blog:write"],"expires_at":null},"site":{"domain":"www.example.com","current_url":"https://www.example.com/"}}"#;

#[test]
fn login_records_the_live_domain_and_site_accepts_it() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/me/")
        .match_header("authorization", "Bearer rg_live_new")
        .with_body(ME_WITH_LIVE_DOMAIN)
        .expect_at_least(1)
        .create();

    let out = env
        .cmd()
        .env_remove("GEEKCLI_API_KEY")
        .env("GEEKCLI_SITE", "example.realgeeks.com")
        .args(["auth", "login", "--api-key", "rg_live_new"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["aliases"], json!(["www.example.com"]));
    let config = std::fs::read_to_string(env.config_dir.path().join("config.toml")).unwrap();
    assert!(
        config.contains("aliases = [\"www.example.com\"]"),
        "{config}"
    );

    // the human status line names both
    env.cmd()
        .env_remove("GEEKCLI_API_KEY")
        .env("GEEKCLI_SITE", "example.realgeeks.com")
        .args(["-o", "table", "auth", "login", "--api-key", "rg_live_new"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Logged in to example.realgeeks.com (live domain: www.example.com)",
        ));

    // --site by the live domain uses the key stored under the login name
    let me = env
        .server
        .mock("GET", "/api/v3/me/")
        .match_header("authorization", "Bearer rg_live_new")
        .with_body(ME_WITH_LIVE_DOMAIN)
        .create();
    env.cmd()
        .env_remove("GEEKCLI_API_KEY")
        .args(["me", "--site", "https://www.example.com/"])
        .assert()
        .success();
    me.assert();

    let out = env
        .cmd()
        .env_remove("GEEKCLI_API_KEY")
        .args(["auth", "sites"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        parse(&out)["results"][0]["aliases"],
        json!(["www.example.com"])
    );

    let out = env
        .cmd()
        .args(["auth", "use", "www.example.com"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["default_site"], "example.realgeeks.com");

    let out = env
        .cmd()
        .args(["auth", "logout", "--site", "www.example.com"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["site"], "example.realgeeks.com");
    let config = std::fs::read_to_string(env.config_dir.path().join("config.toml")).unwrap();
    assert!(!config.contains("rg_live_new"), "{config}");
}

#[test]
fn login_without_site_details_stores_no_aliases() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/me/")
        .with_body(r#"{"api_key":{"name":"CLI","scopes":[]}}"#)
        .create();
    let out = env
        .cmd()
        .env_remove("GEEKCLI_API_KEY")
        .args(["auth", "login", "--api-key", "rg_live_new"])
        .assert()
        .success()
        .stderr(predicate::str::contains("live domain").not())
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["aliases"], json!([]));
    let config = std::fs::read_to_string(env.config_dir.path().join("config.toml")).unwrap();
    assert!(!config.contains("aliases"), "{config}");
}

fn write_config(env: &Env, body: &str) {
    std::fs::write(env.config_dir.path().join("config.toml"), body).unwrap();
}

#[test]
fn sites_refresh_backfills_aliases_for_an_old_config() {
    let mut env = Env::new();
    let url = env.server.url();
    write_config(
        &env,
        &format!(
            "default_site = \"example.realgeeks.com\"\n\n[sites.\"example.realgeeks.com\"]\napi_key = \"rg_live_old\"\nbase_url = \"{url}\"\n"
        ),
    );

    // before the refresh the live domain is unknown, and the error says what to do
    env.cmd()
        .env_remove("GEEKCLI_API_KEY")
        .env_remove("GEEKCLI_BASE_URL")
        .args(["categories", "list", "--site", "www.example.com"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("auth sites --refresh"));

    let me = env
        .server
        .mock("GET", "/api/v3/me/")
        .match_header("authorization", "Bearer rg_live_old")
        .with_body(ME_WITH_LIVE_DOMAIN)
        .create();
    let out = env
        .cmd()
        .env_remove("GEEKCLI_API_KEY")
        .env_remove("GEEKCLI_BASE_URL")
        .args(["auth", "sites", "--refresh"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    me.assert();
    assert_eq!(
        parse(&out)["results"][0]["aliases"],
        json!(["www.example.com"])
    );

    env.server
        .mock("GET", "/api/v3/blog/categories/")
        .match_header("authorization", "Bearer rg_live_old")
        .with_body(r#"{"results":[],"pagination":{}}"#)
        .create();
    env.cmd()
        .env_remove("GEEKCLI_API_KEY")
        .env_remove("GEEKCLI_BASE_URL")
        .args(["categories", "list", "--site", "www.example.com"])
        .assert()
        .success();
}

#[test]
fn an_alias_shared_by_two_sites_exits_2() {
    let env = Env::new();
    write_config(
        &env,
        "[sites.\"a.example.realgeeks.com\"]\napi_key = \"rg_live_a\"\naliases = [\"www.example.com\"]\n\n[sites.\"b.example.realgeeks.com\"]\napi_key = \"rg_live_b\"\naliases = [\"www.example.com\"]\n",
    );
    env.cmd()
        .env_remove("GEEKCLI_API_KEY")
        .env("GEEKCLI_SITE", "www.example.com")
        .args(["me"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "a.example.realgeeks.com, b.example.realgeeks.com",
        ));
}

#[test]
fn home_page_alfred_flags_build_a_partial_patch_and_report_hidden_modules() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/content/home-page/")
        .with_body(
            r#"{"id":1,"section_effects":{"about":"fade","listings":"pinned","contact":"fade"}}"#,
        )
        .create();
    let patch = env
        .server
        .mock("PATCH", "/api/v3/content/home-page/")
        .match_body(Matcher::Json(json!({
            "sections": ["about", "listings", "buy-sell-value"],
            // merged onto the page's effects: listings keeps pinned
            "section_effects": {"about": "parallax", "listings": "pinned", "contact": "fade"},
            "extra_content": {"Hero_Eyebrow": "Westside - Estates", "Hero_Poster": null}
        })))
        .with_body(
            r#"{"id":1,"url":"https://x/","sections":["about","listings","buy-sell-value"],
                "empty_sections":{"buy-sell-value":"No Buy or Sell cards and the home valuation is off.","press":"No press logos yet."},
                "inactive_settings":{"section_effects.about":"Parallax moves the module's photos, and it has none yet."}}"#,
        )
        .create();
    let out = env
        .cmd()
        .args([
            "home-page",
            "update",
            "--sections",
            "about, listings,buy-sell-value",
            "--section-effect",
            "about=parallax",
            "--area",
            "Hero_Eyebrow=Westside - Estates",
            "--area",
            "Hero_Poster=null",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    patch.assert();
    let stderr = String::from_utf8_lossy(&out.stderr);
    // the switched-on empty module is named; press is off, so it isn't
    assert!(stderr.contains(
        "note: the buy-sell-value module is switched on but not on the page: No Buy or Sell cards"
    ));
    assert!(!stderr.contains("press"));
    assert!(stderr.contains("note: section_effects.about does nothing yet: Parallax"));
    assert_eq!(parse(&out.stdout)["sections"][2], "buy-sell-value");

    env.cmd()
        .args(["home-page", "update", "--area", "no-equals-sign"])
        .assert()
        .code(2);
}

#[test]
fn home_page_sections_switch_all_off_with_none() {
    let mut env = Env::new();
    let patch = env
        .server
        .mock("PATCH", "/api/v3/content/home-page/")
        .match_body(Matcher::Json(json!({"sections": []})))
        .with_body(r#"{"id":1,"sections":[]}"#)
        .create();
    env.cmd()
        .args(["home-page", "update", "--sections", "none"])
        .assert()
        .success();
    patch.assert();
}

#[test]
fn home_page_items_set_replaces_one_kind_and_says_when_the_module_still_hides() {
    let mut env = Env::new();
    let put = env
        .server
        .mock("PUT", "/api/v3/content/home-page/sections/tile/")
        .match_body(Matcher::Json(json!({"items": [
            {"text": "Buy", "title": "Find the one.", "link": "/search/"}
        ]})))
        .with_body(
            r#"{"kind":"tile","items":[{"text":"Buy","title":"Find the one.","link":"/search/"}]}"#,
        )
        .create();
    env.server
        .mock("GET", "/api/v3/content/home-page/sections/")
        .with_body(
            r#"{"sections":["buy-sell-value","journal"],
                "empty_sections":{"buy-sell-value":"Needs a link on each card.","journal":"No published blog posts yet."},
                "results":[]}"#,
        )
        .create();
    let out = env
        .cmd()
        .args([
            "home-page",
            "items",
            "set",
            "tile",
            "--data",
            r#"{"items":[{"text":"Buy","title":"Find the one.","link":"/search/"}]}"#,
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    put.assert();
    let stderr = String::from_utf8_lossy(&out.stderr);
    // only the module these rows fill is reported
    assert!(stderr.contains("note: the buy-sell-value module is switched on but not on the page"));
    assert!(!stderr.contains("journal"));
    assert_eq!(parse(&out.stdout)["kind"], "tile");

    env.cmd()
        .args(["home-page", "items", "get", "cards"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "stat, award, press, tile, testimonial, neighborhood",
        ));
}

#[test]
fn featured_areas_set_takes_attribute_value_pairs() {
    let mut env = Env::new();
    let put = env
        .server
        .mock("PUT", "/api/v3/content/featured-areas/")
        .match_body(Matcher::Json(json!({"areas": [
            {"attribute": "city", "value": "Brentwood"},
            {"attribute": "city", "value": "Pacific Palisades"}
        ]})))
        .with_body(r#"{"results":[{"attribute":"city","value":"Brentwood","label":"Brentwood","key":"city||Brentwood","url":"/listings/city/Brentwood/"}],"max_items":30}"#)
        .create();
    let out = env
        .cmd()
        .args([
            "featured-areas",
            "set",
            "city=Brentwood",
            "city=Pacific Palisades",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    put.assert();
    assert_eq!(parse(&out)["results"][0]["key"], "city||Brentwood");
    env.cmd()
        .args(["featured-areas", "set", "Brentwood"])
        .assert()
        .code(2);
}

#[test]
fn categories_crud_and_post_create_categories() {
    let mut env = Env::new();
    server_categories(&mut env);

    let out = env
        .cmd()
        .args(["categories", "get", "1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["slug"], "news");

    let out = env
        .cmd()
        .args(["categories", "get", "news"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["id"], 1);

    let create = env
        .server
        .mock("POST", "/api/v3/blog/categories/")
        .match_body(Matcher::Json(
            json!({ "name": "Market Updates", "slug": "market-updates" }),
        ))
        .with_status(201)
        .with_body(r#"{"id":2,"name":"Market Updates","slug":"market-updates"}"#)
        .create();
    env.cmd()
        .args(["categories", "create", "--name", "Market Updates"])
        .assert()
        .success();
    create.assert();

    env.cmd()
        .args(["categories", "update", "1"])
        .assert()
        .code(2);
    let update = env
        .server
        .mock("PATCH", "/api/v3/blog/categories/1/")
        .match_body(Matcher::Json(json!({ "name": "Local News" })))
        .with_body(r#"{"id":1,"name":"Local News","slug":"news"}"#)
        .create();
    env.cmd()
        .args(["categories", "update", "1", "--name", "Local News"])
        .assert()
        .success();
    update.assert();

    let del = env
        .server
        .mock("DELETE", "/api/v3/blog/categories/1/")
        .match_query(Matcher::UrlEncoded("force".into(), "true".into()))
        .with_status(204)
        .create();
    let out = env
        .cmd()
        .args(["categories", "delete", "1", "--force", "-y"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["deleted"], true);
    del.assert();

    // --create-categories creates missing slugs and skips existing ones
    let ensure_create = env
        .server
        .mock("POST", "/api/v3/blog/categories/")
        .match_body(Matcher::Json(
            json!({ "name": "New Topic", "slug": "new-topic" }),
        ))
        .with_status(201)
        .with_body(r#"{"id":3,"name":"New Topic","slug":"new-topic"}"#)
        .create();
    let post = env
        .server
        .mock("POST", "/api/v3/blog/posts/")
        .match_body(Matcher::PartialJson(json!({
            "categories": ["news", "new-topic"]
        })))
        .with_status(201)
        .with_body(r#"{"id":10,"slug":"p"}"#)
        .create();
    env.cmd()
        .args([
            "posts",
            "create",
            "--title",
            "P",
            "--slug",
            "p",
            "--content",
            "<p>x</p>",
            "--category",
            "news,new-topic",
            "--create-categories",
        ])
        .assert()
        .success();
    ensure_create.assert();
    post.assert();
}

fn server_categories(env: &mut Env) {
    env.server
        .mock("GET", "/api/v3/blog/categories/1/")
        .with_body(r#"{"id":1,"name":"News","slug":"news"}"#)
        .expect_at_least(1)
        .create();
    env.server
        .mock("GET", "/api/v3/blog/categories/")
        .match_query(Matcher::UrlEncoded("slug".into(), "news".into()))
        .with_body(r#"{"results":[{"id":1,"name":"News","slug":"news"}],"pagination":{}}"#)
        .create();
    env.server
        .mock("GET", "/api/v3/blog/categories/")
        .match_query(Matcher::UrlEncoded("page_size".into(), "100".into()))
        .with_body(
            r#"{"results":[{"id":1,"name":"News","slug":"news"}],"pagination":{"next":null}}"#,
        )
        .create();
}

#[test]
fn posts_lifecycle_publish_unpublish_update_variants_delete_and_revert() {
    let mut env = Env::new();
    let list = env
        .server
        .mock("GET", "/api/v3/blog/posts/")
        .match_query(Matcher::UrlEncoded("status".into(), "draft".into()))
        .with_body(r#"{"results":[],"pagination":{}}"#)
        .create();
    env.cmd()
        .args(["posts", "list", "--status", "draft"])
        .assert()
        .success();
    list.assert();

    env.cmd()
        .args([
            "posts",
            "create",
            "--title",
            "Missing Slug",
            "--content",
            "<p>x</p>",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--slug is required"));

    env.server
        .mock("GET", "/api/v3/blog/posts/9/")
        .with_body(r#"{"id":9,"title":"Nine","slug":"nine","categories":[{"id":1,"slug":"news"}]}"#)
        .expect_at_least(3)
        .create();

    // single-resource quiet (-q) and jsonl (-o jsonl) output formats
    env.cmd()
        .args(["posts", "get", "9", "-q"])
        .assert()
        .success()
        .stdout("9\n");
    let jsonl = env
        .cmd()
        .args(["-o", "jsonl", "posts", "get", "9"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let jsonl_str = String::from_utf8_lossy(&jsonl);
    assert_eq!(jsonl_str.lines().count(), 1, "{jsonl_str}");
    assert_eq!(parse(jsonl_str.trim().as_bytes())["slug"], "nine");

    // empty update is usage error
    env.cmd().args(["posts", "update", "9"]).assert().code(2);

    // --clear-categories and --replace (PUT)
    let put = env
        .server
        .mock("PUT", "/api/v3/blog/posts/9/")
        .match_body(Matcher::Json(json!({ "title": "Full", "categories": [] })))
        .with_body(r#"{"id":9,"title":"Full"}"#)
        .create();
    env.cmd()
        .args([
            "posts",
            "update",
            "9",
            "--title",
            "Full",
            "--clear-categories",
            "--replace",
        ])
        .assert()
        .success();
    put.assert();

    // --add-category deduplicates an existing numeric id and existing slug
    let dedup = env
        .server
        .mock("PATCH", "/api/v3/blog/posts/9/")
        .match_body(Matcher::Json(json!({ "categories": [1, 2] })))
        .with_body(r#"{"id":9}"#)
        .create();
    env.cmd()
        .args(["posts", "update", "9", "--add-category", "1,news,2"])
        .assert()
        .success();
    dedup.assert();

    // publish default (sets publish timestamp) and --keep-date (omits publish)
    let pub_now = env
        .server
        .mock("PATCH", "/api/v3/blog/posts/9/")
        .match_body(Matcher::AllOf(vec![
            Matcher::PartialJson(json!({ "status": "published" })),
            Matcher::Regex(r#""publish":"[^"]+""#.into()),
        ]))
        .with_body(r#"{"id":9,"state":"published","url":"https://x/blog/nine/"}"#)
        .create();
    env.cmd()
        .args(["-o", "table", "posts", "publish", "9"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Published id 9"));
    pub_now.assert();

    let pub_keep = env
        .server
        .mock("PATCH", "/api/v3/blog/posts/9/")
        .match_body(Matcher::Json(json!({ "status": "published" })))
        .with_body(r#"{"id":9,"state":"published"}"#)
        .create();
    env.cmd()
        .args(["posts", "publish", "9", "--keep-date"])
        .assert()
        .success();
    pub_keep.assert();

    let unpub = env
        .server
        .mock("PATCH", "/api/v3/blog/posts/9/")
        .match_body(Matcher::Json(json!({ "status": "draft" })))
        .with_body(r#"{"id":9,"state":"draft"}"#)
        .create();
    env.cmd()
        .args(["posts", "unpublish", "9"])
        .assert()
        .success();
    unpub.assert();

    let revert = env
        .server
        .mock("POST", "/api/v3/blog/posts/9/revisions/512/revert/")
        .with_body(r#"{"id":9,"slug":"nine"}"#)
        .create();
    env.cmd()
        .args(["-y", "posts", "revert", "9", "512"])
        .assert()
        .success();
    revert.assert();

    let del = env
        .server
        .mock("DELETE", "/api/v3/blog/posts/9/")
        .with_status(204)
        .create();
    let out = env
        .cmd()
        .args(["posts", "delete", "9", "-y"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["deleted"], true);
    del.assert();
}

#[test]
fn pages_and_area_pages_remaining_subcommands() {
    let mut env = Env::new();
    // parent lookup falls back from content/pages/ to content/area-pages/
    env.server
        .mock("GET", "/api/v3/content/pages/")
        .match_query(Matcher::UrlEncoded("slug".into(), "riverside".into()))
        .with_body(r#"{"results":[],"pagination":{}}"#)
        .create();
    env.server
        .mock("GET", "/api/v3/content/area-pages/")
        .match_query(Matcher::UrlEncoded("slug".into(), "riverside".into()))
        .with_body(r#"{"results":[{"id":23,"slug":"riverside"}],"pagination":{}}"#)
        .create();
    let create = env
        .server
        .mock("POST", "/api/v3/content/pages/")
        .match_body(Matcher::Json(json!({
            "slug": "schools",
            "anchor_text": "Schools",
            "parent": 23,
            "property_display_location": "above",
            "search": 42,
            "landscape_image_override": null
        })))
        .with_status(201)
        .with_body(r#"{"id":8,"path":"/riverside/schools/"}"#)
        .create();
    env.cmd()
        .args([
            "pages",
            "create",
            "--slug",
            "schools",
            "--anchor-text",
            "Schools",
            "--parent",
            "riverside",
            "--property-display-location",
            "above",
            "--search",
            "42",
            "--landscape",
            "null",
        ])
        .assert()
        .success();
    create.assert();

    env.cmd()
        .args([
            "pages",
            "create",
            "--slug",
            "x",
            "--anchor-text",
            "X",
            "--area",
            "=bad",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no name before '='"));

    // pages update empty error, --replace (PUT), revisions, revision, search (no_search 404)
    env.cmd().args(["pages", "update", "8"]).assert().code(2);
    let put = env
        .server
        .mock("PUT", "/api/v3/content/pages/8/")
        .match_body(Matcher::Json(
            json!({ "property_display_location": "below" }),
        ))
        .with_body(r#"{"id":8}"#)
        .create();
    env.cmd()
        .args([
            "pages",
            "update",
            "8",
            "--property-display-location",
            "below",
            "--replace",
        ])
        .assert()
        .success();
    put.assert();

    env.server
        .mock("GET", "/api/v3/content/pages/8/")
        .with_body(r#"{"id":8,"path":"/riverside/schools/"}"#)
        .create();
    env.cmd()
        .args(["pages", "search", "8"])
        .assert()
        .code(4)
        .stderr(predicate::str::contains("no_search"));

    env.server
        .mock("GET", "/api/v3/content/pages/8/revisions/")
        .with_body(r#"{"results":[{"id":1}]}"#)
        .create();
    env.server
        .mock("GET", "/api/v3/content/pages/8/revisions/1/")
        .with_body(r#"{"id":1,"preview":{}}"#)
        .create();
    env.cmd()
        .args(["pages", "revisions", "8"])
        .assert()
        .success();
    env.cmd()
        .args(["pages", "revision", "8", "1"])
        .assert()
        .success();

    // area-pages list --featured, delete --orphan-children, revisions, revision
    let ap_list = env
        .server
        .mock("GET", "/api/v3/content/area-pages/")
        .match_query(Matcher::UrlEncoded("featured".into(), "true".into()))
        .with_body(r#"{"results":[{"id":23,"path":"/riverside/"}],"pagination":{}}"#)
        .create();
    env.cmd()
        .args(["area-pages", "list", "--featured", "true"])
        .assert()
        .success();
    ap_list.assert();

    env.server
        .mock("GET", "/api/v3/content/area-pages/23/")
        .with_body(r#"{"id":23,"path":"/riverside/"}"#)
        .create();
    let ap_del = env
        .server
        .mock("DELETE", "/api/v3/content/area-pages/23/")
        .match_query(Matcher::UrlEncoded("orphan_children".into(), "true".into()))
        .with_status(204)
        .create();
    env.cmd()
        .args(["area-pages", "delete", "23", "--orphan-children", "-y"])
        .assert()
        .success();
    ap_del.assert();

    env.server
        .mock("GET", "/api/v3/content/area-pages/23/revisions/")
        .with_body(r#"{"results":[{"id":4}]}"#)
        .create();
    env.server
        .mock("GET", "/api/v3/content/area-pages/23/revisions/4/")
        .with_body(r#"{"id":4,"preview":{}}"#)
        .create();
    env.cmd()
        .args(["area-pages", "revisions", "23"])
        .assert()
        .success();
    env.cmd()
        .args(["area-pages", "revision", "23", "4"])
        .assert()
        .success();
}

#[test]
fn featured_areas_get_and_data_inputs() {
    let mut env = Env::new();
    let resp = r#"{"results":[{"attribute":"city","value":"Miami","label":"Miami","key":"city||Miami","url":"/listings/city/Miami/"}]}"#;
    env.server
        .mock("GET", "/api/v3/content/featured-areas/")
        .with_body(resp)
        .create();
    let out = env
        .cmd()
        .args(["featured-areas", "get"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["results"][0]["key"], "city||Miami");

    let file = env.config_dir.path().join("areas.json");
    std::fs::write(&file, r#"{"areas":[{"attribute":"city","value":"Miami"}]}"#).unwrap();
    let put = env
        .server
        .mock("PUT", "/api/v3/content/featured-areas/")
        .match_body(Matcher::Json(
            json!({ "areas": [{"attribute": "city", "value": "Miami"}] }),
        ))
        .with_body(resp)
        .expect(2)
        .create();
    env.cmd()
        .args(["featured-areas", "set", "--data"])
        .arg(format!("@{}", file.display()))
        .assert()
        .success();
    env.cmd()
        .args([
            "featured-areas",
            "set",
            "--data",
            r#"[{"attribute":"city","value":"Miami"}]"#,
        ])
        .assert()
        .success();
    put.assert();

    env.cmd()
        .args(["featured-areas", "set", "--data", "{}"])
        .assert()
        .code(2);
}

#[test]
fn auth_status_stdin_browser_print_url_logout_use_and_refresh_errors() {
    let mut env = Env::new();
    // auth status subcommand works like `me`
    env.server
        .mock("GET", "/api/v3/me/")
        .with_body(r#"{"api_key":{"name":"CLI","scopes":["blog:write"]},"site":{"domain":"www.test.local"}}"#)
        .expect_at_least(2)
        .create();
    let out = env
        .cmd()
        .args(["auth", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["site"]["domain"], "www.test.local");

    // login without --site is exit 2
    env.cmd()
        .env_remove("GEEKCLI_SITE")
        .args(["auth", "login", "--api-key", "rg_live_k"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--site <domain> is required"));

    // --api-key-stdin with empty input is exit 2, with valid key succeeds
    env.cmd()
        .env_remove("GEEKCLI_API_KEY")
        .args(["auth", "login", "--api-key-stdin"])
        .write_stdin("   \n")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("empty API key"));
    env.cmd()
        .env_remove("GEEKCLI_API_KEY")
        .args(["auth", "login", "--api-key-stdin"])
        .write_stdin("rg_live_stdin_key\n")
        .assert()
        .success();

    // auth use unknown site fails (exit 3) with refresh hint when stored site has no aliases
    env.cmd()
        .args(["auth", "use", "unknown.example.com"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("auth sites --refresh"));

    // auth logout on a site with no stored key is exit 3
    env.cmd()
        .args(["auth", "logout", "--site", "not-stored.example.com"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("no stored key"));

    // auth sites when empty prints {"results": []} in JSON mode, and auth use reports no stored key
    let empty_dir = tempfile::tempdir().unwrap();
    let empty_sites = env
        .cmd()
        .env("GEEKCLI_CONFIG_DIR", empty_dir.path())
        .args(["auth", "sites"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&empty_sites), json!({"results": []}));
    env.cmd()
        .env("GEEKCLI_CONFIG_DIR", empty_dir.path())
        .args(["auth", "use", "missing.example.com"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "no stored key for missing.example.com",
        ));

    // auth use and auth logout emit structured JSON in non-table mode
    let used = env
        .cmd()
        .args(["auth", "use", "www.test.local"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&used), json!({"default_site": "www.test.local"}));
    let logged_out = env
        .cmd()
        .args(["auth", "logout", "--site", "www.test.local"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        parse(&logged_out),
        json!({"logged_out": true, "site": "www.test.local"})
    );

    // auth login --print-url drives the full browser callback flow through the CLI binary
    // (covering default_key_name, scopes, Created API key note, and missing api_key error).
    for missing_key in [false, true] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mock_port = listener.local_addr().unwrap().port();
        let mock_url = format!("http://127.0.0.1:{mock_port}");
        let server_url = mock_url.clone();
        let server_thread = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let req_count = if missing_key { 2 } else { 3 };
            for _ in 0..req_count {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let mut header_end = None;
                let mut content_len = 0usize;
                loop {
                    let n = stream.read(&mut chunk).unwrap();
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if header_end.is_none() {
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            header_end = Some(pos + 4);
                            let hdrs = String::from_utf8_lossy(&buf[..pos]);
                            for line in hdrs.lines() {
                                if let Some((k, v)) = line.split_once(':') {
                                    if k.eq_ignore_ascii_case("content-length") {
                                        content_len = v.trim().parse().unwrap_or(0);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(end) = header_end {
                        if buf.len() >= end + content_len {
                            break;
                        }
                    }
                }
                let end = header_end.unwrap_or(buf.len());
                let head = String::from_utf8_lossy(&buf[..end]).to_string();
                let first_line = head.lines().next().unwrap_or("");
                let body_bytes = &buf[end..buf.len().min(end + content_len)];
                let (status, resp_body) = if first_line.starts_with("POST /api/v3/auth/cli/start/")
                {
                    let req: Value = serde_json::from_slice(body_bytes).unwrap();
                    let state = req["state"].as_str().unwrap().to_string();
                    let cb_port = req["port"].as_u64().unwrap();
                    let cb_addr = format!("127.0.0.1:{cb_port}");
                    std::thread::spawn(move || {
                        for _ in 0..50 {
                            if let Ok(mut cb) = std::net::TcpStream::connect(&cb_addr) {
                                let _ = write!(
                                    cb,
                                    "GET /callback?code=cli-code&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
                                );
                                let _ = cb.flush();
                                let mut sink = [0u8; 256];
                                let _ = cb.read(&mut sink);
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(15));
                        }
                    });
                    (
                        "201 Created",
                        format!(
                            r#"{{"request_id":"req-cli","authorize_url":"{server_url}/approve/","expires_in":5}}"#
                        ),
                    )
                } else if first_line.starts_with("POST /api/v3/auth/cli/token/") {
                    if missing_key {
                        ("201 Created", r#"{"key":{"name":"Broken"}}"#.to_string())
                    } else {
                        (
                            "201 Created",
                            r#"{"api_key":"rg_live_browser","key":{"name":"Real Geeks CLI"},"site":{"domain":"www.browser.local"}}"#
                                .to_string(),
                        )
                    }
                } else {
                    (
                        "200 OK",
                        r#"{"api_key":{"name":"Real Geeks CLI","scopes":["blog:write"]},"site":{"domain":"www.browser.local"}}"#
                            .to_string(),
                    )
                };
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp_body}",
                    resp_body.len()
                );
                let _ = stream.write_all(reply.as_bytes());
                let _ = stream.flush();
            }
        });

        let cfg = tempfile::tempdir().unwrap();
        let mut cmd = common::geekcli();
        cmd.env("GEEKCLI_CONFIG_DIR", cfg.path())
            .env("GEEKCLI_SITE", "www.browser.local")
            .env("GEEKCLI_BASE_URL", &mock_url)
            .args(["-o", "table", "auth", "login", "--print-url"]);
        if missing_key {
            cmd.assert()
                .code(1)
                .stderr(predicate::str::contains("exchange response had no api_key"));
        } else {
            cmd.assert().success().stderr(predicate::str::contains(
                "Created API key \"Real Geeks CLI\"",
            ));
        }
        server_thread.join().unwrap();
    }

    // auth sites --refresh reports unreachable site in table mode without aborting
    let dead_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_port = dead_listener.local_addr().unwrap().port();
    drop(dead_listener);
    let dead_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dead_dir.path().join("config.toml"),
        format!(
            "[sites.\"dead.test.local\"]\napi_key = \"rg_live_dead\"\nbase_url = \"http://127.0.0.1:{dead_port}\"\n"
        ),
    )
    .unwrap();
    env.cmd()
        .env("GEEKCLI_CONFIG_DIR", dead_dir.path())
        .env_remove("GEEKCLI_API_KEY")
        .env_remove("GEEKCLI_BASE_URL")
        .args(["-o", "table", "auth", "sites", "--refresh"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Could not refresh dead.test.local",
        ));
}
