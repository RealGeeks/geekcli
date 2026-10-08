//! Revision history and undo: list saves, preview what a revert would
//! restore, revert. Shared by every resource that keeps revisions: pages,
//! agent, area and market report pages, posts, footers, the home page,
//! sidebars, navigation bars, featured groups, banners, settings and design.

use serde_json::{json, Value};

use super::{confirm, Context};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::output::{cell, col, Column, Format};

pub const COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("at", "/at"),
    col("action", "/action"),
    col("by", "/by_label"),
    col("changed", "/changed_fields"),
    col("revertible", "/revertible"),
];

pub const PREVIEW_COLUMNS: &[Column] = &[
    col("field", "/field"),
    col("now", "/now"),
    col("after revert", "/restored"),
];

/// `content/pages/12/` → `content/pages/12/revisions/`
fn base(resource_detail: &str) -> String {
    format!(
        "{}revisions/",
        resource_detail.trim_end_matches('/').to_string() + "/"
    )
}

pub fn list(ctx: &Context, resource_detail: &str, limit: Option<usize>) -> Result<()> {
    let body = ctx.client.get(&base(resource_detail), &Query::new())?.body;
    let mut rows = body
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(n) = limit {
        rows.truncate(n);
    }
    if ctx.printer.quiet {
        for r in &rows {
            println!("{}", cell(r.get("id").unwrap_or(&Value::Null)));
        }
        return Ok(());
    }
    if ctx.printer.format == Format::Table {
        for row in &mut rows {
            let who = by_label(row.get("by"));
            row["by_label"] = Value::String(who);
        }
    }
    ctx.printer.list(&rows, None, COLUMNS)
}

/// Who made a revision, for the table: the person or tool, else the API
/// key. Settings and design revisions made through the API carry only the
/// key; older servers send `by` as plain text.
fn by_label(by: Option<&Value>) -> String {
    let Some(by) = by else {
        return String::new();
    };
    let text = |key: &str| {
        by.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    match (text("name"), text("api_key")) {
        (Some(name), _) => crate::output::sanitize(name),
        (None, Some(key)) => crate::output::sanitize(&format!("API key {key}")),
        (None, None) if by.is_object() => String::new(),
        (None, None) => cell(by),
    }
}

pub fn show(ctx: &Context, resource_detail: &str, rev: u64) -> Result<()> {
    let body = ctx
        .client
        .get(&format!("{}{rev}/", base(resource_detail)), &Query::new())?
        .body;
    if ctx.printer.format != Format::Table {
        return ctx.printer.raw(&body);
    }
    ctx.printer.note(&format!(
        "Revision {} · {} · {} · {}{}",
        cell(&body["id"]),
        cell(&body["at"]),
        cell(&body["action"]),
        by_label(body.get("by")),
        if body.get("revertible").and_then(Value::as_bool) == Some(false) {
            " · not revertible"
        } else {
            ""
        }
    ));
    ctx.printer
        .list(&preview_rows(&body), None, PREVIEW_COLUMNS)
}

/// The preview as rows: `{"field", "now", "restored"}`. Long HTML is
/// summarised so the table stays readable; `-o json` has it all.
fn preview_rows(body: &Value) -> Vec<Value> {
    let Some(preview) = body.get("preview") else {
        return vec![];
    };
    let mut rows = Vec::new();
    match preview {
        Value::Object(map) => {
            for (field, change) in map {
                rows.push(json!({
                    "field": field,
                    "now": summarise(change.get("now").or_else(|| change.get("current"))),
                    "restored": summarise(change.get("restored").or_else(|| change.get("after_revert")).or_else(|| change.get("previous"))),
                }));
            }
        }
        Value::Array(items) => {
            for change in items {
                rows.push(json!({
                    "field": change.get("field").cloned().unwrap_or(Value::Null),
                    "now": summarise(change.get("now").or_else(|| change.get("current"))),
                    "restored": summarise(change.get("restored").or_else(|| change.get("after_revert")).or_else(|| change.get("previous"))),
                }));
            }
        }
        _ => {}
    }
    rows
}

/// Longest value shown in a preview cell.
const PREVIEW_CHARS: usize = 90;

fn summarise(value: Option<&Value>) -> Value {
    let Some(v) = value else { return Value::Null };
    match v {
        Value::String(s) => {
            let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
            if flat.chars().count() > PREVIEW_CHARS {
                Value::String(format!(
                    "{}… ({} chars)",
                    flat.chars().take(PREVIEW_CHARS).collect::<String>(),
                    s.chars().count()
                ))
            } else {
                Value::String(flat)
            }
        }
        // a list field (sidebar items, navigation links, tiles, category ids)
        Value::Array(items) if items.is_empty() => Value::String("(empty)".into()),
        Value::Array(items) => {
            let labels: Vec<String> = items.iter().map(entry_label).collect();
            let text = if items.iter().any(|i| i.is_object() || i.is_array()) {
                let noun = if items.len() == 1 { "entry" } else { "entries" };
                format!("{} {noun}: {}", items.len(), labels.join(", "))
            } else {
                labels.join(", ")
            };
            Value::String(clip(&text))
        }
        Value::Object(map) => Value::String(clip(&object_label(v, map))),
        other => other.clone(),
    }
}

fn clip(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > PREVIEW_CHARS {
        format!("{}…", flat.chars().take(PREVIEW_CHARS).collect::<String>())
    } else {
        flat
    }
}

/// One entry of a list field, as a few words: a tile's title, a link's
/// text, a sidebar item's header or the start of its HTML.
fn entry_label(entry: &Value) -> String {
    let Value::Object(map) = entry else {
        return cell(entry);
    };
    object_label(entry, map)
}

fn object_label(value: &Value, map: &serde_json::Map<String, Value>) -> String {
    let text = |key: &str| {
        map.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    // a colour scheme: the variation name says little once variables differ
    if let Some(vars) = map.get("vars").and_then(Value::as_object) {
        let pairs: Vec<String> = vars
            .iter()
            .map(|(k, v)| format!("{k}={}", cell(v)))
            .collect();
        return format!("{}: {}", text("name").unwrap_or("custom"), pairs.join(", "));
    }
    if let Some(found) = [
        "title",
        "anchor_text",
        "anchor",
        "name",
        "path",
        "slug",
        "url",
    ]
    .into_iter()
    .find_map(text)
    {
        return crate::output::sanitize(found);
    }
    if let Some(header) = value.pointer("/header/text").and_then(Value::as_str) {
        return crate::output::sanitize(header);
    }
    if let Some(html) = text("html") {
        let flat: String = html.split_whitespace().collect::<Vec<_>>().join(" ");
        let shown: String = flat.chars().take(30).collect();
        return crate::output::sanitize(&if flat.chars().count() > 30 {
            format!("{shown}…")
        } else {
            shown
        });
    }
    if let Some(kind) = text("type") {
        return crate::output::sanitize(kind);
    }
    crate::output::sanitize(&value.to_string())
}

pub fn revert(ctx: &Context, resource_detail: &str, rev: u64, label: &str) -> Result<()> {
    let page = revert_request(ctx, resource_detail, rev, label)?;
    ctx.printer.raw(&page)
}

/// Confirm, revert and return what the API answered, for resources that
/// print their result their own way (a sidebar's items, the design).
pub fn revert_request(
    ctx: &Context,
    resource_detail: &str,
    rev: u64,
    label: &str,
) -> Result<Value> {
    if !confirm(
        ctx,
        &format!("revert {label} to before revision {rev} (this also undoes every later revision)"),
    )? {
        return Err(Error::Usage("cancelled".into()));
    }
    let restored = ctx
        .client
        .post(
            &format!("{}{rev}/revert/", base(resource_detail)),
            &json!({}),
        )?
        .body;
    ctx.printer.note(&format!(
        "Reverted {label}; the revert is itself a revision and can be undone."
    ));
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_paths() {
        assert_eq!(base("content/pages/12/"), "content/pages/12/revisions/");
        assert_eq!(base("content/home-page"), "content/home-page/revisions/");
    }

    #[test]
    fn preview_rows_from_object_and_list() {
        let obj = json!({ "preview": { "title": { "now": "B", "restored": "A" } } });
        assert_eq!(preview_rows(&obj)[0]["restored"], "A");
        let list = json!({ "preview": [ { "field": "content", "now": "<p>long</p>", "restored": "<p>old</p>" } ] });
        assert_eq!(preview_rows(&list)[0]["field"], "content");
    }

    #[test]
    fn preview_summarises_lists_and_objects() {
        let body = json!({ "preview": {
            "links": {
                "now": [],
                "after_revert": [
                    { "id": 10, "type": "custom", "url": "/", "anchor_text": "Home" },
                    { "id": 12, "type": "contact", "url": null, "anchor_text": null }
                ]
            },
            "items": {
                "now": [{ "type": "html", "html": "<p>Call  us\ntoday</p>" }],
                "after_revert": [{ "type": "links", "header": { "text": "Areas" }, "links": [] }]
            },
            "categories": { "now": [3, 8], "after_revert": [3] },
            "styles": {
                "now": { "name": "coastal", "vars": { "brand": "#fff" } },
                "after_revert": { "name": "coastal", "vars": { "brand": "#000" } }
            },
            "sidebar": { "now": null, "after_revert": { "id": 5, "name": "Blog Sidebar" } }
        } });
        let rows = preview_rows(&body);
        let row = |field: &str| {
            rows.iter()
                .find(|r| r["field"] == field)
                .cloned()
                .unwrap_or(Value::Null)
        };
        assert_eq!(row("links")["now"], "(empty)");
        assert_eq!(row("links")["restored"], "2 entries: Home, contact");
        assert_eq!(row("items")["now"], "1 entry: <p>Call us today</p>");
        assert_eq!(row("items")["restored"], "1 entry: Areas");
        assert_eq!(row("categories")["now"], "3, 8");
        assert_eq!(row("styles")["now"], "coastal: brand=#fff");
        assert_eq!(row("styles")["restored"], "coastal: brand=#000");
        assert_eq!(row("sidebar")["now"], Value::Null);
        assert_eq!(row("sidebar")["restored"], "Blog Sidebar");
    }

    #[test]
    fn long_lists_are_clipped() {
        let tiles: Vec<Value> = (0..40)
            .map(|n| json!({ "title": format!("Neighborhood {n}") }))
            .collect();
        let Value::String(text) = summarise(Some(&Value::Array(tiles))) else {
            return;
        };
        assert!(text.starts_with("40 entries: Neighborhood 0, "));
        assert!(text.ends_with('…'));
        assert_eq!(text.chars().count(), PREVIEW_CHARS + 1);
    }

    #[test]
    fn names_who_made_a_revision() {
        let by = |v: Value| by_label(Some(&v));
        assert_eq!(
            by(json!({ "name": "Jordan Avery", "api_key": "cli" })),
            "Jordan Avery"
        );
        assert_eq!(by(json!({ "name": null, "api_key": "cli" })), "API key cli");
        assert_eq!(by(json!({ "name": null, "api_key": null })), "");
        assert_eq!(by(json!("admin")), "admin");
        assert_eq!(by_label(None), "");
    }
}
