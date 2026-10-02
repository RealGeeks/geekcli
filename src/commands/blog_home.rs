//! `geekcli blog …` — the blog's landing page: its title, meta and the
//! HTML shown above the post list. `PATCH` creates it if the site has none.

use clap::{Args, Subcommand};

use super::{print_written, Context, Payload};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::html;
use crate::output::{col, Column};

pub const PATH: &str = "blog/home-page/";

pub const COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("url", "/url"),
    col("title", "/title"),
    col("meta_description", "/meta_description"),
    col("meta_keywords", "/meta_keywords"),
];

#[derive(Debug, Args)]
pub struct BlogCommand {
    #[command(subcommand)]
    pub command: BlogSub,
}

#[derive(Debug, Subcommand)]
pub enum BlogSub {
    /// Show the blog landing page (title, meta, content)
    Get,
    /// Change the blog landing page; created if the site has none yet
    #[command(after_help = "Notes:
  - The heading visitors see above the post list is the `content` (usually one <h2>); `title` is the browser/SEO title.
  - Same HTML rules as posts (`geekcli guide html`). Posts themselves are `geekcli posts …`.")]
    Update(UpdateArgs),
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Browser and SEO title (127 characters max)
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub meta_description: Option<String>,
    #[arg(long)]
    pub meta_keywords: Option<String>,
    /// HTML shown above the post list (or Markdown with --markdown)
    #[arg(long, value_name = "HTML")]
    pub content: Option<String>,
    /// Read the content from a file, or `-` for stdin; `.md` converts from Markdown
    #[arg(long, value_name = "FILE")]
    pub content_file: Option<String>,
    #[arg(long)]
    pub markdown: bool,
    /// Extra fields as a JSON object, `@file`, or `-` for stdin (flags win)
    #[arg(long, value_name = "JSON")]
    pub data: Option<String>,
}

pub fn run(ctx: &Context, cmd: BlogCommand) -> Result<()> {
    match cmd.command {
        BlogSub::Get => ctx
            .printer
            .one(&ctx.client.get(PATH, &Query::new())?.body, COLUMNS),
        BlogSub::Update(args) => {
            let mut payload = Payload::default();
            payload
                .set("title", args.title.as_deref())
                .set("meta_description", args.meta_description.as_deref())
                .set("meta_keywords", args.meta_keywords.as_deref());
            if let Some(content) = html::read_body(
                args.content.as_deref(),
                args.content_file.as_deref(),
                args.markdown,
            )? {
                payload.set("content", Some(content));
            }
            payload.merge_json(args.data.as_deref())?;
            if payload.is_empty() {
                return Err(Error::Usage(
                    "nothing to update: pass at least one field flag or --data".into(),
                ));
            }
            let updated = ctx.client.patch(PATH, &payload.into_value())?.body;
            print_written(ctx, &updated, COLUMNS, "Updated")
        }
    }
}
