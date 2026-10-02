//! `geekcli nav …` — navigation bars. The bars are fixed (one per
//! position); the API edits their links.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};

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
    col("duplicate_of", "/duplicate_of"),
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
  - Real Geeks recommends 5-6 links in the top bar and 6-8 in the bottom bar, short anchor text and slug-only URLs for your own pages. Going over prints a warning on stderr; the link is still added.
  - A link whose URL is already on the bar is refused (exit 6, nothing sent) and the error names the existing link. URLs match ignoring case, surrounding space, a trailing slash, the scheme and this site's own host; a second --contact link counts as a duplicate too. Pass --allow-duplicate to add it anyway.
  - Bars are fixed per position; you edit links.
  - Refer to a link by id, its text or its URL.
  - Some bars are hidden on some designs (top_secondary on anna), matching the admin.
  - Point agent links at a page that exists: `pages create` first, then `nav update top_primary \"Meet Riley\" --text ... --url ...`.")]
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
    #[command(after_help = "Notes:
  - The list is sent as given. Repeated URLs in it, and more links than Real Geeks recommends (top bar 5-6, bottom bar 6-8), print warnings on stderr; neither blocks the write.
  - `--data '[{\"id\": 12}, {\"id\": 10}]'` keeps those links and reorders them.")]
    Set {
        bar: String,
        /// JSON list of {type, url, anchor_text, nofollow}, `@file`, or `-` for stdin
        #[arg(long, value_name = "JSON")]
        data: String,
    },
    /// Remove every link from a bar
    Clear { bar: String },
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
    /// Add the link even if the bar already has one with the same URL
    #[arg(long)]
    pub allow_duplicate: bool,
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
            warn_repeats(ctx, &bar, &links);
            warn_crowded(&updated);
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

/// Links per bar Real Geeks recommends, as (most, "range"), by bar type.
fn recommended(bar: &Value) -> Option<(usize, &'static str)> {
    match bar.get("type").and_then(Value::as_str)? {
        "top_primary" | "top_secondary" => Some((6, "5-6")),
        "bottom_primary" | "bottom_secondary" => Some((8, "6-8")),
        _ => None,
    }
}

/// Warn on stderr when a bar holds more links than recommended. Never blocks.
fn warn_crowded(bar: &Value) {
    let count = bar_links(bar).len();
    if let Some((most, range)) = recommended(bar) {
        if count > most {
            eprintln!(
                "warning: {} has {count} links; Real Geeks recommends {range}",
                label(bar)
            );
        }
    }
}

/// What makes two links the same. The contact link is one link; custom links
/// compare by URL, ignoring surrounding space, case, the scheme, a trailing
/// slash and this site's own host (so `https://www.site.com/a/` is `/a`).
fn link_key(link: &Value, site_host: Option<&str>) -> Option<String> {
    if link.get("type").and_then(Value::as_str) == Some("contact") {
        return Some("contact".into());
    }
    let url = link
        .get("url")
        .and_then(Value::as_str)?
        .trim()
        .to_ascii_lowercase();
    if url.is_empty() {
        return None;
    }
    let (target, suffix) = match url.find(['?', '#']) {
        Some(at) => url.split_at(at),
        None => (url.as_str(), ""),
    };
    let target = match ["https://", "http://", "//"]
        .iter()
        .find_map(|scheme| target.strip_prefix(scheme))
    {
        Some(rest) => {
            let (host, path) = rest.find('/').map_or((rest, ""), |at| rest.split_at(at));
            let bare = |h: &str| {
                let name = h.split(':').next().unwrap_or(h);
                name.strip_prefix("www.").unwrap_or(name).to_string()
            };
            if site_host.is_some_and(|site| bare(site) == bare(host)) {
                path.to_string()
            } else {
                format!("{host}{path}")
            }
        }
        None if target.starts_with('/') || target.contains(':') => target.to_string(),
        None => format!("/{target}"),
    };
    let target = target.trim_end_matches('/');
    let target = if target.is_empty() { "/" } else { target };
    Some(format!("{target}{suffix}"))
}

/// For each link that repeats an earlier one: (its index, the earlier index).
fn duplicates(links: &[Value], site_host: Option<&str>) -> Vec<(usize, usize)> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    for (i, link) in links.iter().enumerate() {
        if let Some(key) = link_key(link, site_host) {
            match seen.entry(key) {
                Entry::Occupied(first) => out.push((i, *first.get())),
                Entry::Vacant(slot) => {
                    slot.insert(i);
                }
            }
        }
    }
    out
}

/// `link 12 "Luxury" (/luxury/)`, for messages.
fn describe(link: &Value) -> String {
    let mut parts = vec!["link".to_string()];
    if let Some(id) = link.get("id").and_then(Value::as_u64) {
        parts.push(id.to_string());
    }
    if link.get("type").and_then(Value::as_str) == Some("contact") {
        parts.push("(the contact link)".into());
    } else {
        if let Some(text) = link.get("anchor_text").and_then(Value::as_str) {
            parts.push(format!("\"{text}\""));
        }
        if let Some(url) = link.get("url").and_then(Value::as_str) {
            parts.push(format!("({url})"));
        }
    }
    parts.join(" ")
}

fn duplicate_error(bar: &Value, existing: &Value) -> Error {
    let field = if existing.get("type").and_then(Value::as_str) == Some("contact") {
        "type"
    } else {
        "url"
    };
    let what = describe(existing);
    Error::Api {
        status: 409,
        code: "duplicate_link".into(),
        message: format!(
            "{} already has {what}; pass --allow-duplicate to add another, or change that link with `nav update`",
            label(bar)
        ),
        fields: BTreeMap::from([(field.to_string(), vec![format!("already on the bar as {what}")])]),
        retry_after: None,
    }
}

/// Warn about repeated URLs in a `nav set` list. Entries given only by id
/// are looked up on the current bar. Never blocks.
fn warn_repeats(ctx: &Context, bar: &Value, links: &[Value]) {
    let current = bar_links(bar);
    let effective: Vec<Value> = links
        .iter()
        .map(|link| {
            let id = link.get("id").and_then(Value::as_u64);
            let mut merged = current
                .iter()
                .find(|l| id.is_some() && l.get("id").and_then(Value::as_u64) == id)
                .cloned()
                .unwrap_or_else(|| json!({}));
            if let (Some(out), Some(given)) = (merged.as_object_mut(), link.as_object()) {
                for (k, v) in given {
                    out.insert(k.clone(), v.clone());
                }
            }
            merged
        })
        .collect();
    let host = ctx.client.site_host();
    for (i, first) in duplicates(&effective, host.as_deref()) {
        eprintln!(
            "warning: --data item {i} repeats item {first}: {} and {}",
            describe(&effective[i]),
            describe(&effective[first])
        );
    }
}

/// The bar with `duplicate_of: <id>` on every link that repeats an earlier one.
fn mark_duplicates(ctx: &Context, bar: &Value) -> Value {
    let links = bar_links(bar);
    let host = ctx.client.site_host();
    let mut out = bar.clone();
    for (i, first) in duplicates(&links, host.as_deref()) {
        let Some(id) = links[first].get("id").cloned() else {
            continue;
        };
        if let Some(link) = out
            .get_mut("links")
            .and_then(|l| l.get_mut(i))
            .and_then(Value::as_object_mut)
        {
            link.insert("duplicate_of".into(), id);
        }
    }
    out
}

fn print_bar(ctx: &Context, bar: &Value) -> Result<()> {
    let bar = &mark_duplicates(ctx, bar);
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
    if !args.allow_duplicate {
        let host = ctx.client.site_host();
        let key = link_key(&link, host.as_deref());
        if let Some(existing) = bar_links(&bar)
            .iter()
            .find(|l| key.is_some() && link_key(l, host.as_deref()) == key)
        {
            return Err(duplicate_error(&bar, existing));
        }
    }
    let mut link = link;
    if let Some(at) = args.at {
        link["order"] = Value::from(at);
    }
    let updated = ctx.client.post(&links_path(&bar)?, &link)?.body;
    warn_crowded(&updated);
    print_bar(ctx, &updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(url: &str) -> Option<String> {
        link_key(
            &json!({ "type": "custom", "url": url }),
            Some("www.example.com"),
        )
    }

    #[test]
    fn link_keys_ignore_case_space_slash_scheme_and_own_host() {
        let same = [
            "/luxury/",
            " /Luxury ",
            "luxury",
            "https://www.example.com/luxury/",
            "http://example.com/LUXURY",
        ];
        for url in same {
            assert_eq!(key(url).as_deref(), Some("/luxury"), "{url}");
        }
        assert_eq!(key("/").as_deref(), Some("/"));
        assert_eq!(key("https://www.example.com").as_deref(), Some("/"));
        assert_eq!(key("/search/?city=X").as_deref(), Some("/search?city=x"));
        assert_eq!(
            key("https://other.org/luxury/").as_deref(),
            Some("other.org/luxury")
        );
        assert_eq!(key("  "), None);
        assert_eq!(
            link_key(&json!({ "type": "contact" }), None).as_deref(),
            Some("contact")
        );
    }

    #[test]
    fn crowding_follows_the_bar_position() {
        assert_eq!(
            recommended(&json!({ "type": "top_primary" })),
            Some((6, "5-6"))
        );
        assert_eq!(
            recommended(&json!({ "type": "bottom_secondary" })),
            Some((8, "6-8"))
        );
        assert_eq!(recommended(&json!({ "type": "seller_leads" })), None);
    }
}
