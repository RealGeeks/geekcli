//! Drive the browser-approval flow end to end with a mock site: register,
//! simulate the browser hitting the loopback callback, redeem.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::thread;
use std::time::Duration;

use geekcli::auth::browser::{self, LoginRequest, Pkce};
use geekcli::client::Client;
use geekcli::config::Target;
use mockito::{Matcher, Server};
use serde_json::json;

fn anon_client(base: &str) -> Client {
    let target = Target {
        domain: "www.test.local".into(),
        base_url: base.into(),
        api_key: None,
    };
    Client::new(&target, 0, false).unwrap()
}

fn request(state: &str) -> LoginRequest {
    LoginRequest {
        name: "Real Geeks CLI test".into(),
        scopes: vec!["blog:write".into()],
        port: None,
        state: Some(state.into()),
    }
}

/// Pretend to be the browser after the owner clicked Approve/Deny.
fn browser_callback(port: u16, query: &str) {
    thread::sleep(Duration::from_millis(50));
    let body = reqwest::blocking::get(format!("http://127.0.0.1:{port}/callback?{query}"))
        .unwrap()
        .text()
        .unwrap();
    assert!(body.contains("Real Geeks"));
}

#[test]
fn approve_redeems_code_with_verifier() {
    let mut server = Server::new();
    let start = server
        .mock("POST", "/api/v3/auth/cli/start/")
        .match_body(Matcher::AllOf(vec![
            Matcher::PartialJson(json!({ "name": "Real Geeks CLI test", "scopes": ["blog:write"], "state": "state-1234" })),
            Matcher::Regex(r#""port":\d{4,5}"#.into()),
            Matcher::Regex(r#""code_challenge":"[A-Za-z0-9_-]{43}""#.into()),
        ]))
        .with_status(201)
        .with_body(format!(r#"{{"request_id":"req-1","authorize_url":"{}/admin/api_keys/apikey/authorize-cli/req-1/","expires_in":600}}"#, server.url()))
        .create();
    let token = server
        .mock("POST", "/api/v3/auth/cli/token/")
        .match_body(Matcher::AllOf(vec![
            Matcher::PartialJson(json!({ "code": "one-time-code" })),
            Matcher::Regex(r#""code_verifier":"[A-Za-z0-9_-]{43}""#.into()),
        ]))
        .with_status(201)
        .with_body(r#"{"api_key":"rg_live_minted","key":{"name":"Real Geeks CLI test"},"site":{"domain":"www.test.local"}}"#)
        .create();

    let anon = anon_client(&server.url());
    let pending = browser::start(&anon, &request("state-1234")).unwrap();
    assert_eq!(pending.request_id, "req-1");
    assert!(pending.authorize_url.ends_with("/authorize-cli/req-1/"));
    assert_eq!(
        Pkce::from_verifier(&pending.pkce.verifier).challenge,
        pending.pkce.challenge
    );

    let port = pending.port();
    // a stray request first (favicon), then the real callback
    let browser = thread::spawn(move || {
        browser_callback(port, "");
        browser_callback(port, "code=one-time-code&state=state-1234");
    });
    let result = pending.wait_and_redeem(&anon).unwrap();
    browser.join().unwrap();

    assert_eq!(result["api_key"], "rg_live_minted");
    start.assert();
    token.assert();
}

#[test]
fn deny_is_reported_and_nothing_is_redeemed() {
    let mut server = Server::new();
    server
        .mock("POST", "/api/v3/auth/cli/start/")
        .with_status(201)
        .with_body(format!(
            r#"{{"request_id":"req-2","authorize_url":"{}/a/","expires_in":600}}"#,
            server.url()
        ))
        .create();
    let token = server
        .mock("POST", "/api/v3/auth/cli/token/")
        .expect(0)
        .create();

    let anon = anon_client(&server.url());
    let pending = browser::start(&anon, &request("state-abcd")).unwrap();
    let port = pending.port();
    let browser =
        thread::spawn(move || browser_callback(port, "error=access_denied&state=state-abcd"));
    let err = pending.wait_and_redeem(&anon).unwrap_err();
    browser.join().unwrap();
    assert!(err.to_string().contains("denied"), "{err}");
    token.assert();
}

#[test]
fn a_wrong_state_or_a_stray_connection_does_not_end_the_login() {
    let mut server = Server::new();
    server
        .mock("POST", "/api/v3/auth/cli/start/")
        .with_status(201)
        .with_body(format!(
            r#"{{"request_id":"req-3","authorize_url":"{}/a/","expires_in":600}}"#,
            server.url()
        ))
        .create();
    let token = server
        .mock("POST", "/api/v3/auth/cli/token/")
        .match_body(Matcher::PartialJson(json!({ "code": "good-code" })))
        .with_status(201)
        .with_body(r#"{"api_key":"rg_live_minted"}"#)
        .create();
    let anon = anon_client(&server.url());
    let pending = browser::start(&anon, &request("state-good")).unwrap();
    let port = pending.port();
    let browser = thread::spawn(move || {
        // a local connection that never sends anything, then a stale tab
        let idle = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        thread::sleep(Duration::from_millis(100));
        drop(idle);
        let body = reqwest::blocking::get(format!(
            "http://127.0.0.1:{port}/callback?code=evil&state=state-evil"
        ))
        .unwrap()
        .text()
        .unwrap();
        assert!(body.contains("could not be matched"));
        browser_callback(port, "code=good-code&state=state-good");
    });
    let result = pending.wait_and_redeem(&anon).unwrap();
    browser.join().unwrap();
    assert_eq!(result["api_key"], "rg_live_minted");
    token.assert();
}

#[test]
fn an_approval_url_off_the_site_is_refused() {
    for url in [
        "file:///Applications/Calculator.app",
        "https://evil.example.net/approve",
        "javascript:alert(1)",
    ] {
        let mut server = Server::new();
        server
            .mock("POST", "/api/v3/auth/cli/start/")
            .with_status(201)
            .with_body(format!(
                r#"{{"request_id":"r","authorize_url":"{url}","expires_in":600}}"#
            ))
            .create();
        let anon = anon_client(&server.url());
        let Err(err) = browser::start(&anon, &request("state-1234")) else {
            panic!("{url} was accepted");
        };
        assert!(err.to_string().contains("not on"), "{err}");
    }
}

#[test]
fn start_validation_error_surfaces_fields() {
    let mut server = Server::new();
    server
        .mock("POST", "/api/v3/auth/cli/start/")
        .with_status(422)
        .with_body(r#"{"error":{"code":"validation_error","message":"bad","fields":{"scopes":["Unknown scope(s): nope"]}}}"#)
        .create();
    let anon = anon_client(&server.url());
    let err = browser::start(&anon, &request("state-1234")).unwrap_err();
    assert_eq!(err.fields().unwrap()["scopes"][0], "Unknown scope(s): nope");
}

#[test]
fn login_posts_again_after_a_canonical_host_redirect() {
    let mut server = Server::new();
    let redirect = server
        .mock("POST", "/api/v3/auth/cli/start/")
        .with_status(301)
        .with_header(
            "location",
            &format!("{}/api/v3/auth/cli/canonical-start/", server.url()),
        )
        .create();
    let start = server
        .mock("POST", "/api/v3/auth/cli/canonical-start/")
        .with_status(201)
        .with_body(format!(
            r#"{{"request_id":"req-redirect","authorize_url":"{}/a/","expires_in":600}}"#,
            server.url()
        ))
        .create();

    let anon = anon_client(&server.url());
    let pending = browser::start(&anon, &request("state-1234")).unwrap();

    assert_eq!(pending.request_id, "req-redirect");
    redirect.assert();
    start.assert();
}

#[test]
fn login_refuses_to_follow_a_redirect_to_another_host() {
    let mut server = Server::new();
    let elsewhere = server.url().replace("127.0.0.1", "localhost");
    server
        .mock("POST", "/api/v3/auth/cli/start/")
        .with_status(302)
        .with_header("location", &format!("{elsewhere}/api/v3/auth/cli/start/"))
        .expect(1)
        .create();

    let anon = anon_client(&server.url());
    let err = browser::start(&anon, &request("state-1234")).unwrap_err();

    assert!(err.to_string().contains("not the same site"), "{err}");
}
