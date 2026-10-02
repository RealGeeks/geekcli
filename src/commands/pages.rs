//! `geekcli pages …` — content pages. Area pages share almost everything and
//! live in `area_pages.rs` on top of the helpers here.

use clap::{Args, Subcommand, ValueEnum};
use serde_json::{json, Value};

use super::{
    confirm, id_of, list_and_print, lookup_one, parse_id_or_null, print_written, push, Context,
    Paging, Payload,
};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::html;
use crate::output::{col, Column};

pub const PATH: &str = "content/pages/";
pub const AREA_PATH: &str = "content/area-pages/";

pub const LIST_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("path", "/path"),
    col("anchor_text", "/anchor_text"),
    col("title", "/title"),
    col("template", "/template"),
    col("children", "/children_count"),
];

pub const DETAIL_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("path", "/path"),
    col("url", "/url"),
    col("anchor_text", "/anchor_text"),
    col("title", "/title"),
    col("template", "/template"),
    col("parent", "/parent"),
    col("level", "/level"),
    col("children", "/children_count"),
    col("sidebar", "/sidebar/name"),
    col("footer", "/footer/name"),
    col("search", "/search/description"),
    col("landscape", "/landscape_image_override"),
    col("agents", "/agents"),
];

#[derive(Debug, Args)]
pub struct PagesCommand {
    #[command(subcommand)]
    pub command: PagesSub,
}

#[derive(Debug, Subcommand)]
pub enum PagesSub {
    /// List content pages
    List(ListArgs),
    /// Show one page by id, path (/resources/buyers/) or slug
    Get { reference: String },
    #[command(after_help = "Notes:
  - Content is sanitized like a post body (`geekcli guide html`).
  - Themes have no grid helpers you can rely on: a row of cards is <div style=\"display: inline-block; width: 210px; margin: 6px; vertical-align: top\"> inside a centred container; tables overflow phones.
  - Classes from another site (icon-tiles) do nothing unless this theme styles them.
  - Leave link colour to the theme.
  - Template areas (`templates list`): --area \"Agent Name=Jordan Avery\" --area \"Agent Photo=<file URL>\"; an About Page pulls in every Agent Detail Page automatically, so make agents pages, not cards.
  - Contact links: <a class=\"popup\" href=\"/member/contact/\"> opens the contact form as an overlay on designs that support it and works as a plain link elsewhere.
  - --parent takes an id, a /path/ or a slug. Pages nest at most 10 levels deep and a page URL is at most 200 characters.
  - --search-criteria is validated against `search fields`; a rejected key is one the site would have silently ignored.
  - Snapshot the page afterwards.")]
    /// Create a content page
    Create(CreateArgs),
    #[command(after_help = "Notes:
  - Only the flags you pass change.
  - --search-criteria replaces the page's saved search (a page whose search names a field `search fields` does not list shows every listing; fix it here).
  - --sidebar/--footer take an id, a name, or null.
  - --landscape takes a file URL (an .mp4 becomes a video), `none` to hide the header image, or `null` for the site's; add --landscape-alt.
  - Same HTML rules as create (`geekcli guide html`).")]
    /// Change fields on a page (only the flags you pass are changed)
    Update(UpdateArgs),
    /// Delete a page
    Delete {
        reference: String,
        /// Delete even if the page has children; they become top-level pages
        #[arg(long)]
        orphan_children: bool,
    },
    /// Show the saved property search a page displays, with its match count
    Search { reference: String },
    /// List a page's revisions, newest first
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DisplayLocation {
    Above,
    Below,
}

impl DisplayLocation {
    pub fn as_str(self) -> &'static str {
        match self {
            DisplayLocation::Above => "above",
            DisplayLocation::Below => "below",
        }
    }
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[command(flatten)]
    pub tree: TreeListArgs,
    /// Filter by template name
    #[arg(long)]
    pub template: Option<String>,
}

/// List filters shared with area pages.
#[derive(Debug, Args)]
pub struct TreeListArgs {
    /// Exact slug
    #[arg(long)]
    pub slug: Option<String>,
    /// Exact site-relative path, e.g. /resources/buyers/
    #[arg(long)]
    pub path: Option<String>,
    /// Parent page id, or `null` for top-level pages
    #[arg(long, value_name = "ID_OR_NULL")]
    pub parent: Option<String>,
    /// Anchor text, title or slug contains
    #[arg(long, short = 's', value_name = "TEXT")]
    pub search: Option<String>,
    /// No effect: lists never include content (kept so older scripts still run)
    #[arg(long, hide = true)]
    pub no_content: bool,
    #[command(flatten)]
    pub paging: Paging,
}

impl TreeListArgs {
    pub fn query(&self) -> Query {
        let mut query = Query::new();
        push(&mut query, "slug", self.slug.as_deref());
        push(&mut query, "path", self.path.as_deref());
        push(&mut query, "parent", self.parent.as_deref());
        push(&mut query, "q", self.search.as_deref());
        self.paging.apply(&mut query);
        query
    }
}

/// Writable fields shared by content pages and area pages.
#[derive(Debug, Args, Default)]
pub struct TreeFields {
    /// URL segment; letters, numbers, `-`, `_`
    #[arg(long)]
    pub slug: Option<String>,
    /// Parent page: id, path (/resources/), slug, or `null` for top level
    #[arg(long, value_name = "ID_PATH_OR_NULL")]
    pub parent: Option<String>,
    /// Page name used in navigation (required on create)
    #[arg(long)]
    pub anchor_text: Option<String>,
    /// HTML <title> and page heading
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
    /// Heading above the search form
    #[arg(long)]
    pub search_header: Option<String>,
    /// Heading above the property listings
    #[arg(long)]
    pub listing_header: Option<String>,
    /// How many listings to show (default 12)
    #[arg(long)]
    pub number_of_properties: Option<u32>,
    /// Show listings above or below the content (default below)
    #[arg(long, value_enum)]
    pub property_display_location: Option<DisplayLocation>,
    #[command(flatten)]
    pub attach: AttachArgs,
    /// Template area, `Name=value` (see `templates list`); repeatable. `Name=null` clears it
    #[arg(long = "area", value_name = "NAME=VALUE")]
    pub areas: Vec<String>,
    /// Extra fields as a JSON object, `@file`, or `-` for stdin (flags win)
    #[arg(long, value_name = "JSON")]
    pub data: Option<String>,
}

/// `Agent Name=Jordan` → `{"Agent_Name": "Jordan"}`; area names take spaces or underscores.
pub fn areas_object(items: &[String]) -> Result<Value> {
    let mut out = serde_json::Map::new();
    for item in items {
        let Some((name, value)) = item.split_once('=') else {
            return Err(Error::Usage(format!(
                "--area expects NAME=VALUE, got '{item}'"
            )));
        };
        let key = name.trim().replace(' ', "_");
        if key.is_empty() {
            return Err(Error::Usage(format!(
                "--area '{item}' has no name before '='"
            )));
        }
        let value = value.trim();
        out.insert(
            key,
            match value.to_ascii_lowercase().as_str() {
                "null" | "none" => Value::Null,
                _ => Value::String(value.to_string()),
            },
        );
    }
    Ok(Value::Object(out))
}

/// The sidebar and saved search a page displays. Shared with the home page.
#[derive(Debug, Args, Default, Clone)]
pub struct AttachArgs {
    /// Sidebar to show: id, name, or `null` to detach
    #[arg(long, value_name = "ID_NAME_OR_NULL")]
    pub sidebar: Option<String>,
    /// Saved search to show: its short id (from search URLs), numeric id, or `null`
    #[arg(long, value_name = "ID_OR_NULL", conflicts_with = "search_criteria")]
    pub search: Option<String>,
    /// Build the page's search from criteria, key=value; repeatable (see `search fields`)
    #[arg(long = "search-criteria", value_name = "KEY=VALUE")]
    pub search_criteria: Vec<String>,
    /// Footer to show: id, name, or `null` for the default
    #[arg(long, value_name = "ID_NAME_OR_NULL")]
    pub footer: Option<String>,
    /// Which search form the page shows: default or typeahead
    #[arg(long, value_name = "TYPE")]
    pub search_form_type: Option<String>,
    /// Saved search whose criteria pre-fill the form: short id, numeric id, or `null`
    #[arg(
        long,
        value_name = "ID_OR_NULL",
        conflicts_with = "search_field_defaults_criteria"
    )]
    pub search_field_defaults: Option<String>,
    /// Pre-fill the search form from criteria, key=value; repeatable
    #[arg(long = "search-field-defaults-criteria", value_name = "KEY=VALUE")]
    pub search_field_defaults_criteria: Vec<String>,
    /// Header image: a file URL (.jpg/.png/.mp4), `none` to hide it, or `null` for the site's
    #[arg(long, value_name = "URL_NONE_OR_NULL")]
    pub landscape: Option<String>,
    /// Alt text for the header image override
    #[arg(long, value_name = "TEXT")]
    pub landscape_alt: Option<String>,
    /// Override type when the extension is ambiguous: image or video
    #[arg(long, value_name = "TYPE")]
    pub landscape_content_type: Option<String>,
}

impl AttachArgs {
    pub fn apply(&self, ctx: &Context, payload: &mut Payload) -> Result<()> {
        if let Some(sidebar) = &self.sidebar {
            let value = match parse_id_or_null(sidebar) {
                Ok(v) => v,
                Err(_) => Value::from(super::id_of(&super::sidebars::resolve(ctx, sidebar)?)?),
            };
            payload.set_value("sidebar", value);
        }
        if let Some(search) = &self.search {
            payload.set_value("search", search_ref(search));
        }
        if !self.search_criteria.is_empty() {
            payload.set_value("search", criteria_object(&self.search_criteria)?);
        }
        if let Some(footer) = &self.footer {
            let value = match parse_id_or_null(footer) {
                Ok(v) => v,
                Err(_) => Value::from(super::id_of(&super::footers::resolve(ctx, footer)?)?),
            };
            payload.set_value("footer", value);
        }
        if let Some(kind) = &self.search_form_type {
            payload.set("search_form_type", Some(kind.trim().to_ascii_lowercase()));
        }
        if let Some(defaults) = &self.search_field_defaults {
            payload.set_value("search_field_defaults", search_ref(defaults));
        }
        if !self.search_field_defaults_criteria.is_empty() {
            payload.set_value(
                "search_field_defaults",
                criteria_object(&self.search_field_defaults_criteria)?,
            );
        }
        if let Some(landscape) = &self.landscape {
            payload.set_value(
                "landscape_image_override",
                match landscape.trim().to_ascii_lowercase().as_str() {
                    "null" | "default" | "-" => Value::Null,
                    "none" | "hide" | "hidden" => Value::String("none".into()),
                    _ => Value::String(landscape.trim().to_string()),
                },
            );
        }
        payload
            .set(
                "landscape_image_override_alt_text",
                self.landscape_alt.as_deref(),
            )
            .set(
                "landscape_image_override_content_type",
                self.landscape_content_type.as_deref(),
            );
        Ok(())
    }
}

/// `null`, a numeric id, or a short id string.
pub fn search_ref(text: &str) -> Value {
    let trimmed = text.trim();
    match trimmed.to_ascii_lowercase().as_str() {
        "null" | "none" | "-" => Value::Null,
        _ => match trimmed.parse::<u64>() {
            Ok(id) => Value::from(id),
            Err(_) => Value::String(trimmed.to_string()),
        },
    }
}

/// `key=value` items → `{key: [values]}`, the shape the API validates.
pub fn criteria_object(items: &[String]) -> Result<Value> {
    let query = super::search::parse_criteria(items)?;
    let mut criteria = serde_json::Map::new();
    for (k, v) in query {
        if let Some(list) = criteria
            .entry(k)
            .or_insert_with(|| Value::Array(vec![]))
            .as_array_mut()
        {
            list.push(Value::String(v));
        }
    }
    Ok(Value::Object(criteria))
}

impl TreeFields {
    /// Build the payload; `resource_path` is used to resolve a parent given
    /// as a path or slug.
    pub fn payload(&self, ctx: &Context, resource_path: &str) -> Result<Payload> {
        let mut payload = Payload::default();
        payload
            .set("slug", self.slug.as_deref().map(|s| s.trim_matches('/')))
            .set("anchor_text", self.anchor_text.as_deref())
            .set("title", self.title.as_deref())
            .set("meta_description", self.meta_description.as_deref())
            .set("meta_keywords", self.meta_keywords.as_deref())
            .set("search_header", self.search_header.as_deref())
            .set("listing_header", self.listing_header.as_deref())
            .set("number_of_properties", self.number_of_properties)
            .set(
                "property_display_location",
                self.property_display_location.map(DisplayLocation::as_str),
            );
        if let Some(parent) = &self.parent {
            payload.set_value("parent", resolve_parent(ctx, resource_path, parent)?);
        }
        if let Some(content) = html::read_body(
            self.content.as_deref(),
            self.content_file.as_deref(),
            self.markdown,
        )? {
            payload.set("content", Some(content));
        }
        self.attach.apply(ctx, &mut payload)?;
        if !self.areas.is_empty() {
            payload.set_value("extra_content", areas_object(&self.areas)?);
        }
        payload.merge_json(self.data.as_deref())?;
        Ok(payload)
    }
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[command(flatten)]
    pub fields: TreeFields,
    /// Page template; see `geekcli templates list`
    #[arg(long)]
    pub template: Option<String>,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Page id, path or slug
    pub reference: String,
    #[command(flatten)]
    pub fields: TreeFields,
    /// Page template; see `geekcli templates list`
    #[arg(long)]
    pub template: Option<String>,
    /// Send a full replace (PUT): fields you omit reset to their defaults
    #[arg(long)]
    pub replace: bool,
}

pub fn run(ctx: &Context, cmd: PagesCommand) -> Result<()> {
    match cmd.command {
        PagesSub::List(args) => {
            let mut query = args.tree.query();
            push(&mut query, "template", args.template.as_deref());
            list_and_print(ctx, PATH, &query, &args.tree.paging, LIST_COLUMNS)
        }
        PagesSub::Get { reference } => ctx
            .printer
            .one(&resolve(ctx, PATH, &reference, "page")?, DETAIL_COLUMNS),
        PagesSub::Create(args) => {
            let mut payload = args.fields.payload(ctx, PATH)?;
            payload.set("template", args.template.as_deref());
            require(&payload, &["slug", "anchor_text"], "page")?;
            let created = ctx.client.post(PATH, &payload.into_value())?.body;
            print_written(ctx, &created, DETAIL_COLUMNS, "Created")
        }
        PagesSub::Update(args) => {
            let id = id_of(&resolve(ctx, PATH, &args.reference, "page")?)?;
            let mut payload = args.fields.payload(ctx, PATH)?;
            payload.set("template", args.template.as_deref());
            write(
                ctx,
                &detail_path(PATH, id),
                payload,
                args.replace,
                DETAIL_COLUMNS,
            )
        }
        PagesSub::Delete {
            reference,
            orphan_children,
        } => delete(
            ctx,
            PATH,
            &reference,
            "page",
            orphan_children.then_some("orphan_children"),
        ),
        PagesSub::Search { reference } => page_search(ctx, PATH, &reference, "page"),
        PagesSub::Revisions { reference, limit } => {
            let id = id_of(&resolve(ctx, PATH, &reference, "page")?)?;
            super::revisions::list(ctx, &detail_path(PATH, id), limit)
        }
        PagesSub::Revision { reference, rev } => {
            let id = id_of(&resolve(ctx, PATH, &reference, "page")?)?;
            super::revisions::show(ctx, &detail_path(PATH, id), rev)
        }
        PagesSub::Revert { reference, rev } => {
            let page = resolve(ctx, PATH, &reference, "page")?;
            let id = id_of(&page)?;
            let label = format!(
                "page {}",
                page.get("path").and_then(Value::as_str).unwrap_or("")
            );
            super::revisions::revert(ctx, &detail_path(PATH, id), rev, &label)
        }
    }
}

/// Describe the saved search a page displays, with its live match count.
pub fn page_search(ctx: &Context, resource_path: &str, reference: &str, label: &str) -> Result<()> {
    let page = resolve(ctx, resource_path, reference, label)?;
    let search_id = page
        .pointer("/search/id")
        .and_then(Value::as_u64)
        .or_else(|| page.get("search_id").and_then(Value::as_u64));
    let Some(search_id) = search_id else {
        return Err(Error::Api {
            status: 404,
            code: "no_search".into(),
            message: format!(
                "{label} {} has no saved search attached",
                page.get("path").and_then(Value::as_str).unwrap_or("")
            ),
            fields: std::collections::BTreeMap::new(),
            retry_after: None,
        });
    };
    let mut doc = super::search::describe_saved(ctx, search_id)?;
    doc["page"] = json!({ "id": page.get("id"), "path": page.get("path") });
    ctx.printer.raw(&doc)
}

pub fn detail_path(resource_path: &str, id: u64) -> String {
    format!("{resource_path}{id}/")
}

pub fn require(payload: &Payload, names: &[&str], label: &str) -> Result<()> {
    for name in names {
        if !payload.contains(name) {
            return Err(Error::Usage(format!(
                "--{} is required to create a {label}",
                name.replace('_', "-")
            )));
        }
    }
    Ok(())
}

pub fn write(
    ctx: &Context,
    path: &str,
    payload: Payload,
    replace: bool,
    columns: &[Column],
) -> Result<()> {
    if payload.is_empty() {
        return Err(Error::Usage(
            "nothing to update: pass at least one field flag or --data".into(),
        ));
    }
    let updated = if replace {
        ctx.client.put(path, &payload.into_value())?.body
    } else {
        ctx.client.patch(path, &payload.into_value())?.body
    };
    print_written(ctx, &updated, columns, "Updated")
}

pub fn delete(
    ctx: &Context,
    resource_path: &str,
    reference: &str,
    label: &str,
    flag: Option<&str>,
) -> Result<()> {
    let page = resolve(ctx, resource_path, reference, label)?;
    let id = id_of(&page)?;
    let path = page.get("path").and_then(Value::as_str).unwrap_or("");
    if !confirm(ctx, &format!("{label} {id} ({path})"))? {
        return Err(Error::Usage("cancelled".into()));
    }
    let mut query = Query::new();
    if let Some(flag) = flag {
        query.push((flag.to_string(), "true".to_string()));
    }
    ctx.client.delete(&detail_path(resource_path, id), &query)?;
    ctx.printer.note(&format!("Deleted {label} {id}"));
    if ctx.printer.format != crate::output::Format::Table {
        ctx.printer.raw(&json!({ "deleted": true, "id": id }))?;
    }
    Ok(())
}

/// A page by id, `/path/`, or slug.
pub fn resolve(ctx: &Context, resource_path: &str, reference: &str, label: &str) -> Result<Value> {
    let reference = reference.trim();
    if let Ok(id) = reference.parse::<u64>() {
        return Ok(ctx
            .client
            .get(&detail_path(resource_path, id), &Query::new())?
            .body);
    }
    if reference.starts_with('/') {
        return lookup_one(&ctx.client, resource_path, "path", reference, label);
    }
    lookup_one(&ctx.client, resource_path, "slug", reference, label)
}

/// A parent given as id, null, path or slug. Paths are looked up on the
/// same resource first, then the other tree resource, because a content
/// page may sit under an area page and vice versa.
fn resolve_parent(ctx: &Context, resource_path: &str, reference: &str) -> Result<Value> {
    if let Ok(value) = parse_id_or_null(reference) {
        return Ok(value);
    }
    let other = if resource_path == PATH {
        AREA_PATH
    } else {
        PATH
    };
    let found = resolve(ctx, resource_path, reference, "parent page")
        .or_else(|first| resolve(ctx, other, reference, "parent page").map_err(|_| first))?;
    Ok(Value::from(id_of(&found)?))
}
