//! `geekcli area-pages …` — community/neighbourhood pages. Same tree as content
//! pages, plus `area_name` and `featured`.

use clap::{Args, Subcommand};
use serde_json::Value;

use super::pages::{self, TreeFields, TreeListArgs, AREA_PATH};
use super::{id_of, list_and_print, parse_bool, print_written, push, Context};
use crate::error::{Error, Result};
use crate::output::{col, Column};

pub const LIST_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("path", "/path"),
    col("area_name", "/area_name"),
    col("anchor_text", "/anchor_text"),
    col("featured", "/featured"),
    col("children", "/children_count"),
];

pub const DETAIL_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("path", "/path"),
    col("url", "/url"),
    col("area_name", "/area_name"),
    col("anchor_text", "/anchor_text"),
    col("title", "/title"),
    col("featured", "/featured"),
    col("parent", "/parent"),
    col("level", "/level"),
    col("sidebar", "/sidebar/name"),
    col("footer", "/footer/name"),
    col("search", "/search/description"),
    col("landscape", "/landscape_image_override"),
];

#[derive(Debug, Args)]
pub struct AreaPagesCommand {
    #[command(subcommand)]
    pub command: AreaPagesSub,
}

#[derive(Debug, Subcommand)]
pub enum AreaPagesSub {
    /// List area pages
    List(ListArgs),
    /// Show one area page by id, path or slug
    Get { reference: String },
    #[command(after_help = "Notes:
  - --area-name is display text only (the listing header, titles). It does not decide which listings the page shows.
  - --search-criteria (or --search with a saved search id) is what scopes the listings. Common keys are city, county, subdivision and zip, but names and values are site specific: check them with `search fields` and `search choices <field>`.
  - Run `search check` and `search run` with the same criteria first to confirm the keys are understood and listings come back.
  - Create fails without --search-criteria or --search; pass --no-search to create a page whose listings are not scoped to the area.
  - There is no draft state: the page is public as soon as it is created.
  - --parent takes an id, a /path/ or a slug.
  - Snapshot the page afterwards.")]
    /// Create an area page (public immediately)
    Create(CreateArgs),
    #[command(after_help = "Notes:
  - Only the flags you pass change.
  - --area-name is display text only (the listing header, titles); changing it does not change which listings the page shows.
  - --search-criteria replaces the page's saved search, which is what scopes the listings (common keys: city, county, subdivision, zip; check them with `search fields`, then `search check` and `search run`).
  - Changes are live immediately; area pages have no draft state.")]
    /// Change fields on an area page (changes are live immediately)
    Update(UpdateArgs),
    /// Delete an area page
    Delete {
        reference: String,
        /// Delete even if the page has children; they become top-level pages
        #[arg(long)]
        orphan_children: bool,
    },
    /// Show the saved property search an area page displays (its search_id)
    Search { reference: String },
    /// List an area page's revisions, newest first
    Revisions {
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
pub struct ListArgs {
    #[command(flatten)]
    pub tree: TreeListArgs,
    /// Only featured (true) or non-featured (false) areas
    #[arg(long, value_name = "BOOL", value_parser = parse_bool)]
    pub featured: Option<bool>,
}

#[derive(Debug, Args)]
pub struct FieldArgs {
    #[command(flatten)]
    pub fields: TreeFields,
    /// Display name of the area for headers and titles (required on create); a label only, it does not filter listings
    #[arg(long)]
    pub area_name: Option<String>,
    #[arg(long, value_name = "BOOL", value_parser = parse_bool)]
    pub featured: Option<bool>,
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[command(flatten)]
    pub fields: FieldArgs,
    /// Create the page without a search, so its listings are not scoped to the area
    #[arg(long, conflicts_with_all = ["search", "search_criteria"])]
    pub no_search: bool,
}

const NO_SEARCH: &str = "an area page needs a search to scope its listings: --area-name is only a label. \
Pass --search-criteria KEY=VALUE (for example city=… or subdivision=…; check keys with `search fields` and \
results with `search run`) or --search <saved search id>, or --no-search to create it without one. \
The page is public as soon as it is created";

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Area page id, path or slug
    pub reference: String,
    #[command(flatten)]
    pub fields: FieldArgs,
    /// Send a full replace (PUT): fields you omit reset to their defaults
    #[arg(long)]
    pub replace: bool,
}

pub fn run(ctx: &Context, cmd: AreaPagesCommand) -> Result<()> {
    match cmd.command {
        AreaPagesSub::List(args) => {
            let mut query = args.tree.query();
            push(
                &mut query,
                "featured",
                args.featured.map(|f| f.to_string()).as_deref(),
            );
            list_and_print(ctx, AREA_PATH, &query, &args.tree.paging, LIST_COLUMNS)
        }
        AreaPagesSub::Get { reference } => ctx.printer.one(
            &pages::resolve(ctx, AREA_PATH, &reference, "area page")?,
            DETAIL_COLUMNS,
        ),
        AreaPagesSub::Create(CreateArgs {
            fields: args,
            no_search,
        }) => {
            let attach = &args.fields.attach;
            // Fail before any request when nothing could supply a search;
            // --data may carry one, so that case is checked on the payload.
            if !no_search
                && attach.search.is_none()
                && attach.search_criteria.is_empty()
                && args.fields.data.is_none()
            {
                return Err(Error::Usage(NO_SEARCH.into()));
            }
            let mut payload = args.fields.payload(ctx, AREA_PATH)?;
            payload
                .set("area_name", args.area_name.as_deref())
                .set("featured", args.featured);
            pages::require(&payload, &["slug", "anchor_text", "area_name"], "area page")?;
            if !no_search && payload.0.get("search").is_none_or(Value::is_null) {
                return Err(Error::Usage(NO_SEARCH.into()));
            }
            let created = ctx.client.post(AREA_PATH, &payload.into_value())?.body;
            print_written(ctx, &created, DETAIL_COLUMNS, "Created")
        }
        AreaPagesSub::Update(args) => {
            let id = id_of(&pages::resolve(
                ctx,
                AREA_PATH,
                &args.reference,
                "area page",
            )?)?;
            let mut payload = args.fields.fields.payload(ctx, AREA_PATH)?;
            payload
                .set("area_name", args.fields.area_name.as_deref())
                .set("featured", args.fields.featured);
            pages::write(
                ctx,
                &pages::detail_path(AREA_PATH, id),
                payload,
                args.replace,
                DETAIL_COLUMNS,
            )
        }
        AreaPagesSub::Delete {
            reference,
            orphan_children,
        } => pages::delete(
            ctx,
            AREA_PATH,
            &reference,
            "area page",
            orphan_children.then_some("orphan_children"),
        ),
        AreaPagesSub::Search { reference } => {
            pages::page_search(ctx, AREA_PATH, &reference, "area page")
        }
        AreaPagesSub::Revisions { reference, limit } => {
            let id = id_of(&pages::resolve(ctx, AREA_PATH, &reference, "area page")?)?;
            super::revisions::list(ctx, &pages::detail_path(AREA_PATH, id), limit)
        }
        AreaPagesSub::Revision { reference, rev } => {
            let id = id_of(&pages::resolve(ctx, AREA_PATH, &reference, "area page")?)?;
            super::revisions::show(ctx, &pages::detail_path(AREA_PATH, id), rev)
        }
        AreaPagesSub::Revert { reference, rev } => {
            let page = pages::resolve(ctx, AREA_PATH, &reference, "area page")?;
            let label = format!(
                "area page {}",
                page.get("path").and_then(Value::as_str).unwrap_or("")
            );
            super::revisions::revert(
                ctx,
                &pages::detail_path(AREA_PATH, id_of(&page)?),
                rev,
                &label,
            )
        }
    }
}
