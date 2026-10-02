//! `geekcli featured …` — the "Featured Pages" tile block on the
//! anna-modern home page: a titled group of up to twelve tiles, attached to
//! the home page through its `tile_group`.

use clap::{Args, Subcommand};
use serde_json::{json, Value};

use super::{confirm, id_of, print_written, Context, Payload};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::output::{cell, col, Column, Format};

pub const PATH: &str = "content/featured-pages/";

pub const GROUP_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("title", "/title"),
    col("blurb", "/blurb"),
    col("tiles", "/tiles_count"),
];

pub const TILE_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("title", "/title"),
    col("link", "/link"),
    col("cta", "/cta"),
    col("image", "/image"),
];

#[derive(Debug, Args)]
pub struct FeaturedCommand {
    #[command(subcommand)]
    pub command: FeaturedSub,
}

#[derive(Debug, Subcommand)]
pub enum FeaturedSub {
    /// List tile groups
    List,
    /// Show a group and its tiles, by id or title
    Get { reference: String },
    /// Create a tile group (attach it with `home-page update --tile-group`)
    #[command(after_help = "Notes:
  - Titles are unique per site (422 otherwise); `featured get <title>` finds an existing group.
  - Up to twelve tiles, shown in order. A tile is a short title, a site path (an area page /riverside/ or a search URL /search/results/?city=Riverside), button text and an optional image URL from `files upload -q`.
  - The block renders on anna-modern only; other designs ignore the home page's tile group.
  - `home-page update --tile-group <id or title>` shows it; `--tile-group null` detaches. Then take any hand-built area grid out of the home content and move photo credits to the footer.
  - Long titles truncate on phones (\"North Riverside Hei…\"); keep them to two words where you can.")]
    Create(GroupArgs),
    /// Change a group's title or blurb
    Update {
        /// Group id or title
        reference: String,
        /// New title (unique per site)
        #[arg(long)]
        title: Option<String>,
        /// Short text under the heading
        #[arg(long)]
        blurb: Option<String>,
    },
    /// Delete a group (409 while the home page shows it, unless --force)
    #[command(after_help = "Notes:
  - The group the home page shows is refused with 409; `--force` detaches it and deletes, or `home-page update --tile-group null` first.
  - `featured get` shows `used_by_home_page`.")]
    Delete {
        /// Group id or title
        reference: String,
        /// Delete even if the home page shows it (the home page loses its tiles)
        #[arg(long)]
        force: bool,
    },
    /// Append a tile (up to twelve)
    AddTile(TileArgs),
    /// Change one tile (only the flags you pass)
    UpdateTile(UpdateTileArgs),
    /// Remove one tile
    RemoveTile {
        /// Group id or title
        reference: String,
        /// Tile id (from `featured get`)
        tile: u64,
    },
    /// Replace every tile from a JSON list, `@file`, or `-` (entries with an id are kept)
    #[command(after_help = "Notes:
  - Entries carrying an id keep that id; entries without one are created; tiles missing from the list are deleted.
  - Each entry is {title, link, cta, image}; order in the list is the display order.")]
    SetTiles {
        /// Group id or title
        reference: String,
        /// JSON list of tiles, `@file`, or `-` for stdin
        #[arg(long, value_name = "JSON")]
        data: String,
    },
}

#[derive(Debug, Args)]
pub struct GroupArgs {
    /// Heading above the tiles; unique per site
    #[arg(long)]
    pub title: String,
    /// Short text under the heading
    #[arg(long)]
    pub blurb: Option<String>,
    /// Initial tiles as a JSON list, `@file`, or `-`
    #[arg(long, value_name = "JSON")]
    pub data: Option<String>,
}

#[derive(Debug, Args)]
pub struct TileArgs {
    /// Group id or title
    pub reference: String,
    /// Tile title (the area name; short, it truncates on phones)
    #[arg(long)]
    pub title: String,
    /// Site path (/riverside/, /search/results/?city=Riverside) or an http(s)/mailto/tel URL
    #[arg(long)]
    pub link: String,
    /// Button text
    #[arg(long, default_value = "View Homes")]
    pub cta: String,
    /// Background image URL from `files upload -q` (no spaces, quotes or parentheses)
    #[arg(long, value_name = "URL")]
    pub image: Option<String>,
}

#[derive(Debug, Args)]
pub struct UpdateTileArgs {
    /// Group id or title
    pub reference: String,
    /// Tile id (from `featured get`)
    pub tile: u64,
    /// Tile title (the area name)
    #[arg(long)]
    pub title: Option<String>,
    /// Site path: an area page (/riverside/) or a search URL
    #[arg(long)]
    pub link: Option<String>,
    /// Button text
    #[arg(long)]
    pub cta: Option<String>,
    /// Background image URL, or `null` to clear
    #[arg(long, value_name = "URL_OR_NULL")]
    pub image: Option<String>,
}

pub fn run(ctx: &Context, cmd: FeaturedCommand) -> Result<()> {
    match cmd.command {
        FeaturedSub::List => {
            let body = ctx.client.get(PATH, &Query::new())?.body;
            let rows: Vec<Value> = body
                .get("results")
                .and_then(Value::as_array)
                .map(|l| l.iter().map(summarize).collect())
                .unwrap_or_default();
            ctx.printer.list(&rows, None, GROUP_COLUMNS)
        }
        FeaturedSub::Get { reference } => print_group(ctx, &resolve(ctx, &reference)?),
        FeaturedSub::Create(args) => {
            let mut body = json!({ "title": args.title });
            if let Some(b) = &args.blurb {
                body["blurb"] = Value::String(b.clone());
            }
            if let Some(data) = &args.data {
                body["tiles"] = Value::Array(read_tiles_json(data)?);
            }
            let created = ctx.client.post(PATH, &body)?.body;
            print_written(ctx, &summarize(&created), GROUP_COLUMNS, "Created")
        }
        FeaturedSub::Update {
            reference,
            title,
            blurb,
        } => {
            let id = id_of(&resolve(ctx, &reference)?)?;
            let mut payload = Payload::default();
            payload.set("title", title).set("blurb", blurb);
            if payload.is_empty() {
                return Err(Error::Usage("pass --title and/or --blurb".into()));
            }
            let updated = ctx
                .client
                .patch(&detail_path(id), &payload.into_value())?
                .body;
            print_written(ctx, &summarize(&updated), GROUP_COLUMNS, "Updated")
        }
        FeaturedSub::Delete { reference, force } => {
            let group = resolve(ctx, &reference)?;
            let id = id_of(&group)?;
            if !confirm(
                ctx,
                &format!("featured group {id} \"{}\"", cell(&group["title"])),
            )? {
                return Err(Error::Usage("cancelled".into()));
            }
            let mut query = Query::new();
            if force {
                query.push(("force".into(), "true".into()));
            }
            ctx.client.delete(&detail_path(id), &query)?;
            ctx.printer.note(&format!("Deleted featured group {id}"));
            if ctx.printer.format != Format::Table {
                ctx.printer.raw(&json!({ "deleted": true, "id": id }))?;
            }
            Ok(())
        }
        FeaturedSub::AddTile(args) => {
            let id = id_of(&resolve(ctx, &args.reference)?)?;
            let mut tile = json!({ "title": args.title, "link": args.link, "cta": args.cta });
            if let Some(img) = &args.image {
                tile["image"] = Value::String(img.clone());
            }
            let updated = ctx.client.post(&tiles_path(id), &tile)?.body;
            print_group(ctx, &updated)
        }
        FeaturedSub::UpdateTile(args) => {
            let id = id_of(&resolve(ctx, &args.reference)?)?;
            let mut payload = Payload::default();
            payload
                .set("title", args.title.as_deref())
                .set("link", args.link.as_deref())
                .set("cta", args.cta.as_deref());
            if let Some(img) = &args.image {
                payload.set_value(
                    "image",
                    match img.trim().to_ascii_lowercase().as_str() {
                        "null" | "none" | "-" => Value::Null,
                        _ => Value::String(img.trim().to_string()),
                    },
                );
            }
            if payload.is_empty() {
                return Err(Error::Usage(
                    "pass --title, --link, --cta or --image".into(),
                ));
            }
            let updated = ctx
                .client
                .patch(&tile_path(id, args.tile), &payload.into_value())?
                .body;
            ctx.printer.one(&updated, TILE_COLUMNS)
        }
        FeaturedSub::RemoveTile { reference, tile } => {
            let id = id_of(&resolve(ctx, &reference)?)?;
            ctx.client.delete(&tile_path(id, tile), &Query::new())?;
            ctx.printer.note(&format!("Removed tile {tile}"));
            if ctx.printer.format != Format::Table {
                ctx.printer.raw(&json!({ "deleted": true, "id": tile }))?;
            }
            Ok(())
        }
        FeaturedSub::SetTiles { reference, data } => {
            let id = id_of(&resolve(ctx, &reference)?)?;
            let tiles = read_tiles_json(&data)?;
            let updated = ctx
                .client
                .put(&tiles_path(id), &json!({ "tiles": tiles }))?
                .body;
            print_group(ctx, &updated)
        }
    }
}

pub fn detail_path(id: u64) -> String {
    format!("{PATH}{id}/")
}

fn tiles_path(id: u64) -> String {
    format!("{PATH}{id}/tiles/")
}

fn tile_path(id: u64, tile: u64) -> String {
    format!("{PATH}{id}/tiles/{tile}/")
}

fn tiles_of(group: &Value) -> Vec<Value> {
    group
        .get("tiles")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn summarize(group: &Value) -> Value {
    let mut out = group.clone();
    out["tiles_count"] = Value::from(tiles_of(group).len());
    out
}

/// A group by id or by title (case-insensitive).
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
                .find(|g| {
                    g.get("title")
                        .and_then(Value::as_str)
                        .map(str::to_ascii_lowercase)
                        == Some(wanted.clone())
                })
                .cloned()
        })
        .ok_or_else(|| Error::Api {
            status: 404,
            code: "not_found".into(),
            message: format!("no featured group titled '{reference}'"),
            fields: std::collections::BTreeMap::new(),
            retry_after: None,
        })?;
    Ok(ctx
        .client
        .get(&detail_path(id_of(&found)?), &Query::new())?
        .body)
}

fn print_group(ctx: &Context, group: &Value) -> Result<()> {
    if ctx.printer.quiet {
        println!("{}", cell(&group["id"]));
        return Ok(());
    }
    if ctx.printer.format == Format::Table {
        ctx.printer
            .note(&format!("{} (id {})", cell(&group["title"]), group["id"]));
        return ctx.printer.list(&tiles_of(group), None, TILE_COLUMNS);
    }
    ctx.printer.raw(group)
}

fn read_tiles_json(source: &str) -> Result<Vec<Value>> {
    let text = if let Some(path) = source.strip_prefix('@') {
        crate::html::read_file_or_stdin(path)?
    } else if source == "-" {
        crate::html::read_file_or_stdin("-")?
    } else {
        source.to_string()
    };
    let value: Value = serde_json::from_str(&text)?;
    match value {
        Value::Array(items) => Ok(items),
        Value::Object(ref map) if map.get("tiles").is_some_and(Value::is_array) => {
            Ok(map["tiles"].as_array().cloned().unwrap_or_default())
        }
        _ => Err(Error::Usage(
            "--data must be a JSON list of tiles, or {\"tiles\": [...]}".into(),
        )),
    }
}
