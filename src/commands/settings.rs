//! `geekcli settings …` — the site settings an owner can change.
//! Values are JSON-typed; `set` coerces `NAME=value` text using the
//! setting's declared type so agents can pass plain strings.

use clap::{Args, Subcommand};
use serde_json::{json, Map, Value};

use super::{parse_bool, Context};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::html;
use crate::output::{col, Column};

pub const PATH: &str = "settings/";

pub const COLUMNS: &[Column] = &[
    col("name", "/name"),
    col("type", "/type"),
    col("value", "/value"),
    col("overridden", "/overridden"),
    col("group", "/group"),
];

#[derive(Debug, Args)]
pub struct SettingsCommand {
    #[command(subcommand)]
    pub command: SettingsSub,
}

#[derive(Debug, Subcommand)]
pub enum SettingsSub {
    /// List editable settings
    List(ListArgs),
    /// Show one setting with its description, choices and dependencies
    Get { name: String },
    /// Change settings: NAME=value pairs (typed from the setting's definition)
    #[command(after_help = "Notes:
  - A batch with one invalid setting is rejected whole; fix or drop the named one and resend the rest.
  - Some settings exist only on certain designs (TAGLINE, HEADER_IMAGE_ALT, MOBILE_HEADER_LOGO): the error names the TEMPLATE values that allow them.
  - File settings take a URL, normally one from `files upload -q`; a stored value with a server path in front of https:// is corrupt and the bare URL fixes it.
  - TEMPLATE is not a setting: change the design with `geekcli design set --template ...`.")]
    Set(SetArgs),
    /// Reset settings to their inherited defaults
    Clear {
        #[arg(value_name = "NAME", required = true)]
        names: Vec<String>,
    },
    /// List the setting groups
    Groups,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Only this group (substring match)
    #[arg(long, value_name = "GROUP")]
    pub group: Option<String>,
    /// Name, label or description contains
    #[arg(long, short = 's', value_name = "TEXT")]
    pub search: Option<String>,
    /// Only settings this site overrides
    #[arg(long)]
    pub overridden: bool,
}

#[derive(Debug, Args)]
pub struct SetArgs {
    /// NAME=value; value is coerced to the setting's type. `null` clears it.
    #[arg(value_name = "NAME=VALUE")]
    pub pairs: Vec<String>,
    /// Raw JSON object of {NAME: value}, `@file`, or `-` (values sent as-is)
    #[arg(long, value_name = "JSON")]
    pub data: Option<String>,
}

pub fn run(ctx: &Context, cmd: SettingsCommand) -> Result<()> {
    match cmd.command {
        SettingsSub::List(args) => {
            let mut rows = all(ctx)?;
            if let Some(group) = &args.group {
                let g = group.to_ascii_lowercase();
                rows.retain(|s| text(s, "group").to_ascii_lowercase().contains(&g));
            }
            if let Some(needle) = &args.search {
                let n = needle.to_ascii_lowercase();
                rows.retain(|s| {
                    ["name", "label", "description"]
                        .iter()
                        .any(|k| text(s, k).to_ascii_lowercase().contains(&n))
                });
            }
            if args.overridden {
                rows.retain(|s| {
                    s.get("overridden")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                });
            }
            ctx.printer.list(&rows, None, COLUMNS)
        }
        SettingsSub::Get { name } => {
            let setting = ctx
                .client
                .get(
                    &format!("{PATH}{}/", name.trim().to_ascii_uppercase()),
                    &Query::new(),
                )?
                .body;
            ctx.printer.raw(&setting)
        }
        SettingsSub::Groups => {
            let mut groups: Vec<String> = all(ctx)?.iter().map(|s| text(s, "group")).collect();
            groups.sort();
            groups.dedup();
            let rows: Vec<Value> = groups.into_iter().map(|g| json!({ "group": g })).collect();
            ctx.printer.list(&rows, None, &[col("group", "/group")])
        }
        SettingsSub::Set(args) => {
            let mut body = Map::new();
            for pair in &args.pairs {
                let Some((name, raw)) = pair.split_once('=') else {
                    return Err(Error::Usage(format!("expected NAME=value, got '{pair}'")));
                };
                let name = name.trim().to_ascii_uppercase();
                let definition = ctx
                    .client
                    .get(&format!("{PATH}{name}/"), &Query::new())?
                    .body;
                body.insert(name, coerce(raw.trim(), &definition)?);
            }
            if let Some(source) = &args.data {
                let text = if let Some(path) = source.strip_prefix('@') {
                    html::read_file_or_stdin(path)?
                } else if source == "-" {
                    html::read_file_or_stdin("-")?
                } else {
                    source.clone()
                };
                let value: Value = serde_json::from_str(&text)?;
                let Value::Object(map) = value else {
                    return Err(Error::Usage(
                        "--data must be a JSON object of {NAME: value}".into(),
                    ));
                };
                for (k, v) in map {
                    body.entry(k.to_ascii_uppercase()).or_insert(v);
                }
            }
            if body.is_empty() {
                return Err(Error::Usage("pass NAME=value pairs or --data".into()));
            }
            apply(ctx, &Value::Object(body))
        }
        SettingsSub::Clear { names } => {
            let body: Map<String, Value> = names
                .iter()
                .map(|n| (n.trim().to_ascii_uppercase(), Value::Null))
                .collect();
            apply(ctx, &Value::Object(body))
        }
    }
}

fn all(ctx: &Context) -> Result<Vec<Value>> {
    Ok(ctx
        .client
        .get(PATH, &Query::new())?
        .body
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

fn apply(ctx: &Context, body: &Value) -> Result<()> {
    let response = ctx.client.patch(PATH, body)?.body;
    let rows = response
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    ctx.printer
        .note(&format!("Updated {} setting(s)", rows.len()));
    ctx.printer.list(&rows, None, COLUMNS)
}

fn text(setting: &Value, key: &str) -> String {
    setting
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Turn command-line text into the JSON value the setting expects.
pub fn coerce(raw: &str, definition: &Value) -> Result<Value> {
    let kind = definition
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("string");
    let name = definition
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("setting");
    if raw.eq_ignore_ascii_case("null") {
        return Ok(Value::Null);
    }
    let bad = |what: &str| Error::Usage(format!("{name} expects {what}, got '{raw}'"));
    match kind {
        "boolean" => parse_bool(raw)
            .map(Value::Bool)
            .map_err(|_| bad("true or false")),
        "integer" => raw
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| bad("an integer")),
        "integer_list" => raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<i64>().map(Value::from))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map(Value::Array)
            .map_err(|_| bad("comma-separated integers")),
        "list" => {
            if raw.trim_start().starts_with('[') {
                serde_json::from_str(raw).map_err(|_| bad("a JSON list or comma-separated values"))
            } else {
                Ok(Value::Array(
                    raw.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|s| Value::String(s.into()))
                        .collect(),
                ))
            }
        }
        "object" | "yaml" => match serde_json::from_str::<Value>(raw) {
            Ok(v @ (Value::Object(_) | Value::Array(_))) => Ok(v),
            _ if kind == "yaml" => Ok(Value::String(raw.to_string())),
            _ => Err(bad("a JSON object")),
        },
        "choice" => {
            let choices = definition
                .get("choices")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            // match the choice's own JSON type: 1 vs "1", true vs "true"
            let candidates = [
                raw.parse::<i64>().ok().map(Value::from),
                parse_bool(raw).ok().map(Value::Bool),
                Some(Value::String(raw.to_string())),
            ];
            for candidate in candidates.into_iter().flatten() {
                if choices.is_empty() || choices.contains(&candidate) {
                    return Ok(candidate);
                }
            }
            let shown: Vec<String> = choices.iter().map(crate::output::cell).collect();
            Err(bad(&format!("one of: {}", shown.join(", "))))
        }
        _ => Ok(Value::String(raw.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coerces_by_type() {
        let def = |t: &str| json!({ "name": "X", "type": t });
        assert_eq!(coerce("yes", &def("boolean")).ok(), Some(json!(true)));
        assert_eq!(coerce("12", &def("integer")).ok(), Some(json!(12)));
        assert_eq!(
            coerce("1, 2,3", &def("integer_list")).ok(),
            Some(json!([1, 2, 3]))
        );
        assert_eq!(coerce("a, b", &def("list")).ok(), Some(json!(["a", "b"])));
        assert_eq!(coerce(r#"["a"]"#, &def("list")).ok(), Some(json!(["a"])));
        assert_eq!(
            coerce(r#"{"k": 1}"#, &def("object")).ok(),
            Some(json!({"k": 1}))
        );
        assert_eq!(coerce("hello", &def("string")).ok(), Some(json!("hello")));
        assert_eq!(coerce("null", &def("string")).ok(), Some(Value::Null));
        assert!(coerce("abc", &def("integer")).is_err());
    }

    #[test]
    fn choice_matches_the_choice_type() {
        let def = json!({ "name": "LEAD", "type": "choice", "choices": [0, 1, 2] });
        assert_eq!(coerce("1", &def).ok(), Some(json!(1)));
        assert!(coerce("9", &def).is_err());
        let def = json!({ "name": "T", "type": "choice", "choices": ["miranda", "anna"] });
        assert_eq!(coerce("anna", &def).ok(), Some(json!("anna")));
    }
}
