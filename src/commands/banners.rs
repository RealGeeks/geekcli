//! `geekcli banners …` — a one-line message with a button, shown across
//! the top of the pages it is attached to (`--banner` on pages, area pages,
//! agent pages and the home page).

use clap::{Args, Subcommand};
use serde_json::{json, Value};

use super::{confirm, id_of, print_written, Context, Payload};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::output::{cell, col, Column, Format};

pub const PATH: &str = "content/banners/";

pub const COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("name", "/name"),
    col("message", "/message"),
    col("call_to_action", "/call_to_action"),
    col("url", "/url"),
    col("used_by_count", "/used_by_count"),
    col("used_by", "/used_by"),
];

#[derive(Debug, Args)]
pub struct BannersCommand {
    #[command(subcommand)]
    pub command: BannersSub,
}

#[derive(Debug, Subcommand)]
pub enum BannersSub {
    /// List banners
    List,
    /// Show a banner by id or name (with the pages that show it)
    Get { reference: String },
    /// Create a banner (attach it with `--banner` on a page)
    #[command(after_help = "Notes:
  - A banner is the message strip with a button across the top of a page, not the header image (that is --landscape or the HEADER_IMAGE setting).
  - --name, --call-to-action and --url are required; --message is the text (150 characters at most).
  - It shows nowhere until a page uses it: `pages update <page> --banner <id or name>`, and the same flag on area-pages, agent-pages and home-page. There is no site-wide banner; each page chooses.
  - --url is a site path (/open-house/) or an http(s), mailto or tel URL.")]
    Create(Fields),
    /// Change a banner (only the flags you pass are changed)
    #[command(after_help = "Notes:
  - Every page showing the banner changes with it; `banners get` lists them under used_by.
  - To take a banner off one page, use `--banner null` on that page instead.")]
    Update(UpdateArgs),
    /// Delete a banner
    Delete {
        reference: String,
        /// Delete even if pages show it; it comes off them
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Args)]
pub struct Fields {
    /// Internal name, unique per site
    #[arg(long)]
    pub name: Option<String>,
    /// The text on the banner (150 characters at most)
    #[arg(long, value_name = "TEXT")]
    pub message: Option<String>,
    /// Where the button goes: a site path or an http(s), mailto or tel URL
    #[arg(long)]
    pub url: Option<String>,
    /// The button text (50 characters at most)
    #[arg(long, visible_alias = "cta", value_name = "TEXT")]
    pub call_to_action: Option<String>,
    /// Extra fields as a JSON object, `@file`, or `-` for stdin (flags win)
    #[arg(long, value_name = "JSON")]
    pub data: Option<String>,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Banner id or name
    pub reference: String,
    #[command(flatten)]
    pub fields: Fields,
}

impl Fields {
    fn payload(&self) -> Result<Payload> {
        let mut payload = Payload::default();
        payload
            .set("name", self.name.as_deref())
            .set("message", self.message.as_deref())
            .set("url", self.url.as_deref())
            .set("call_to_action", self.call_to_action.as_deref());
        payload.merge_json(self.data.as_deref())?;
        Ok(payload)
    }
}

pub fn run(ctx: &Context, cmd: BannersCommand) -> Result<()> {
    match cmd.command {
        BannersSub::List => {
            let body = ctx.client.get(PATH, &Query::new())?.body;
            let rows = body
                .get("results")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            ctx.printer.list(&rows, None, &COLUMNS[..5])
        }
        BannersSub::Get { reference } => ctx.printer.one(&resolve(ctx, &reference)?, COLUMNS),
        BannersSub::Create(fields) => {
            let payload = fields.payload()?;
            super::pages::require(&payload, &["name", "url", "call_to_action"], "banner")?;
            let created = ctx.client.post(PATH, &payload.into_value())?.body;
            print_written(ctx, &created, COLUMNS, "Created")
        }
        BannersSub::Update(args) => {
            let id = id_of(&resolve(ctx, &args.reference)?)?;
            let payload = args.fields.payload()?;
            if payload.is_empty() {
                return Err(Error::Usage(
                    "nothing to update: pass at least one field flag or --data".into(),
                ));
            }
            let updated = ctx
                .client
                .patch(&detail_path(id), &payload.into_value())?
                .body;
            print_written(ctx, &updated, COLUMNS, "Updated")
        }
        BannersSub::Delete { reference, force } => {
            let banner = resolve(ctx, &reference)?;
            let id = id_of(&banner)?;
            if !confirm(ctx, &format!("banner {id} \"{}\"", cell(&banner["name"])))? {
                return Err(Error::Usage("cancelled".into()));
            }
            let mut query = Query::new();
            if force {
                query.push(("force".into(), "true".into()));
            }
            ctx.client.delete(&detail_path(id), &query)?;
            ctx.printer.note(&format!("Deleted banner {id}"));
            if ctx.printer.format != Format::Table {
                ctx.printer.raw(&json!({ "deleted": true, "id": id }))?;
            }
            Ok(())
        }
    }
}

pub fn detail_path(id: u64) -> String {
    format!("{PATH}{id}/")
}

/// A banner by numeric id or by name (case-insensitive).
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
                .find(|b| {
                    b.get("name")
                        .and_then(Value::as_str)
                        .map(str::to_ascii_lowercase)
                        == Some(wanted.clone())
                })
                .cloned()
        })
        .ok_or_else(|| Error::Api {
            status: 404,
            code: "not_found".into(),
            message: format!("no banner named '{reference}'"),
            fields: std::collections::BTreeMap::new(),
            retry_after: None,
        })?;
    Ok(ctx
        .client
        .get(&detail_path(id_of(&found)?), &Query::new())?
        .body)
}
