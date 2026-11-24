//! `geekcli valuation-pages …` — home valuation landing pages: a hero
//! with a headline and an address box that starts the site's seller lead
//! flow ("What's your home worth?").

use clap::{Args, Subcommand};
use serde_json::Value;

use super::pages::{self, TreeListArgs};
use super::{list_and_print, print_written, Context, Payload};
use crate::error::Result;
use crate::html;
use crate::output::{col, Column};

pub const PATH: &str = "content/valuation-pages/";
const LABEL: &str = "valuation page";

pub const LIST_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("path", "/path"),
    col("anchor_text", "/anchor_text"),
    col("title", "/title"),
];

pub const DETAIL_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("path", "/path"),
    col("url", "/url"),
    col("anchor_text", "/anchor_text"),
    col("title", "/title"),
    col("search_input_placeholder", "/search_input_placeholder"),
    col("landscape", "/landscape_image_override"),
    col("search_headline", "/search_headline"),
];

/// The note both write commands carry; a macro so `concat!` can use it.
macro_rules! seller_leads_note {
    () => {
        "  - The pages only show while the site has Seller Leads (the home valuation tool) on. While it is off they answer 404, reads print a `valuation_disabled` warning and writes fail with `valuation_disabled` (exit 6). Only a site admin can turn it on, at /admin/valuation/: tell your human."
    };
}

#[derive(Debug, Args)]
pub struct ValuationPagesCommand {
    #[command(subcommand)]
    pub command: ValuationPagesSub,
}

#[derive(Debug, Subcommand)]
pub enum ValuationPagesSub {
    /// List home valuation pages
    List(TreeListArgs),
    /// Show one by id, path or slug
    Get { reference: String },
    /// Create a home valuation page
    #[command(after_help = concat!("Notes:
  - --slug and --anchor-text are required. Without --search-headline the page gets the standard \"What's Your Home Worth?\" headline.
  - The page is live at once; there is no draft.
  - The layout is fixed: a hero image (--landscape), the headline and the address box. No content, template, sidebar, footer, banner or search.
  - These pages are separate from `pages`: neither list shows the other.
", seller_leads_note!()))]
    Create(Fields),
    /// Change fields on a home valuation page (only the flags you pass are changed)
    #[command(after_help = concat!("Notes:\n", seller_leads_note!()))]
    Update(UpdateArgs),
    /// Delete a home valuation page
    Delete {
        reference: String,
        #[arg(long)]
        orphan_children: bool,
    },
    /// List revisions, newest first
    Revisions {
        reference: String,
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
    },
    /// Show one revision with what a revert would restore
    Revision { reference: String, rev: u64 },
    /// Undo a revision and everything after it
    Revert { reference: String, rev: u64 },
}

#[derive(Debug, Args)]
pub struct Fields {
    /// URL segment; letters, numbers, `-`, `_`
    #[arg(long)]
    #[arg(help_heading = super::heading::REQUIRED)]
    pub slug: Option<String>,
    /// Page name used in navigation (required on create)
    #[arg(long)]
    #[arg(help_heading = super::heading::REQUIRED)]
    pub anchor_text: Option<String>,
    /// Parent page: id, path (/resources/), slug, or `null` for top level
    #[arg(long, value_name = "ID_PATH_OR_NULL")]
    #[arg(help_heading = super::heading::CONTENT)]
    pub parent: Option<String>,
    /// The text over the address box, as inline HTML (or Markdown with --markdown)
    #[arg(long, value_name = "HTML", alias = "headline")]
    #[arg(help_heading = super::heading::CONTENT)]
    pub search_headline: Option<String>,
    /// Read the headline from a file, or `-` for stdin; `.md` files are converted from Markdown
    #[arg(long, value_name = "FILE", alias = "headline-file")]
    #[arg(help_heading = super::heading::CONTENT)]
    pub search_headline_file: Option<String>,
    /// Treat the headline as Markdown and convert it to HTML
    #[arg(long)]
    #[arg(help_heading = super::heading::CONTENT)]
    pub markdown: bool,
    /// Example text in the address box, up to 50 characters; `""` for none
    #[arg(long, value_name = "TEXT", alias = "placeholder")]
    #[arg(help_heading = super::heading::CONTENT)]
    pub search_input_placeholder: Option<String>,
    /// HTML <title>
    #[arg(long)]
    #[arg(help_heading = super::heading::SEO)]
    pub title: Option<String>,
    /// SEO description
    #[arg(long)]
    #[arg(help_heading = super::heading::SEO)]
    pub meta_description: Option<String>,
    /// SEO keywords
    #[arg(long)]
    #[arg(help_heading = super::heading::SEO)]
    pub meta_keywords: Option<String>,
    /// Hero image: a file URL (.jpg/.png/.mp4), `none` to hide it, or `null` for the site's
    #[arg(long, value_name = "URL_NONE_OR_NULL")]
    #[arg(help_heading = super::heading::LAYOUT)]
    pub landscape: Option<String>,
    /// Alt text for the hero image
    #[arg(long, value_name = "TEXT")]
    #[arg(help_heading = super::heading::LAYOUT)]
    pub landscape_alt: Option<String>,
    /// Override type when the extension is ambiguous: image or video
    #[arg(long, value_name = "TYPE")]
    #[arg(help_heading = super::heading::LAYOUT)]
    pub landscape_content_type: Option<String>,
    /// Extra fields as a JSON object, `@file`, or `-` for stdin (flags win)
    #[arg(long, value_name = "JSON")]
    pub data: Option<String>,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Page id, path or slug
    pub reference: String,
    #[command(flatten)]
    pub fields: Fields,
    /// Send a full replace (PUT): fields you omit reset to their defaults
    #[arg(long)]
    pub replace: bool,
}

impl Fields {
    fn payload(&self, ctx: &Context) -> Result<Payload> {
        let mut payload = Payload::default();
        payload
            .set("slug", self.slug.as_deref().map(|s| s.trim_matches('/')))
            .set("anchor_text", self.anchor_text.as_deref())
            .set("title", self.title.as_deref())
            .set("meta_description", self.meta_description.as_deref())
            .set("meta_keywords", self.meta_keywords.as_deref())
            .set(
                "search_input_placeholder",
                self.search_input_placeholder.as_deref(),
            );
        if let Some(parent) = &self.parent {
            payload.set_value("parent", pages::resolve_parent(ctx, PATH, parent)?);
        }
        if let Some(headline) = html::read_body(
            self.search_headline.as_deref(),
            self.search_headline_file.as_deref(),
            self.markdown,
        )? {
            payload.set("search_headline", Some(headline));
        }
        if let Some(landscape) = &self.landscape {
            payload.set_value(
                "landscape_image_override",
                match landscape.trim().to_ascii_lowercase().as_str() {
                    "null" | "default" | "-" => Value::Null,
                    "none" | "hide" | "hidden" => Value::String("none".into()),
                    _ => Value::String(landscape.trim().to_string()),
                },
            );
        }
        payload
            .set(
                "landscape_image_override_alt_text",
                self.landscape_alt.as_deref(),
            )
            .set(
                "landscape_image_override_content_type",
                self.landscape_content_type.as_deref(),
            );
        payload.merge_json(self.data.as_deref())?;
        Ok(payload)
    }
}

pub fn run(ctx: &Context, cmd: ValuationPagesCommand) -> Result<()> {
    match cmd.command {
        ValuationPagesSub::List(args) => {
            list_and_print(ctx, PATH, &args.query(), &args.paging, LIST_COLUMNS)
        }
        ValuationPagesSub::Get { reference } => ctx.printer.one(
            &pages::resolve(ctx, PATH, &reference, LABEL)?,
            DETAIL_COLUMNS,
        ),
        ValuationPagesSub::Create(fields) => {
            let payload = fields.payload(ctx)?;
            pages::require(&payload, &["slug", "anchor_text"], LABEL)?;
            let created = ctx.client.post(PATH, &payload.into_value())?.body;
            print_written(ctx, &created, DETAIL_COLUMNS, "Created")
        }
        ValuationPagesSub::Update(args) => {
            let id = pages::resolve_id(ctx, PATH, &args.reference, LABEL)?;
            let payload = args.fields.payload(ctx)?;
            pages::write(
                ctx,
                &pages::detail_path(PATH, id),
                payload,
                args.replace,
                DETAIL_COLUMNS,
            )
        }
        ValuationPagesSub::Delete {
            reference,
            orphan_children,
        } => pages::delete(
            ctx,
            PATH,
            &reference,
            LABEL,
            orphan_children.then_some("orphan_children"),
        ),
        ValuationPagesSub::Revisions { reference, limit } => {
            let id = pages::resolve_id(ctx, PATH, &reference, LABEL)?;
            super::revisions::list(ctx, &pages::detail_path(PATH, id), limit)
        }
        ValuationPagesSub::Revision { reference, rev } => {
            let id = pages::resolve_id(ctx, PATH, &reference, LABEL)?;
            super::revisions::show(ctx, &pages::detail_path(PATH, id), rev)
        }
        ValuationPagesSub::Revert { reference, rev } => {
            let id = pages::resolve_id(ctx, PATH, &reference, LABEL)?;
            super::revisions::revert(ctx, &pages::detail_path(PATH, id), rev, LABEL)
        }
    }
}
