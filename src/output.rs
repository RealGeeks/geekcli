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
        let mut out = io::stdout().lock();
        self.write_list(&mut out, items, pagination, columns)
    }

    fn write_list(
        self,
        out: &mut impl Write,
        items: &[Value],
        pagination: Option<&Value>,
        columns: &[Column],
    ) -> Result<()> {
        if self.quiet {
            return write_ids(out, items);
        }
        match self.format {
            Format::Json => {
                let mut doc = serde_json::Map::new();
                doc.insert("results".into(), Value::Array(items.to_vec()));
                if let Some(p) = pagination {
                    doc.insert("pagination".into(), p.clone());
                }
                write_json(out, &Value::Object(doc))
            }
            Format::Jsonl => write_jsonl(out, items),
            Format::Table => {
                write_table(out, items, columns)?;
                if let Some(p) = pagination {
                    let page = p.get("page").and_then(Value::as_u64).unwrap_or(1);
                    let pages = p.get("total_pages").and_then(Value::as_u64).unwrap_or(1);
                    let total = p
                        .get("total")
                        .and_then(Value::as_u64)
                        .unwrap_or(items.len() as u64);
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
    write_json(&mut out, value)
}

fn write_json(out: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer_pretty(&mut *out, value)?;
    writeln!(out)?;
    Ok(())
}

fn print_jsonl(items: &[Value]) -> Result<()> {
    let mut out = io::stdout().lock();
    write_jsonl(&mut out, items)
}

fn write_jsonl(out: &mut impl Write, items: &[Value]) -> Result<()> {
    for item in items {
        serde_json::to_writer(&mut *out, item)?;
        writeln!(out)?;
    }
    Ok(())
}

fn print_ids(items: &[Value]) -> Result<()> {
    let mut out = io::stdout().lock();
    write_ids(&mut out, items)
}

fn write_ids(out: &mut impl Write, items: &[Value]) -> Result<()> {
    for item in items {
        match item.get("id") {
            Some(id) => writeln!(out, "{}", cell(id))?,
            None => writeln!(out, "{}", cell(item))?,
        }
    }
    Ok(())
}

fn write_table(out: &mut impl Write, items: &[Value], columns: &[Column]) -> Result<()> {
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
    writeln!(out, "{table}")?;
    Ok(())
}

/// Key/value view of a single resource. Long HTML fields are truncated in
/// the table; use `-o json` for the full document.
fn print_detail(value: &Value, columns: &[Column]) -> Result<()> {
    let mut out = io::stdout().lock();
    write_detail(&mut out, value, columns)
}

fn write_detail(out: &mut impl Write, value: &Value, columns: &[Column]) -> Result<()> {
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
                // keys come from the server too
                table.add_row([sanitize(key), truncate(&cell(v))]);
            }
        }
    }
    writeln!(out, "{table}")?;
    Ok(())
}

fn print_flat(value: &Value) -> Result<()> {
    let mut out = io::stdout().lock();
    write_flat(&mut out, value)
}

fn write_flat(out: &mut impl Write, value: &Value) -> Result<()> {
    let mut rows = Vec::new();
    flatten("", value, &mut rows);
    let mut table = Table::new();
    table
        .load_style(UTF8_FULL_CONDENSED)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(["field", "value"]);
    for (k, v) in rows {
        table.add_row([sanitize(&k), truncate(&v)]);
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
    let mut stderr = io::stderr().lock();
    let _ = write_error(&mut stderr, err, format);
}

fn write_error(out: &mut impl Write, err: &Error, format: Format) -> io::Result<()> {
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
            writeln!(
                out,
                "{}",
                serde_json::to_string(&envelope).unwrap_or_default()
            )
        }
        Format::Table => {
            writeln!(out, "error: {}", sanitize(&err.to_string()))?;
            if let Some(fields) = err.fields() {
                for (name, messages) in fields {
                    for message in messages {
                        writeln!(out, "  {}: {}", sanitize(name), sanitize(message))?;
                    }
                }
            }
            if let Some(retry) = err.retry_after() {
                writeln!(out, "  retry after {retry}s")?;
            }
            if let Some(hint) = err.hint() {
                writeln!(out, "  hint: {}", sanitize(hint))?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    #[test]
    fn strips_terminal_escapes_from_cells() {
        let title = Value::String("Hi\u{1b}]0;PWNED\u{7}\u{1b}[31m red\u{9b}2J\nnext\tcol".into());
        assert_eq!(cell(&title), "Hi]0;PWNED[31m red2J\nnext\tcol");
    }

    #[test]
    fn flat_and_detail_tables_strip_terminal_escapes_from_server_keys() -> Result<()> {
        let doc = json!({
            "id": 1,
            "evil\u{1b}]52;c;cHduZWQ=\u{7}": {"\u{1b}[2Jkey": "v"}
        });
        let mut flat = Vec::new();
        write_flat(&mut flat, &doc)?;
        let flat = String::from_utf8_lossy(&flat);
        assert!(flat.contains("evil]52;c;cHduZWQ=.[2Jkey"), "{flat}");
        assert!(
            !flat.contains('\u{1b}') && !flat.contains('\u{7}'),
            "{flat:?}"
        );

        let mut detail = Vec::new();
        write_detail(&mut detail, &doc, &[col("id", "/id")])?;
        let detail = String::from_utf8_lossy(&detail);
        assert!(detail.contains("evil]52;c;cHduZWQ="), "{detail}");
        assert!(
            !detail.contains('\u{1b}') && !detail.contains('\u{7}'),
            "{detail:?}"
        );
        Ok(())
    }

    #[test]
    fn list_renders_jsonl_ids_and_paginated_tables() -> Result<()> {
        let items = vec![
            json!({"id": 1, "title": "First"}),
            json!({"id": 2, "title": "Second"}),
        ];
        let cols = &[col("id", "/id"), col("title", "/title")];

        let mut jsonl = Vec::new();
        Printer::new(Some(Format::Jsonl), false).write_list(&mut jsonl, &items, None, cols)?;
        let lines: Vec<&str> = std::str::from_utf8(&jsonl)
            .unwrap_or_default()
            .lines()
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(serde_json::from_str::<Value>(lines[1])?["id"], 2);

        let mut table = Vec::new();
        Printer::new(Some(Format::Table), false).write_list(
            &mut table,
            &items,
            Some(&json!({"page": 2, "total_pages": 5, "total": 42})),
            cols,
        )?;
        let table = String::from_utf8_lossy(&table);
        assert!(
            table.contains("First") && table.contains("Second"),
            "{table}"
        );
        assert!(table.contains("page 2 of 5, 42 total"), "{table}");

        let mut ids = Vec::new();
        write_ids(&mut ids, &[json!({"id": 7}), json!("bare")])?;
        assert_eq!(String::from_utf8_lossy(&ids), "7\nbare\n");
        Ok(())
    }

    #[test]
    fn table_and_json_errors_include_fields_retry_and_hint() -> io::Result<()> {
        let mut fields = BTreeMap::new();
        fields.insert("slug\u{1b}[2J".into(), vec!["already taken\u{7}".into()]);
        let err = Error::Api {
            status: 409,
            code: "conflict".into(),
            message: "duplicate\u{1b}[31m".into(),
            fields,
            retry_after: Some(15),
        };
        let mut table = Vec::new();
        write_error(&mut table, &err, Format::Table)?;
        let table = String::from_utf8_lossy(&table);
        assert!(table.contains("error: conflict: duplicate[31m"), "{table}");
        assert!(table.contains("  slug[2J: already taken"), "{table}");
        assert!(table.contains("  retry after 15s"), "{table}");
        assert!(table.contains("  hint: "), "{table}");
        assert!(!table.contains('\u{1b}') && !table.contains('\u{7}'));

        let mut json_buf = Vec::new();
        write_error(&mut json_buf, &err, Format::Json)?;
        let parsed: Value = serde_json::from_slice(&json_buf).unwrap_or_default();
        assert_eq!(parsed["error"]["retry_after"], 15);
        assert_eq!(parsed["error"]["status"], 409);
        assert_eq!(parsed["error"]["exit_code"], 6);
        assert!(parsed["error"]["hint"].is_string());
        Ok(())
    }

    #[test]
    fn cells_summarise_nested_values() {
        assert_eq!(
            cell(&json!([{"id": 1, "slug": "a"}, {"id": 2, "name": "b"}, {"x": 1}])),
            "a, b, {\"x\":1}"
        );
        assert_eq!(cell(&json!({"id": 3, "slug": "p", "path": "/p/"})), "/p/");
        assert_eq!(cell(&json!({"id": 3, "name": "n"})), "n");
        assert_eq!(cell(&json!({"raw": 1})), "{\"raw\":1}");
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
