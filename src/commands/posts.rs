//! `geekcli posts …` — blog posts.

use clap::{Args, Subcommand, ValueEnum};
use serde_json::{json, Value};

use super::{
    confirm, id_of, list_and_print, lookup_one, parse_bool, print_written, push, Context, Paging,
    Payload,
};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::html;
use crate::output::{col, Column};

pub const PATH: &str = "blog/posts/";

pub const LIST_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("state", "/state"),
    col("publish", "/publish"),
    col("slug", "/slug"),
    col("title", "/title"),
    col("categories", "/categories"),
];

pub const DETAIL_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("title", "/title"),
    col("slug", "/slug"),
    col("status", "/status"),
    col("state", "/state"),
    col("publish", "/publish"),
    col("categories", "/categories"),
    col("url", "/url"),
];

#[derive(Debug, Args)]
pub struct PostsCommand {
    #[command(subcommand)]
    pub command: PostsSub,
}

#[derive(Debug, Subcommand)]
pub enum PostsSub {
    /// List posts
    List(ListArgs),
    /// Show one post by id or slug
    Get {
        /// Post id or slug
        reference: String,
    },
    #[command(after_help = "Notes:
  - The body is sanitized: script, style, button, form and svg are dropped, iframes only for YouTube/Vimeo, inline style keeps only text-align, color, background-color, font-weight, font-style, font-size, text-decoration, width, height, max-width, margin*, padding*, float, display, border*, border-collapse, list-style-type, vertical-align and line-height.
  - See `geekcli guide html`.
  - Content columns are Windows-1252: arrows, CJK text and emoji are rejected with a 422 naming them; write such a character as a numeric entity (&#8594;) or use &raquo;.
  - Contact links: <a class=\"popup\" href=\"/member/contact/\"> opens the contact form as an overlay on designs that support it and works as a plain link elsewhere.
  - Put <!--read more--> where the summary ends.
  - Markdown (--markdown or a .md file) converts before sending.
  - Creating with --status published can read `scheduled` for a second; it is published.
  - Drafts and scheduled posts return 404 to visitors and search engines and stay out of the blog home, categories, archives, feed and sitemap; a logged-in site admin who can edit posts sees them at their URL as a noindex preview.
  - `state` (draft, scheduled, published) reflects visibility; `status` alone does not, since a published post with a future publish date is scheduled.")]
    /// Create a post (saved as a draft unless --status published)
    Create(CreateArgs),
    #[command(after_help = "Notes:
  - Only the flags you pass change.
  - Do not resend HTML you read back unless you changed it.
  - --category replaces the set; --add-category/--remove-category adjust it.
  - Same HTML rules as create (`geekcli guide html`).")]
    /// Change fields on a post (only the flags you pass are changed)
    Update(UpdateArgs),
    /// Delete a post
    Delete {
        /// Post id or slug
        reference: String,
    },
    /// Publish a post now, or at a given time
    #[command(after_help = "Notes:
  - --at in the future makes the post `scheduled` until then.
  - Drafts and scheduled posts return 404 to visitors and search engines and stay out of the blog home, categories, archives, feed and sitemap; a logged-in site admin who can edit posts sees them at their URL as a noindex preview.
  - `state` (draft, scheduled, published) reflects visibility; `status` alone does not, since a published post with a future publish date is scheduled.")]
    Publish {
        /// Post id or slug
        reference: String,
        /// Publish at this ISO-8601 time instead of now (a future time schedules it)
        #[arg(long, value_name = "DATETIME")]
        at: Option<String>,
        /// Keep the post's existing publish date instead of setting it to now
        #[arg(long, conflicts_with = "at")]
        keep_date: bool,
    },
    /// Take a post down: set it back to a draft
    #[command(after_help = "Notes:
  - Drafts and scheduled posts return 404 to visitors and search engines and stay out of the blog home, categories, archives, feed and sitemap; a logged-in site admin who can edit posts sees them at their URL as a noindex preview.
  - `state` (draft, scheduled, published) reflects visibility; `status` alone does not, since a published post with a future publish date is scheduled.")]
    Unpublish {
        /// Post id or slug
        reference: String,
    },
    /// List a post's revisions, newest first
    Revisions {
        /// Post id or slug
        reference: String,
        /// Show only the latest N
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
    },
    /// Show one revision with what a revert would restore
    Revision { reference: String, rev: u64 },
    /// Undo a revision and everything after it (the revert is itself undoable)
    #[command(after_help = "Notes:
  - Revisions track title, slug, body, status, publish, page_title, meta_* and facebook_image, so a revert can change the post's URL or publish/unpublish it. Check `posts revision <ref> <rev>` first.
  - Category changes are not tracked; a revert leaves categories as they are.")]
    Revert { reference: String, rev: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Status {
    Draft,
    Published,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Draft => "draft",
            Status::Published => "published",
        }
    }
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Filter by status
    #[arg(long, value_enum)]
    pub status: Option<Status>,
    /// Filter by computed state: draft, scheduled or published
    #[arg(long, value_name = "STATE")]
    pub state: Option<String>,
    /// Exact slug
    #[arg(long)]
    pub slug: Option<String>,
    /// Title contains
    #[arg(long, short = 's', value_name = "TEXT")]
    pub search: Option<String>,
    /// Category id or slug
    #[arg(long, value_name = "ID_OR_SLUG")]
    pub category: Option<String>,
    /// Omit body and summary from the results
    #[arg(long)]
    pub no_body: bool,
    #[command(flatten)]
    pub paging: Paging,
}

/// Fields shared by create and update.
#[derive(Debug, Args, Default)]
pub struct PostFields {
    /// Post title (required on create)
    #[arg(long)]
    pub title: Option<String>,
    /// URL slug, unique per site (required on create)
    #[arg(long)]
    pub slug: Option<String>,
    /// Body as inline HTML (or Markdown with --markdown)
    #[arg(long, value_name = "HTML", visible_alias = "content", alias = "html")]
    pub body: Option<String>,
    /// Read the body from a file, or `-` for stdin; `.md` files are converted from Markdown
    #[arg(
        long,
        value_name = "FILE",
        visible_alias = "content-file",
        alias = "html-file"
    )]
    pub body_file: Option<String>,
    /// Treat the body as Markdown and convert it to HTML
    #[arg(long)]
    pub markdown: bool,
    /// draft or published (create defaults to draft)
    #[arg(long, value_enum)]
    pub status: Option<Status>,
    /// Publish date, ISO-8601 (a future date schedules the post)
    #[arg(long, value_name = "DATETIME")]
    pub publish: Option<String>,
    /// Category id or slug; repeat or comma-separate for several. Replaces the whole set.
    #[arg(long = "category", value_name = "ID_OR_SLUG", value_delimiter = ',')]
    pub categories: Option<Vec<String>>,
    /// Create any category slug that does not exist yet
    #[arg(long)]
    pub create_categories: bool,
    /// HTML <title> override
    #[arg(long)]
    pub page_title: Option<String>,
    /// SEO description
    #[arg(long)]
    pub meta_description: Option<String>,
    /// SEO keywords
    #[arg(long)]
    pub meta_keywords: Option<String>,
    /// Image URL used when the post is shared on social media
    #[arg(long, value_name = "URL")]
    pub facebook_image: Option<String>,
    /// Allow comments on the post (default true)
    #[arg(long, value_name = "BOOL", value_parser = parse_bool)]
    pub allow_comments: Option<bool>,
    /// Add rel=nofollow to links in comments (default true)
    #[arg(long, value_name = "BOOL", value_parser = parse_bool)]
    pub nofollow_comments: Option<bool>,
    /// Extra fields as a JSON object, `@file`, or `-` for stdin (flags win)
    #[arg(long, value_name = "JSON")]
    pub data: Option<String>,
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[command(flatten)]
    pub fields: PostFields,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Post id or slug
    pub reference: String,
    #[command(flatten)]
    pub fields: PostFields,
    /// Add these categories to the post's existing ones
    #[arg(
        long,
        value_name = "ID_OR_SLUG",
        value_delimiter = ',',
        conflicts_with = "categories"
    )]
    pub add_category: Vec<String>,
    /// Remove these categories from the post
    #[arg(
        long,
        value_name = "ID_OR_SLUG",
        value_delimiter = ',',
        conflicts_with = "categories"
    )]
    pub remove_category: Vec<String>,
    /// Remove every category from the post
    #[arg(long, conflicts_with_all = ["categories", "add_category", "remove_category"])]
    pub clear_categories: bool,
    /// Send a full replace (PUT): fields you omit reset to their defaults
    #[arg(long)]
    pub replace: bool,
}

pub fn run(ctx: &Context, cmd: PostsCommand) -> Result<()> {
    match cmd.command {
        PostsSub::List(args) => list(ctx, &args),
        PostsSub::Get { reference } => {
            let post = resolve(ctx, &reference)?;
            ctx.printer.one(&post, DETAIL_COLUMNS)
        }
        PostsSub::Create(args) => create(ctx, &args),
        PostsSub::Update(args) => update(ctx, &args),
        PostsSub::Delete { reference } => delete(ctx, &reference),
        PostsSub::Publish {
            reference,
            at,
            keep_date,
        } => publish(ctx, &reference, at.as_deref(), keep_date),
        PostsSub::Unpublish { reference } => {
            let post = resolve(ctx, &reference)?;
            let body = json!({ "status": "draft" });
            let updated = ctx.client.patch(&detail_path(id_of(&post)?), &body)?.body;
            print_written(ctx, &updated, DETAIL_COLUMNS, "Unpublished")
        }
        PostsSub::Revisions { reference, limit } => {
            let id = id_of(&resolve(ctx, &reference)?)?;
            super::revisions::list(ctx, &detail_path(id), limit)
        }
        PostsSub::Revision { reference, rev } => {
            let id = id_of(&resolve(ctx, &reference)?)?;
            super::revisions::show(ctx, &detail_path(id), rev)
        }
        PostsSub::Revert { reference, rev } => {
            let post = resolve(ctx, &reference)?;
            let label = format!(
                "post \"{}\"",
                post.get("slug").and_then(Value::as_str).unwrap_or("")
            );
            super::revisions::revert(ctx, &detail_path(id_of(&post)?), rev, &label)
        }
    }
}

pub fn detail_path(id: u64) -> String {
    format!("{PATH}{id}/")
}

/// A post by numeric id or slug.
pub fn resolve(ctx: &Context, reference: &str) -> Result<Value> {
    if let Ok(id) = reference.trim().parse::<u64>() {
        return Ok(ctx.client.get(&detail_path(id), &Query::new())?.body);
    }
    lookup_one(&ctx.client, PATH, "slug", reference.trim(), "post")
}

fn list(ctx: &Context, args: &ListArgs) -> Result<()> {
    let mut query = Query::new();
    push(&mut query, "status", args.status.map(Status::as_str));
    push(&mut query, "state", args.state.as_deref());
    push(&mut query, "slug", args.slug.as_deref());
    push(&mut query, "q", args.search.as_deref());
    push(&mut query, "category", args.category.as_deref());
    if args.no_body {
        query.push(("include_body".into(), "false".into()));
    }
    args.paging.apply(&mut query);
    list_and_print(ctx, PATH, &query, &args.paging, LIST_COLUMNS)
}

fn build_payload(ctx: &Context, fields: &PostFields) -> Result<Payload> {
    let mut payload = Payload::default();
    payload
        .set("title", fields.title.as_deref())
        .set("slug", fields.slug.as_deref())
        .set("status", fields.status.map(Status::as_str))
        .set("publish", fields.publish.as_deref())
        .set("page_title", fields.page_title.as_deref())
        .set("meta_description", fields.meta_description.as_deref())
        .set("meta_keywords", fields.meta_keywords.as_deref())
        .set("facebook_image", fields.facebook_image.as_deref())
        .set("allow_comments", fields.allow_comments)
        .set("nofollow_comments", fields.nofollow_comments);
    if let Some(body) = html::read_body(
        fields.body.as_deref(),
        fields.body_file.as_deref(),
        fields.markdown,
    )? {
        payload.set("body", Some(body));
    }
    if let Some(categories) = &fields.categories {
        let values = category_values(ctx, categories, fields.create_categories)?;
        payload.set_value("categories", Value::Array(values));
    }
    payload.merge_json(fields.data.as_deref())?;
    Ok(payload)
}

/// Turn category references into JSON values, optionally creating missing
/// slugs first so an agent can tag a post without a separate step.
fn category_values(ctx: &Context, refs: &[String], create_missing: bool) -> Result<Vec<Value>> {
    let refs: Vec<String> = refs
        .iter()
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty())
        .collect();
    if create_missing {
        let slugs: Vec<&String> = refs.iter().filter(|r| r.parse::<u64>().is_err()).collect();
        if !slugs.is_empty() {
            super::categories::ensure_exist(ctx, &slugs)?;
        }
    }
    Ok(refs
        .into_iter()
        .map(|r| {
            r.parse::<u64>()
                .map_or_else(|_| Value::String(r), Value::from)
        })
        .collect())
}

fn create(ctx: &Context, args: &CreateArgs) -> Result<()> {
    let mut payload = build_payload(ctx, &args.fields)?;
    // The API defaults to published; the CLI defaults to draft so an agent
    // never publishes by accident. `--status published` opts in.
    if !payload.contains("status") {
        payload.set("status", Some("draft"));
    }
    for required in ["title", "slug", "body"] {
        if !payload.contains(required) {
            return Err(Error::Usage(format!(
                "--{} is required to create a post",
                required.replace('_', "-")
            )));
        }
    }
    let created = ctx.client.post(PATH, &payload.into_value())?.body;
    print_written(ctx, &created, DETAIL_COLUMNS, "Created")
}

fn update(ctx: &Context, args: &UpdateArgs) -> Result<()> {
    let post = resolve(ctx, &args.reference)?;
    let id = id_of(&post)?;
    let mut payload = build_payload(ctx, &args.fields)?;

    if args.clear_categories {
        payload.set_value("categories", Value::Array(vec![]));
    } else if !args.add_category.is_empty() || !args.remove_category.is_empty() {
        let mut current: Vec<Value> = post
            .get("categories")
            .and_then(Value::as_array)
            .map(|cats| cats.iter().filter_map(|c| c.get("id").cloned()).collect())
            .unwrap_or_default();
        let current_slugs: Vec<(u64, String)> = post
            .get("categories")
            .and_then(Value::as_array)
            .map(|cats| {
                cats.iter()
                    .filter_map(|c| {
                        Some((c.get("id")?.as_u64()?, c.get("slug")?.as_str()?.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        for r in &args.remove_category {
            let r = r.trim();
            current.retain(|id| {
                let id_num = id.as_u64().unwrap_or_default();
                let matches_id = r.parse::<u64>().ok() == Some(id_num);
                let matches_slug = current_slugs.iter().any(|(i, s)| *i == id_num && s == r);
                !(matches_id || matches_slug)
            });
        }
        let added = category_values(ctx, &args.add_category, args.fields.create_categories)?;
        for value in added {
            let already = match &value {
                Value::Number(n) => {
                    current.iter().any(|c| c == &value)
                        || current_slugs.iter().any(|(i, _)| Some(*i) == n.as_u64())
                }
                Value::String(s) => current_slugs.iter().any(|(_, slug)| slug == s),
                _ => false,
            };
            if !already {
                current.push(value);
            }
        }
        payload.set_value("categories", Value::Array(current));
    }

    if payload.is_empty() {
        return Err(Error::Usage(
            "nothing to update: pass at least one field flag or --data".into(),
        ));
    }
    let path = detail_path(id);
    let updated = if args.replace {
        ctx.client.put(&path, &payload.into_value())?.body
    } else {
        ctx.client.patch(&path, &payload.into_value())?.body
    };
    print_written(ctx, &updated, DETAIL_COLUMNS, "Updated")
}

fn delete(ctx: &Context, reference: &str) -> Result<()> {
    let post = resolve(ctx, reference)?;
    let id = id_of(&post)?;
    let title = post.get("title").and_then(Value::as_str).unwrap_or("");
    if !confirm(ctx, &format!("post {id} \"{title}\""))? {
        return Err(Error::Usage("cancelled".into()));
    }
    ctx.client.delete(&detail_path(id), &Query::new())?;
    ctx.printer.note(&format!("Deleted post {id}"));
    if ctx.printer.format != crate::output::Format::Table {
        ctx.printer.raw(&json!({ "deleted": true, "id": id }))?;
    }
    Ok(())
}

fn publish(ctx: &Context, reference: &str, at: Option<&str>, keep_date: bool) -> Result<()> {
    let post = resolve(ctx, reference)?;
    let id = id_of(&post)?;
    let mut body = json!({ "status": "published" });
    if let Some(at) = at {
        body["publish"] = Value::String(at.to_string());
    } else if !keep_date {
        body["publish"] = Value::String(chrono::Local::now().to_rfc3339());
    }
    let updated = ctx.client.patch(&detail_path(id), &body)?.body;
    let verb = if updated.get("state").and_then(Value::as_str) == Some("scheduled") {
        "Scheduled"
    } else {
        "Published"
    };
    print_written(ctx, &updated, DETAIL_COLUMNS, verb)
}
