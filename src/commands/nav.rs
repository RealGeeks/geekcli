//! `geekcli nav …` — navigation bars. The bars are fixed (one per
//! position); the API edits their links.

use clap::{Args, Subcommand};
use serde_json::{json, Value};

use super::{confirm, parse_bool, Context, Payload};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::output::{cell, col, Column, Format};

pub const PATH: &str = "content/navigation-bars/";

pub const BAR_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("type", "/type"),
    col("label", "/label"),
    col("links", "/links_count"),
];

pub const LINK_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("order", "/order"),
    col("type", "/type"),
    col("anchor_text", "/anchor_text"),
    col("url", "/url"),
    col("nofollow", "/nofollow"),
];

#[derive(Debug, Args)]
pub struct NavCommand {
    #[command(subcommand)]
    pub command: NavSub,
}

#[derive(Debug, Subcommand)]
pub enum NavSub {
    /// List the navigation bars
    List,
    /// Show a bar and its links, by id or position (top_primary, bottom_primary, …)
    Get { bar: String },
    /// Append a link to a bar
    #[command(after_help = "Notes:
  - Real Geeks recommends 5-6 links in the top bar and 6-8 in the bottom bar, short anchor text and slug-only URLs for your own pages.
  - Bars are fixed per position; you edit links.
  - Refer to a link by id, its text or its URL.
  - Some bars are hidden on some designs (top_secondary on anna), matching the admin.
  - Point agent links at a page that exists: `pages create` first, then `nav update top_primary \"Meet Riley\" --text ...
  - --url ...`.")]
    Add(AddArgs),
    /// Change one link
    Update(UpdateArgs),
    /// Remove one link
    Remove {
        bar: String,
        /// Link id, anchor text or URL
        link: String,
    },
    /// Move a link to a new position (0-based) within its bar
    Move {
        bar: String,
        /// Link id, anchor text or URL
        link: String,
        /// New 0-based position
        #[arg(long)]
        to: usize,
    },
    /// Replace every link on a bar from JSON: a list of links, `@file`, or `-`
    Set {
        bar: String,
        /// JSON list of {type, url, anchor_text, nofollow}, `@file`, or `-` for stdin
        #[arg(long, value_name = "JSON")]
        data: String,
    },
    /// Remove every link from a bar
    Clear { bar: String },
    /// List a bar's revisions, newest first
    Revisions {
        /// Bar id or position (top_primary, bottom_primary, …)
        bar: String,
        /// Show only the latest N
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
    },
    /// Show one revision with what a revert would restore
    Revision { bar: String, rev: u64 },
    /// Undo a revision and everything after it (the revert is itself undoable)
    #[command(after_help = "Notes:
  - Revisions track the bar's whole link list and its order. One request is one revision, so `nav set` and `nav clear` are undone in one step.
  - Links a revert brings back are new rows with new ids; links that still exist keep theirs. Read the ids again before `nav update`.")]
    Revert { bar: String, rev: u64 },
}

#[derive(Debug, Args)]
pub struct AddArgs {
    pub bar: String,
    /// Link target (required unless --contact)
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    /// Link text (required unless --contact)
    #[arg(long, value_name = "TEXT")]
    pub text: Option<String>,
    /// Add the site's contact link instead of a custom one
    #[arg(long, conflicts_with_all = ["url", "text"])]
    pub contact: bool,
    #[arg(long)]
    pub nofollow: bool,
    /// Insert at this 0-based position instead of the end
    #[arg(long, value_name = "N")]
    pub at: Option<usize>,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    pub bar: String,
    /// Link id, anchor text or URL
    pub link: String,
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, value_name = "TEXT")]
    pub text: Option<String>,
    /// custom or contact
    #[arg(long, value_name = "TYPE")]
    pub r#type: Option<String>,
    #[arg(long, value_name = "BOOL", value_parser = parse_bool)]
    pub nofollow: Option<bool>,
}

pub fn run(ctx: &Context, cmd: NavCommand) -> Result<()> {
    match cmd.command {
        NavSub::List => {
            let bars = ctx.client.get(PATH, &Query::new())?.body;
            let rows: Vec<Value> = bars
                .get("results")
                .and_then(Value::as_array)
                .map(|list| list.iter().map(with_count).collect())
                .unwrap_or_default();
            ctx.printer.list(&rows, None, BAR_COLUMNS)
        }
        NavSub::Get { bar } => print_bar(ctx, &resolve(ctx, &bar)?),
        NavSub::Add(args) => add(ctx, &args),
        NavSub::Update(args) => {
            let bar = resolve(ctx, &args.bar)?;
            let mut payload = Payload::default();
            payload
                .set("url", args.url.as_deref())
                .set("anchor_text", args.text.as_deref())
                .set("type", args.r#type.as_deref())
                .set("nofollow", args.nofollow);
            if payload.is_empty() {
                return Err(Error::Usage(
                    "pass at least one of --url, --text, --type, --nofollow".into(),
                ));
            }
            let link = resolve_link(&bar, &args.link)?;
            let updated = ctx
                .client
                .patch(&link_path(&bar, link)?, &payload.into_value())?
                .body;
            ctx.printer.one(&updated, LINK_COLUMNS)
        }
        NavSub::Remove { bar, link } => {
            let bar = resolve(ctx, &bar)?;
            let link = resolve_link(&bar, &link)?;
            if !confirm(ctx, &format!("link {link} from {}", label(&bar)))? {
                return Err(Error::Usage("cancelled".into()));
            }
            ctx.client.delete(&link_path(&bar, link)?, &Query::new())?;
            done(
                ctx,
                &json!({ "deleted": true, "id": link }),
                &format!("Removed link {link}"),
            )
        }
        NavSub::Move { bar, link, to } => {
            let bar = resolve(ctx, &bar)?;
            let link = resolve_link(&bar, &link)?;
            ctx.client
                .patch(&link_path(&bar, link)?, &json!({ "order": to }))?;
            let updated = ctx
                .client
                .get(&format!("{PATH}{}/", bar_id(&bar)?), &Query::new())?
                .body;
            print_bar(ctx, &updated)
        }
        NavSub::Set { bar, data } => {
            let bar = resolve(ctx, &bar)?;
            let links = read_links_json(&data)?;
            let updated = replace_links(ctx, &bar, &links)?;
            print_bar(ctx, &updated)
        }
        NavSub::Clear { bar } => {
            let bar = resolve(ctx, &bar)?;
            if !confirm(ctx, &format!("every link on {}", label(&bar)))? {
                return Err(Error::Usage("cancelled".into()));
            }
            let updated = replace_links(ctx, &bar, &[])?;
            print_bar(ctx, &updated)
        }
        NavSub::Revisions { bar, limit } => {
            let bar = resolve(ctx, &bar)?;
            super::revisions::list(ctx, &detail_path(&bar)?, limit)
        }
        NavSub::Revision { bar, rev } => {
            let bar = resolve(ctx, &bar)?;
            super::revisions::show(ctx, &detail_path(&bar)?, rev)
        }
        NavSub::Revert { bar, rev } => {
            let bar = resolve(ctx, &bar)?;
            let restored =
                super::revisions::revert_request(ctx, &detail_path(&bar)?, rev, &label(&bar))?;
            print_bar(ctx, &restored)
        }
    }
}

fn not_found(message: &str) -> Error {
    Error::Api {
        status: 404,
        code: "not_found".into(),
        message: message.to_string(),
        fields: std::collections::BTreeMap::new(),
        retry_after: None,
    }
}

fn label(bar: &Value) -> String {
    bar.get("label")
        .and_then(Value::as_str)
        .map_or_else(|| "the bar".to_string(), str::to_string)
}

fn with_count(bar: &Value) -> Value {
    let mut out = bar.clone();
    out["links_count"] = Value::from(bar_links(bar).len());
    out
}

fn bar_links(bar: &Value) -> Vec<Value> {
    bar.get("links")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn bar_id(bar: &Value) -> Result<u64> {
    super::id_of(bar)
}

fn detail_path(bar: &Value) -> Result<String> {
    Ok(format!("{PATH}{}/", bar_id(bar)?))
}

fn link_path(bar: &Value, link: u64) -> Result<String> {
    Ok(format!("{PATH}{}/links/{link}/", bar_id(bar)?))
}

fn links_path(bar: &Value) -> Result<String> {
    Ok(format!("{PATH}{}/links/", bar_id(bar)?))
}

/// A bar by numeric id or by position name.
pub fn resolve(ctx: &Context, reference: &str) -> Result<Value> {
    let reference = reference.trim();
    if let Ok(id) = reference.parse::<u64>() {
        return Ok(ctx.client.get(&format!("{PATH}{id}/"), &Query::new())?.body);
    }
    let wanted = reference.to_ascii_lowercase().replace('-', "_");
    let bars = ctx.client.get(PATH, &Query::new())?.body;
    bars.get("results")
        .and_then(Value::as_array)
        .and_then(|list| {
            list.iter()
                .find(|b| b.get("type").and_then(Value::as_str).map(str::to_ascii_lowercase) == Some(wanted.clone()))
                .cloned()
        })
        .ok_or_else(|| not_found(&format!("no navigation bar '{reference}'; use an id or one of top_primary, bottom_primary, top_secondary, bottom_secondary, seller_leads")))
}

/// A link on a bar by id, anchor text (case-insensitive) or exact URL.
pub fn resolve_link(bar: &Value, reference: &str) -> Result<u64> {
    let reference = reference.trim();
    let links = bar_links(bar);
    if let Ok(id) = reference.parse::<u64>() {
        if links
            .iter()
            .any(|l| l.get("id").and_then(Value::as_u64) == Some(id))
        {
            return Ok(id);
        }
        return Err(not_found(&format!("link {id} is not on {}", label(bar))));
    }
    let wanted = reference.to_ascii_lowercase();
    let matches: Vec<u64> = links
        .iter()
        .filter(|l| {
            let text = l
                .get("anchor_text")
                .and_then(Value::as_str)
                .map(str::to_ascii_lowercase);
            let url = l.get("url").and_then(Value::as_str);
            text.as_deref() == Some(wanted.as_str()) || url == Some(reference)
        })
        .filter_map(|l| l.get("id").and_then(Value::as_u64))
        .collect();
    match matches.as_slice() {
        [id] => Ok(*id),
        [] => Err(not_found(&format!(
            "no link '{reference}' on {}",
            label(bar)
        ))),
        _ => Err(Error::Usage(format!(
            "'{reference}' matches {} links on {}; use the id",
            matches.len(),
            label(bar)
        ))),
    }
}

fn print_bar(ctx: &Context, bar: &Value) -> Result<()> {
    if ctx.printer.quiet {
        for link in bar_links(bar) {
            println!("{}", cell(link.get("id").unwrap_or(&Value::Null)));
        }
        return Ok(());
    }
    if ctx.printer.format == Format::Table {
        ctx.printer.note(&format!(
            "{} (id {}, {})",
            label(bar),
            bar["id"],
            bar["type"]
        ));
        return ctx.printer.list(&bar_links(bar), None, LINK_COLUMNS);
    }
    ctx.printer.raw(bar)
}

fn done(ctx: &Context, doc: &Value, note: &str) -> Result<()> {
    ctx.printer.note(note);
    if ctx.printer.format != Format::Table {
        ctx.printer.raw(doc)?;
    }
    Ok(())
}

fn replace_links(ctx: &Context, bar: &Value, links: &[Value]) -> Result<Value> {
    Ok(ctx
        .client
        .put(&links_path(bar)?, &json!({ "links": links }))?
        .body)
}

fn read_links_json(source: &str) -> Result<Vec<Value>> {
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
        Value::Object(ref map) if map.get("links").is_some_and(Value::is_array) => {
            Ok(map["links"].as_array().cloned().unwrap_or_default())
        }
        _ => Err(Error::Usage(
            "--data must be a JSON list of links, or {\"links\": [...]}".into(),
        )),
    }
}

fn add(ctx: &Context, args: &AddArgs) -> Result<()> {
    let bar = resolve(ctx, &args.bar)?;
    let link = if args.contact {
        json!({ "type": "contact", "nofollow": args.nofollow })
    } else {
        let (Some(url), Some(text)) = (&args.url, &args.text) else {
            return Err(Error::Usage(
                "--url and --text are required (or --contact)".into(),
            ));
        };
        json!({ "type": "custom", "url": url, "anchor_text": text, "nofollow": args.nofollow })
    };
    let mut link = link;
    if let Some(at) = args.at {
        link["order"] = Value::from(at);
    }
    let updated = ctx.client.post(&links_path(&bar)?, &link)?.body;
    print_bar(ctx, &updated)
}
