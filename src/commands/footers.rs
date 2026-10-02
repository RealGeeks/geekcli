//! `geekcli footers …` — blocks of HTML that pages share in the footer's
//! third column. Footer 1 is the site default.

use clap::{Args, Subcommand};
use serde_json::{json, Value};

use super::{confirm, id_of, print_written, Context};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::html;
use crate::output::{cell, col, Column, Format};

pub const PATH: &str = "content/footers/";

pub const COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("name", "/name"),
    col("default", "/default"),
    col("used_by_count", "/used_by_count"),
    col("used_by", "/used_by"),
    col("content", "/content"),
];

#[derive(Debug, Args)]
pub struct FootersCommand {
    #[command(subcommand)]
    pub command: FootersSub,
}

#[derive(Debug, Subcommand)]
pub enum FootersSub {
    /// List footers
    List,
    /// Show a footer by id or name (with the pages that use it)
    Get { reference: String },
    /// Create a footer from HTML (or Markdown)
    Create(ContentArgs),
    /// Replace a footer's content
    #[command(after_help = "Notes:
  - Footer 1 is what every page shows unless a page has its own (--footer on pages update).
  - Scripts are stripped; use the logo's u.realgeeks.media URL from `files upload -q`.
  - Same HTML rules as posts.")]
    Update(UpdateArgs),
    /// Delete a footer (the default footer cannot be deleted)
    Delete {
        reference: String,
        /// Delete even if pages use it; they fall back to the default
        #[arg(long)]
        force: bool,
    },
    /// List a footer's revisions, newest first
    Revisions {
        /// Footer id or name
        reference: String,
        /// Show only the latest N
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
    },
    /// Show one revision with what a revert would restore
    Revision { reference: String, rev: u64 },
    /// Undo a revision and everything after it (the revert is itself undoable)
    Revert { reference: String, rev: u64 },
}

#[derive(Debug, Args)]
pub struct ContentArgs {
    /// Inline HTML (or Markdown with --markdown)
    #[arg(long, value_name = "HTML", aliases = ["body", "html"])]
    pub content: Option<String>,
    /// Read the content from a file, or `-` for stdin; `.md` converts from Markdown
    #[arg(long, value_name = "FILE", aliases = ["body-file", "html-file"])]
    pub content_file: Option<String>,
    #[arg(long)]
    pub markdown: bool,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Footer id or name
    pub reference: String,
    #[command(flatten)]
    pub content: ContentArgs,
}

pub fn run(ctx: &Context, cmd: FootersCommand) -> Result<()> {
    match cmd.command {
        FootersSub::List => {
            let body = ctx.client.get(PATH, &Query::new())?.body;
            let rows = body
                .get("results")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            ctx.printer.list(&rows, None, COLUMNS)
        }
        FootersSub::Get { reference } => {
            let footer = resolve(ctx, &reference)?;
            if ctx.printer.format == Format::Table {
                // the table truncates content; show it whole below
                ctx.printer.one(&footer, &COLUMNS[..5])?;
                println!("{}", cell(footer.get("content").unwrap_or(&Value::Null)));
                return Ok(());
            }
            ctx.printer.raw(&footer)
        }
        FootersSub::Create(args) => {
            let content = read_content(&args)?;
            let created = ctx.client.post(PATH, &json!({ "content": content }))?.body;
            print_written(ctx, &created, COLUMNS, "Created")
        }
        FootersSub::Update(args) => {
            let id = id_of(&resolve(ctx, &args.reference)?)?;
            let content = read_content(&args.content)?;
            let updated = ctx
                .client
                .patch(&detail_path(id), &json!({ "content": content }))?
                .body;
            print_written(ctx, &updated, COLUMNS, "Updated")
        }
        FootersSub::Delete { reference, force } => {
            let footer = resolve(ctx, &reference)?;
            let id = id_of(&footer)?;
            if !confirm(ctx, &format!("footer {id} \"{}\"", cell(&footer["name"])))? {
                return Err(Error::Usage("cancelled".into()));
            }
            let mut query = Query::new();
            if force {
                query.push(("force".into(), "true".into()));
            }
            ctx.client.delete(&detail_path(id), &query)?;
            ctx.printer.note(&format!("Deleted footer {id}"));
            if ctx.printer.format != Format::Table {
                ctx.printer.raw(&json!({ "deleted": true, "id": id }))?;
            }
            Ok(())
        }
        FootersSub::Revisions { reference, limit } => {
            let id = id_of(&resolve(ctx, &reference)?)?;
            super::revisions::list(ctx, &detail_path(id), limit)
        }
        FootersSub::Revision { reference, rev } => {
            let id = id_of(&resolve(ctx, &reference)?)?;
            super::revisions::show(ctx, &detail_path(id), rev)
        }
        FootersSub::Revert { reference, rev } => {
            let footer = resolve(ctx, &reference)?;
            let id = id_of(&footer)?;
            let label = format!("footer {id} \"{}\"", cell(&footer["name"]));
            super::revisions::revert(ctx, &detail_path(id), rev, &label)
        }
    }
}

pub fn detail_path(id: u64) -> String {
    format!("{PATH}{id}/")
}

fn read_content(args: &ContentArgs) -> Result<String> {
    html::read_body(
        args.content.as_deref(),
        args.content_file.as_deref(),
        args.markdown,
    )?
    .ok_or_else(|| Error::Usage("pass --content or --content-file".into()))
}

/// A footer by numeric id or by name (case-insensitive).
pub fn resolve(ctx: &Context, reference: &str) -> Result<Value> {
    let reference = reference.trim();
    if let Ok(id) = reference.parse::<u64>() {
        return Ok(ctx.client.get(&detail_path(id), &Query::new())?.body);
    }
    let wanted = reference.to_ascii_lowercase();
    let list = ctx.client.get(PATH, &Query::new())?.body;
    let found = list
        .get("results")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|f| {
                    f.get("name")
                        .and_then(Value::as_str)
                        .map(str::to_ascii_lowercase)
                        == Some(wanted.clone())
                })
                .cloned()
        })
        .ok_or_else(|| Error::Api {
            status: 404,
            code: "not_found".into(),
            message: format!("no footer named '{reference}'"),
            fields: std::collections::BTreeMap::new(),
            retry_after: None,
        })?;
    Ok(ctx
        .client
        .get(&detail_path(id_of(&found)?), &Query::new())?
        .body)
}
