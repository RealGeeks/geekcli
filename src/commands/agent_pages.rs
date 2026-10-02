//! `geekcli agent-pages …` — content pages tied to a CRM agent, so leads
//! from the page route to that agent. Same fields as content pages plus
//! `agent_id`. `geekcli agents` lists the CRM agents to pick from.

use clap::{Args, Subcommand};
use serde_json::Value;

use super::pages::{self, TreeFields, TreeListArgs};
use super::{list_and_print, print_written, push, Context};
use crate::client::Query;
use crate::error::Result;
use crate::output::{col, Column};

pub const PATH: &str = "content/agent-pages/";
pub const AGENTS_PATH: &str = "content/agents/";

pub const LIST_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("path", "/path"),
    col("anchor_text", "/anchor_text"),
    col("agent", "/agent_name"),
    col("agent_id", "/agent_id"),
    col("template", "/template"),
];

pub const DETAIL_COLUMNS: &[Column] = &[
    col("id", "/id"),
    col("path", "/path"),
    col("url", "/url"),
    col("anchor_text", "/anchor_text"),
    col("title", "/title"),
    col("agent", "/agent_name"),
    col("agent_id", "/agent_id"),
    col("template", "/template"),
    col("sidebar", "/sidebar/name"),
    col("search", "/search/description"),
    col("landscape", "/landscape_image_override"),
];

pub const AGENT_COLUMNS: &[Column] = &[col("id", "/id"), col("name", "/name")];

#[derive(Debug, Args)]
pub struct AgentPagesCommand {
    #[command(subcommand)]
    pub command: AgentPagesSub,
}

#[derive(Debug, Subcommand)]
pub enum AgentPagesSub {
    /// List agent landing pages
    List(ListArgs),
    /// Show one by id, path or slug
    Get { reference: String },
    /// Create an agent landing page
    #[command(after_help = "Notes:
  - Same fields as `pages create`, plus --agent-id from `geekcli agents`. Leads from the page go to that agent.
  - Use --template \"Agent Detail Page\" with --area \"Agent Name=…\" and --area \"Agent Photo=<file URL>\" for a profile page; an About Page lists every Agent Detail Page, whether it is an agent landing page or a plain page.
  - These pages are separate from `pages`: neither list shows the other.")]
    Create(CreateArgs),
    /// Change fields on an agent landing page
    Update(UpdateArgs),
    /// Delete an agent landing page
    Delete {
        reference: String,
        #[arg(long)]
        orphan_children: bool,
    },
    /// List revisions, newest first
    Revisions {
        reference: String,
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
    },
    /// Show one revision with what a revert would restore
    Revision { reference: String, rev: u64 },
    /// Undo a revision and everything after it
    Revert { reference: String, rev: u64 },
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[command(flatten)]
    pub tree: TreeListArgs,
    #[arg(long)]
    pub template: Option<String>,
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[command(flatten)]
    pub fields: TreeFields,
    /// Page template; see `geekcli templates list`
    #[arg(long)]
    pub template: Option<String>,
    /// CRM agent id (see `geekcli agents`), or `null`
    #[arg(long, value_name = "ID_OR_NULL")]
    pub agent_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    pub reference: String,
    #[command(flatten)]
    pub fields: TreeFields,
    #[arg(long)]
    pub template: Option<String>,
    /// CRM agent id (see `geekcli agents`), or `null`
    #[arg(long, value_name = "ID_OR_NULL")]
    pub agent_id: Option<String>,
    /// Send a full replace (PUT)
    #[arg(long)]
    pub replace: bool,
}

fn agent_value(text: &str) -> Value {
    match text.trim().to_ascii_lowercase().as_str() {
        "null" | "none" | "-" | "" => Value::Null,
        _ => Value::String(text.trim().to_string()),
    }
}

pub fn run(ctx: &Context, cmd: AgentPagesCommand) -> Result<()> {
    match cmd.command {
        AgentPagesSub::List(args) => {
            let mut query = args.tree.query();
            push(&mut query, "template", args.template.as_deref());
            list_and_print(ctx, PATH, &query, &args.tree.paging, LIST_COLUMNS)
        }
        AgentPagesSub::Get { reference } => ctx.printer.one(
            &pages::resolve(ctx, PATH, &reference, "agent page")?,
            DETAIL_COLUMNS,
        ),
        AgentPagesSub::Create(args) => {
            let mut payload = args.fields.payload(ctx, PATH)?;
            payload.set("template", args.template.as_deref());
            if let Some(agent) = &args.agent_id {
                payload.set_value("agent_id", agent_value(agent));
            }
            pages::require(&payload, &["slug", "anchor_text"], "agent page")?;
            let created = ctx.client.post(PATH, &payload.into_value())?.body;
            print_written(ctx, &created, DETAIL_COLUMNS, "Created")
        }
        AgentPagesSub::Update(args) => {
            let id = super::id_of(&pages::resolve(ctx, PATH, &args.reference, "agent page")?)?;
            let mut payload = args.fields.payload(ctx, PATH)?;
            payload.set("template", args.template.as_deref());
            if let Some(agent) = &args.agent_id {
                payload.set_value("agent_id", agent_value(agent));
            }
            pages::write(
                ctx,
                &pages::detail_path(PATH, id),
                payload,
                args.replace,
                DETAIL_COLUMNS,
            )
        }
        AgentPagesSub::Delete {
            reference,
            orphan_children,
        } => pages::delete(
            ctx,
            PATH,
            &reference,
            "agent page",
            orphan_children.then_some("orphan_children"),
        ),
        AgentPagesSub::Revisions { reference, limit } => {
            let id = super::id_of(&pages::resolve(ctx, PATH, &reference, "agent page")?)?;
            super::revisions::list(ctx, &pages::detail_path(PATH, id), limit)
        }
        AgentPagesSub::Revision { reference, rev } => {
            let id = super::id_of(&pages::resolve(ctx, PATH, &reference, "agent page")?)?;
            super::revisions::show(ctx, &pages::detail_path(PATH, id), rev)
        }
        AgentPagesSub::Revert { reference, rev } => {
            let id = super::id_of(&pages::resolve(ctx, PATH, &reference, "agent page")?)?;
            super::revisions::revert(ctx, &pages::detail_path(PATH, id), rev, "agent page")
        }
    }
}

/// `geekcli agents` — the CRM agents an agent page can be tied to.
pub fn agents(ctx: &Context) -> Result<()> {
    let body = ctx.client.get(AGENTS_PATH, &Query::new())?.body;
    let rows = body
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    ctx.printer.list(&rows, None, AGENT_COLUMNS)
}
