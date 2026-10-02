//! `geekcli sidebars …` — sidebars and their items. An item is either
//! sanitized HTML or a list of links with an optional header.

use clap::{Args, Subcommand};
use serde_json::{json, Value};

use super::{confirm, id_of, lookup_one, print_written, push, Context, Payload};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::html;
use crate::output::{cell, col, Column, Format};

pub const PATH: &str = "content/sidebars/";

pub const COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("name", "/name"),
    col("special", "/special"),
    col("items", "/items_count"),
    col("used_by", "/used_by"),
];

pub const ITEM_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("order", "/order"),
    col("type", "/type"),
    col("summary", "/summary"),
];

#[derive(Debug, Args)]
pub struct SidebarsCommand {
    #[command(subcommand)]
    pub command: SidebarsSub,
}

#[derive(Debug, Subcommand)]
pub enum SidebarsSub {
    /// List sidebars
    List {
        /// Name contains
        #[arg(long, short = 's', value_name = "TEXT")]
        search: Option<String>,
    },
    /// Show a sidebar and its items, by id or name
    Get { reference: String },
    /// Create a sidebar
    Create {
        #[arg(long)]
        name: String,
        /// Initial items as a JSON list, `@file`, or `-`
        #[arg(long, value_name = "JSON")]
        data: Option<String>,
    },
    /// Rename a sidebar
    Rename {
        reference: String,
        #[arg(long)]
        name: String,
    },
    /// Delete a sidebar
    Delete {
        reference: String,
        /// Delete even if pages use it; they lose the sidebar
        #[arg(long)]
        force: bool,
    },
    /// Show one item
    Item { reference: String, item: u64 },
    /// Append an HTML item
    #[command(after_help = "Notes:
  - <button>, <script> and non-YouTube iframes are stripped; make a button an <a> with display: inline-block, padding and background-color.
  - Themes render the sidebar differently: a right column on molly, stacked full-width sections under the content on anna-modern.
  - Items with a data-domain are system widgets.")]
    AddHtml(HtmlArgs),
    /// Append a links item
    AddLinks(LinksArgs),
    /// Change one item (only the flags you pass)
    UpdateItem(UpdateItemArgs),
    /// Remove one item
    RemoveItem { reference: String, item: u64 },
    /// Move an item to a new 0-based position
    MoveItem {
        reference: String,
        item: u64,
        #[arg(long)]
        to: usize,
    },
    /// Replace every item from a JSON list, `@file`, or `-`
    SetItems {
        reference: String,
        #[arg(long, value_name = "JSON")]
        data: String,
    },
}

#[derive(Debug, Args)]
pub struct HtmlArgs {
    pub reference: String,
    /// Inline HTML (or Markdown with --markdown)
    #[arg(long, value_name = "HTML")]
    pub html: Option<String>,
    /// Read the HTML from a file, or `-` for stdin; `.md` converts from Markdown
    #[arg(long, value_name = "FILE")]
    pub html_file: Option<String>,
    #[arg(long)]
    pub markdown: bool,
    /// Insert at this 0-based position instead of the end
    #[arg(long, value_name = "N")]
    pub at: Option<usize>,
}

#[derive(Debug, Args)]
pub struct LinksArgs {
    pub reference: String,
    /// Header text above the links
    #[arg(long, value_name = "TEXT")]
    pub header: Option<String>,
    /// Make the header a link to this URL
    #[arg(long, value_name = "URL", requires = "header")]
    pub header_url: Option<String>,
    /// A link as `Text=/url/` (or just `Text` for plain text); repeatable
    #[arg(long = "link", value_name = "TEXT=URL", required = true)]
    pub links: Vec<String>,
    /// 1 or 2 columns
    #[arg(long, default_value_t = 1)]
    pub columns: u8,
    /// Insert at this 0-based position instead of the end
    #[arg(long, value_name = "N")]
    pub at: Option<usize>,
}

#[derive(Debug, Args)]
pub struct UpdateItemArgs {
    pub reference: String,
    pub item: u64,
    #[arg(long, value_name = "HTML")]
    pub html: Option<String>,
    #[arg(long, value_name = "FILE")]
    pub html_file: Option<String>,
    #[arg(long)]
    pub markdown: bool,
    #[arg(long, value_name = "TEXT")]
    pub header: Option<String>,
    #[arg(long, value_name = "URL")]
    pub header_url: Option<String>,
    /// Remove the header
    #[arg(long, conflicts_with_all = ["header", "header_url"])]
    pub no_header: bool,
    /// Replace the links: `Text=/url/`; repeatable
    #[arg(long = "link", value_name = "TEXT=URL")]
    pub links: Vec<String>,
    #[arg(long)]
    pub columns: Option<u8>,
}

pub fn run(ctx: &Context, cmd: SidebarsCommand) -> Result<()> {
    match cmd.command {
        SidebarsSub::List { search } => {
            let mut query = Query::new();
            push(&mut query, "q", search.as_deref());
            let body = ctx.client.get(PATH, &query)?.body;
            let rows: Vec<Value> = body
                .get("results")
                .and_then(Value::as_array)
                .map(|list| list.iter().map(summarize).collect())
                .unwrap_or_default();
            ctx.printer.list(&rows, None, COLUMNS)
        }
        SidebarsSub::Get { reference } => print_sidebar(ctx, &resolve(ctx, &reference)?),
        SidebarsSub::Create { name, data } => {
            let mut body = json!({ "name": name });
            if let Some(data) = data {
                body["items"] = Value::Array(read_items_json(&data)?);
            }
            let created = ctx.client.post(PATH, &body)?.body;
            print_written(ctx, &summarize(&created), COLUMNS, "Created")
        }
        SidebarsSub::Rename { reference, name } => {
            let id = id_of(&resolve(ctx, &reference)?)?;
            let updated = ctx
                .client
                .patch(&detail_path(id), &json!({ "name": name }))?
                .body;
            print_written(ctx, &summarize(&updated), COLUMNS, "Renamed")
        }
        SidebarsSub::Delete { reference, force } => {
            let sidebar = resolve(ctx, &reference)?;
            let id = id_of(&sidebar)?;
            if !confirm(ctx, &format!("sidebar {id} \"{}\"", cell(&sidebar["name"])))? {
                return Err(Error::Usage("cancelled".into()));
            }
            let mut query = Query::new();
            if force {
                query.push(("force".into(), "true".into()));
            }
            ctx.client.delete(&detail_path(id), &query)?;
            ctx.printer.note(&format!("Deleted sidebar {id}"));
            if ctx.printer.format != Format::Table {
                ctx.printer.raw(&json!({ "deleted": true, "id": id }))?;
            }
            Ok(())
        }
        SidebarsSub::Item { reference, item } => {
            let id = id_of(&resolve(ctx, &reference)?)?;
            let value = ctx.client.get(&item_path(id, item), &Query::new())?.body;
            ctx.printer.raw(&value)
        }
        SidebarsSub::AddHtml(args) => {
            let sidebar = resolve(ctx, &args.reference)?;
            let Some(html) = html::read_body(
                args.html.as_deref(),
                args.html_file.as_deref(),
                args.markdown,
            )?
            else {
                return Err(Error::Usage("pass --html or --html-file".into()));
            };
            add_item(
                ctx,
                &sidebar,
                json!({ "type": "html", "html": html }),
                args.at,
            )
        }
        SidebarsSub::AddLinks(args) => {
            let sidebar = resolve(ctx, &args.reference)?;
            let mut item = json!({ "type": "links", "links": parse_links(&args.links)?, "columns": args.columns });
            if let Some(text) = &args.header {
                item["header"] = json!({ "text": text, "url": args.header_url });
            }
            add_item(ctx, &sidebar, item, args.at)
        }
        SidebarsSub::UpdateItem(args) => {
            let id = id_of(&resolve(ctx, &args.reference)?)?;
            let mut payload = Payload::default();
            if let Some(html) = html::read_body(
                args.html.as_deref(),
                args.html_file.as_deref(),
                args.markdown,
            )? {
                payload.set("type", Some("html")).set("html", Some(html));
            }
            if args.no_header {
                payload
                    .set("type", Some("links"))
                    .set_value("header", Value::Null);
            } else if args.header.is_some() || args.header_url.is_some() {
                let current = ctx
                    .client
                    .get(&item_path(id, args.item), &Query::new())?
                    .body;
                let text = args
                    .header
                    .clone()
                    .or_else(|| {
                        current
                            .pointer("/header/text")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .ok_or_else(|| {
                        Error::Usage("--header-url needs a header; pass --header too".into())
                    })?;
                let url = args.header_url.clone().or_else(|| {
                    current
                        .pointer("/header/url")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
                payload
                    .set("type", Some("links"))
                    .set_value("header", json!({ "text": text, "url": url }));
            }
            if !args.links.is_empty() {
                payload
                    .set("type", Some("links"))
                    .set_value("links", Value::Array(parse_links(&args.links)?));
            }
            if let Some(columns) = args.columns {
                payload
                    .set("type", Some("links"))
                    .set("columns", Some(columns));
            }
            if payload.is_empty() {
                return Err(Error::Usage(
                    "nothing to update: pass --html, --header, --link or --columns".into(),
                ));
            }
            let updated = ctx
                .client
                .patch(&item_path(id, args.item), &payload.into_value())?
                .body;
            ctx.printer.raw(&updated)
        }
        SidebarsSub::RemoveItem { reference, item } => {
            let id = id_of(&resolve(ctx, &reference)?)?;
            if !confirm(ctx, &format!("item {item} from sidebar {id}"))? {
                return Err(Error::Usage("cancelled".into()));
            }
            ctx.client.delete(&item_path(id, item), &Query::new())?;
            ctx.printer.note(&format!("Removed item {item}"));
            if ctx.printer.format != Format::Table {
                ctx.printer.raw(&json!({ "deleted": true, "id": item }))?;
            }
            Ok(())
        }
        SidebarsSub::MoveItem {
            reference,
            item,
            to,
        } => {
            let id = id_of(&resolve(ctx, &reference)?)?;
            ctx.client
                .patch(&item_path(id, item), &json!({ "order": to }))?;
            let updated = ctx.client.get(&detail_path(id), &Query::new())?.body;
            print_sidebar(ctx, &updated)
        }
        SidebarsSub::SetItems { reference, data } => {
            let id = id_of(&resolve(ctx, &reference)?)?;
            let updated = replace_items(ctx, id, &read_items_json(&data)?)?;
            print_sidebar(ctx, &updated)
        }
    }
}

pub fn detail_path(id: u64) -> String {
    format!("{PATH}{id}/")
}

fn items_path(id: u64) -> String {
    format!("{PATH}{id}/items/")
}

fn item_path(id: u64, item: u64) -> String {
    format!("{PATH}{id}/items/{item}/")
}

pub fn resolve(ctx: &Context, reference: &str) -> Result<Value> {
    if let Ok(id) = reference.trim().parse::<u64>() {
        return Ok(ctx.client.get(&detail_path(id), &Query::new())?.body);
    }
    let found = lookup_one(&ctx.client, PATH, "name", reference.trim(), "sidebar")?;
    // the list omits used_by; fetch the detail
    Ok(ctx
        .client
        .get(&detail_path(id_of(&found)?), &Query::new())?
        .body)
}

fn items_of(sidebar: &Value) -> Vec<Value> {
    sidebar
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn summarize(sidebar: &Value) -> Value {
    let mut out = sidebar.clone();
    out["items_count"] = Value::from(items_of(sidebar).len());
    out
}

/// One line describing an item for the table view.
fn item_summary(item: &Value) -> String {
    if item.get("type").and_then(Value::as_str) == Some("html") {
        let html = item.get("html").and_then(Value::as_str).unwrap_or("");
        let text: String = strip_html(html)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let shown: String = text.chars().take(80).collect();
        return if text.chars().count() > 80 {
            format!("{shown}…")
        } else if shown.is_empty() {
            html.chars().take(80).collect()
        } else {
            shown
        };
    }
    let header = item
        .pointer("/header/text")
        .and_then(Value::as_str)
        .unwrap_or("(no header)");
    let count = item
        .get("links")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    format!("{header}: {count} link(s)")
}

fn strip_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn print_sidebar(ctx: &Context, sidebar: &Value) -> Result<()> {
    if ctx.printer.format == Format::Table {
        let used = sidebar
            .get("used_by")
            .and_then(Value::as_array)
            .map(|pages| {
                // `used_by` holds at most 500 slugs; `used_by_count` is the real total
                let total = sidebar
                    .get("used_by_count")
                    .and_then(Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(pages.len());
                let shown: Vec<String> = pages.iter().take(5).map(cell).collect();
                let more = total.saturating_sub(shown.len());
                let tail = if more > 0 {
                    format!(", +{more} more")
                } else {
                    String::new()
                };
                format!("{total} page(s): {}{tail}", shown.join(", "))
            })
            .unwrap_or_default();
        ctx.printer.note(&format!(
            "{} (id {}){}{}",
            cell(&sidebar["name"]),
            sidebar["id"],
            if sidebar["special"].as_bool().unwrap_or(false) {
                ", built-in"
            } else {
                ""
            },
            if used.is_empty() {
                String::new()
            } else {
                format!(", used by: {used}")
            }
        ));
        let rows: Vec<Value> = items_of(sidebar)
            .iter()
            .map(|i| {
                let mut row = i.clone();
                row["summary"] = Value::String(item_summary(i));
                row
            })
            .collect();
        return ctx.printer.list(&rows, None, ITEM_COLUMNS);
    }
    ctx.printer.raw(sidebar)
}

fn replace_items(ctx: &Context, id: u64, items: &[Value]) -> Result<Value> {
    Ok(ctx
        .client
        .put(&items_path(id), &json!({ "items": items }))?
        .body)
}

fn add_item(ctx: &Context, sidebar: &Value, item: Value, at: Option<usize>) -> Result<()> {
    let id = id_of(sidebar)?;
    let mut item = item;
    if let Some(at) = at {
        item["order"] = Value::from(at);
    }
    let updated = ctx.client.post(&items_path(id), &item)?.body;
    print_sidebar(ctx, &updated)
}

/// `Text=/url/` → {"anchor": "Text", "url": "/url/"}; bare text is a plain entry.
pub fn parse_links(items: &[String]) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for item in items {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        match item.split_once('=') {
            Some((text, url)) if !text.trim().is_empty() => {
                out.push(json!({ "anchor": text.trim(), "url": url.trim() }));
            }
            Some(_) => {
                return Err(Error::Usage(format!(
                    "--link '{item}' has no text before '='"
                )))
            }
            None => out.push(json!({ "anchor": item })),
        }
    }
    Ok(out)
}

fn read_items_json(source: &str) -> Result<Vec<Value>> {
    let text = if let Some(path) = source.strip_prefix('@') {
        html::read_file_or_stdin(path)?
    } else if source == "-" {
        html::read_file_or_stdin("-")?
    } else {
        source.to_string()
    };
    let value: Value = serde_json::from_str(&text)?;
    match value {
        Value::Array(items) => Ok(items),
        Value::Object(ref map) if map.get("items").is_some_and(Value::is_array) => {
            Ok(map["items"].as_array().cloned().unwrap_or_default())
        }
        _ => Err(Error::Usage(
            "--data must be a JSON list of items, or {\"items\": [...]}".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_links() {
        let links =
            parse_links(&["Riverside=/riverside/".into(), "Plain text".into()]).unwrap_or_default();
        assert_eq!(
            links,
            vec![
                json!({"anchor": "Riverside", "url": "/riverside/"}),
                json!({"anchor": "Plain text"})
            ]
        );
        assert!(parse_links(&["=/x/".into()]).is_err());
    }

    #[test]
    fn summarizes_items() {
        assert_eq!(
            item_summary(&json!({"type": "html", "html": "<h2>Hi</h2><p>there</p>"})),
            "Hi there"
        );
        assert_eq!(
            item_summary(
                &json!({"type": "links", "header": {"text": "Areas"}, "links": [{"anchor": "a"}]})
            ),
            "Areas: 1 link(s)"
        );
    }
}
