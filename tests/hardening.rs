//! What geekcli refuses to read and send, how it keeps its config private,
//! which names a site may claim, and what reaches the terminal.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use mockito::{Matcher, Server, ServerGuard};
use predicates::prelude::*;
use serde_json::{json, Value};

const KEY: &str = "rg_live_testkey0000000000000000000000";
const LOGO: &str = r#"{"name":"logo.png","path":"images/logo.png","type":"file","url":"https://u.realgeeks.media/example/images/logo.png","size":5,"content_type":"image/png"}"#;

struct Env {
    server: ServerGuard,
    config_dir: tempfile::TempDir,
    work: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        Self {
            server: Server::new(),
            config_dir: tempfile::tempdir().unwrap(),
            work: tempfile::tempdir().unwrap(),
        }
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

    fn file(&self, name: &str, body: &[u8]) -> std::path::PathBuf {
        let path = self.work.path().join(name);
        std::fs::write(&path, body).unwrap();
        path
    }
}

fn parse(out: &[u8]) -> Value {
    serde_json::from_slice(out)
        .unwrap_or_else(|e| panic!("not JSON: {e}\n{}", String::from_utf8_lossy(out)))
}

// --- C1: local files ---------------------------------------------------------

#[test]
fn a_hidden_body_file_is_refused_unless_allowed() {
    let mut env = Env::new();
    let secret = env.file(".env", br#"{"title":"AWS_SECRET=abc"}"#);
    let post = env
        .server
        .mock("POST", "/api/v3/blog/posts/")
        .with_status(201)
        .with_body(r#"{"id":1}"#)
        .expect(1)
        .create();

    env.cmd()
        .args(["api", "POST", "blog/posts/", "-d"])
        .arg(format!("@{}", secret.display()))
        .assert()
        .code(2)
        .stderr(predicate::str::contains("it is a hidden file"))
        .stderr(predicate::str::contains("--allow-any-source"));

    env.cmd()
        .args(["posts", "create", "--title", "x", "--body-file"])
        .arg(&secret)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("GEEKCLI_ALLOW_ANY_SOURCE=1"));

    // the override, by flag
    env.cmd()
        .args(["api", "POST", "blog/posts/", "--allow-any-source", "-d"])
        .arg(format!("@{}", secret.display()))
        .assert()
        .success();
    post.assert();
}

#[test]
fn the_config_file_is_never_a_body() {
    let env = Env::new();
    let config = env.config_dir.path().join("config.toml");
    std::fs::write(&config, "[sites.\"a.com\"]\napi_key = \"rg_live_secret\"\n").unwrap();
    env.cmd()
        .args(["api", "POST", "blog/posts/", "-d"])
        .arg(format!("@{}", config.display()))
        .assert()
        .code(2)
        .stderr(predicate::str::contains("holds your API keys"))
        .stderr(predicate::str::contains("rg_live_secret").not());
}

#[test]
fn an_ordinary_body_file_still_works() {
    let mut env = Env::new();
    let body = env.file("post.json", br#"{"title":"Hi"}"#);
    let post = env
        .server
        .mock("POST", "/api/v3/blog/posts/")
        .match_body(Matcher::Json(json!({"title": "Hi"})))
        .with_status(201)
        .with_body(r#"{"id":1}"#)
        .create();
    env.cmd()
        .args(["api", "POST", "blog/posts/", "-d"])
        .arg(format!("@{}", body.display()))
        .assert()
        .success();
    post.assert();
}

#[test]
fn an_upload_source_must_be_the_type_it_is_stored_as() {
    let mut env = Env::new();
    let key = env.file("id_rsa", b"-----BEGIN OPENSSH PRIVATE KEY-----");
    let notes = env.file("notes.md", b"# notes");
    let hidden = env.file(".hero.png", b"\x89PNG fake");
    let none = env
        .server
        .mock("POST", "/api/v3/files/upload/")
        .expect(0)
        .create();

    let refused = |args: &[&str], file: &std::path::Path, why: &str| {
        env.cmd()
            .args(["files", "upload"])
            .args(args)
            .arg(file)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("nothing uploaded"))
            .stderr(predicate::str::contains(why))
            .stderr(predicate::str::contains("--allow-any-source"));
    };
    refused(&["--name", "x.txt"], &key, "must itself be one of");
    refused(&["--name", "x.txt"], &notes, "must itself be one of");
    refused(&[], &hidden, "hidden file");
    none.assert();

    let png = env.file("hero.png", b"\x89PNG fake");
    env.cmd()
        .args(["files", "upload", "--name", "photo.jpg"])
        .arg(&png)
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "a .png file cannot be stored as photo.jpg",
        ));
}

#[test]
fn an_upload_source_check_allows_same_type_and_the_override() {
    let mut env = Env::new();
    let jpg = env.file("Photo.JPG", b"\xff\xd8 fake");
    let notes = env.file("notes.md", b"# notes");
    let upload = env
        .server
        .mock("POST", "/api/v3/files/upload/")
        .with_status(201)
        .with_body(LOGO)
        .expect(2)
        .create();

    // .JPG stored as .jpeg: same type, any case
    env.cmd()
        .args(["files", "upload", "--name", "photo.jpeg"])
        .arg(&jpg)
        .assert()
        .success();
    // a Markdown file stored as text, by explicit choice
    env.cmd()
        .env("GEEKCLI_ALLOW_ANY_SOURCE", "1")
        .args(["files", "upload", "--name", "notes.txt"])
        .arg(&notes)
        .assert()
        .success();
    upload.assert();
}

// --- C2: config permissions ----------------------------------------------------

#[cfg(unix)]
#[test]
fn a_config_others_can_read_is_made_private_with_a_warning() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new();
    let dir = env.config_dir.path();
    let config = dir.join("config.toml");
    std::fs::write(&config, "[sites.\"a.com\"]\napi_key = \"rg_live_a\"\n").unwrap();
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    env.cmd()
        .args(["auth", "sites"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "accessible to other users (mode 644)",
        ))
        .stderr(predicate::str::contains(
            "accessible to other users (mode 755)",
        ));

    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&config), 0o600);
    assert_eq!(mode(dir), 0o700);

    // once private, nothing more is said
    env.cmd()
        .args(["auth", "sites"])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning").not());
}

// --- C4: aliases ---------------------------------------------------------------

#[test]
fn login_drops_names_the_site_cannot_claim() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/me/")
        .with_body(r#"{"api_key":{"name":"CLI","scopes":[]},"site":{"domain":"www.victim.com","current_url":"https://evil.com/"}}"#)
        .create();

    let out = env
        .cmd()
        .env_remove("GEEKCLI_API_KEY")
        .env("GEEKCLI_SITE", "www.evil.com")
        .args(["-v", "auth", "login", "--api-key", "rg_live_new"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "not recording www.victim.com as another name for www.evil.com",
        ))
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["aliases"], json!(["evil.com"]));
    let config = std::fs::read_to_string(env.config_dir.path().join("config.toml")).unwrap();
    assert!(!config.contains("victim"), "{config}");
}

// --- C10: terminal output -------------------------------------------------------

#[test]
fn server_field_names_cannot_send_terminal_escapes() {
    let mut env = Env::new();
    env.server
        .mock("GET", "/api/v3/thing/")
        .with_body(r#"{"evil\u001b]52;c;cHduZWQ=\u0007":"v","nested":{"\u001b[2Jx":"y"}}"#)
        .create();
    let out = env
        .cmd()
        .args(["-o", "table", "api", "GET", "thing/"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out);
    assert!(
        !text.contains('\u{1b}') && !text.contains('\u{7}'),
        "{text:?}"
    );
    assert!(text.contains("evil]52;c;cHduZWQ="), "{text}");
}

#[test]
fn quiet_file_urls_cannot_send_terminal_escapes() {
    let mut env = Env::new();
    let png = env.file("hero.png", b"\x89PNG fake");
    env.server
        .mock("POST", "/api/v3/files/upload/")
        .with_status(201)
        .with_body(r#"{"path":"hero.png","url":"https://u.realgeeks.media/x/hero.png\u001b]0;pwned\u0007"}"#)
        .create();
    env.cmd()
        .args(["-q", "files", "upload"])
        .arg(&png)
        .assert()
        .success()
        .stdout("https://u.realgeeks.media/x/hero.png]0;pwned\n");
}
