//! `geekcli update` against a mock of GitHub's releases API. The installing
//! tests run a copy of the binary, since a successful update replaces it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use mockito::{Matcher, Mock, Server, ServerGuard};
use serde_json::{json, Value};

const CURRENT: &str = env!("CARGO_PKG_VERSION");
const RELEASES: &str = "/repos/RealGeeks/geekcli/releases";

struct Env {
    server: ServerGuard,
}

impl Env {
    fn new() -> Self {
        Self {
            server: Server::new(),
        }
    }

    fn cmd(&self) -> Command {
        self.configure(Command::cargo_bin("geekcli").unwrap())
    }

    fn configure(&self, mut cmd: Command) -> Command {
        cmd.env_clear()
            .env(
                "SYSTEMROOT",
                std::env::var("SYSTEMROOT").unwrap_or_default(),
            )
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            // a site key in the environment must never reach the release host
            .env("GEEKCLI_SITE", "www.test.local")
            .env("GEEKCLI_API_KEY", "rg_live_testkey0000000000000000000000")
            .env("GEEKCLI_UPDATE_API_URL", self.server.url());
        cmd
    }

    fn release(&mut self, path: &str, tag: &str, assets: &Value) -> Mock {
        self.server
            .mock("GET", format!("{RELEASES}/{path}").as_str())
            .match_header("authorization", Matcher::Missing)
            .with_body(
                json!({
                    "tag_name": tag,
                    "name": tag,
                    "created_at": "2026-01-01T00:00:00Z",
                    "assets": assets,
                })
                .to_string(),
            )
            .create()
    }
}

fn parse(out: &[u8]) -> Value {
    serde_json::from_slice(out)
        .unwrap_or_else(|e| panic!("not JSON: {e}\n{}", String::from_utf8_lossy(out)))
}

#[test]
fn check_reports_a_newer_release_and_installs_nothing() {
    let mut env = Env::new();
    let latest = env.release("latest", "v99.0.0", &json!([]));

    let out = env.cmd().args(["update", "--check"]).output().unwrap();

    assert!(out.status.success());
    latest.assert();
    let doc = parse(&out.stdout);
    assert_eq!(doc["current_version"], CURRENT);
    assert_eq!(doc["release_version"], "99.0.0");
    assert_eq!(doc["update_available"], true);
    assert_eq!(doc["updated"], false);
}

#[test]
fn the_current_release_is_up_to_date_without_downloading() {
    let mut env = Env::new();
    // no assets and no download mocks: anything fetched beyond this would fail
    env.release("latest", &format!("v{CURRENT}"), &json!([]));

    let out = env.cmd().arg("update").output().unwrap();

    assert!(out.status.success());
    let doc = parse(&out.stdout);
    assert_eq!(doc["update_available"], false);
    assert_eq!(doc["updated"], false);
}

#[test]
fn an_unknown_tag_is_not_found() {
    let mut env = Env::new();
    env.server
        .mock("GET", format!("{RELEASES}/tags/v0.0.1").as_str())
        .with_status(404)
        .with_body(r#"{"message":"Not Found"}"#)
        .create();

    let out = env
        .cmd()
        .args(["update", "--tag", "0.0.1"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(4));
    let err = parse(&out.stderr);
    assert_eq!(err["error"]["code"], "not_found");
    assert!(err["error"]["message"].as_str().unwrap().contains("v0.0.1"));
}

#[test]
fn a_release_without_this_platform_fails_before_downloading() {
    let mut env = Env::new();
    env.release("latest", "v99.0.0", &json!([]));

    let out = env.cmd().arg("update").output().unwrap();

    assert_eq!(out.status.code(), Some(1));
    let err = parse(&out.stderr);
    assert!(err["error"]["message"]
        .as_str()
        .unwrap()
        .contains("has no archive for"));
}

#[test]
fn the_release_host_must_be_https() {
    let out = Env::new()
        .cmd()
        .env("GEEKCLI_UPDATE_API_URL", "http://releases.example.com")
        .args(["update", "--check"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(3));
}

/// Replacing the binary: unix only, where a shell script can stand in for
/// the new release.
#[cfg(unix)]
mod install {
    use std::fmt::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use flate2::{write::GzEncoder, Compression};
    use geekcli::commands::update::{archive_name, release_target};
    use sha2::{Digest, Sha256};

    use super::*;

    const TAG: &str = "v99.0.0";

    /// A copy of the built binary that the test is free to replace.
    fn copy_of_binary(dir: &Path) -> PathBuf {
        let path = dir.join("geekcli");
        std::fs::copy(assert_cmd::cargo::cargo_bin("geekcli"), &path).unwrap();
        path
    }

    /// The archive `release.yml` would publish, holding a script that
    /// answers `--version` as the new release.
    fn archive(target: &str) -> Vec<u8> {
        let script = b"#!/bin/sh\necho 'geekcli 99.0.0'\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(script.len() as u64);
        header.set_mode(0o755);
        let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        tar.append_data(
            &mut header,
            format!("geekcli-{TAG}-{target}/geekcli"),
            &script[..],
        )
        .unwrap();
        tar.into_inner().unwrap().finish().unwrap()
    }

    fn sha256(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .fold(String::new(), |mut hex, b| {
                write!(hex, "{b:02x}").unwrap();
                hex
            })
    }

    /// Mock the release and its two assets; `sum` is what the `.sha256` says.
    fn publish(env: &mut Env, bytes: &[u8], sum: &str) -> Mock {
        let target = release_target().unwrap();
        let name = archive_name(TAG, &target);
        let base = env.server.url();
        let assets = json!([
            // the checksum first, as GitHub may list it: selection is by name
            {"name": format!("{name}.sha256"), "url": format!("{base}/assets/2")},
            {"name": name, "url": format!("{base}/assets/1")},
        ]);
        env.release("latest", TAG, &assets);
        env.release(&format!("tags/{TAG}"), TAG, &assets);
        env.server
            .mock("GET", "/assets/2")
            .with_body(format!("{sum}  {name}\n"))
            .create();
        env.server
            .mock("GET", "/assets/1")
            .match_header("authorization", Matcher::Missing)
            .with_body(bytes)
            .create()
    }

    fn version_of(binary: &Path) -> String {
        let out = std::process::Command::new(binary)
            .arg("--version")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{} --version: {:?} {}",
            binary.display(),
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn update_replaces_the_binary_after_the_checksum_passes() {
        let mut env = Env::new();
        let dir = tempfile::tempdir().unwrap();
        let binary = copy_of_binary(dir.path());
        let bytes = archive(&release_target().unwrap());
        let download = publish(&mut env, &bytes, &sha256(&bytes));

        let out = env
            .configure(Command::new(&binary))
            .arg("update")
            .output()
            .unwrap();

        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        download.assert();
        let doc = parse(&out.stdout);
        assert_eq!(doc["release_version"], "99.0.0");
        assert_eq!(doc["updated"], true);
        assert_eq!(version_of(&binary), "geekcli 99.0.0");
        let mode = std::fs::metadata(&binary).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "the new binary is executable");
    }

    #[test]
    fn a_read_only_directory_fails_before_downloading() {
        let mut env = Env::new();
        let dir = tempfile::tempdir().unwrap();
        let binary = copy_of_binary(dir.path());
        let bytes = archive(&release_target().unwrap());
        let download = publish(&mut env, &bytes, &sha256(&bytes)).expect(0);
        let read_only = std::fs::Permissions::from_mode(0o555);
        std::fs::set_permissions(dir.path(), read_only).unwrap();

        let out = env
            .configure(Command::new(&binary))
            .arg("update")
            .output()
            .unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(out.status.code(), Some(1));
        download.assert();
        assert!(parse(&out.stderr)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("cannot write to"));
        assert_eq!(version_of(&binary), format!("geekcli {CURRENT}"));
    }

    #[test]
    fn a_checksum_mismatch_leaves_the_binary_alone() {
        let mut env = Env::new();
        let dir = tempfile::tempdir().unwrap();
        let binary = copy_of_binary(dir.path());
        let bytes = archive(&release_target().unwrap());
        publish(&mut env, &bytes, &"0".repeat(64));

        let out = env
            .configure(Command::new(&binary))
            .arg("update")
            .output()
            .unwrap();

        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty(), "no result document on failure");
        assert!(parse(&out.stderr)["error"]["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("checksum"));
        assert_eq!(version_of(&binary), format!("geekcli {CURRENT}"));
    }
}
