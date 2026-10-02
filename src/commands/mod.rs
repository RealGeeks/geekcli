//! Shared plumbing for the resource commands: the run context, payload
//! building from flags, and resolving human references (slug, path) to ids.

pub mod agent_pages;
pub mod api;
pub mod area_pages;
pub mod auth;
pub mod blog_home;
pub mod categories;
pub mod design;
pub mod featured;
pub mod files;
pub mod footers;
pub mod guide;
pub mod home_page;
pub mod inspect;
pub mod nav;
pub mod pages;
pub mod posts;
pub mod revisions;
pub mod search;
pub mod settings;
pub mod sidebars;
pub mod snapshot;
pub mod templates;

use std::io::{self, IsTerminal, Write};

use serde_json::{Map, Value};

use crate::client::{Client, Query};
use crate::error::{Error, Result};
use crate::html;
use crate::output::Printer;

pub struct Context {
    pub client: Client,
    pub printer: Printer,
    pub yes: bool,
}

/// A JSON object assembled from optional flags. Only flags that were given
/// end up in the body, which is what makes PATCH partial.
#[derive(Debug, Default, Clone)]
pub struct Payload(pub Map<String, Value>);

impl Payload {
    pub fn set<T: Into<Value>>(&mut self, name: &str, value: Option<T>) -> &mut Self {
        if let Some(v) = value {
            self.0.insert(name.to_string(), v.into());
        }
        self
    }

    pub fn set_value(&mut self, name: &str, value: Value) -> &mut Self {
        self.0.insert(name.to_string(), value);
        self
    }

    /// Merge a JSON object given inline, as `@file`, or `-` for stdin.
    /// Flags win over the JSON document when both set a field.
    pub fn merge_json(&mut self, source: Option<&str>) -> Result<&mut Self> {
        let Some(source) = source else {
            return Ok(self);
        };
        let text = if let Some(path) = source.strip_prefix('@') {
            html::read_file_or_stdin(path)?
        } else if source == "-" {
            html::read_file_or_stdin("-")?
        } else {
            source.to_string()
        };
        let value: Value = serde_json::from_str(&text)?;
        let Value::Object(map) = value else {
            return Err(Error::Usage("--json must be a JSON object".into()));
        };
        for (k, v) in map {
            self.0.entry(k).or_insert(v);
        }
        Ok(self)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn into_value(self) -> Value {
        Value::Object(self.0)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }
}

/// Parse `true/false/yes/no/1/0` for flags that take an explicit boolean so
/// "not given" stays distinguishable from "false" on updates.
pub fn parse_bool(text: &str) -> std::result::Result<bool, String> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "y" | "1" | "on" => Ok(true),
        "false" | "no" | "n" | "0" | "off" => Ok(false),
        other => Err(format!("expected true or false, got '{other}'")),
    }
}

/// Parse `null`/`none`/`-` as JSON null, otherwise an integer id.
pub fn parse_id_or_null(text: &str) -> std::result::Result<Value, String> {
    match text.trim().to_ascii_lowercase().as_str() {
        "null" | "none" | "-" | "" => Ok(Value::Null),
        other => other
            .parse::<u64>()
            .map(Value::from)
            .map_err(|_| format!("expected an integer id or null, got '{other}'")),
    }
}

pub fn push(query: &mut Query, name: &str, value: Option<&str>) {
    if let Some(v) = value {
        query.push((name.to_string(), v.to_string()));
    }
}

pub fn push_flag(query: &mut Query, name: &str, on: bool) {
    if on {
        query.push((name.to_string(), "true".to_string()));
    }
}

/// Common list-paging flags.
#[derive(Debug, Clone, clap::Args)]
pub struct Paging {
    /// Page number (1-based)
    #[arg(long)]
    pub page: Option<u32>,
    /// Results per page (max 100)
    #[arg(long)]
    pub page_size: Option<u32>,
    /// Fetch every page and print all results
    #[arg(long, conflicts_with = "page")]
    pub all: bool,
    /// Sort field; prefix with `-` for descending
    #[arg(long, allow_hyphen_values = true, value_name = "FIELD")]
    pub ordering: Option<String>,
}

impl Paging {
    pub fn apply(&self, query: &mut Query) {
        push(query, "page", self.page.map(|p| p.to_string()).as_deref());
        push(
            query,
            "page_size",
            self.page_size.map(|p| p.to_string()).as_deref(),
        );
        push(query, "ordering", self.ordering.as_deref());
    }
}

/// Run a list query and print it, honouring `--all`.
pub fn list_and_print(
    ctx: &Context,
    path: &str,
    query: &Query,
    paging: &Paging,
    columns: &[crate::output::Column],
) -> Result<()> {
    if paging.all {
        let items = ctx.client.get_all(path, query)?;
        return ctx.printer.list(&items, None, columns);
    }
    let response = ctx.client.get(path, query)?;
    let items = response
        .body
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    ctx.printer
        .list(&items, response.body.get("pagination"), columns)
}

/// Find a single resource by a non-numeric reference using a list filter.
pub fn lookup_one(
    client: &Client,
    path: &str,
    filter: &str,
    value: &str,
    label: &str,
) -> Result<Value> {
    let query: Query = vec![
        (filter.to_string(), value.to_string()),
        ("page_size".into(), "2".into()),
    ];
    let response = client.get(path, &query)?;
    let results = response
        .body
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    match results.len() {
        0 => Err(Error::Api {
            status: 404,
            code: "not_found".into(),
            message: format!("no {label} with {filter} '{value}'"),
            fields: std::collections::BTreeMap::new(),
            retry_after: None,
        }),
        1 => results
            .into_iter()
            .next()
            .ok_or_else(|| Error::Other("empty result".into())),
        _ => Err(Error::Usage(format!(
            "more than one {label} matches {filter} '{value}'; use the numeric id"
        ))),
    }
}

pub fn id_of(value: &Value) -> Result<u64> {
    value
        .get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Other("response has no id".into()))
}

/// Ask before a delete when a human is at the keyboard. Non-interactive
/// callers (agents, scripts) proceed without a prompt.
pub fn confirm(ctx: &Context, what: &str) -> Result<bool> {
    if ctx.yes || !io::stdin().is_terminal() {
        return Ok(true);
    }
    eprint!("Delete {what}? [y/N] ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// Print the resource returned by a write, adding a note in table mode.
pub fn print_written(
    ctx: &Context,
    value: &Value,
    columns: &[crate::output::Column],
    verb: &str,
) -> Result<()> {
    if let (Some(id), Some(url)) = (value.get("id"), value.get("url").and_then(Value::as_str)) {
        ctx.printer
            .note(&format!("{verb} id {} at {url}", crate::output::cell(id)));
    }
    ctx.printer.one(value, columns)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn payload_keeps_only_given_flags() {
        let mut p = Payload::default();
        p.set("title", Some("t"))
            .set::<&str>("slug", None)
            .set("n", Some(3));
        assert_eq!(p.clone().into_value(), json!({"title": "t", "n": 3}));
    }

    #[test]
    fn flags_win_over_json() {
        let mut p = Payload::default();
        p.set("title", Some("flag"));
        assert!(p
            .merge_json(Some(r#"{"title": "json", "slug": "s"}"#))
            .is_ok());
        assert_eq!(p.into_value(), json!({"title": "flag", "slug": "s"}));
    }

    #[test]
    fn json_must_be_object() {
        let mut p = Payload::default();
        assert!(matches!(p.merge_json(Some("[1]")), Err(Error::Usage(_))));
        assert!(matches!(p.merge_json(Some("{nope")), Err(Error::Usage(_))));
    }

    #[test]
    fn parses_bools_and_ids() {
        assert_eq!(parse_bool("Yes"), Ok(true));
        assert_eq!(parse_bool("0"), Ok(false));
        assert!(parse_bool("maybe").is_err());
        assert_eq!(parse_id_or_null("null"), Ok(Value::Null));
        assert_eq!(parse_id_or_null("12"), Ok(json!(12)));
        assert!(parse_id_or_null("abc").is_err());
    }
}
