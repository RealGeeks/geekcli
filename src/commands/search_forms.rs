//! `geekcli search-forms …` — the site's search forms: the advanced search
//! tabs (`pages`) and the home page search (`home_page_forms`), which
//! fields each shows and how. The whole document is read and sent back, so
//! `update` takes JSON rather than flags.

use clap::{Args, Subcommand};
use serde_json::{json, Value};

use super::{confirm, Context};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::html;
use crate::output::{col, Column, Format};

pub const PATH: &str = "search-forms/";

pub const PAGE_COLUMNS: &[Column] = &[
    col("form", "/form"),
    col("id", "/id"),
    col("name", "/name"),
    col("fields", "/field_count"),
    col("ready-made", "/prepackaged"),
];

pub const FIELD_COLUMNS: &[Column] = &[
    col("key", "/key"),
    col("label", "/label"),
    col("group", "/group"),
    col("kind", "/kind"),
    col("choices", "/choices"),
];

const RULES: &str = "Notes:
  - Workflow: `search-forms get > forms.json`, edit it, `search-forms update --data @forms.json --dry-run`, then the same without --dry-run.
  - `pages` is the full list of advanced search tabs in order: a page left out is removed, a page without an id is created (its id comes from its name). `home_page_forms` can be changed, not added or removed. Leave either key out to keep it.
  - An item is {\"field\": \"city\", \"label\": ..., \"widget\": ...}; a min/max field (price, beds, area) is one item: {\"field\": \"beds\", \"range\": \"both\"|\"min\"|\"max\", \"label_min\": ..., \"label_max\": ...}.
  - Each page's `rules` say what its layout shows; anything else is refused (exit 5): advanced pages show only drop-downs and lists (select, multiselect) in `primary`; home page forms show only price, beds, baths and property types in `secondary`; some labels (Price, Sold Within) are written by the theme.
  - Built-in filters (`\"fixed\": true` items, such as Condo Search's) can't be removed: remove the whole page instead.
  - MLS Number Search and Address Search are ready-made (`\"prepackaged\": true`): nothing on them changes. Leave one out to remove it; add it back with just {\"id\": \"mls_search\"}. `prepackaged_pages` lists what the site can add, including the board's filter pages.
  - A field whose choices come from the MLS is refused when it would load more than 3,000 of them (street names): `search-forms fields` shows each one's `choices`.
  - Warnings, not errors: `long_form` (more than 14 drop-downs on a page) and `area_directory_changes` (Advanced Search starts with a different field, which reorganizes the area pages' Browse All directory).
  - The first save copies the board's forms onto the site; from then on board changes no longer reach it. `search-forms reset` goes back to the board's forms.";

#[derive(Debug, Args)]
pub struct SearchFormsCommand {
    #[command(subcommand)]
    pub command: SearchFormsSub,
}

#[derive(Debug, Subcommand)]
pub enum SearchFormsSub {
    /// Show the search forms: every page with its fields, the field catalog and the rules
    #[command(after_help = RULES)]
    Get,
    /// List the fields the site's MLS offers for a form
    #[command(after_help = "Notes:
  - `kind` is choice, checkbox, text or range. A choice field lists its `widgets`; one whose choices come from the MLS (`from_mls`) gives its number of `choices` with nothing narrowing it.
  - Over 3,000 choices only works under a parent field that narrows it (`narrowed_by_parent`), such as Subdivision under City.")]
    Fields(FieldsArgs),
    /// Replace the search forms from JSON, `@file`, or `-`
    #[command(after_help = RULES)]
    Update(UpdateArgs),
    /// Throw away the site's own search forms and use its MLS board's again
    Reset,
    /// List changes to the search forms, newest first
    #[command(after_help = "Notes:
  - Every change to the site's search forms, whoever made it (the site backend, support in MCP, this CLI).")]
    Revisions {
        /// Show only the latest N
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
    },
    /// Show one revision with the search pages now and after a revert
    Revision { rev: u64 },
    /// Undo a revision and everything after it (the revert is itself undoable)
    #[command(after_help = "Notes:
  - Reverting the first change puts the site back on its board's forms.
  - 409 (exit 6) when there is nothing left to undo.")]
    Revert { rev: u64 },
}

#[derive(Debug, Args)]
pub struct FieldsArgs {
    /// Key or label contains
    #[arg(long, short = 's', value_name = "TEXT")]
    pub search: Option<String>,
    /// Only this group (Location, Property type, Price & size, Features, Other)
    #[arg(long, value_name = "GROUP")]
    pub group: Option<String>,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// {"pages": [...], "home_page_forms": [...]} as JSON, `@file`, or `-` for stdin
    #[arg(long, value_name = "JSON")]
    pub data: String,
    /// Check the change (every rule, including MLS choice counts) and save nothing
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(ctx: &Context, cmd: SearchFormsCommand) -> Result<()> {
    match cmd.command {
        SearchFormsSub::Get => {
            let body = ctx.client.get(PATH, &Query::new())?.body;
            print_forms(ctx, &body)
        }
        SearchFormsSub::Fields(args) => {
            let body = ctx.client.get(PATH, &Query::new())?.body;
            let mut rows = body
                .get("fields")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if let Some(group) = &args.group {
                let g = group.to_lowercase();
                rows.retain(|f| text(f, "group").to_lowercase().contains(&g));
            }
            if let Some(needle) = &args.search {
                let n = needle.to_lowercase();
                rows.retain(|f| {
                    ["key", "label"]
                        .iter()
                        .any(|k| text(f, k).to_lowercase().contains(&n))
                });
            }
            ctx.printer.list(&rows, None, FIELD_COLUMNS)
        }
        SearchFormsSub::Update(args) => {
            let raw = if let Some(path) = args.data.strip_prefix('@') {
                html::read_file_or_stdin(path)?
            } else if args.data == "-" {
                html::read_file_or_stdin("-")?
            } else {
                args.data.clone()
            };
            let body: Value = serde_json::from_str(&raw)
                .map_err(|e| Error::Usage(format!("--data is not valid JSON: {e}")))?;
            if !body.is_object() {
                return Err(Error::Usage(
                    "--data must be an object with \"pages\" and/or \"home_page_forms\"".into(),
                ));
            }
            let path = if args.dry_run {
                format!("{PATH}?dry_run=1")
            } else {
                PATH.to_string()
            };
            let result = ctx.client.put(&path, &body)?.body;
            ctx.printer.note(if args.dry_run {
                "Dry run: every check passed and nothing was saved."
            } else {
                "Saved. Visitors see the new search forms within a few seconds."
            });
            print_forms(ctx, &result)
        }
        SearchFormsSub::Reset => {
            if !confirm(
                ctx,
                "the site's own search forms (the site goes back to its MLS board's forms)",
            )? {
                return Err(Error::Usage("cancelled".into()));
            }
            let result = ctx.client.delete(PATH, &Query::new())?.body;
            ctx.printer
                .note("The site uses its MLS board's search forms again.");
            print_forms(ctx, &result)
        }
        SearchFormsSub::Revisions { limit } => super::revisions::list(ctx, PATH, limit),
        SearchFormsSub::Revision { rev } => {
            let body = ctx
                .client
                .get(&format!("{PATH}revisions/{rev}/"), &Query::new())?
                .body;
            if ctx.printer.format != Format::Table {
                return ctx.printer.raw(&body);
            }
            let preview = body.get("preview").cloned().unwrap_or(Value::Null);
            let describe = |v: &Value| {
                let pages: Vec<&str> = v
                    .get("pages")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                format!("{} forms: {}", text(v, "source"), pages.join(", "))
            };
            ctx.printer.note(&format!(
                "Revision {} · {}",
                crate::output::cell(&body["id"]),
                text(&body, "message")
            ));
            let rows = vec![
                json!({"field": "now", "value": describe(&preview["now"])}),
                json!({"field": "after revert", "value": describe(&preview["after_revert"])}),
            ];
            ctx.printer.list(
                &rows,
                None,
                &[col("", "/field"), col("search pages", "/value")],
            )
        }
        SearchFormsSub::Revert { rev } => {
            let result = super::revisions::revert_request(ctx, PATH, rev, "the search forms")?;
            print_forms(ctx, &result)
        }
    }
}

/// The search forms document: as-is in JSON, a page list in a table.
fn print_forms(ctx: &Context, body: &Value) -> Result<()> {
    if ctx.printer.format != Format::Table {
        return ctx.printer.raw(body);
    }
    let mut rows = Vec::new();
    for (key, form) in [("pages", "search page"), ("home_page_forms", "home page")] {
        for page in body
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let count = ["primary", "secondary"]
                .iter()
                .filter_map(|p| page.get(*p).and_then(Value::as_array))
                .map(Vec::len)
                .sum::<usize>();
            rows.push(json!({
                "form": form,
                "id": page.get("id").cloned().unwrap_or(Value::Null),
                "name": page.get("name").cloned().unwrap_or(Value::Null),
                "field_count": count,
                "prepackaged": page.get("prepackaged").and_then(Value::as_bool).unwrap_or(false),
            }));
        }
    }
    ctx.printer.note(&format!(
        "Source: {} (the site's own copy is \"site\", the MLS board's is \"board\")",
        text(body, "source")
    ));
    ctx.printer.list(&rows, None, PAGE_COLUMNS)
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
