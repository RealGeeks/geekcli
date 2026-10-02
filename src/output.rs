//! Printing results. JSON is the default when stdout is not a terminal so
//! agents and scripts get something parseable; a table when it is.

use std::io::{self, IsTerminal, Write};

use comfy_table::{presets::UTF8_FULL_CONDENSED, ContentArrangement, Table};
use serde_json::Value;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    /// Pretty-printed JSON (default when piped)
    Json,
    /// One JSON document per line, for lists
    Jsonl,
    /// A human-readable table (default on a terminal)
    Table,
}

#[derive(Debug, Clone, Copy)]
pub struct Printer {
    pub format: Format,
    /// Print only ids (one per line) instead of the full resource.
    pub quiet: bool,
}

impl Printer {
    pub fn new(format: Option<Format>, quiet: bool) -> Self {
        let format = format.unwrap_or_else(|| {
            if io::stdout().is_terminal() {
                Format::Table
            } else {
                Format::Json
            }
        });
        Self { format, quiet }
    }

    /// Print one resource.
    pub fn one(&self, value: &Value, columns: &[Column]) -> Result<()> {
        if self.quiet {
            return print_ids(std::slice::from_ref(value));
        }
        match self.format {
            Format::Json => print_json(value),
            Format::Jsonl => print_jsonl(std::slice::from_ref(value)),
            Format::Table => print_detail(value, columns),
        }
    }

    /// Print a list of resources plus optional pagination metadata.
    pub fn list(
        &self,
        items: &[Value],
        pagination: Option<&Value>,
        columns: &[Column],
    ) -> Result<()> {
        if self.quiet {
            return print_ids(items);
        }
        match self.format {
            Format::Json => {
                let mut doc = serde_json::Map::new();
                doc.insert("results".into(), Value::Array(items.to_vec()));
                if let Some(p) = pagination {
                    doc.insert("pagination".into(), p.clone());
                }
                print_json(&Value::Object(doc))
            }
            Format::Jsonl => print_jsonl(items),
            Format::Table => {
                print_table(items, columns)?;
                if let Some(p) = pagination {
                    let page = p.get("page").and_then(Value::as_u64).unwrap_or(1);
                    let pages = p.get("total_pages").and_then(Value::as_u64).unwrap_or(1);
                    let total = p
                        .get("total")
                        .and_then(Value::as_u64)
                        .unwrap_or(items.len() as u64);
                    let mut out = io::stdout().lock();
                    writeln!(out, "page {page} of {pages}, {total} total")?;
                }
                Ok(())
            }
        }
    }

    /// Print a free-form JSON document (the `/me/` response, a raw call).
    pub fn raw(&self, value: &Value) -> Result<()> {
        match self.format {
            Format::Table => print_flat(value),
            Format::Json | Format::Jsonl => print_json(value),
        }
    }

    /// A line of status for humans; suppressed in JSON mode so stdout stays
    /// parseable. Goes to stderr.
    pub fn note(&self, text: &str) {
        if self.format == Format::Table {
            eprintln!("{}", sanitize(text));
        }
    }
}

/// A column in a table: header and a JSON pointer (`/id`, `/parent/slug`).
#[derive(Debug, Clone, Copy)]
pub struct Column {
    pub header: &'static str,
    pub pointer: &'static str,
}

pub const fn col(header: &'static str, pointer: &'static str) -> Column {
    Column { header, pointer }
}

pub fn print_json(value: &Value) -> Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer_pretty(&mut out, value)?;
    writeln!(out)?;
    Ok(())
}

fn print_jsonl(items: &[Value]) -> Result<()> {
    let mut out = io::stdout().lock();
    for item in items {
        serde_json::to_writer(&mut out, item)?;
        writeln!(out)?;
    }
    Ok(())
}

fn print_ids(items: &[Value]) -> Result<()> {
    let mut out = io::stdout().lock();
    for item in items {
        match item.get("id") {
            Some(id) => writeln!(out, "{}", cell(id))?,
            None => writeln!(out, "{}", cell(item))?,
        }
    }
    Ok(())
}

fn print_table(items: &[Value], columns: &[Column]) -> Result<()> {
    let mut table = Table::new();
    table
        .load_style(UTF8_FULL_CONDENSED)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(columns.iter().map(|c| c.header));
    for item in items {
        table.add_row(
            columns
                .iter()
                .map(|c| cell(item.pointer(c.pointer).unwrap_or(&Value::Null))),
        );
    }
    let mut out = io::stdout().lock();
    writeln!(out, "{table}")?;
    Ok(())
}

/// Key/value view of a single resource. Long HTML fields are truncated in
/// the table; use `-o json` for the full document.
fn print_detail(value: &Value, columns: &[Column]) -> Result<()> {
    let mut table = Table::new();
    table
        .load_style(UTF8_FULL_CONDENSED)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(["field", "value"]);
    let mut shown = std::collections::BTreeSet::new();
    for column in columns {
        if let Some(v) = value.pointer(column.pointer) {
            table.add_row([column.header.to_string(), truncate(&cell(v))]);
            shown.insert(column.pointer.trim_start_matches('/').to_string());
        }
    }
    if let Some(map) = value.as_object() {
        for (key, v) in map {
            if !shown.contains(key) {
                table.add_row([key.clone(), truncate(&cell(v))]);
            }
        }
    }
    let mut out = io::stdout().lock();
    writeln!(out, "{table}")?;
    Ok(())
}

fn print_flat(value: &Value) -> Result<()> {
    let mut out = io::stdout().lock();
    let mut rows = Vec::new();
    flatten("", value, &mut rows);
    let mut table = Table::new();
    table
        .load_style(UTF8_FULL_CONDENSED)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(["field", "value"]);
    for (k, v) in rows {
        table.add_row([k, truncate(&v)]);
    }
    writeln!(out, "{table}")?;
    Ok(())
}

fn flatten(prefix: &str, value: &Value, rows: &mut Vec<(String, String)>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&key, v, rows);
            }
        }
        other => rows.push((prefix.to_string(), cell(other))),
    }
}

/// Render a JSON value as a table cell.
/// Drop control characters (keeping newlines and tabs) from text bound for a
/// terminal: site content must not be able to send escape sequences that
/// rewrite the screen, the window title or the clipboard.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .filter(|&c| c == '\n' || c == '\t' || !c.is_control())
        .collect()
}

pub fn cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => sanitize(s),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(items) => items
            .iter()
            .map(|i| match i {
                // categories come back as {id,name,slug}; show the slug
                Value::Object(o) => o
                    .get("slug")
                    .or_else(|| o.get("name"))
                    .map_or_else(|| i.to_string(), cell),
                other => cell(other),
            })
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(o) => o
            .get("path")
            .or_else(|| o.get("slug"))
            .or_else(|| o.get("name"))
            .map_or_else(|| value.to_string(), cell),
    }
}

fn truncate(text: &str) -> String {
    const LIMIT: usize = 120;
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > LIMIT {
        let cut: String = flat.chars().take(LIMIT).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

/// Print an error to stderr in the selected format.
pub fn print_error(err: &Error, format: Format) {
    match format {
        Format::Json | Format::Jsonl => {
            let mut doc = serde_json::Map::new();
            doc.insert("code".into(), Value::String(err.code().to_string()));
            doc.insert("message".into(), Value::String(err.message()));
            if let Some(status) = err.status() {
                doc.insert("status".into(), Value::from(status));
            }
            if let Some(fields) = err.fields() {
                doc.insert(
                    "fields".into(),
                    serde_json::to_value(fields).unwrap_or(Value::Null),
                );
            }
            if let Some(retry) = err.retry_after() {
                doc.insert("retry_after".into(), Value::from(retry));
            }
            if let Some(hint) = err.hint() {
                doc.insert("hint".into(), Value::String(hint.to_string()));
            }
            doc.insert("exit_code".into(), Value::from(err.exit_code()));
            let envelope = serde_json::json!({ "error": Value::Object(doc) });
            eprintln!("{}", serde_json::to_string(&envelope).unwrap_or_default());
        }
        Format::Table => {
            eprintln!("error: {}", sanitize(&err.to_string()));
            if let Some(fields) = err.fields() {
                for (name, messages) in fields {
                    for message in messages {
                        eprintln!("  {}: {}", sanitize(name), sanitize(message));
                    }
                }
            }
            if let Some(retry) = err.retry_after() {
                eprintln!("  retry after {retry}s");
            }
            if let Some(hint) = err.hint() {
                eprintln!("  hint: {hint}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_terminal_escapes_from_cells() {
        let title = Value::String("Hi\u{1b}]0;PWNED\u{7}\u{1b}[31m red\u{9b}2J\nnext\tcol".into());
        assert_eq!(cell(&title), "Hi]0;PWNED[31m red2J\nnext\tcol");
    }
    use serde_json::json;

    #[test]
    fn cells_summarise_nested_values() {
        assert_eq!(
            cell(&json!([{"id": 1, "slug": "a"}, {"id": 2, "slug": "b"}])),
            "a, b"
        );
        assert_eq!(cell(&json!({"id": 3, "slug": "p", "path": "/p/"})), "/p/");
        assert_eq!(cell(&json!(null)), "");
        assert_eq!(cell(&json!(true)), "true");
    }

    #[test]
    fn truncates_long_html() {
        let long = "<p>".to_string() + &"word ".repeat(100) + "</p>";
        let out = truncate(&long);
        assert!(out.ends_with('…'));
        assert!(out.chars().count() <= 121);
    }
}
