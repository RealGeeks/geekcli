//! `geekcli area-pages …` — community/neighbourhood pages. Same tree as content
//! pages, plus `area_name` and `featured`.

use clap::{Args, Subcommand};
use serde_json::Value;

use super::pages::{self, TreeFields, TreeListArgs, AREA_PATH};
use super::{id_of, list_and_print, parse_bool, print_written, push, Context};
use crate::error::Result;
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
    col("content", "/content"),
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
    /// Create an area page
    Create(FieldArgs),
    /// Change fields on an area page
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
    /// Name of the area (required on create)
    #[arg(long)]
    pub area_name: Option<String>,
    #[arg(long, value_name = "BOOL", value_parser = parse_bool)]
    pub featured: Option<bool>,
}

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
        AreaPagesSub::Create(args) => {
            let mut payload = args.fields.payload(ctx, AREA_PATH)?;
            payload
                .set("area_name", args.area_name.as_deref())
                .set("featured", args.featured);
            pages::require(&payload, &["slug", "anchor_text", "area_name"], "area page")?;
            let created = ctx.client.post(AREA_PATH, &payload.into_value())?.body;
            print_written(ctx, &created, DETAIL_COLUMNS, "Created")
        }
        AreaPagesSub::Update(args) => {
            let id = pages::resolve_id(ctx, AREA_PATH, &args.reference, "area page")?;
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
            let id = pages::resolve_id(ctx, AREA_PATH, &reference, "area page")?;
            super::revisions::list(ctx, &pages::detail_path(AREA_PATH, id), limit)
        }
        AreaPagesSub::Revision { reference, rev } => {
            let id = pages::resolve_id(ctx, AREA_PATH, &reference, "area page")?;
            super::revisions::show(ctx, &pages::detail_path(AREA_PATH, id), rev)
        }
        AreaPagesSub::Revert { reference, rev } => {
            let page = pages::find(ctx, AREA_PATH, &reference, "area page")?;
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
