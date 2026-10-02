//! `geekcli home-page …` — the site's single home page.

use clap::{Args, Subcommand};

use super::pages::DisplayLocation;
use super::{parse_bool, print_written, Context, Payload};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::html;
use crate::output::{col, Column};

pub const PATH: &str = "content/home-page/";

pub const COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("url", "/url"),
    col("title", "/title"),
    col("search_header", "/search_header"),
    col("search_subheader", "/search_subheader"),
    col("listing_header", "/listing_header"),
    col("sidebar", "/sidebar/name"),
    col("footer", "/footer/name"),
    col("search", "/search/description"),
    col("tile_group", "/tile_group/title"),
    col("landscape", "/landscape_image_override"),
];

#[derive(Debug, Args)]
pub struct HomePageCommand {
    #[command(subcommand)]
    pub command: HomePageSub,
}

#[derive(Debug, Subcommand)]
pub enum HomePageSub {
    /// Show the home page
    Get,
    /// List the home page's revisions, newest first
    Revisions {
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
    },
    /// Show one revision with what a revert would restore
    Revision { rev: u64 },
    /// Undo a revision and everything after it
    Revert { rev: u64 },
    /// Change fields on the home page (only the flags you pass are changed)
    #[command(after_help = "Notes:
  - --search-criteria drives the listings strip; --search-field-defaults-criteria pre-fills the search form (county, price floor, types).
  - --property-display-type, --search-form-tabs and --tile-group render on anna-modern only; build the tiles with `geekcli featured` first.
  - --search-header (alias --page-heading) is the page's main hero heading, not a small label; unset, the site shows the BIG_SEARCH_TITLE setting (\"Real Estate Search\"). Use the page's target keyword.
  - --landscape takes a file URL (an .mp4 becomes a video), `none` to hide the header image, or `null` for the site's; `geekcli guide featured`.
  - The hero background is --landscape, or the sitewide HEADER_IMAGE setting when the page has none. --search-image is not a background: it renders as an image (logo-style) inside the hero.
  - Content rules: `geekcli guide html`.
  - Snapshot with --full and --mobile after changes.")]
    Update(Box<UpdateArgs>),
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// HTML <title>
    #[arg(long)]
    pub title: Option<String>,
    /// SEO description
    #[arg(long)]
    pub meta_description: Option<String>,
    /// SEO keywords
    #[arg(long)]
    pub meta_keywords: Option<String>,
    /// Content as inline HTML (or Markdown with --markdown)
    #[arg(long, value_name = "HTML", aliases = ["body", "html"])]
    pub content: Option<String>,
    /// Read the content from a file, or `-` for stdin; `.md` files are converted from Markdown
    #[arg(long, value_name = "FILE", aliases = ["body-file", "html-file"])]
    pub content_file: Option<String>,
    /// Treat the content as Markdown and convert it to HTML
    #[arg(long)]
    pub markdown: bool,
    /// The page's main hero heading (above the search form); unset, the site shows the BIG_SEARCH_TITLE setting ("Real Estate Search"). Use the page's target keyword
    #[arg(long, visible_alias = "page-heading", value_name = "TEXT")]
    pub search_header: Option<String>,
    /// Text under the search heading
    #[arg(long)]
    pub search_subheader: Option<String>,
    /// Heading above the property listings
    #[arg(long)]
    pub listing_header: Option<String>,
    /// How many listings to show
    #[arg(long)]
    pub number_of_properties: Option<u32>,
    /// Show listings above or below the content
    #[arg(long, value_enum)]
    pub property_display_location: Option<DisplayLocation>,
    #[command(flatten)]
    pub attach: super::pages::AttachArgs,
    /// How listings display: carousel, grid, or `null` (anna-modern designs)
    #[arg(long, value_name = "TYPE")]
    pub property_display_type: Option<String>,
    /// Show the Advanced Search / Sell Your Home tabs above the form (anna-modern)
    #[arg(long, value_name = "BOOL", value_parser = parse_bool)]
    pub search_form_tabs: Option<bool>,
    /// Image shown in the hero (logo-style, not the background): a file URL, or `null` to clear
    #[arg(long, value_name = "URL_OR_NULL")]
    pub search_image: Option<String>,
    /// Featured Pages tile group to show (anna-modern): id, title, or `null`
    #[arg(long, value_name = "ID_TITLE_OR_NULL")]
    pub tile_group: Option<String>,
    /// Extra fields as a JSON object, `@file`, or `-` for stdin (flags win)
    #[arg(long, value_name = "JSON")]
    pub data: Option<String>,
}

pub fn run(ctx: &Context, cmd: HomePageCommand) -> Result<()> {
    match cmd.command {
        HomePageSub::Revisions { limit } => super::revisions::list(ctx, PATH, limit),
        HomePageSub::Revision { rev } => super::revisions::show(ctx, PATH, rev),
        HomePageSub::Revert { rev } => super::revisions::revert(ctx, PATH, rev, "the home page"),
        HomePageSub::Get => ctx
            .printer
            .one(&ctx.client.get(PATH, &Query::new())?.body, COLUMNS),
        HomePageSub::Update(args) => {
            let mut payload = Payload::default();
            payload
                .set("title", args.title.as_deref())
                .set("meta_description", args.meta_description.as_deref())
                .set("meta_keywords", args.meta_keywords.as_deref())
                .set("search_header", args.search_header.as_deref())
                .set("search_subheader", args.search_subheader.as_deref())
                .set("listing_header", args.listing_header.as_deref())
                .set("number_of_properties", args.number_of_properties)
                .set(
                    "property_display_location",
                    args.property_display_location.map(DisplayLocation::as_str),
                );
            if let Some(content) = html::read_body(
                args.content.as_deref(),
                args.content_file.as_deref(),
                args.markdown,
            )? {
                payload.set("content", Some(content));
            }
            args.attach.apply(ctx, &mut payload)?;
            if let Some(kind) = &args.property_display_type {
                payload.set_value(
                    "property_display_type",
                    match kind.trim().to_ascii_lowercase().as_str() {
                        "null" | "none" | "-" => serde_json::Value::Null,
                        other => serde_json::Value::String(other.to_string()),
                    },
                );
            }
            payload.set("search_form_tabs", args.search_form_tabs);
            if let Some(image) = &args.search_image {
                payload.set_value(
                    "search_image",
                    match image.trim().to_ascii_lowercase().as_str() {
                        "null" | "none" | "-" => serde_json::Value::Null,
                        _ => serde_json::Value::String(image.trim().to_string()),
                    },
                );
            }
            if let Some(group) = &args.tile_group {
                let value = match super::parse_id_or_null(group) {
                    Ok(v) => v,
                    Err(_) => serde_json::Value::from(super::id_of(&super::featured::resolve(
                        ctx, group,
                    )?)?),
                };
                payload.set_value("tile_group", value);
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
