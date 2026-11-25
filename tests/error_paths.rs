//! In-process contract tests for command error propagation and remaining
//! command edge branches. Running commands directly against `mockito::Server`
//! verifies that every resource command surfaces API errors (404/422/500) and
//! usage errors cleanly without spawning dozens of subprocesses.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use clap::Parser;
use geekcli::cli::{Cli, Command};
use geekcli::client::Client;
use geekcli::commands::{
    agent_pages, api, area_pages, auth, banners, blog_home, categories, design, featured,
    featured_areas, files, footers, home_page, market_reports, nav, pages, posts, search,
    search_forms, settings, sidebars, templates, valuation_pages, Context,
};
use geekcli::config::Target;
use geekcli::error::{Error, Result};
use geekcli::output::{Format, Printer};
use mockito::{Matcher, Server, ServerGuard};
use serde_json::json;

fn ctx_for(server: &ServerGuard, format: Format) -> Context {
    let target = Target {
        domain: "www.test.local".into(),
        base_url: server.url(),
        api_key: Some("rg_live_k".into()),
    };
    Context {
        client: Client::new(&target, 0, false).unwrap(),
        printer: Printer::new(Some(format), false),
        yes: true,
    }
}

fn dispatch(ctx: &Context, args: &[&str]) -> Result<()> {
    let cli = Cli::try_parse_from(std::iter::once("geekcli").chain(args.iter().copied())).unwrap();
    match cli.command {
        Command::Me => auth::status(ctx),
        Command::Posts(cmd) => posts::run(ctx, cmd),
        Command::Categories(cmd) => categories::run(ctx, cmd),
        Command::Blog(cmd) => blog_home::run(ctx, cmd),
        Command::Pages(cmd) => pages::run(ctx, cmd),
        Command::AreaPages(cmd) => area_pages::run(ctx, cmd),
        Command::AgentPages(cmd) => agent_pages::run(ctx, cmd),
        Command::Agents => agent_pages::agents(ctx),
        Command::MarketReports(cmd) => market_reports::run(ctx, cmd),
        Command::ValuationPages(cmd) => valuation_pages::run(ctx, cmd),
        Command::HomePage(cmd) => home_page::run(ctx, cmd),
        Command::Templates(cmd) => templates::run(ctx, &cmd),
        Command::Search(cmd) => search::run(ctx, cmd),
        Command::Nav(cmd) => nav::run(ctx, cmd),
        Command::Sidebars(cmd) => sidebars::run(ctx, cmd),
        Command::Footers(cmd) => footers::run(ctx, cmd),
        Command::Banners(cmd) => banners::run(ctx, cmd),
        Command::Featured(cmd) => featured::run(ctx, cmd),
        Command::FeaturedAreas(cmd) => featured_areas::run(ctx, cmd),
        Command::Settings(cmd) => settings::run(ctx, cmd),
        Command::SearchForms(cmd) => search_forms::run(ctx, cmd),
        Command::Design(cmd) => design::run(ctx, cmd),
        Command::Files(cmd) => files::run(ctx, cmd),
        Command::Api(args) => api::run(ctx, &args),
        other => panic!("unexpected command in dispatch: {other:?}"),
    }
}

fn assert_api_status(res: Result<()>, expected_status: u16, label: &str) {
    match res {
        Err(Error::Api { status, .. }) => {
            assert_eq!(status, expected_status, "{label}: wrong HTTP status");
        }
        other => panic!("{label}: expected Error::Api({expected_status}), got {other:?}"),
    }
}

/// Every resource command propagates HTTP 500 server errors without swallowing them.
#[test]
fn all_resource_commands_propagate_api_errors() {
    let mut server = Server::new();
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
        server
            .mock(method, Matcher::Any)
            .with_status(500)
            .with_body(r#"{"error":{"code":"server_error","message":"boom"}}"#)
            .create();
    }
    let ctx = ctx_for(&server, Format::Json);

    let cases: &[(&str, &[&str])] = &[
        // blog posts, categories, blog home
        ("posts list", &["posts", "list"]),
        ("posts get", &["posts", "get", "1"]),
        (
            "posts create",
            &[
                "posts",
                "create",
                "--title",
                "T",
                "--slug",
                "t",
                "--content",
                "<p>x</p>",
                "--status",
                "published",
                "--category",
                "news",
            ],
        ),
        ("posts update", &["posts", "update", "1", "--title", "New"]),
        ("posts publish", &["posts", "publish", "1"]),
        ("posts unpublish", &["posts", "unpublish", "1"]),
        ("posts delete", &["posts", "delete", "1"]),
        ("posts revisions", &["posts", "revisions", "1"]),
        ("posts revision", &["posts", "revision", "1", "10"]),
        ("posts revert", &["posts", "revert", "1", "10"]),
        ("categories list", &["categories", "list"]),
        ("categories get", &["categories", "get", "1"]),
        (
            "categories create",
            &["categories", "create", "--name", "News"],
        ),
        (
            "categories update",
            &["categories", "update", "1", "--name", "New"],
        ),
        ("categories delete", &["categories", "delete", "1"]),
        ("blog get", &["blog", "get"]),
        ("blog update", &["blog", "update", "--title", "Blog"]),
        // content, area, agent, market-report, valuation pages
        ("pages list", &["pages", "list"]),
        ("pages get", &["pages", "get", "1"]),
        (
            "pages create",
            &[
                "pages",
                "create",
                "--title",
                "P",
                "--slug",
                "p",
                "--anchor-text",
                "P",
            ],
        ),
        ("pages update", &["pages", "update", "1", "--title", "New"]),
        ("pages delete", &["pages", "delete", "1"]),
        ("pages revisions", &["pages", "revisions", "1"]),
        ("pages revision", &["pages", "revision", "1", "2"]),
        ("pages revert", &["pages", "revert", "1", "2"]),
        ("area-pages list", &["area-pages", "list"]),
        ("area-pages get", &["area-pages", "get", "1"]),
        (
            "area-pages create",
            &[
                "area-pages",
                "create",
                "--title",
                "A",
                "--slug",
                "a",
                "--anchor-text",
                "A",
                "--area-name",
                "A",
                "--no-search",
            ],
        ),
        (
            "area-pages update",
            &["area-pages", "update", "1", "--title", "B"],
        ),
        ("area-pages delete", &["area-pages", "delete", "1"]),
        ("area-pages revisions", &["area-pages", "revisions", "1"]),
        ("area-pages revision", &["area-pages", "revision", "1", "2"]),
        ("area-pages revert", &["area-pages", "revert", "1", "2"]),
        ("agents", &["agents"]),
        ("agent-pages list", &["agent-pages", "list"]),
        ("agent-pages get", &["agent-pages", "get", "1"]),
        (
            "agent-pages create",
            &[
                "agent-pages",
                "create",
                "--title",
                "Ag",
                "--slug",
                "ag",
                "--anchor-text",
                "Ag",
                "--agent-id",
                "1",
            ],
        ),
        (
            "agent-pages update",
            &["agent-pages", "update", "1", "--title", "B"],
        ),
        ("agent-pages delete", &["agent-pages", "delete", "1"]),
        ("agent-pages revisions", &["agent-pages", "revisions", "1"]),
        (
            "agent-pages revision",
            &["agent-pages", "revision", "1", "2"],
        ),
        ("agent-pages revert", &["agent-pages", "revert", "1", "2"]),
        ("market-reports list", &["market-reports", "list"]),
        ("market-reports get", &["market-reports", "get", "1"]),
        (
            "market-reports create",
            &[
                "market-reports",
                "create",
                "--title",
                "M",
                "--slug",
                "m",
                "--anchor-text",
                "M",
                "--search",
                "12",
            ],
        ),
        (
            "market-reports update",
            &["market-reports", "update", "1", "--title", "B"],
        ),
        ("market-reports delete", &["market-reports", "delete", "1"]),
        (
            "market-reports revisions",
            &["market-reports", "revisions", "1"],
        ),
        (
            "market-reports revision",
            &["market-reports", "revision", "1", "2"],
        ),
        (
            "market-reports revert",
            &["market-reports", "revert", "1", "2"],
        ),
        ("valuation-pages list", &["valuation-pages", "list"]),
        ("valuation-pages get", &["valuation-pages", "get", "1"]),
        (
            "valuation-pages create",
            &[
                "valuation-pages",
                "create",
                "--title",
                "V",
                "--slug",
                "v",
                "--anchor-text",
                "V",
            ],
        ),
        (
            "valuation-pages update",
            &["valuation-pages", "update", "1", "--title", "B"],
        ),
        (
            "valuation-pages delete",
            &["valuation-pages", "delete", "1"],
        ),
        (
            "valuation-pages revisions",
            &["valuation-pages", "revisions", "1"],
        ),
        (
            "valuation-pages revision",
            &["valuation-pages", "revision", "1", "2"],
        ),
        (
            "valuation-pages revert",
            &["valuation-pages", "revert", "1", "2"],
        ),
        // home page, templates, featured areas
        ("home-page get", &["home-page", "get"]),
        ("home-page update", &["home-page", "update", "--title", "H"]),
        ("home-page sections", &["home-page", "sections"]),
        (
            "home-page items get",
            &["home-page", "items", "get", "testimonial"],
        ),
        (
            "home-page items set",
            &["home-page", "items", "set", "testimonial", "--data", "[]"],
        ),
        ("home-page revisions", &["home-page", "revisions"]),
        ("home-page revision", &["home-page", "revision", "1"]),
        ("home-page revert", &["home-page", "revert", "1"]),
        ("templates list", &["templates", "list"]),
        ("featured-areas get", &["featured-areas", "get"]),
        (
            "featured-areas set",
            &["featured-areas", "set", "--data", "[]"],
        ),
        // nav, sidebars, footers, banners, featured
        ("nav list", &["nav", "list"]),
        ("nav get", &["nav", "get", "1"]),
        ("nav add", &["nav", "add", "1", "--contact"]),
        ("nav update", &["nav", "update", "1", "10", "--text", "X"]),
        ("nav remove", &["nav", "remove", "1", "10"]),
        ("nav move", &["nav", "move", "1", "10", "--to", "0"]),
        ("nav set", &["nav", "set", "1", "--data", "[]"]),
        ("nav clear", &["nav", "clear", "1"]),
        ("nav revisions", &["nav", "revisions", "1"]),
        ("nav revision", &["nav", "revision", "1", "2"]),
        ("nav revert", &["nav", "revert", "1", "2"]),
        ("sidebars list", &["sidebars", "list"]),
        ("sidebars get", &["sidebars", "get", "1"]),
        ("sidebars create", &["sidebars", "create", "--name", "S"]),
        (
            "sidebars rename",
            &["sidebars", "rename", "1", "--name", "S2"],
        ),
        ("sidebars delete", &["sidebars", "delete", "1"]),
        ("sidebars item", &["sidebars", "item", "1", "10"]),
        (
            "sidebars add-html",
            &["sidebars", "add-html", "1", "--content", "<p>x</p>"],
        ),
        (
            "sidebars add-links",
            &["sidebars", "add-links", "1", "--link", "A=/a/"],
        ),
        (
            "sidebars update-item",
            &["sidebars", "update-item", "1", "10", "--columns", "2"],
        ),
        (
            "sidebars remove-item",
            &["sidebars", "remove-item", "1", "10"],
        ),
        (
            "sidebars move-item",
            &["sidebars", "move-item", "1", "10", "--to", "0"],
        ),
        (
            "sidebars set-items",
            &["sidebars", "set-items", "1", "--data", "[]"],
        ),
        ("sidebars revisions", &["sidebars", "revisions", "1"]),
        ("sidebars revision", &["sidebars", "revision", "1", "2"]),
        ("sidebars revert", &["sidebars", "revert", "1", "2"]),
        ("footers list", &["footers", "list"]),
        ("footers get", &["footers", "get", "1"]),
        (
            "footers create",
            &["footers", "create", "--content", "<p>x</p>"],
        ),
        (
            "footers update",
            &["footers", "update", "1", "--content", "<p>new</p>"],
        ),
        ("footers delete", &["footers", "delete", "1"]),
        ("footers revisions", &["footers", "revisions", "1"]),
        ("footers revision", &["footers", "revision", "1", "2"]),
        ("footers revert", &["footers", "revert", "1", "2"]),
        ("banners list", &["banners", "list"]),
        ("banners get", &["banners", "get", "1"]),
        (
            "banners create",
            &[
                "banners",
                "create",
                "--name",
                "B",
                "--message",
                "Hi",
                "--cta",
                "Go",
                "--url",
                "/go/",
            ],
        ),
        (
            "banners update",
            &["banners", "update", "1", "--message", "Hi2"],
        ),
        ("banners delete", &["banners", "delete", "1"]),
        ("banners revisions", &["banners", "revisions", "1"]),
        ("banners revision", &["banners", "revision", "1", "2"]),
        ("banners revert", &["banners", "revert", "1", "2"]),
        ("featured list", &["featured", "list"]),
        ("featured get", &["featured", "get", "1"]),
        ("featured create", &["featured", "create", "--title", "G"]),
        (
            "featured update",
            &["featured", "update", "1", "--title", "G2"],
        ),
        ("featured delete", &["featured", "delete", "1"]),
        (
            "featured add-tile",
            &["featured", "add-tile", "1", "--title", "T", "--link", "/t/"],
        ),
        (
            "featured update-tile",
            &["featured", "update-tile", "1", "9", "--title", "T2"],
        ),
        (
            "featured remove-tile",
            &["featured", "remove-tile", "1", "9"],
        ),
        (
            "featured set-tiles",
            &["featured", "set-tiles", "1", "--data", "[]"],
        ),
        ("featured revisions", &["featured", "revisions", "1"]),
        ("featured revision", &["featured", "revision", "1", "2"]),
        ("featured revert", &["featured", "revert", "1", "2"]),
        // settings, search-forms, design, files, search, api, me
        ("settings list", &["settings", "list"]),
        ("settings get", &["settings", "get", "SITE_NAME"]),
        ("settings set", &["settings", "set", "SITE_NAME=X"]),
        ("settings clear", &["settings", "clear", "SITE_NAME"]),
        ("settings revisions", &["settings", "revisions"]),
        ("settings revision", &["settings", "revision", "1"]),
        ("settings revert", &["settings", "revert", "1"]),
        ("search-forms get", &["search-forms", "get"]),
        ("search-forms fields", &["search-forms", "fields"]),
        (
            "search-forms update",
            &["search-forms", "update", "--data", r#"{"pages":[]}"#],
        ),
        ("search-forms reset", &["search-forms", "reset"]),
        ("search-forms revisions", &["search-forms", "revisions"]),
        ("search-forms revision", &["search-forms", "revision", "1"]),
        ("search-forms revert", &["search-forms", "revert", "1"]),
        ("design get", &["design", "get"]),
        ("design templates", &["design", "templates"]),
        ("design variation", &["design", "variation", "molly", "red"]),
        ("design set", &["design", "set", "--template", "molly"]),
        (
            "design preview",
            &["design", "preview", "--template", "molly"],
        ),
        ("design revisions", &["design", "revisions"]),
        ("design revision", &["design", "revision", "1"]),
        ("design revert", &["design", "revert", "1"]),
        ("files list", &["files", "list"]),
        ("files get", &["files", "get", "a.png"]),
        ("files mkdir", &["files", "mkdir", "a/b"]),
        ("files move", &["files", "move", "a.png", "b.png"]),
        ("files delete", &["files", "delete", "a.png"]),
        ("files versions", &["files", "versions", "a.png"]),
        ("files deleted", &["files", "deleted"]),
        ("files restore", &["files", "restore", "a.png"]),
        ("search fields", &["search", "fields"]),
        ("search choices", &["search", "choices", "city"]),
        ("search check", &["search", "check", "city=Miami"]),
        ("search run", &["search", "run", "city=Miami"]),
        ("search url", &["search", "url", "city=Miami"]),
        ("area-pages search", &["area-pages", "search", "1"]),
        ("api get", &["api", "GET", "me/"]),
        ("me", &["me"]),
    ];

    for (label, args) in cases {
        assert_api_status(dispatch(&ctx, args), 500, label);
    }
}

/// Secondary request failures (after initial resolve/GET succeeds) propagate cleanly,
/// plus remaining flag/rendering edge cases across resource commands.
#[test]
fn secondary_step_errors_and_remaining_command_branches() {
    let mut server = Server::new();
    let json_ctx = ctx_for(&server, Format::Json);
    let table_ctx = ctx_for(&server, Format::Table);

    // 1. Sidebars: table view with special=false, empty used_by, small used_by (<=5),
    //    update-item preserving existing header.url, plus secondary write failures.
    server
        .mock("GET", "/api/v3/content/sidebars/5/")
        .with_body(r#"{"id":5,"name":"Custom","special":false,"used_by":[],"items":[]}"#)
        .create();
    dispatch(&table_ctx, &["sidebars", "get", "5"]).unwrap();

    server
        .mock("GET", "/api/v3/content/sidebars/6/")
        .with_body(r#"{"id":6,"name":"Small","special":false,"used_by":["/a/","/b/"],"used_by_count":2,"items":[]}"#)
        .create();
    dispatch(&table_ctx, &["sidebars", "get", "6"]).unwrap();

    server
        .mock("GET", "/api/v3/content/sidebars/5/items/50/")
        .with_body(r#"{"id":50,"type":"links","header":{"text":"Old","url":"/old/"},"links":[]}"#)
        .create();
    let keep_url = server
        .mock("PATCH", "/api/v3/content/sidebars/5/items/50/")
        .match_body(Matcher::Json(json!({
            "type": "links",
            "header": {"text": "New Header", "url": "/old/"}
        })))
        .with_body(r#"{"id":50}"#)
        .create();
    dispatch(
        &json_ctx,
        &[
            "sidebars",
            "update-item",
            "5",
            "50",
            "--header",
            "New Header",
        ],
    )
    .unwrap();
    keep_url.assert();

    // Secondary write failures after sidebar resolve succeeds
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        server
            .mock(
                method,
                Matcher::Regex("^/api/v3/content/sidebars/6/".into()),
            )
            .with_status(422)
            .with_body(r#"{"error":{"code":"validation_error","message":"bad"}}"#)
            .create();
    }
    assert_api_status(
        dispatch(&json_ctx, &["sidebars", "rename", "6", "--name", "X"]),
        422,
        "sidebars rename 422",
    );
    assert_api_status(
        dispatch(&json_ctx, &["sidebars", "delete", "6"]),
        422,
        "sidebars delete 422",
    );
    assert_api_status(
        dispatch(
            &json_ctx,
            &["sidebars", "add-html", "6", "--content", "<p>x</p>"],
        ),
        422,
        "sidebars add-html 422",
    );
    assert_api_status(
        dispatch(
            &json_ctx,
            &["sidebars", "add-links", "6", "--link", "A=/a/"],
        ),
        422,
        "sidebars add-links 422",
    );
    assert_api_status(
        dispatch(&json_ctx, &["sidebars", "remove-item", "6", "10"]),
        422,
        "sidebars remove-item 422",
    );
    assert_api_status(
        dispatch(
            &json_ctx,
            &["sidebars", "move-item", "6", "10", "--to", "0"],
        ),
        422,
        "sidebars move-item 422",
    );
    assert_api_status(
        dispatch(&json_ctx, &["sidebars", "set-items", "6", "--data", "[]"]),
        422,
        "sidebars set-items 422",
    );

    // 2. Nav: secondary write failures after bar resolve succeeds
    server
        .mock("GET", "/api/v3/content/navigation-bars/1/")
        .with_body(r#"{"id":1,"type":"top_primary","label":"Top","links":[{"id":10,"type":"custom","anchor_text":"Home","url":"/","nofollow":false,"order":0}]}"#)
        .create();
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        server
            .mock(
                method,
                Matcher::Regex("^/api/v3/content/navigation-bars/1/".into()),
            )
            .with_status(422)
            .with_body(r#"{"error":{"code":"validation_error","message":"bad"}}"#)
            .create();
    }
    assert_api_status(
        dispatch(&json_ctx, &["nav", "add", "1", "--contact"]),
        422,
        "nav add 422",
    );
    assert_api_status(
        dispatch(&json_ctx, &["nav", "update", "1", "10", "--text", "X"]),
        422,
        "nav update 422",
    );
    assert_api_status(
        dispatch(&json_ctx, &["nav", "remove", "1", "10"]),
        422,
        "nav remove 422",
    );
    assert_api_status(
        dispatch(&json_ctx, &["nav", "move", "1", "10", "--to", "0"]),
        422,
        "nav move 422",
    );
    assert_api_status(
        dispatch(&json_ctx, &["nav", "set", "1", "--data", "[]"]),
        422,
        "nav set 422",
    );
    assert_api_status(
        dispatch(&json_ctx, &["nav", "clear", "1"]),
        422,
        "nav clear 422",
    );

    // 3. Featured: add-tile and update-tile with non-empty --image + secondary write failures
    server
        .mock("GET", "/api/v3/content/featured-pages/3/")
        .with_body(r#"{"id":3,"title":"Areas","tiles":[{"id":9,"title":"A"}]}"#)
        .create();
    let add_tile_img = server
        .mock("POST", "/api/v3/content/featured-pages/3/tiles/")
        .match_body(Matcher::Json(json!({
            "title": "Beach",
            "link": "/beach/",
            "cta": "View Homes",
            "image": "https://u.realgeeks.media/x/beach.jpg"
        })))
        .with_status(201)
        .with_body(r#"{"id":3,"title":"Areas","tiles":[{"id":10,"title":"Beach"}]}"#)
        .create();
    dispatch(
        &json_ctx,
        &[
            "featured",
            "add-tile",
            "3",
            "--title",
            "Beach",
            "--link",
            "/beach/",
            "--image",
            "https://u.realgeeks.media/x/beach.jpg",
        ],
    )
    .unwrap();
    add_tile_img.assert();

    let upd_tile_img = server
        .mock("PATCH", "/api/v3/content/featured-pages/3/tiles/10/")
        .match_body(Matcher::Json(json!({
            "image": "https://u.realgeeks.media/x/new.jpg"
        })))
        .with_body(r#"{"id":10,"title":"Beach","image":"https://u.realgeeks.media/x/new.jpg"}"#)
        .create();
    dispatch(
        &json_ctx,
        &[
            "featured",
            "update-tile",
            "3",
            "10",
            "--image",
            "https://u.realgeeks.media/x/new.jpg",
        ],
    )
    .unwrap();
    upd_tile_img.assert();

    for method in ["PUT", "PATCH", "DELETE"] {
        server
            .mock(
                method,
                Matcher::Regex("^/api/v3/content/featured-pages/3/".into()),
            )
            .with_status(422)
            .with_body(r#"{"error":{"code":"validation_error","message":"bad"}}"#)
            .create();
    }
    assert_api_status(
        dispatch(&json_ctx, &["featured", "update", "3", "--title", "New"]),
        422,
        "featured update 422",
    );
    assert_api_status(
        dispatch(&json_ctx, &["featured", "delete", "3"]),
        422,
        "featured delete 422",
    );
    assert_api_status(
        dispatch(
            &json_ctx,
            &["featured", "update-tile", "3", "9", "--title", "B"],
        ),
        422,
        "featured update-tile 422",
    );
    assert_api_status(
        dispatch(&json_ctx, &["featured", "remove-tile", "3", "9"]),
        422,
        "featured remove-tile 422",
    );
    assert_api_status(
        dispatch(&json_ctx, &["featured", "set-tiles", "3", "--data", "[]"]),
        422,
        "featured set-tiles 422",
    );

    // 4. Banners & footers secondary write failures after resolve succeeds
    server
        .mock("GET", "/api/v3/content/banners/4/")
        .with_body(r#"{"id":4,"name":"Sale","message":"Now"}"#)
        .create();
    server
        .mock("GET", "/api/v3/content/footers/2/")
        .with_body(r#"{"id":2,"name":"Main","content":"<p>f</p>"}"#)
        .create();
    for method in ["PATCH", "DELETE"] {
        server
            .mock(method, "/api/v3/content/banners/4/")
            .with_status(422)
            .with_body(r#"{"error":{"code":"validation_error","message":"bad"}}"#)
            .create();
        server
            .mock(method, "/api/v3/content/footers/2/")
            .with_status(422)
            .with_body(r#"{"error":{"code":"validation_error","message":"bad"}}"#)
            .create();
    }
    assert_api_status(
        dispatch(&json_ctx, &["banners", "update", "4", "--message", "x"]),
        422,
        "banners update 422",
    );
    assert_api_status(
        dispatch(&json_ctx, &["banners", "delete", "4"]),
        422,
        "banners delete 422",
    );
    assert_api_status(
        dispatch(
            &json_ctx,
            &["footers", "update", "2", "--content", "<p>x</p>"],
        ),
        422,
        "footers update 422",
    );
    assert_api_status(
        dispatch(&json_ctx, &["footers", "delete", "2"]),
        422,
        "footers delete 422",
    );

    // 5. Posts: explicit `--status published`, `--remove-category` non-matching entry,
    //    and secondary PATCH/DELETE failures
    let pub_post = server
        .mock("POST", "/api/v3/blog/posts/")
        .match_body(Matcher::Json(json!({
            "title": "Live",
            "slug": "live",
            "body": "<p>ok</p>",
            "status": "published"
        })))
        .with_status(201)
        .with_body(r#"{"id":11,"slug":"live","status":"published"}"#)
        .create();
    dispatch(
        &json_ctx,
        &[
            "posts",
            "create",
            "--title",
            "Live",
            "--slug",
            "live",
            "--content",
            "<p>ok</p>",
            "--status",
            "published",
        ],
    )
    .unwrap();
    pub_post.assert();

    server
        .mock("GET", "/api/v3/blog/posts/11/")
        .with_body(r#"{"id":11,"slug":"live","categories":[{"id":1,"slug":"news"}]}"#)
        .create();
    let keep_other = server
        .mock("PATCH", "/api/v3/blog/posts/11/")
        .match_body(Matcher::Json(json!({ "categories": [1] })))
        .with_body(r#"{"id":11,"categories":[{"id":1,"slug":"news"}]}"#)
        .create();
    dispatch(
        &json_ctx,
        &["posts", "update", "11", "--remove-category", "other"],
    )
    .unwrap();
    keep_other.assert();

    // 6. Pages: area page parent lookup falling back to content pages (`other = PATH`),
    //    plus numeric --footer id attachment
    server
        .mock("GET", "/api/v3/content/area-pages/")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("path".into(), "/communities/".into()),
            Matcher::UrlEncoded("page_size".into(), "2".into()),
        ]))
        .with_body(r#"{"results":[]}"#)
        .create();
    server
        .mock("GET", "/api/v3/content/pages/")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("path".into(), "/communities/".into()),
            Matcher::UrlEncoded("page_size".into(), "2".into()),
        ]))
        .with_body(r#"{"results":[{"id":77,"path":"/communities/"}]}"#)
        .create();
    let area_create = server
        .mock("POST", "/api/v3/content/area-pages/")
        .match_body(Matcher::Json(json!({
            "title": "East",
            "slug": "east",
            "anchor_text": "East",
            "area_name": "East",
            "parent": 77,
            "footer": 2
        })))
        .with_status(201)
        .with_body(r#"{"id":88,"path":"/communities/east/"}"#)
        .create();
    dispatch(
        &json_ctx,
        &[
            "area-pages",
            "create",
            "--title",
            "East",
            "--slug",
            "east",
            "--anchor-text",
            "East",
            "--area-name",
            "East",
            "--parent",
            "/communities/",
            "--footer",
            "2",
            "--no-search",
        ],
    )
    .unwrap();
    area_create.assert();

    // 7. Home page items get in table mode
    server
        .mock("GET", "/api/v3/content/home-page/sections/testimonial/")
        .with_body(r#"{"kind":"testimonial","columns":["title","text"],"items":[{"id":1,"order":0,"title":"Great","text":"Loved it"}]}"#)
        .create();
    dispatch(&table_ctx, &["home-page", "items", "get", "testimonial"]).unwrap();

    // 8. Search fields: catalog returns non-list JSON -> triggers catalog_fields Err -> falls back to form
    server
        .mock("GET", "/search_forms/api/dump_uberform_fields.json")
        .with_body(r#"{"not_a_list": true}"#)
        .create();
    server
        .mock("GET", "/search_forms/api/advanced_search_form.json")
        .with_body(r#"{"primary":[{"attr":"city","label":"City","widget_type":"select"}]}"#)
        .create();
    dispatch(&json_ctx, &["search", "fields"]).unwrap();

    // 9. Files upload: invalid --content-type fails cleanly with Usage error
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("ok.png");
    std::fs::write(&png, b"\x89PNG").unwrap();
    let bad_mime = dispatch(
        &json_ctx,
        &[
            "files",
            "upload",
            png.to_str().unwrap(),
            "--content-type",
            "bad\nmime",
        ],
    );
    assert!(
        matches!(bad_mime, Err(Error::Usage(ref m)) if m.contains("bad content type")),
        "{bad_mime:?}"
    );
}

/// Malformed server responses (missing `id`), invalid inputs, named-lookup
/// failures, and table-mode deletes across resource commands.
#[test]
fn malformed_responses_invalid_inputs_and_table_deletes() {
    // --- Part A: Responses missing `id` fail cleanly with Error::Other("response has no id")
    let mut no_id_server = Server::new();
    no_id_server
        .mock("GET", Matcher::Any)
        .with_body("{}")
        .create();
    let no_id_ctx = ctx_for(&no_id_server, Format::Json);

    let missing_id_cases: &[(&str, &[&str])] = &[
        (
            "sidebars rename",
            &["sidebars", "rename", "1", "--name", "X"],
        ),
        ("sidebars delete", &["sidebars", "delete", "1"]),
        ("sidebars item", &["sidebars", "item", "1", "10"]),
        (
            "sidebars update-item",
            &["sidebars", "update-item", "1", "10", "--columns", "2"],
        ),
        (
            "sidebars remove-item",
            &["sidebars", "remove-item", "1", "10"],
        ),
        (
            "sidebars move-item",
            &["sidebars", "move-item", "1", "10", "--to", "0"],
        ),
        (
            "sidebars set-items",
            &["sidebars", "set-items", "1", "--data", "[]"],
        ),
        ("sidebars revisions", &["sidebars", "revisions", "1"]),
        ("sidebars revision", &["sidebars", "revision", "1", "2"]),
        ("sidebars revert", &["sidebars", "revert", "1", "2"]),
        ("nav add", &["nav", "add", "1", "--contact"]),
        ("nav set", &["nav", "set", "1", "--data", "[]"]),
        ("nav clear", &["nav", "clear", "1"]),
        ("nav revisions", &["nav", "revisions", "1"]),
        ("nav revision", &["nav", "revision", "1", "2"]),
        ("nav revert", &["nav", "revert", "1", "2"]),
        (
            "featured update",
            &["featured", "update", "1", "--title", "X"],
        ),
        ("featured delete", &["featured", "delete", "1"]),
        (
            "featured add-tile",
            &["featured", "add-tile", "1", "--title", "T", "--link", "/t/"],
        ),
        (
            "featured update-tile",
            &["featured", "update-tile", "1", "9", "--title", "T"],
        ),
        (
            "featured remove-tile",
            &["featured", "remove-tile", "1", "9"],
        ),
        (
            "featured set-tiles",
            &["featured", "set-tiles", "1", "--data", "[]"],
        ),
        ("featured revisions", &["featured", "revisions", "1"]),
        ("featured revision", &["featured", "revision", "1", "2"]),
        ("featured revert", &["featured", "revert", "1", "2"]),
        (
            "banners update",
            &["banners", "update", "1", "--message", "X"],
        ),
        ("banners delete", &["banners", "delete", "1"]),
        ("banners revisions", &["banners", "revisions", "1"]),
        ("banners revision", &["banners", "revision", "1", "2"]),
        ("banners revert", &["banners", "revert", "1", "2"]),
        (
            "footers update",
            &["footers", "update", "1", "--content", "<p>x</p>"],
        ),
        ("footers delete", &["footers", "delete", "1"]),
        ("footers revisions", &["footers", "revisions", "1"]),
        ("footers revision", &["footers", "revision", "1", "2"]),
        ("footers revert", &["footers", "revert", "1", "2"]),
        ("posts publish", &["posts", "publish", "1"]),
        ("posts unpublish", &["posts", "unpublish", "1"]),
        ("posts update", &["posts", "update", "1", "--title", "X"]),
        ("posts delete", &["posts", "delete", "1"]),
        ("posts revisions", &["posts", "revisions", "1"]),
        ("posts revision", &["posts", "revision", "1", "2"]),
        ("posts revert", &["posts", "revert", "1", "2"]),
        ("pages delete", &["pages", "delete", "1"]),
        ("pages revert", &["pages", "revert", "1", "2"]),
        ("area-pages delete", &["area-pages", "delete", "1"]),
        ("market-reports delete", &["market-reports", "delete", "1"]),
        (
            "valuation-pages delete",
            &["valuation-pages", "delete", "1"],
        ),
        ("agent-pages delete", &["agent-pages", "delete", "1"]),
    ];
    for (label, args) in missing_id_cases {
        let res = dispatch(&no_id_ctx, args);
        assert!(
            matches!(res, Err(Error::Other(ref m)) if m.contains("no id")),
            "{label}: expected missing-id error, got {res:?}"
        );
    }

    // --- Part B: Named-ref list lookup failures & usage errors before any write
    let mut err_server = Server::new();
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
        err_server
            .mock(method, Matcher::Any)
            .with_status(500)
            .with_body(r#"{"error":{"code":"server_error","message":"boom"}}"#)
            .create();
    }
    let err_ctx = ctx_for(&err_server, Format::Json);

    let named_fail_cases: &[(&str, &[&str])] = &[
        ("nav get by name", &["nav", "get", "top_primary"]),
        ("featured get by name", &["featured", "get", "Areas"]),
        ("banners get by name", &["banners", "get", "Sale"]),
        ("footers get by name", &["footers", "get", "Default"]),
        ("sidebars get by name", &["sidebars", "get", "Main"]),
        (
            "pages update by slug",
            &["pages", "update", "about", "--title", "X"],
        ),
        ("pages revisions by slug", &["pages", "revisions", "about"]),
        (
            "pages revision by slug",
            &["pages", "revision", "about", "1"],
        ),
        (
            "pages create sidebar name",
            &[
                "pages",
                "create",
                "--title",
                "T",
                "--slug",
                "s",
                "--anchor-text",
                "A",
                "--sidebar",
                "Main",
            ],
        ),
        (
            "pages create footer name",
            &[
                "pages",
                "create",
                "--title",
                "T",
                "--slug",
                "s",
                "--anchor-text",
                "A",
                "--footer",
                "Main",
            ],
        ),
        (
            "pages create banner name",
            &[
                "pages",
                "create",
                "--title",
                "T",
                "--slug",
                "s",
                "--anchor-text",
                "A",
                "--banner",
                "Main",
            ],
        ),
        (
            "pages create parent path",
            &[
                "pages",
                "create",
                "--title",
                "T",
                "--slug",
                "s",
                "--anchor-text",
                "A",
                "--parent",
                "/p/",
            ],
        ),
        (
            "market-reports create footer name",
            &[
                "market-reports",
                "create",
                "--title",
                "M",
                "--slug",
                "m",
                "--anchor-text",
                "M",
                "--search",
                "1",
                "--footer",
                "Main",
            ],
        ),
        (
            "market-reports update slug",
            &["market-reports", "update", "m", "--title", "X"],
        ),
        (
            "market-reports revisions slug",
            &["market-reports", "revisions", "m"],
        ),
        (
            "market-reports revision slug",
            &["market-reports", "revision", "m", "1"],
        ),
        (
            "market-reports revert slug",
            &["market-reports", "revert", "m", "1"],
        ),
        (
            "valuation-pages update slug",
            &["valuation-pages", "update", "v", "--title", "X"],
        ),
        (
            "valuation-pages revisions slug",
            &["valuation-pages", "revisions", "v"],
        ),
        (
            "valuation-pages revision slug",
            &["valuation-pages", "revision", "v", "1"],
        ),
        (
            "valuation-pages revert slug",
            &["valuation-pages", "revert", "v", "1"],
        ),
        (
            "agent-pages update slug",
            &["agent-pages", "update", "a", "--title", "X"],
        ),
        (
            "agent-pages revisions slug",
            &["agent-pages", "revisions", "a"],
        ),
        (
            "agent-pages revision slug",
            &["agent-pages", "revision", "a", "1"],
        ),
        (
            "agent-pages revert slug",
            &["agent-pages", "revert", "a", "1"],
        ),
        (
            "area-pages update slug",
            &["area-pages", "update", "a", "--title", "X"],
        ),
        (
            "area-pages revisions slug",
            &["area-pages", "revisions", "a"],
        ),
        (
            "area-pages revision slug",
            &["area-pages", "revision", "a", "1"],
        ),
        (
            "area-pages revert slug",
            &["area-pages", "revert", "a", "1"],
        ),
        (
            "home-page update sidebar",
            &["home-page", "update", "--sidebar", "Main"],
        ),
        (
            "home-page update tile-group",
            &["home-page", "update", "--tile-group", "Areas"],
        ),
        (
            "home-page update featured-agents",
            &["home-page", "update", "--featured-agents", "alice"],
        ),
        (
            "home-page update section-effect",
            &["home-page", "update", "--section-effect", "hero=fade"],
        ),
        (
            "posts create categories",
            &[
                "posts",
                "create",
                "--title",
                "T",
                "--slug",
                "t",
                "--content",
                "<p>x</p>",
                "--category",
                "news",
                "--create-categories",
            ],
        ),
        (
            "files upload from-url",
            &["files", "upload", "--from-url", "https://example.com/a.png"],
        ),
        (
            "search choices all",
            &["search", "choices", "city", "--all"],
        ),
    ];
    for (label, args) in named_fail_cases {
        assert_api_status(dispatch(&err_ctx, args), 500, label);
    }

    let usage_err_cases: &[(&str, &[&str])] = &[
        (
            "sidebars create bad data",
            &["sidebars", "create", "--name", "S", "--data", "{bad"],
        ),
        (
            "featured create bad data",
            &["featured", "create", "--title", "G", "--data", "{bad"],
        ),
        (
            "banners create bad data",
            &["banners", "create", "--name", "B", "--data", "{bad"],
        ),
        ("footers create empty", &["footers", "create"]),
        (
            "pages create missing slug",
            &["pages", "create", "--title", "P", "--anchor-text", "P"],
        ),
        (
            "pages create bad defaults criteria",
            &[
                "pages",
                "create",
                "--title",
                "P",
                "--slug",
                "p",
                "--anchor-text",
                "P",
                "--search-field-defaults-criteria",
                "badpair",
            ],
        ),
        (
            "pages create bad data",
            &[
                "pages",
                "create",
                "--title",
                "P",
                "--slug",
                "p",
                "--anchor-text",
                "P",
                "--data",
                "{bad",
            ],
        ),
        (
            "market-reports create missing slug",
            &[
                "market-reports",
                "create",
                "--title",
                "M",
                "--anchor-text",
                "M",
                "--search",
                "1",
            ],
        ),
        (
            "market-reports create bad defaults",
            &[
                "market-reports",
                "create",
                "--title",
                "M",
                "--slug",
                "m",
                "--anchor-text",
                "M",
                "--search",
                "1",
                "--search-field-defaults-criteria",
                "bad",
            ],
        ),
        (
            "market-reports create bad data",
            &[
                "market-reports",
                "create",
                "--title",
                "M",
                "--slug",
                "m",
                "--anchor-text",
                "M",
                "--search",
                "1",
                "--data",
                "{bad",
            ],
        ),
        (
            "valuation-pages create missing slug",
            &[
                "valuation-pages",
                "create",
                "--title",
                "V",
                "--anchor-text",
                "V",
            ],
        ),
        (
            "valuation-pages create bad data",
            &[
                "valuation-pages",
                "create",
                "--title",
                "V",
                "--slug",
                "v",
                "--anchor-text",
                "V",
                "--data",
                "{bad",
            ],
        ),
        (
            "agent-pages create missing slug",
            &[
                "agent-pages",
                "create",
                "--title",
                "A",
                "--anchor-text",
                "A",
                "--agent-id",
                "1",
            ],
        ),
        (
            "agent-pages create bad data",
            &[
                "agent-pages",
                "create",
                "--title",
                "A",
                "--slug",
                "a",
                "--anchor-text",
                "A",
                "--agent-id",
                "1",
                "--data",
                "{bad",
            ],
        ),
        (
            "area-pages create missing slug",
            &[
                "area-pages",
                "create",
                "--title",
                "A",
                "--anchor-text",
                "A",
                "--area-name",
                "A",
                "--no-search",
            ],
        ),
        (
            "home-page update bad data",
            &["home-page", "update", "--data", "{bad"],
        ),
        (
            "home-page items set bad kind",
            &["home-page", "items", "set", "bogus", "--data", "[]"],
        ),
        (
            "posts create bad data",
            &[
                "posts",
                "create",
                "--title",
                "T",
                "--slug",
                "t",
                "--content",
                "<p>x</p>",
                "--data",
                "{bad",
            ],
        ),
        (
            "design preview bad var",
            &["design", "preview", "--var", "nope"],
        ),
        ("files get bad path", &["files", "get", "../etc/passwd"]),
        ("files mkdir root", &["files", "mkdir", "/"]),
        (
            "files delete bad path",
            &["files", "delete", "../etc/passwd"],
        ),
    ];
    for (label, args) in usage_err_cases {
        let res = dispatch(&err_ctx, args);
        assert!(
            matches!(res, Err(Error::Usage(_))),
            "{label}: expected Error::Usage, got {res:?}"
        );
    }

    // Missing local files fail with Io error
    for (label, args) in [
        (
            "pages create missing file",
            &[
                "pages",
                "create",
                "--title",
                "P",
                "--slug",
                "p",
                "--anchor-text",
                "P",
                "--content-file",
                "/nonexistent/geekcli-missing.html",
            ][..],
        ),
        (
            "home-page update missing file",
            &[
                "home-page",
                "update",
                "--content-file",
                "/nonexistent/geekcli-missing.html",
            ][..],
        ),
        (
            "home-page items set missing file",
            &[
                "home-page",
                "items",
                "set",
                "testimonial",
                "--data",
                "@/nonexistent/geekcli-missing.json",
            ][..],
        ),
        (
            "files upload missing file",
            &["files", "upload", "/nonexistent/geekcli-missing.png"][..],
        ),
    ] {
        let res = dispatch(&err_ctx, args);
        assert!(
            matches!(res, Err(Error::Io(_) | Error::Usage(_))),
            "{label}: expected Io/Usage error, got {res:?}"
        );
    }

    // --- Part C: Table-mode deletes, secondary failures after resolve, and edge branches
    let mut server = Server::new();
    let json_ctx = ctx_for(&server, Format::Json);
    let table_ctx = ctx_for(&server, Format::Table);

    // Table-mode deletes across sidebars, featured, banners, footers, posts, pages, files
    server
        .mock("GET", "/api/v3/content/sidebars/1/")
        .with_body(r#"{"id":1,"name":"S","items":[{"id":10,"type":"html","html":"<p>x</p>"}]}"#)
        .create();
    server
        .mock("DELETE", "/api/v3/content/sidebars/1/")
        .with_status(204)
        .create();
    server
        .mock("DELETE", "/api/v3/content/sidebars/1/items/10/")
        .with_status(204)
        .create();
    dispatch(&table_ctx, &["sidebars", "delete", "1"]).unwrap();
    dispatch(&table_ctx, &["sidebars", "remove-item", "1", "10"]).unwrap();

    // Sidebars: bad inputs after resolve succeeds
    assert!(matches!(
        dispatch(
            &json_ctx,
            &[
                "sidebars",
                "add-html",
                "1",
                "--content-file",
                "/nonexistent/missing.html"
            ]
        ),
        Err(Error::Io(_))
    ));
    assert!(matches!(
        dispatch(&json_ctx, &["sidebars", "add-links", "1", "--link", "=/x"]),
        Err(Error::Usage(_))
    ));
    assert!(matches!(
        dispatch(
            &json_ctx,
            &[
                "sidebars",
                "update-item",
                "1",
                "10",
                "--content-file",
                "/nonexistent/missing.html"
            ]
        ),
        Err(Error::Io(_))
    ));
    assert!(matches!(
        dispatch(
            &json_ctx,
            &["sidebars", "update-item", "1", "10", "--link", "=/x"]
        ),
        Err(Error::Usage(_))
    ));
    server
        .mock("GET", "/api/v3/content/sidebars/1/items/99/")
        .with_status(500)
        .with_body("{}")
        .create();
    assert_api_status(
        dispatch(
            &json_ctx,
            &["sidebars", "update-item", "1", "99", "--header", "Hi"],
        ),
        500,
        "sidebars update-item GET 500",
    );

    // Featured, banners, footers, posts, pages, files table deletes + bad inputs after resolve
    server
        .mock("GET", "/api/v3/content/featured-pages/1/")
        .with_body(r#"{"id":1,"title":"F","tiles":[{"id":9}]}"#)
        .create();
    server
        .mock("DELETE", "/api/v3/content/featured-pages/1/")
        .with_status(204)
        .create();
    server
        .mock("DELETE", "/api/v3/content/featured-pages/1/tiles/9/")
        .with_status(204)
        .create();
    dispatch(&table_ctx, &["featured", "delete", "1"]).unwrap();
    dispatch(&table_ctx, &["featured", "remove-tile", "1", "9"]).unwrap();

    server
        .mock("GET", "/api/v3/content/banners/1/")
        .with_body(r#"{"id":1,"name":"B"}"#)
        .create();
    server
        .mock("DELETE", "/api/v3/content/banners/1/")
        .with_status(204)
        .create();
    dispatch(&table_ctx, &["banners", "delete", "1"]).unwrap();
    assert!(matches!(
        dispatch(&json_ctx, &["banners", "update", "1", "--data", "{bad"]),
        Err(Error::Usage(_))
    ));

    server
        .mock("GET", "/api/v3/content/footers/1/")
        .with_body(r#"{"id":1,"name":"F"}"#)
        .create();
    server
        .mock("DELETE", "/api/v3/content/footers/1/")
        .with_status(204)
        .create();
    dispatch(&table_ctx, &["footers", "delete", "1"]).unwrap();
    assert!(matches!(
        dispatch(&json_ctx, &["footers", "update", "1"]),
        Err(Error::Usage(_))
    ));

    // Nav: resolve_link 404 on update/remove/move + secondary GET fail on move
    server
        .mock("GET", "/api/v3/content/navigation-bars/2/")
        .with_body(r#"{"id":2,"type":"top_primary","label":"Top","links":[{"id":20,"type":"custom","anchor_text":"A","url":"/a/","nofollow":false,"order":0}]}"#)
        .create();
    assert_api_status(
        dispatch(&json_ctx, &["nav", "remove", "2", "999"]),
        404,
        "nav remove unknown link",
    );
    assert_api_status(
        dispatch(&json_ctx, &["nav", "move", "2", "999", "--to", "0"]),
        404,
        "nav move unknown link",
    );
    assert_api_status(
        dispatch(&json_ctx, &["nav", "update", "2", "999", "--text", "X"]),
        404,
        "nav update unknown link",
    );

    // Posts: table delete, --replace PUT 500, publish PATCH 500, --remove-category malformed items,
    // --update bad data, --add-category lookup fail
    server
        .mock("GET", "/api/v3/blog/posts/2/")
        .with_body(r#"{"id":2,"slug":"p","categories":[{},{"id":1},{"id":2,"slug":123},true,{"id":3,"slug":"keep"}]}"#)
        .create();
    server
        .mock("DELETE", "/api/v3/blog/posts/2/")
        .with_status(204)
        .create();
    server
        .mock("PUT", "/api/v3/blog/posts/2/")
        .with_status(500)
        .with_body("{}")
        .create();
    server
        .mock("PATCH", "/api/v3/blog/posts/2/")
        .with_status(500)
        .with_body("{}")
        .create();
    dispatch(&table_ctx, &["posts", "delete", "2"]).unwrap();
    assert_api_status(
        dispatch(&json_ctx, &["posts", "publish", "2"]),
        500,
        "posts publish PATCH 500",
    );
    assert_api_status(
        dispatch(
            &json_ctx,
            &["posts", "update", "2", "--title", "X", "--replace"],
        ),
        500,
        "posts update PUT 500",
    );
    assert_api_status(
        dispatch(
            &json_ctx,
            &["posts", "update", "2", "--remove-category", "other"],
        ),
        500,
        "posts update PATCH 500",
    );
    assert!(matches!(
        dispatch(&json_ctx, &["posts", "update", "2", "--data", "{bad"]),
        Err(Error::Usage(_))
    ));
    server
        .mock("GET", "/api/v3/blog/categories/")
        .match_query(Matcher::Any)
        .with_status(500)
        .with_body("{}")
        .create();
    assert_api_status(
        dispatch(
            &json_ctx,
            &[
                "posts",
                "update",
                "2",
                "--add-category",
                "news",
                "--create-categories",
            ],
        ),
        500,
        "posts add-category fail",
    );

    // Pages: table delete, --replace PUT 500, area-pages search saved fail
    server
        .mock("GET", "/api/v3/content/pages/2/")
        .with_body(r#"{"id":2,"path":"/p/"}"#)
        .create();
    server
        .mock("DELETE", "/api/v3/content/pages/2/")
        .with_status(204)
        .create();
    server
        .mock("PUT", "/api/v3/content/pages/2/")
        .with_status(500)
        .with_body("{}")
        .create();
    dispatch(&table_ctx, &["pages", "delete", "2"]).unwrap();
    assert_api_status(
        dispatch(
            &json_ctx,
            &["pages", "update", "2", "--title", "X", "--replace"],
        ),
        500,
        "pages update PUT 500",
    );
    server
        .mock("GET", "/api/v3/content/area-pages/2/")
        .with_body(r#"{"id":2,"path":"/a/","search":{"id":55}}"#)
        .create();
    server
        .mock("GET", Matcher::Regex("^/api/v2/search/".into()))
        .match_query(Matcher::Any)
        .with_status(500)
        .with_body("{}")
        .create();
    assert_api_status(
        dispatch(&json_ctx, &["area-pages", "search", "2"]),
        500,
        "area-pages search saved fail",
    );

    // Market reports: create with --search-field-defaults & --footer numeric id
    let mr_create = server
        .mock("POST", "/api/v3/content/market-report-pages/")
        .match_body(Matcher::Json(json!({
            "title": "MR",
            "slug": "mr",
            "anchor_text": "MR",
            "search": 12,
            "search_field_defaults": 34,
            "footer": 2
        })))
        .with_status(201)
        .with_body(r#"{"id":9,"path":"/mr/"}"#)
        .create();
    dispatch(
        &json_ctx,
        &[
            "market-reports",
            "create",
            "--title",
            "MR",
            "--slug",
            "mr",
            "--anchor-text",
            "MR",
            "--search",
            "12",
            "--search-field-defaults",
            "34",
            "--footer",
            "2",
        ],
    )
    .unwrap();
    mr_create.assert();

    // Design get -o table with empty vars
    server
        .mock("GET", "/api/v3/design/")
        .with_body(r#"{"template":"molly","styles":{"name":"classic","vars":{}}}"#)
        .create();
    dispatch(&table_ctx, &["design", "get"]).unwrap();

    // Home page items set when secondary GET sections/ fails
    server
        .mock("PUT", "/api/v3/content/home-page/sections/testimonial/")
        .with_body(r#"{"kind":"testimonial","items":[]}"#)
        .create();
    server
        .mock("GET", "/api/v3/content/home-page/sections/")
        .with_status(500)
        .with_body("{}")
        .create();
    dispatch(
        &table_ctx,
        &["home-page", "items", "set", "testimonial", "--data", "[]"],
    )
    .unwrap();

    // Files: table delete, list/deleted -o table with no next_cursor
    server
        .mock("DELETE", "/api/v3/files/a.png/")
        .with_status(204)
        .create();
    server
        .mock("GET", "/api/v3/files/")
        .match_query(Matcher::Any)
        .with_body(r#"{"path":"","results":[],"next_cursor":null}"#)
        .create();
    server
        .mock("GET", "/api/v3/files/deleted/")
        .match_query(Matcher::Any)
        .with_body(r#"{"path":"","results":[],"next_cursor":null}"#)
        .create();
    dispatch(&table_ctx, &["files", "delete", "a.png"]).unwrap();
    dispatch(&table_ctx, &["files", "list"]).unwrap();
    dispatch(&table_ctx, &["files", "deleted"]).unwrap();

    // Search: check --count with count > 0, check --count failing, run without x-total-count-limited
    let mut s_server = Server::new();
    let s_ctx = ctx_for(&s_server, Format::Json);
    let s_table = ctx_for(&s_server, Format::Table);
    s_server
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::Any)
        .with_body(r#"{"description":"City=Miami","criteria":{"city":["Miami"]}}"#)
        .create();
    s_server
        .mock("GET", "/search_forms/api/dump_uberform_fields.json")
        .with_body(r#"[{"id":"city","label":"City","widget":"select","choices":[]}]"#)
        .create();
    s_server
        .mock("GET", "/search_forms/api/autocomplete_index.json")
        .with_body(r#"{"other":["X"]}"#)
        .create();
    s_server
        .mock("GET", "/api/v2/search/")
        .match_query(Matcher::Any)
        .with_header("x-total-count", "4")
        .with_body(r#"[{"mls_id":"1","address":"1 Main St","price":500000}]"#)
        .create();
    dispatch(&s_ctx, &["search", "check", "city=Miami", "--count"]).unwrap();
    dispatch(&s_table, &["search", "run", "city=Miami"]).unwrap();

    let mut s_err = Server::new();
    let s_err_ctx = ctx_for(&s_err, Format::Json);
    s_err
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::Any)
        .with_body(r#"{"description":"City=Miami","criteria":{"city":["Miami"]}}"#)
        .create();
    s_err
        .mock("GET", "/search_forms/api/dump_uberform_fields.json")
        .with_status(404)
        .with_body("{}")
        .create();
    s_err
        .mock("GET", "/search_forms/api/advanced_search_form.json")
        .with_status(404)
        .with_body("{}")
        .create();
    s_err
        .mock("GET", "/api/v2/search/")
        .match_query(Matcher::Any)
        .with_status(500)
        .with_body("{}")
        .create();
    assert_api_status(
        dispatch(&s_err_ctx, &["search", "check", "city=Miami", "--count"]),
        500,
        "search check --count 500",
    );
}
