//! `geekcli categories …` — blog categories.

use clap::{Args, Subcommand};
use serde_json::{json, Value};

use super::{
    confirm, id_of, list_and_print, lookup_one, print_written, push, Context, Paging, Payload,
};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::output::{col, Column};

pub const PATH: &str = "blog/categories/";

pub const COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("slug", "/slug"),
    col("name", "/name"),
    col("posts", "/post_count"),
    col("path", "/path"),
];

#[derive(Debug, Args)]
pub struct CategoriesCommand {
    #[command(subcommand)]
    pub command: CategoriesSub,
}

#[derive(Debug, Subcommand)]
pub enum CategoriesSub {
    /// List categories
    List(ListArgs),
    /// Show one category by id or slug
    Get {
        /// Category id or slug
        reference: String,
    },
    /// Create a category
    Create {
        /// Display name
        #[arg(long)]
        name: String,
        /// Defaults to a slug derived from the name
        #[arg(long)]
        slug: Option<String>,
    },
    /// Rename or re-slug a category
    Update {
        /// Category id or slug
        reference: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        slug: Option<String>,
    },
    /// Delete a category
    Delete {
        /// Category id or slug
        reference: String,
        /// Also remove the category from posts that use it
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[arg(long)]
    pub slug: Option<String>,
    /// Name or slug contains
    #[arg(long, short = 's', value_name = "TEXT")]
    pub search: Option<String>,
    #[command(flatten)]
    pub paging: Paging,
}

pub fn run(ctx: &Context, cmd: CategoriesCommand) -> Result<()> {
    match cmd.command {
        CategoriesSub::List(args) => {
            let mut query = Query::new();
            push(&mut query, "slug", args.slug.as_deref());
            push(&mut query, "q", args.search.as_deref());
            args.paging.apply(&mut query);
            list_and_print(ctx, PATH, &query, &args.paging, COLUMNS)
        }
        CategoriesSub::Get { reference } => ctx.printer.one(&resolve(ctx, &reference)?, COLUMNS),
        CategoriesSub::Create { name, slug } => {
            let slug = slug.unwrap_or_else(|| slugify(&name));
            let created = ctx
                .client
                .post(PATH, &json!({ "name": name, "slug": slug }))?
                .body;
            print_written(ctx, &created, COLUMNS, "Created")
        }
        CategoriesSub::Update {
            reference,
            name,
            slug,
        } => {
            let id = id_of(&resolve(ctx, &reference)?)?;
            let mut payload = Payload::default();
            payload.set("name", name).set("slug", slug);
            if payload.is_empty() {
                return Err(Error::Usage("pass --name and/or --slug".into()));
            }
            let updated = ctx
                .client
                .patch(&detail_path(id), &payload.into_value())?
                .body;
            print_written(ctx, &updated, COLUMNS, "Updated")
        }
        CategoriesSub::Delete { reference, force } => {
            let category = resolve(ctx, &reference)?;
            let id = id_of(&category)?;
            let name = category.get("name").and_then(Value::as_str).unwrap_or("");
            if !confirm(ctx, &format!("category {id} \"{name}\""))? {
                return Err(Error::Usage("cancelled".into()));
            }
            let mut query = Query::new();
            if force {
                query.push(("force".into(), "true".into()));
            }
            ctx.client.delete(&detail_path(id), &query)?;
            ctx.printer.note(&format!("Deleted category {id}"));
            if ctx.printer.format != crate::output::Format::Table {
                ctx.printer.raw(&json!({ "deleted": true, "id": id }))?;
            }
            Ok(())
        }
    }
}

pub fn detail_path(id: u64) -> String {
    format!("{PATH}{id}/")
}

pub fn resolve(ctx: &Context, reference: &str) -> Result<Value> {
    if let Ok(id) = reference.trim().parse::<u64>() {
        return Ok(ctx.client.get(&detail_path(id), &Query::new())?.body);
    }
    lookup_one(&ctx.client, PATH, "slug", reference.trim(), "category")
}

/// Create any of `slugs` that do not exist yet. Names are derived from the
/// slug (`market-updates` → `Market Updates`).
pub fn ensure_exist(ctx: &Context, slugs: &[&String]) -> Result<()> {
    let existing = ctx.client.get_all(PATH, &Query::new())?;
    let known: std::collections::HashSet<String> = existing
        .iter()
        .filter_map(|c| c.get("slug").and_then(Value::as_str).map(str::to_string))
        .collect();
    for slug in slugs {
        if known.contains(slug.as_str()) {
            continue;
        }
        let name = title_from_slug(slug);
        ctx.client
            .post(PATH, &json!({ "name": name, "slug": slug }))?;
        ctx.printer.note(&format!("Created category '{slug}'"));
    }
    Ok(())
}

pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = true;
    for ch in name.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

fn title_from_slug(slug: &str) -> String {
    slug.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugifies() {
        assert_eq!(slugify("Market Updates!"), "market-updates");
        assert_eq!(slugify("  Buyers & Sellers "), "buyers-sellers");
    }

    #[test]
    fn titles_from_slugs() {
        assert_eq!(title_from_slug("market-updates"), "Market Updates");
        assert_eq!(title_from_slug("faq"), "Faq");
    }
}
