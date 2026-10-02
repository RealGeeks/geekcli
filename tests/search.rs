//! `geekcli search …` against a mock site: field listing, criteria
//! checking with ignored keys, running a search, and page saved searches.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use mockito::{Matcher, Server, ServerGuard};
use predicates::prelude::*;
use serde_json::Value;

const FORM: &str = r#"{"primary":[{"attr":"city","label":"City","widget_type":"multiselect","default_value":"Miami","choices":[["all","All"],["Miami","Miami"],["Key Biscayne","Key Biscayne"]],"dependent_fields":["subdivision"]}],
"secondary":[{"attr":"type","label":"Home","widget_type":"checkbox","default_value":true,"value":"res"},{"attr":"type","label":"Condo","widget_type":"checkbox","default_value":true,"value":"con"},{"attr":"list_price_min","label":"Minimum Price","widget_type":"select","default_value":"all","choices":[["all","No Limit"],[1000000,"$1,000,000"]]}]}"#;

fn cmd(server: &ServerGuard, dir: &tempfile::TempDir) -> Command {
    let mut c = Command::cargo_bin("geekcli").unwrap();
    c.env_clear()
        .env(
            "SYSTEMROOT",
            std::env::var("SYSTEMROOT").unwrap_or_default(),
        )
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("GEEKCLI_SITE", "www.test.local")
        .env("GEEKCLI_BASE_URL", server.url())
        .env("GEEKCLI_CONFIG_DIR", dir.path());
    c
}

fn parse(out: &[u8]) -> Value {
    serde_json::from_slice(out)
        .unwrap_or_else(|e| panic!("not JSON: {e}\n{}", String::from_utf8_lossy(out)))
}

#[test]
fn fields_and_choices_need_no_key() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/search_forms/api/advanced_search_form.json")
        .with_body(FORM)
        .expect(2)
        .create();

    let out = cmd(&server, &dir)
        .args(["search", "fields"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc = parse(&out);
    let attrs: Vec<&str> = doc["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["attr"].as_str().unwrap())
        .collect();
    assert_eq!(attrs, ["city", "type", "type", "list_price_min", "polygon"]);
    let polygon = &doc["results"][4];
    assert_eq!(polygon["section"], "builtin");
    assert_eq!(polygon["widget"], "polygon");
    assert_eq!(polygon["choices_count"], 0);
    let mut keys: Vec<&String> = polygon.as_object().unwrap().keys().collect();
    let mut form_keys: Vec<&String> = doc["results"][0].as_object().unwrap().keys().collect();
    keys.sort();
    form_keys.sort();
    assert_eq!(
        keys, form_keys,
        "polygon row has the same shape as form rows"
    );

    let out = cmd(&server, &dir)
        .args(["search", "choices", "type"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc = parse(&out);
    assert_eq!(doc["results"][0]["value"], "res");
    assert_eq!(doc["results"][1]["label"], "Condo");
}

#[test]
fn check_reports_ignored_criteria_and_exits_5() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("list_price_min".into(), "1000000".into()),
            Matcher::UrlEncoded("q".into(), "Buckhead".into()),
        ]))
        .with_body(r#"{"description":"price at least $1,000,000","criteria":{"list_price_min":["1000000"]}}"#)
        .create();

    let assert = cmd(&server, &dir)
        .args(["search", "check", "list_price_min=1000000", "q=Buckhead"])
        .assert()
        .code(5);
    let doc = parse(&assert.get_output().stdout);
    assert_eq!(doc["ignored"], serde_json::json!(["q"]));
    assert_eq!(
        doc["results_url"],
        format!("{}/search/results/?list_price_min=1000000", server.url())
    );
    let err: Value = serde_json::from_slice(&assert.get_output().stderr).unwrap();
    assert_eq!(err["error"]["code"], "ignored_criteria");
    assert!(err["error"]["fields"]["q"][0]
        .as_str()
        .unwrap()
        .contains("ignored"));
}

#[test]
fn check_accepts_a_pasted_url() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("city".into(), "Key Biscayne".into()),
            Matcher::UrlEncoded("type".into(), "res".into()),
        ]))
        .with_body(r#"{"description":"in Key Biscayne","criteria":{"city":["Key Biscayne"],"type":["res"]}}"#)
        .create();
    cmd(&server, &dir)
        .args([
            "search",
            "check",
            "/search/results/?city=Key+Biscayne&type=res",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""ignored": []"#));
}

#[test]
fn run_flattens_results_and_reads_headers() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::Any)
        .with_body(r#"{"description":"d","criteria":{"city":["Miami"]}}"#)
        .create();
    server
        .mock("GET", "/api/v2/search/")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("city".into(), "Miami".into()),
            Matcher::UrlEncoded("per_page".into(), "2".into()),
            Matcher::UrlEncoded("sort_highest".into(), "true".into()),
            Matcher::UrlEncoded("include_description".into(), "long".into()),
        ]))
        .with_header("x-total-count", "1234")
        .with_header("x-description", "Search in <strong>Miami</strong>")
        .with_body(r#"[{"board":5050,"type":"res","fields":{"address":{"data":"1 Main St"},"list_price":{"data":"$2,000,000","raw":2000000},"url":{"data":"/property/A1/"}}}]"#)
        .create();

    let out = cmd(&server, &dir)
        .args([
            "search",
            "run",
            "city=Miami",
            "--per-page",
            "2",
            "--sort",
            "highest",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc = parse(&out);
    assert_eq!(doc["total"], 1234);
    assert_eq!(doc["description"], "Search in Miami");
    assert_eq!(doc["results"][0]["address"], "1 Main St");
    assert_eq!(doc["results"][0]["list_price_raw"], 2_000_000);
    assert_eq!(
        doc["results"][0]["url"],
        format!("{}/property/A1/", server.url())
    );
}

#[test]
fn run_strict_fails_on_ignored() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::Any)
        .with_body(r#"{"description":"d","criteria":{}}"#)
        .create();
    cmd(&server, &dir)
        .args(["search", "run", "q=x", "--strict"])
        .assert()
        .code(5);
}

#[test]
fn url_save_returns_saved_search_paths() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::Any)
        .with_body(r#"{"description":"d","criteria":{"city":["Miami"]}}"#)
        .create();
    server
        .mock("POST", "/api/v2/search/")
        .match_query(Matcher::UrlEncoded("city".into(), "Miami".into()))
        .with_status(201)
        .with_body(r#"{"search_id":"a","search_url":"/api/v2/search/a/","search_id_int":10}"#)
        .create();
    let out = cmd(&server, &dir)
        .args(["search", "url", "city=Miami", "--save"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc = parse(&out);
    assert_eq!(doc["search_id"], "a");
    assert_eq!(doc["results_path"], "/search/results/a/");
    assert_eq!(doc["map_path"], "/map_search/results/a/1/");
}

#[test]
fn area_page_search_describes_saved_search() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v3/content/area-pages/23/")
        .match_header("authorization", "Bearer rg_live_k")
        .with_body(r#"{"id":23,"path":"/lakeside/","search":{"id":10,"short_id":"a","description":"d","criteria":{}}}"#)
        .create();
    server
        .mock("GET", "/api/v2/search/a/")
        .match_query(Matcher::Any)
        .with_header("x-total-count", "1200")
        .with_header(
            "x-description",
            "Search having <strong>Community</strong>=Lakeside",
        )
        .with_body("[]")
        .create();
    let out = cmd(&server, &dir)
        .env("GEEKCLI_API_KEY", "rg_live_k")
        .args(["area-pages", "search", "23"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc = parse(&out);
    assert_eq!(doc["search_id"], "a");
    assert_eq!(doc["total"], 1200);
    assert_eq!(doc["description"], "Search having Community=Lakeside");
    assert_eq!(doc["page"]["path"], "/lakeside/");
}

#[test]
fn choices_all_uses_the_autocomplete_index() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v2/search/autocomplete-options/")
        .with_body(r#"[{"field":"city","value":"Riverside"},{"field":"subdivision","value":"Island at Oak Ridge"},{"field":"subdivision","value":"Oak Ridge Town Center 3"},{"field":"subdivision","value":"Cedar Point"}]"#)
        .create();
    let out = cmd(&server, &dir)
        .args([
            "search",
            "choices",
            "subdivision",
            "--all",
            "-s",
            "oak",
            "-q",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc = String::from_utf8_lossy(&out);
    assert!(
        doc.contains("Oak Ridge Town Center 3")
            && doc.contains("Island at Oak Ridge")
            && !doc.contains("Cedar Point"),
        "{doc}"
    );
}

const AUTOCOMPLETE: &str = r#"[{"field":"city","value":"Mclean"},{"field":"city","value":"Riverside"},{"field":"city","value":"Miami Beach"},{"field":"subdivision","value":"Cedar Point"}]"#;

fn mock_form_and_index(server: &mut ServerGuard) -> (mockito::Mock, mockito::Mock) {
    let form = server
        .mock("GET", "/search_forms/api/advanced_search_form.json")
        .with_body(FORM)
        .expect(1)
        .create();
    let index = server
        .mock("GET", "/api/v2/search/autocomplete-options/")
        .with_body(AUTOCOMPLETE)
        .expect(1)
        .create();
    (form, index)
}

#[test]
fn check_warns_on_a_case_mismatch() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::Any)
        .with_body(
            r#"{"description":"d","criteria":{"city":["McLean"],"list_price_min":["1000000"]}}"#,
        )
        .create();
    let (form, index) = mock_form_and_index(&mut server);
    let assert = cmd(&server, &dir)
        .args(["search", "check", "city=McLean", "list_price_min=1000000"])
        .assert()
        .success()
        .stderr(predicate::str::contains("did you mean `Mclean`?"));
    form.assert();
    index.assert();
    let doc = parse(&assert.get_output().stdout);
    assert_eq!(doc["values_checked"], serde_json::json!(["city"]));
    let w = &doc["value_warnings"][0];
    assert_eq!(w["kind"], "case_mismatch");
    assert_eq!(w["field"], "city");
    assert_eq!(w["suggestions"], serde_json::json!(["Mclean"]));
    assert_eq!(doc["value_warnings"].as_array().unwrap().len(), 1);
}

#[test]
fn check_suggests_close_values_for_an_unknown_one() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::Any)
        .with_body(
            r#"{"description":"d","criteria":{"city":["Riversdie","Miami"],"type":["res"]}}"#,
        )
        .create();
    let (_form, _index) = mock_form_and_index(&mut server);
    let out = cmd(&server, &dir)
        .args([
            "search",
            "check",
            "city=Riversdie",
            "city=Miami",
            "type=res",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc = parse(&out);
    let warnings = doc["value_warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{doc}");
    assert_eq!(warnings[0]["kind"], "unknown_value");
    assert_eq!(warnings[0]["value"], "Riversdie");
    assert_eq!(warnings[0]["suggestions"][0], "Riverside");
    assert!(warnings[0]["suggestions"].as_array().unwrap().len() <= 3);
    assert_eq!(doc["values_checked"], serde_json::json!(["city", "type"]));
}

#[test]
fn check_exact_form_values_skip_the_index() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::Any)
        .with_body(r#"{"description":"d","criteria":{"city":["Miami"]}}"#)
        .create();
    server
        .mock("GET", "/search_forms/api/advanced_search_form.json")
        .with_body(FORM)
        .expect(1)
        .create();
    let index = server
        .mock("GET", "/api/v2/search/autocomplete-options/")
        .expect(0)
        .create();
    cmd(&server, &dir)
        .args(["search", "check", "city=Miami"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""value_warnings": []"#));
    index.assert();
}

#[test]
fn check_strict_fails_on_value_warnings() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::Any)
        .with_body(r#"{"description":"d","criteria":{"city":["McLean"]}}"#)
        .create();
    let (_form, _index) = mock_form_and_index(&mut server);
    let assert = cmd(&server, &dir)
        .args(["search", "check", "city=McLean", "--strict"])
        .assert()
        .code(5);
    let err: Value = serde_json::from_slice(&assert.get_output().stderr).unwrap();
    assert_eq!(err["error"]["code"], "value_mismatch");
    assert!(err["error"]["fields"]["city"][0]
        .as_str()
        .unwrap()
        .contains("Mclean"));
}

#[test]
fn check_count_reports_matches_and_warns_on_zero() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::Any)
        .with_body(r#"{"description":"d","criteria":{"list_price_min":["1000000"]}}"#)
        .create();
    server
        .mock("GET", "/search_forms/api/advanced_search_form.json")
        .with_body(FORM)
        .create();
    let search = server
        .mock("GET", "/api/v2/search/")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("list_price_min".into(), "1000000".into()),
            Matcher::UrlEncoded("per_page".into(), "1".into()),
        ]))
        .with_header("x-total-count", "0")
        .with_body("[]")
        .expect(2)
        .create();
    let assert = cmd(&server, &dir)
        .args(["search", "check", "list_price_min=1000000", "--count"])
        .assert()
        .success()
        .stderr(predicate::str::contains("0 listings"));
    let doc = parse(&assert.get_output().stdout);
    assert_eq!(doc["count"], 0);
    assert_eq!(
        doc["warnings"],
        serde_json::json!(["the search matches 0 listings"])
    );

    let assert = cmd(&server, &dir)
        .args([
            "search",
            "check",
            "list_price_min=1000000",
            "--count",
            "--strict",
        ])
        .assert()
        .code(5);
    let err: Value = serde_json::from_slice(&assert.get_output().stderr).unwrap();
    assert_eq!(err["error"]["code"], "no_matches");
    search.assert();
}

#[test]
fn choices_fuzzy_ranks_by_edit_distance() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v2/search/autocomplete-options/")
        .with_body(AUTOCOMPLETE)
        .create();
    let out = cmd(&server, &dir)
        .args(["search", "choices", "city", "--all", "--fuzzy", "mclaen"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc = parse(&out);
    let values: Vec<&str> = doc["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["value"].as_str().unwrap())
        .collect();
    assert_eq!(values[0], "Mclean");
    assert_eq!(doc["results"][0]["distance"], 2);
    assert_eq!(values.len(), 3);

    server
        .mock("GET", "/search_forms/api/advanced_search_form.json")
        .with_body(FORM)
        .create();
    let out = cmd(&server, &dir)
        .args(["search", "choices", "city", "--fuzzy", "biscayne"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc = parse(&out);
    assert_eq!(doc["results"][0]["value"], "Key Biscayne");
}

#[test]
fn check_strict_fails_when_values_cannot_be_checked() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::Any)
        .with_body(r#"{"description":"d","criteria":{"city":["Springfield"]}}"#)
        .create();
    server
        .mock("GET", "/search_forms/api/advanced_search_form.json")
        .with_status(500)
        .create();

    // without --strict the check still passes and says what it skipped
    cmd(&server, &dir)
        .args(["search", "check", "city=Springfield"])
        .assert()
        .success()
        .stderr(predicate::str::contains("values not checked"));

    let assert = cmd(&server, &dir)
        .args(["search", "check", "city=Springfield", "--strict"])
        .assert()
        .code(1);
    let err: Value = serde_json::from_slice(&assert.get_output().stderr).unwrap();
    assert_eq!(err["error"]["code"], "value_check_unavailable");
}

const POLYGON: &str = "38.78512,-77.24901;38.79870,-77.21544;38.76632,-77.19902;38.75421,-77.23718;38.78512,-77.24901";

#[test]
fn malformed_polygon_is_a_usage_error_before_any_request() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let any = server
        .mock("GET", Matcher::Any)
        .match_query(Matcher::Any)
        .expect(0)
        .create();
    for bad in [
        "polygon=38.7,-77.2;38.8,-77.1",
        "polygon=38.7,-77.2;oops;38.6,-77.0;38.7,-77.2",
        "polygon=-120.8,38.1;38.8,-77.1;38.6,-77.0;-120.8,38.1",
    ] {
        cmd(&server, &dir)
            .args(["search", "check", bad])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("polygon"));
        cmd(&server, &dir)
            .args(["search", "run", bad, "type=res"])
            .assert()
            .code(2);
    }
    cmd(&server, &dir)
        .env("GEEKCLI_API_KEY", "rg_live_k")
        .args([
            "area-pages",
            "create",
            "--slug",
            "zone",
            "--area-name",
            "Zone",
            "--anchor-text",
            "Zone",
            "--search-criteria",
            "polygon=1,2;3,4",
        ])
        .assert()
        .code(2);
    any.assert();
}

#[test]
fn valid_polygon_passes_through_unchanged() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let meta = server
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("polygon".into(), POLYGON.into()),
            Matcher::UrlEncoded("type".into(), "res".into()),
        ]))
        .with_body(format!(
            r#"{{"description":"in a custom area","criteria":{{"polygon":["{POLYGON}"],"type":["res"]}}}}"#
        ))
        .expect(2)
        .create();
    // `search check` reads the form to check type=res; polygon itself has no
    // choice list, so the autocomplete index is never fetched
    server
        .mock("GET", "/search_forms/api/advanced_search_form.json")
        .with_body(FORM)
        .create();
    let index = server
        .mock("GET", "/api/v2/search/autocomplete-options/")
        .expect(0)
        .create();
    let search = server
        .mock("GET", "/api/v2/search/")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("polygon".into(), POLYGON.into()),
            Matcher::UrlEncoded("type".into(), "res".into()),
        ]))
        .with_header("x-total-count", "12")
        .with_body("[]")
        .create();
    cmd(&server, &dir)
        .args(["search", "check", &format!("polygon={POLYGON}"), "type=res"])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning").not());
    let out = cmd(&server, &dir)
        .args(["search", "run", &format!("polygon={POLYGON}"), "type=res"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["total"], 12);
    meta.assert();
    search.assert();
    index.assert();

    let create = server
        .mock("POST", "/api/v3/content/area-pages/")
        .match_body(Matcher::PartialJson(serde_json::json!({
            "slug": "my-zone",
            "search": { "polygon": [POLYGON], "type": ["res"] }
        })))
        .with_body(r#"{"id":31,"path":"/my-zone/"}"#)
        .create();
    cmd(&server, &dir)
        .env("GEEKCLI_API_KEY", "rg_live_k")
        .args([
            "area-pages",
            "create",
            "--slug",
            "my-zone",
            "--area-name",
            "My Zone",
            "--anchor-text",
            "My Zone",
            "--search-criteria",
            &format!("polygon={POLYGON}"),
            "--search-criteria",
            "type=res",
        ])
        .assert()
        .success();
    create.assert();
}

#[test]
fn unclosed_polygon_warns_but_is_sent() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let open = "38.78512,-77.24901;38.79870,-77.21544;38.76632,-77.19902";
    let meta = server
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::UrlEncoded("polygon".into(), open.into()))
        .with_body(format!(
            r#"{{"description":"d","criteria":{{"polygon":["{open}"]}}}}"#
        ))
        .create();
    cmd(&server, &dir)
        .args(["search", "check", &format!("polygon={open}")])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "warning: polygon: the ring is not closed",
        ));
    meta.assert();
}

/// The site's field catalog, in the shape the site serves: city and
/// subdivision are filled from listings (`dynamic`, no choices), price is a
/// range searched as `list_price_min` / `list_price_max`.
const CATALOG: &str = r#"[
{"id":"city","label":"City","widget":"multiselect","flags":["all_option","dynamic"],"choices":[],"child":["subdivision"]},
{"id":"subdivision","label":"Subdivision","widget":"multiselect","flags":["dynamic"],"all_key":"All Subdivisions","choices":[]},
{"id":"list_price","label":"Price","widget":"min_max_field","flags":["min_max_field"],"default_min":50000,"default_max":"all","choices":[{"label":"$1,000,000","val":1000000}]},
{"id":"pool","label":"Pool","widget":"boolean","flags":[],"choices":[{"label":"Yes","val":true},{"label":"No","val":false}]},
{"id":"view","label":"View","widget":"some_new_widget"},
{"label":"no id, skipped"},
{"id":"type","label":"Type","widget":"multiselect","flags":null,"choices":[{"label":"Home","val":"res"},{"label":"Condo","val":"con"}]}
]"#;

fn mock_catalog(server: &mut ServerGuard) -> mockito::Mock {
    server
        .mock("GET", "/search_forms/api/dump_uberform_fields.json")
        .with_body(CATALOG)
        .expect(1)
        .create()
}

fn attrs(doc: &Value) -> Vec<String> {
    doc["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["attr"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn fields_come_from_the_catalog() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let catalog = mock_catalog(&mut server);
    let form = server
        .mock("GET", "/search_forms/api/advanced_search_form.json")
        .expect(0)
        .create();

    let assert = cmd(&server, &dir)
        .args(["search", "fields"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    catalog.assert();
    form.assert();
    let doc = parse(&assert.get_output().stdout);
    assert_eq!(
        attrs(&doc),
        [
            "city",
            "subdivision",
            "list_price",
            "pool",
            "view",
            "type",
            "polygon"
        ]
    );
    let rows = doc["results"].as_array().unwrap();
    assert_eq!(rows[0]["section"], "catalog");
    assert_eq!(rows[0]["dynamic"], true);
    assert_eq!(rows[0]["depends_on"], serde_json::json!(["subdivision"]));
    assert_eq!(
        rows[2]["params"],
        serde_json::json!(["list_price_min", "list_price_max"])
    );
    assert_eq!(rows[2]["default_min"], 50000);
    assert_eq!(rows[2]["choices"][0]["value"], 1_000_000);
    assert_eq!(rows[4]["widget"], "some_new_widget");
    assert_eq!(rows[4]["choices_count"], 0);
    assert_eq!(rows[5]["choices_count"], 2);
    assert_eq!(rows[5]["params"], serde_json::json!(["type"]));
    let polygon = &rows[6];
    assert_eq!(polygon["section"], "builtin");
    let mut keys: Vec<&String> = polygon.as_object().unwrap().keys().collect();
    let mut catalog_keys: Vec<&String> = rows[0].as_object().unwrap().keys().collect();
    keys.sort();
    catalog_keys.sort();
    assert_eq!(keys, catalog_keys, "polygon row has the same shape");
}

#[test]
fn fields_fall_back_to_the_form_without_a_catalog() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/search_forms/api/dump_uberform_fields.json")
        .with_status(404)
        .create();
    server
        .mock("GET", "/search_forms/api/advanced_search_form.json")
        .with_body(FORM)
        .expect(1)
        .create();
    let assert = cmd(&server, &dir)
        .args(["search", "fields"])
        .assert()
        .success()
        .stderr(predicate::str::contains("field catalog could not be read"));
    let doc = parse(&assert.get_output().stdout);
    assert_eq!(
        attrs(&doc),
        ["city", "type", "type", "list_price_min", "polygon"]
    );
    assert_eq!(doc["results"][0]["section"], "primary");
    assert_eq!(doc["results"][0]["params"], serde_json::json!(["city"]));
}

#[test]
fn choices_come_from_the_catalog() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/search_forms/api/dump_uberform_fields.json")
        .with_body(CATALOG)
        .create();
    let form = server
        .mock("GET", "/search_forms/api/advanced_search_form.json")
        .with_body(FORM)
        .expect(1)
        .create();

    let out = cmd(&server, &dir)
        .args(["search", "choices", "type"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc = parse(&out);
    assert_eq!(doc["results"][1]["value"], "con");
    assert_eq!(doc["results"][1]["label"], "Condo");

    // a range param finds its catalog field
    let out = cmd(&server, &dir)
        .args(["search", "choices", "list_price_min"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["results"][0]["value"], 1_000_000);

    // a listings-driven field has no catalog values: the form's default list
    let out = cmd(&server, &dir)
        .args(["search", "choices", "city"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(parse(&out)["results"][1]["value"], "Miami");
    form.assert();

    cmd(&server, &dir)
        .args(["search", "choices", "q"])
        .assert()
        .code(4);
}

#[test]
fn check_validates_values_against_the_catalog() {
    let mut server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server
        .mock("GET", "/api/v2/search/metadata/")
        .match_query(Matcher::Any)
        .with_body(
            r#"{"description":"d","criteria":{"city":["McLean"],"type":["rez"],"pool":["1"],"subdivision":["Cedar Point"]}}"#,
        )
        .create();
    let catalog = mock_catalog(&mut server);
    let form = server
        .mock("GET", "/search_forms/api/advanced_search_form.json")
        .expect(0)
        .create();
    let index = server
        .mock("GET", "/api/v2/search/autocomplete-options/")
        .with_body(AUTOCOMPLETE)
        .expect(1)
        .create();
    let out = cmd(&server, &dir)
        .args([
            "search",
            "check",
            "city=McLean",
            "type=rez",
            "pool=1",
            "subdivision=Cedar Point",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    catalog.assert();
    form.assert();
    index.assert();
    let doc = parse(&out);
    assert_eq!(doc["choices_source"], "catalog");
    let warnings = doc["value_warnings"].as_array().unwrap();
    let by_field = |f: &str| warnings.iter().find(|w| w["field"] == f).cloned();
    assert_eq!(by_field("city").unwrap()["kind"], "case_mismatch");
    let ty = by_field("type").unwrap();
    assert_eq!(ty["kind"], "unknown_value");
    assert_eq!(ty["suggestions"][0], "res");
    assert!(by_field("pool").is_none(), "yes-no fields are not checked");
    assert!(by_field("subdivision").is_none(), "{doc}");
    assert_eq!(
        doc["values_checked"],
        serde_json::json!(["city", "type", "subdivision"])
    );
}
