//! `geekcli market-reports …` — market report pages: market statistics
//! and tables of active, pending and sold listings for one search.

use clap::{Args, Subcommand};
use serde_json::Value;

use super::pages::{self, TreeListArgs};
use super::{list_and_print, parse_id_or_null, print_written, Context, Payload};
use crate::error::{Error, Result};
use crate::html;
use crate::output::{col, Column};

pub const PATH: &str = "content/market-report-pages/";
const LABEL: &str = "market report";

pub const LIST_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("path", "/path"),
    col("anchor_text", "/anchor_text"),
    col("search", "/search/description"),
    col("sold_within", "/sold_within"),
];

pub const DETAIL_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("path", "/path"),
    col("url", "/url"),
    col("anchor_text", "/anchor_text"),
    col("title", "/title"),
    col("header", "/header"),
    col("search", "/search/description"),
    col("sold_within", "/sold_within"),
    col("number_of_properties", "/number_of_properties"),
    col("footer", "/footer/name"),
    col("content", "/content"),
];

#[derive(Debug, Args)]
pub struct MarketReportsCommand {
    #[command(subcommand)]
    pub command: MarketReportsSub,
}

#[derive(Debug, Subcommand)]
pub enum MarketReportsSub {
    /// List market report pages
    List(TreeListArgs),
    /// Show one by id, path or slug
    Get { reference: String },
    /// Create a market report page
    #[command(after_help = "Notes:
  - --slug, --anchor-text and a search (--search-criteria or --search) are required: the statistics and listing tables come from the search. Check it first with `search check --count --strict`.
  - The page is live at once; there is no draft.
  - --sold-within is months of sold listings (1-6, 12 or 18; default 6); --number-of-properties is rows per table (1-50; default 6).
  - No template, sidebar, banner or landscape options: a market report has its own layout.
  - These pages are separate from `pages`: neither list shows the other.")]
    Create(Fields),
    /// Change fields on a market report page (only the flags you pass are changed)
    Update(UpdateArgs),
    /// Delete a market report page
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
    /// The area the report covers: a saved search's short id or numeric id
    #[arg(long, value_name = "ID", conflicts_with = "search_criteria")]
    #[arg(help_heading = super::heading::REQUIRED)]
    pub search: Option<String>,
    /// Build the report's search from criteria, key=value; repeatable (see `search fields`)
    #[arg(long = "search-criteria", value_name = "KEY=VALUE")]
    #[arg(help_heading = super::heading::REQUIRED)]
    pub search_criteria: Vec<String>,
    /// Parent page: id, path (/resources/), slug, or `null` for top level
    #[arg(long, value_name = "ID_PATH_OR_NULL")]
    #[arg(help_heading = super::heading::CONTENT)]
    pub parent: Option<String>,
    /// Text at the top of the report, above the sign-up call to action
    #[arg(long, value_name = "TEXT")]
    #[arg(help_heading = super::heading::CONTENT)]
    pub header: Option<String>,
    /// Content as inline HTML (or Markdown with --markdown)
    #[arg(long, value_name = "HTML", aliases = ["body", "html"])]
    #[arg(help_heading = super::heading::CONTENT)]
    pub content: Option<String>,
    /// Read the content from a file, or `-` for stdin; `.md` files are converted from Markdown
    #[arg(long, value_name = "FILE", aliases = ["body-file", "html-file"])]
    #[arg(help_heading = super::heading::CONTENT)]
    pub content_file: Option<String>,
    /// Treat the content as Markdown and convert it to HTML
    #[arg(long)]
    #[arg(help_heading = super::heading::CONTENT)]
    pub markdown: bool,
    /// Months of sold listings in the Sold table: 1-6, 12 or 18 (default 6)
    #[arg(long, value_name = "MONTHS")]
    #[arg(help_heading = super::heading::SEARCH)]
    pub sold_within: Option<u32>,
    /// Rows in each listings table, 1-50 (default 6)
    #[arg(long)]
    #[arg(help_heading = super::heading::SEARCH)]
    pub number_of_properties: Option<u32>,
    /// Saved search whose criteria pre-fill the search form: short id, numeric id, or `null`
    #[arg(
        long,
        value_name = "ID_OR_NULL",
        conflicts_with = "search_field_defaults_criteria"
    )]
    #[arg(help_heading = super::heading::SEARCH)]
    pub search_field_defaults: Option<String>,
    /// Pre-fill the search form from criteria, key=value; repeatable
    #[arg(long = "search-field-defaults-criteria", value_name = "KEY=VALUE")]
    #[arg(help_heading = super::heading::SEARCH)]
    pub search_field_defaults_criteria: Vec<String>,
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
    /// Footer to show: id, name, or `null` for the default
    #[arg(long, value_name = "ID_NAME_OR_NULL")]
    #[arg(help_heading = super::heading::LAYOUT)]
    pub footer: Option<String>,
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
            .set("header", self.header.as_deref())
            .set("sold_within", self.sold_within)
            .set("number_of_properties", self.number_of_properties);
        if let Some(parent) = &self.parent {
            let value = match parse_id_or_null(parent) {
                Ok(v) => v,
                Err(_) => Value::from(
                    pages::resolve_id(ctx, PATH, parent, "parent page").or_else(|first| {
                        pages::resolve_id(ctx, pages::PATH, parent, "parent page")
                            .map_err(|_| first)
                    })?,
                ),
            };
            payload.set_value("parent", value);
        }
        if let Some(content) = html::read_body(
            self.content.as_deref(),
            self.content_file.as_deref(),
            self.markdown,
        )? {
            payload.set("content", Some(content));
        }
        if let Some(search) = &self.search {
            payload.set_value("search", pages::search_ref(search));
        }
        if !self.search_criteria.is_empty() {
            payload.set_value("search", pages::criteria_object(&self.search_criteria)?);
        }
        if let Some(defaults) = &self.search_field_defaults {
            payload.set_value("search_field_defaults", pages::search_ref(defaults));
        }
        if !self.search_field_defaults_criteria.is_empty() {
            payload.set_value(
                "search_field_defaults",
                pages::criteria_object(&self.search_field_defaults_criteria)?,
            );
        }
        if let Some(footer) = &self.footer {
            let value = match parse_id_or_null(footer) {
                Ok(v) => v,
                Err(_) => Value::from(super::id_of(&super::footers::resolve(ctx, footer)?)?),
            };
            payload.set_value("footer", value);
        }
        payload.merge_json(self.data.as_deref())?;
        Ok(payload)
    }
}

pub fn run(ctx: &Context, cmd: MarketReportsCommand) -> Result<()> {
    match cmd.command {
        MarketReportsSub::List(args) => {
            list_and_print(ctx, PATH, &args.query(), &args.paging, LIST_COLUMNS)
        }
        MarketReportsSub::Get { reference } => ctx.printer.one(
            &pages::resolve(ctx, PATH, &reference, LABEL)?,
            DETAIL_COLUMNS,
        ),
        MarketReportsSub::Create(fields) => {
            let payload = fields.payload(ctx)?;
            pages::require(&payload, &["slug", "anchor_text"], LABEL)?;
            if !payload.contains("search") {
                return Err(Error::Usage(
                    "--search-criteria or --search is required to create a market report".into(),
                ));
            }
            let created = ctx.client.post(PATH, &payload.into_value())?.body;
            print_written(ctx, &created, DETAIL_COLUMNS, "Created")
        }
        MarketReportsSub::Update(args) => {
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
        MarketReportsSub::Delete {
            reference,
            orphan_children,
        } => pages::delete(
            ctx,
            PATH,
            &reference,
            LABEL,
            orphan_children.then_some("orphan_children"),
        ),
        MarketReportsSub::Revisions { reference, limit } => {
            let id = pages::resolve_id(ctx, PATH, &reference, LABEL)?;
            super::revisions::list(ctx, &pages::detail_path(PATH, id), limit)
        }
        MarketReportsSub::Revision { reference, rev } => {
            let id = pages::resolve_id(ctx, PATH, &reference, LABEL)?;
            super::revisions::show(ctx, &pages::detail_path(PATH, id), rev)
        }
        MarketReportsSub::Revert { reference, rev } => {
            let id = pages::resolve_id(ctx, PATH, &reference, LABEL)?;
            super::revisions::revert(ctx, &pages::detail_path(PATH, id), rev, LABEL)
        }
    }
}
