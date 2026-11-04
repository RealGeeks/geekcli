//! `geekcli featured-areas …` — the SEO Fast Track area pages a site
//! features: the footer's areas column and, on alfred, the home page's
//! Neighborhoods module. Replaced as a whole.

use clap::{Args, Subcommand};
use serde_json::{json, Value};

use super::Context;
use crate::client::Query;
use crate::error::{Error, Result};
use crate::html;
use crate::output::{col, Column};

pub const PATH: &str = "content/featured-areas/";

pub const COLUMNS: &[Column] = &[
    col("key", "/key"),
    col("label", "/label"),
    col("url", "/url"),
];

#[derive(Debug, Args)]
pub struct FeaturedAreasCommand {
    #[command(subcommand)]
    pub command: FeaturedAreasSub,
}

#[derive(Debug, Subcommand)]
pub enum FeaturedAreasSub {
    /// List the featured areas, in order
    Get,
    /// Replace the featured areas (the full list, in order)
    #[command(after_help = "Notes:
  - Each area is ATTRIBUTE=VALUE, a search field and one of its values: city=Brentwood, zip=90049, \"subdivision=Bel Air\". Check values with `geekcli search check`; the API answers with the MLS's own spelling and a warning when it changes one.
  - On alfred the first six are the Neighborhoods module's cards; their photos are `home-page items set neighborhood` rows keyed attribute||value (the `key` here).
  - They show only while SEO Fast Track area pages are on for the site (a warning says so when they are off).
  - --data takes the API's shape instead: [{\"attribute\": \"city\", \"value\": \"Brentwood\"}].")]
    Set(SetArgs),
}

#[derive(Debug, Args)]
pub struct SetArgs {
    /// Areas as ATTRIBUTE=VALUE, in order; none clears the list
    #[arg(value_name = "ATTRIBUTE=VALUE", conflicts_with = "data")]
    pub areas: Vec<String>,
    /// JSON list of {attribute, value}, `{"areas": [...]}`, `@file`, or `-` for stdin
    #[arg(long, value_name = "JSON")]
    pub data: Option<String>,
}

pub fn run(ctx: &Context, cmd: FeaturedAreasCommand) -> Result<()> {
    match cmd.command {
        FeaturedAreasSub::Get => print(ctx, &ctx.client.get(PATH, &Query::new())?.body),
        FeaturedAreasSub::Set(args) => {
            let areas = match &args.data {
                Some(source) => read_json(source)?,
                None => args
                    .areas
                    .iter()
                    .map(|pair| parse_area(pair))
                    .collect::<Result<Vec<Value>>>()?,
            };
            let body = ctx.client.put(PATH, &json!({ "areas": areas }))?.body;
            print(ctx, &body)
        }
    }
}

fn print(ctx: &Context, body: &Value) -> Result<()> {
    let rows = body
        .get("results")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    ctx.printer.list(rows, None, COLUMNS)
}

fn parse_area(pair: &str) -> Result<Value> {
    match pair.split_once('=') {
        Some((attribute, value)) if !attribute.trim().is_empty() && !value.trim().is_empty() => {
            Ok(json!({ "attribute": attribute.trim(), "value": value.trim() }))
        }
        _ => Err(Error::Usage(format!(
            "an area is ATTRIBUTE=VALUE (e.g. city=Brentwood), got {pair:?}"
        ))),
    }
}

fn read_json(source: &str) -> Result<Vec<Value>> {
    let text = if let Some(path) = source.strip_prefix('@') {
        html::read_file_or_stdin(path)?
    } else if source == "-" {
        html::read_file_or_stdin("-")?
    } else {
        source.to_string()
    };
    match serde_json::from_str::<Value>(&text)? {
        Value::Array(items) => Ok(items),
        Value::Object(map) if map.get("areas").is_some_and(Value::is_array) => {
            Ok(map["areas"].as_array().cloned().unwrap_or_default())
        }
        _ => Err(Error::Usage(
            "--data must be a JSON list of {\"attribute\", \"value\"}, or {\"areas\": [...]}"
                .into(),
        )),
    }
}
