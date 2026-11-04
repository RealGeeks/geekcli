//! `geekcli home-page …` — the site's single home page.

use clap::{Args, Subcommand};
use serde_json::{json, Map, Value};

use super::pages::DisplayLocation;
use super::{parse_bool, print_written, Context, Payload};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::html;
use crate::output::{col, Column};

pub const PATH: &str = "content/home-page/";
const SECTIONS_PATH: &str = "content/home-page/sections/";

pub const COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("url", "/url"),
    col("title", "/title"),
    col("search_header", "/search_header"),
    col("search_subheader", "/search_subheader"),
    col("listing_header", "/listing_header"),
    col("sidebar", "/sidebar/name"),
    col("footer", "/footer/name"),
    col("banner", "/banner/name"),
    col("search", "/search/description"),
    col("tile_group", "/tile_group/title"),
    col("featured_agents", "/featured_agents"),
    col("sections", "/sections"),
    col("landscape", "/landscape_image_override"),
];

/// alfred's list modules: the row kind the API takes, and the module it fills.
const KINDS: &[(&str, &str)] = &[
    ("stat", "recognition"),
    ("award", "about"),
    ("press", "press"),
    ("tile", "buy-sell-value"),
    ("testimonial", "testimonials"),
    ("neighborhood", "neighborhoods"),
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
    /// alfred: the module list, every list module's rows, and which modules have nothing to show
    #[command(after_help = "Notes:
  - `sections` is the module order; `empty_sections` names each module with nothing to show (left off the page even when switched on) and what it needs.
  - Each entry of `results` is one row kind with its columns, its rows and its limit. The press entry's `library` has ready-made publication logos.
  - `geekcli guide alfred` maps what people call each part of the page to where it lives.")]
    Sections,
    /// alfred: read or replace the rows of one list module (stats, press logos, Buy/Sell cards...)
    Items(ItemsCommand),
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
  - --property-display-type, --search-form-tabs and --tile-group render on anna-modern only; build the tiles with `geekcli featured` first. alfred's Buy and Sell cards are `home-page items set tile`, not --tile-group.
  - --featured-agents is the full list of agent cards (photo and bio) on the home page: Agent Detail pages by id, /path/ or slug, comma-separated; `null` removes them all. These are content pages (`pages list --template \"Agent Detail Page\"`), not CRM agents from `agents`. The list picks who shows, not the order. A card's name, bio and photo come from that page: change them with `pages update`. Renders on anna-modern, and in the anna sidebar with the ENABLE_FEATURED_AGENTS_IN_SIDEBAR setting. On alfred the first one is the About module (when the page has no content).
  - alfred: --sections, --section-effect and --area; `geekcli guide alfred`. A module switched on with nothing to show stays off the page without an error: a `note:` line on stderr says which and what it needs (`empty_sections` in the result), and another names any choice that does nothing yet (`inactive_settings`).
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
    /// The page's main hero heading (above the search form); unset, the site shows the BIG_SEARCH_TITLE setting ("Real Estate Search"). Use the page's target keyword
    #[arg(long, visible_alias = "page-heading", value_name = "TEXT")]
    #[arg(help_heading = super::heading::SEARCH)]
    pub search_header: Option<String>,
    /// Text under the search heading
    #[arg(long)]
    #[arg(help_heading = super::heading::SEARCH)]
    pub search_subheader: Option<String>,
    /// Heading above the property listings
    #[arg(long)]
    #[arg(help_heading = super::heading::SEARCH)]
    pub listing_header: Option<String>,
    /// How many listings to show
    #[arg(long)]
    #[arg(help_heading = super::heading::SEARCH)]
    pub number_of_properties: Option<u32>,
    /// Show listings above or below the content
    #[arg(long, value_enum)]
    #[arg(help_heading = super::heading::SEARCH)]
    pub property_display_location: Option<DisplayLocation>,
    #[command(flatten)]
    pub attach: super::pages::AttachArgs,
    /// How listings display: carousel, grid, or `null` (anna-modern designs)
    #[arg(long, value_name = "TYPE")]
    #[arg(help_heading = super::heading::SEARCH)]
    pub property_display_type: Option<String>,
    /// Show the Advanced Search / Sell Your Home tabs above the form (anna-modern)
    #[arg(long, value_name = "BOOL", value_parser = parse_bool)]
    #[arg(help_heading = super::heading::SEARCH)]
    pub search_form_tabs: Option<bool>,
    /// Image shown in the hero (logo-style, not the background): a file URL, or `null` to clear
    #[arg(long, value_name = "URL_OR_NULL")]
    #[arg(help_heading = super::heading::SEARCH)]
    pub search_image: Option<String>,
    /// Featured Pages tile group to show (anna-modern): id, title, or `null`
    #[arg(long, value_name = "ID_TITLE_OR_NULL")]
    #[arg(help_heading = super::heading::LAYOUT)]
    pub tile_group: Option<String>,
    /// Agent Detail pages to feature with photo and bio (the full list): ids, paths or slugs, comma-separated, or `null` for none
    #[arg(long, value_name = "PAGES_OR_NULL")]
    #[arg(help_heading = super::heading::LAYOUT)]
    pub featured_agents: Option<String>,
    /// alfred: the modules between the hero and the contact form, top to bottom, comma-separated (left out = off), or `none`
    #[arg(long, value_name = "MODULES")]
    #[arg(help_heading = super::heading::ALFRED)]
    pub sections: Option<String>,
    /// alfred: a module's scroll effect, MODULE=EFFECT (fade, stagger, parallax, pinned, unveil, watermark, horizontal, none); repeatable, the other modules keep theirs
    #[arg(long, value_name = "MODULE=EFFECT")]
    #[arg(help_heading = super::heading::ALFRED)]
    pub section_effect: Vec<String>,
    /// alfred: an extra content area, KEY=VALUE (e.g. Hero_Eyebrow=..., Listings_Layout=carousel), `KEY=null` clears it; repeatable. `home-page get` lists them in extra_content_areas
    #[arg(long, value_name = "KEY=VALUE")]
    #[arg(help_heading = super::heading::ALFRED)]
    pub area: Vec<String>,
    /// Extra fields as a JSON object, `@file`, or `-` for stdin (flags win)
    #[arg(long, value_name = "JSON")]
    pub data: Option<String>,
}

#[derive(Debug, Args)]
pub struct ItemsCommand {
    #[command(subcommand)]
    pub command: ItemsSub,
}

#[derive(Debug, Subcommand)]
pub enum ItemsSub {
    /// Show one kind's rows and the columns it uses
    Get {
        /// stat, award, press, tile, testimonial or neighborhood
        kind: String,
    },
    /// Replace one kind's rows (the full list, in order: read them first and send every row back)
    #[command(after_help = "Notes:
  - Kinds: stat (Recognition band: title = figure, text = label; up to 4), award (under the About bio: title, text = year; up to 3), press (logos: title, image, link; up to 8), tile (the Buy and Sell cards: text = eyebrow, title, link, meta = button label, square image; 2), testimonial (text = quote, title = name, meta = role), neighborhood (an image per featured area: meta = attribute||value, image, text = hook, title, link; up to 6).
  - `home-page items get <kind>` lists the columns a kind uses; others are refused.
  - Turning the module on or off is `home-page update --sections`.
  - Example: geekcli home-page items set tile --data '[{\"text\":\"Buy\",\"title\":\"Find the one.\",\"link\":\"/search/\",\"meta\":\"Start your search\"}]'")]
    Set {
        /// stat, award, press, tile, testimonial or neighborhood
        kind: String,
        /// JSON list of rows, `{\"items\": [...]}`, `@file`, or `-` for stdin; `[]` clears the module
        #[arg(long, value_name = "JSON")]
        data: String,
    },
}

pub fn run(ctx: &Context, cmd: HomePageCommand) -> Result<()> {
    match cmd.command {
        HomePageSub::Revisions { limit } => super::revisions::list(ctx, PATH, limit),
        HomePageSub::Revision { rev } => super::revisions::show(ctx, PATH, rev),
        HomePageSub::Revert { rev } => super::revisions::revert(ctx, PATH, rev, "the home page"),
        HomePageSub::Get => ctx
            .printer
            .one(&ctx.client.get(PATH, &Query::new())?.body, COLUMNS),
        HomePageSub::Sections => {
            let sections = ctx.client.get(SECTIONS_PATH, &Query::new())?.body;
            report_empty(&sections, None);
            ctx.printer.raw(&sections)
        }
        HomePageSub::Items(items) => run_items(ctx, items),
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
            if let Some(agents) = &args.featured_agents {
                payload.set_value("featured_agents", featured_agents(ctx, agents)?);
            }
            if let Some(sections) = &args.sections {
                payload.set_value("sections", sections_value(sections));
            }
            if !args.section_effect.is_empty() {
                payload.set_value(
                    "section_effects",
                    section_effects(ctx, &args.section_effect)?,
                );
            }
            if !args.area.is_empty() {
                payload.set_value("extra_content", areas(&args.area)?);
            }
            payload.merge_json(args.data.as_deref())?;
            if payload.is_empty() {
                return Err(Error::Usage(
                    "nothing to update: pass at least one field flag or --data".into(),
                ));
            }
            let updated = ctx.client.patch(PATH, &payload.into_value())?.body;
            report_empty(&updated, None);
            print_written(ctx, &updated, COLUMNS, "Updated")
        }
    }
}

/// `--featured-agents`: Agent Detail pages by id, path or slug, as the list
/// of page ids the API takes; `null` (or nothing) is the empty list.
fn featured_agents(ctx: &Context, text: &str) -> Result<serde_json::Value> {
    if matches!(
        text.trim().to_ascii_lowercase().as_str(),
        "null" | "none" | "-" | ""
    ) {
        return Ok(serde_json::Value::Array(Vec::new()));
    }
    let ids = text
        .split(',')
        .map(str::trim)
        .filter(|reference| !reference.is_empty())
        .map(|reference| super::pages::resolve_id(ctx, super::pages::PATH, reference, "page"))
        .collect::<Result<Vec<u64>>>()?;
    Ok(serde_json::Value::from(ids))
}

fn run_items(ctx: &Context, cmd: ItemsCommand) -> Result<()> {
    match cmd.command {
        ItemsSub::Get { kind } => {
            let kind = check_kind(&kind)?;
            let body = ctx
                .client
                .get(&format!("{SECTIONS_PATH}{kind}/"), &Query::new())?
                .body;
            ctx.printer.raw(&body)
        }
        ItemsSub::Set { kind, data } => {
            let kind = check_kind(&kind)?;
            let items = read_items_json(&data)?;
            let body = ctx
                .client
                .put(
                    &format!("{SECTIONS_PATH}{kind}/"),
                    &json!({ "items": items }),
                )?
                .body;
            ctx.printer
                .note(&format!("Saved {} {kind} row(s)", items.len()));
            // the rows alone don't say whether the module shows: ask the page
            if let Ok(sections) = ctx.client.get(SECTIONS_PATH, &Query::new()) {
                report_empty(&sections.body, module_of(kind));
            }
            ctx.printer.raw(&body)
        }
    }
}

fn check_kind(kind: &str) -> Result<&'static str> {
    let wanted = kind.trim().to_ascii_lowercase();
    KINDS
        .iter()
        .find(|(k, _)| *k == wanted)
        .map(|(k, _)| *k)
        .ok_or_else(|| {
            Error::Usage(format!(
                "unknown kind {kind:?}: one of {}",
                KINDS.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", ")
            ))
        })
}

fn module_of(kind: &str) -> Option<&'static str> {
    KINDS.iter().find(|(k, _)| *k == kind).map(|(_, m)| *m)
}

fn read_items_json(source: &str) -> Result<Vec<Value>> {
    let text = if let Some(path) = source.strip_prefix('@') {
        html::read_file_or_stdin(path)?
    } else if source == "-" {
        html::read_file_or_stdin("-")?
    } else {
        source.to_string()
    };
    match serde_json::from_str::<Value>(&text)? {
        Value::Array(items) => Ok(items),
        Value::Object(map) if map.get("items").is_some_and(Value::is_array) => {
            Ok(map["items"].as_array().cloned().unwrap_or_default())
        }
        _ => Err(Error::Usage(
            "--data must be a JSON list of rows, or {\"items\": [...]}".into(),
        )),
    }
}

/// `--sections a,b,c`: the module keys in order; `none` switches them all off.
fn sections_value(text: &str) -> Value {
    if matches!(
        text.trim().to_ascii_lowercase().as_str(),
        "none" | "null" | "-" | ""
    ) {
        return Value::Array(Vec::new());
    }
    Value::from(
        text.split(',')
            .map(|key| key.trim().to_ascii_lowercase())
            .filter(|key| !key.is_empty())
            .collect::<Vec<_>>(),
    )
}

/// `--section-effect MODULE=EFFECT`, merged onto the page's current effects:
/// the API takes the whole object and puts a module left out back on fade.
fn section_effects(ctx: &Context, pairs: &[String]) -> Result<Value> {
    let current = ctx.client.get(PATH, &Query::new())?.body;
    let mut effects = current
        .get("section_effects")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for pair in pairs {
        let (module, effect) = split_pair(pair, "--section-effect", "MODULE=EFFECT")?;
        effects.insert(
            module.to_ascii_lowercase(),
            Value::from(effect.to_ascii_lowercase()),
        );
    }
    Ok(Value::Object(effects))
}

/// `--area KEY=VALUE`: extra content areas, merged by the API; `KEY=null` clears one.
fn areas(pairs: &[String]) -> Result<Value> {
    let mut out = Map::new();
    for pair in pairs {
        let (key, value) = split_pair(pair, "--area", "KEY=VALUE")?;
        let value = if value.eq_ignore_ascii_case("null") {
            Value::Null
        } else {
            Value::from(value)
        };
        out.insert(key.to_string(), value);
    }
    Ok(Value::Object(out))
}

fn split_pair<'a>(pair: &'a str, flag: &str, shape: &str) -> Result<(&'a str, &'a str)> {
    match pair.split_once('=') {
        Some((key, value)) if !key.trim().is_empty() => Ok((key.trim(), value.trim())),
        _ => Err(Error::Usage(format!("{flag} takes {shape}, got {pair:?}"))),
    }
}

/// `note:` lines on stderr, in every output mode (agents run in JSON mode
/// and need them most), for what alfred leaves off the page without an
/// error: each switched-on module with nothing to show and what it needs
/// (`empty_sections`), and each choice that does nothing yet
/// (`inactive_settings`). With `only`, just that module.
fn report_empty(body: &Value, only: Option<&str>) {
    let on: Vec<&str> = body
        .get("sections")
        .and_then(Value::as_array)
        .map(|keys| keys.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if let Some(empty) = body.get("empty_sections").and_then(Value::as_object) {
        for (module, why) in empty {
            let wanted = only.is_none_or(|m| m == module);
            if let (true, true, Some(why)) = (wanted, on.contains(&module.as_str()), why.as_str()) {
                eprintln!(
                    "note: the {module} module is switched on but not on the page: {}",
                    crate::output::sanitize(why)
                );
            }
        }
    }
    if only.is_none() {
        if let Some(inactive) = body.get("inactive_settings").and_then(Value::as_object) {
            for (setting, why) in inactive {
                if let Some(why) = why.as_str() {
                    eprintln!(
                        "note: {setting} does nothing yet: {}",
                        crate::output::sanitize(why)
                    );
                }
            }
        }
    }
}
