//! `geekcli templates list` — page templates available to this site.

use clap::{Args, Subcommand};
use serde_json::Value;

use super::Context;
use crate::client::Query;
use crate::error::Result;
use crate::output::{col, Column};

pub const PATH: &str = "content/templates/";

pub const COLUMNS: &[Column] = &[
    col("name", "/name"),
    col("description", "/description"),
    col("extra_content_areas", "/extra_content_areas"),
];

#[derive(Debug, Args)]
pub struct TemplatesCommand {
    #[command(subcommand)]
    pub command: TemplatesSub,
}

#[derive(Debug, Subcommand)]
pub enum TemplatesSub {
    /// List the page templates this site can use
    List,
}

pub fn run(ctx: &Context, cmd: &TemplatesCommand) -> Result<()> {
    match cmd.command {
        TemplatesSub::List => {
            let response = ctx.client.get(PATH, &Query::new())?;
            let items = response
                .body
                .get("results")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            ctx.printer.list(&items, None, COLUMNS)
        }
    }
}
