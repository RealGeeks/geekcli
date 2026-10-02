//! Revision history and undo for content pages, agent landing pages, area
//! pages, blog posts, footers and the home page: list saves, preview what a
//! revert would restore, revert.

use serde_json::{json, Value};

use super::{confirm, Context};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::output::{cell, col, Column, Format};

pub const COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("at", "/at"),
    col("action", "/action"),
    col("by", "/by"),
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
    ctx.printer.list(&rows, None, COLUMNS)
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
        cell(&body["by"]),
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

fn summarise(value: Option<&Value>) -> Value {
    let Some(v) = value else { return Value::Null };
    match v {
        Value::String(s) => {
            let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
            if flat.chars().count() > 90 {
                Value::String(format!(
                    "{}… ({} chars)",
                    flat.chars().take(90).collect::<String>(),
                    s.chars().count()
                ))
            } else {
                Value::String(flat)
            }
        }
        other => other.clone(),
    }
}

pub fn revert(ctx: &Context, resource_detail: &str, rev: u64, label: &str) -> Result<()> {
    if !confirm(
        ctx,
        &format!("revert {label} to before revision {rev} (this also undoes every later revision)"),
    )? {
        return Err(Error::Usage("cancelled".into()));
    }
    let page = ctx
        .client
        .post(
            &format!("{}{rev}/revert/", base(resource_detail)),
            &json!({}),
        )?
        .body;
    ctx.printer.note(&format!(
        "Reverted {label}; the revert is itself a revision and can be undone."
    ));
    ctx.printer.raw(&page)
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
}
