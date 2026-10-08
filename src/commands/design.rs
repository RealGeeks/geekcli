//! `geekcli design …` — the site's template (design family) and colour
//! scheme: a named variation plus variable overrides. CSS is generated from
//! these on request, so a change is live at once; `preview` renders a
//! change without saving it.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use serde_json::{json, Map, Value};

use super::{snapshot, Context};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::output::{cell, col, Column, Format};

pub const PATH: &str = "design/";

pub const TEMPLATE_COLUMNS: &[Column] = &[
    col("name", "/name"),
    col("variations", "/variations_count"),
    col("examples", "/variations_sample"),
];

pub const VAR_COLUMNS: &[Column] = &[col("variable", "/name"), col("value", "/value")];

#[derive(Debug, Args)]
pub struct DesignCommand {
    #[command(subcommand)]
    pub command: DesignSub,
}

#[derive(Debug, Subcommand)]
pub enum DesignSub {
    /// Show the current template and colour scheme
    Get,
    /// List the templates and their colour variations
    Templates,
    /// Show a variation's variables, e.g. `design variation anna-modern coastal`
    Variation { template: String, name: String },
    /// Change the template, the colour variation and/or individual variables
    #[command(after_help = "Notes:
  - Changing --template alone applies that template's default variation, because variables belong to a template family.
  - --var takes the variable names from `design variation <template> <name>`; an unknown name is a 422 listing the valid ones. Values are plain CSS (no ; { } /* */ \\ @ $ ! < >); on miranda/molly they are compiled first, so a value that breaks the stylesheet is a 422.
  - Try it first: `design preview` with the same flags returns a link that renders the change without saving. Open it in a browser signed in to the site's admin: unsaved variables are applied only for a logged-in admin, so --snapshot on preview is allowed for a --template-only change and refused otherwise.
  - After a real change, snapshot the home page, a content page and a post: sidebars, tiles and search forms differ between designs.")]
    Set(ChangeArgs),
    /// Validate a change and get a link that renders it without saving
    #[command(after_help = "Notes:
  - Open the link in a browser signed in to the site's admin: unsaved --variation/--var values are applied only for a logged-in admin.
  - --snapshot is allowed for a --template-only preview (the template switch renders for everyone) and refused when --variation or --var is present; use `design set --snapshot` for those.")]
    Preview(ChangeArgs),
    /// List changes to the template and colour scheme, newest first
    #[command(after_help = "Notes:
  - Taken from the site's settings history: every design change, whoever made it (the admin, support, this CLI), over the site's most recent 500 settings saves.
  - `changed` is `template`, `styles` (the colour scheme and its variables) or both.")]
    Revisions {
        /// Show only the latest N
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
    },
    /// Show one revision with what a revert would restore
    Revision { rev: u64 },
    /// Undo a design revision and everything after it (the revert is itself undoable)
    #[command(after_help = "Notes:
  - Puts back the template and colour scheme from before that revision; the change is live at once, so snapshot afterwards.
  - 409 (exit 6) when the earlier template is no longer offered, or when there is nothing left to undo.")]
    Revert { rev: u64 },
}

#[derive(Debug, Args, Clone)]
pub struct ChangeArgs {
    /// Template (design family): miranda, miranda-thin, molly, anna, anna-modern
    #[arg(long)]
    pub template: Option<String>,
    /// Colour variation name within the template
    #[arg(long)]
    pub variation: Option<String>,
    /// Variable override, name=value; repeatable
    #[arg(long = "var", value_name = "NAME=VALUE")]
    pub vars: Vec<String>,
    /// Render the result to this PNG: the live home page after `set`, or a template-only preview
    #[arg(long, value_name = "FILE")]
    pub snapshot: Option<PathBuf>,
}

impl ChangeArgs {
    pub fn body(&self) -> Result<Value> {
        let mut body = Map::new();
        if let Some(t) = &self.template {
            body.insert("template".into(), Value::String(t.trim().to_string()));
        }
        if let Some(v) = &self.variation {
            body.insert("variation".into(), Value::String(v.trim().to_string()));
        }
        if !self.vars.is_empty() {
            let mut vars = Map::new();
            for item in &self.vars {
                let Some((k, v)) = item.split_once('=') else {
                    return Err(Error::Usage(format!(
                        "--var expects NAME=VALUE, got '{item}'"
                    )));
                };
                vars.insert(k.trim().to_string(), Value::String(v.trim().to_string()));
            }
            body.insert("vars".into(), Value::Object(vars));
        }
        if body.is_empty() {
            return Err(Error::Usage(
                "pass --template, --variation and/or --var".into(),
            ));
        }
        Ok(Value::Object(body))
    }
}

pub fn run(ctx: &Context, cmd: DesignCommand) -> Result<()> {
    match cmd.command {
        DesignSub::Get => {
            let current = ctx.client.get(PATH, &Query::new())?.body;
            print_design(ctx, &current)
        }
        DesignSub::Templates => {
            let body = ctx
                .client
                .get(&format!("{PATH}templates/"), &Query::new())?
                .body;
            let rows: Vec<Value> = body
                .get("results")
                .and_then(Value::as_array)
                .map(|list| list.iter().map(template_row).collect())
                .unwrap_or_default();
            ctx.printer.list(&rows, None, TEMPLATE_COLUMNS)
        }
        DesignSub::Variation { template, name } => {
            let body = ctx
                .client
                .get(
                    &format!(
                        "{PATH}templates/{}/variations/{}/",
                        template.trim(),
                        name.trim()
                    ),
                    &Query::new(),
                )?
                .body;
            if ctx.printer.format == Format::Table {
                ctx.printer.note(&format!(
                    "{} / {}",
                    cell(&body["template"]),
                    cell(&body["name"])
                ));
                return ctx
                    .printer
                    .list(&var_rows(body.get("vars")), None, VAR_COLUMNS);
            }
            ctx.printer.raw(&body)
        }
        DesignSub::Set(args) => {
            let result = ctx.client.patch(PATH, &args.body()?)?.body;
            ctx.printer.note(&format!(
                "Design is now {} / {}",
                cell(&result["template"]),
                cell(result.pointer("/styles/name").unwrap_or(&Value::Null))
            ));
            if let Some(out) = &args.snapshot {
                let url = snapshot::resolve_url(&ctx.client, "/");
                render_to(ctx, &url, out)?;
            }
            print_design(ctx, &result)
        }
        DesignSub::Preview(args) => {
            let mut result = ctx
                .client
                .post(&format!("{PATH}preview/"), &args.body()?)?
                .body;
            let url = rehome(
                &ctx.client,
                result
                    .get("preview_url")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            result["preview_url"] = Value::String(url.clone());
            if let Some(out) = &args.snapshot {
                if args.variation.is_some() || !args.vars.is_empty() {
                    return Err(Error::Usage(
                        "the preview link applies --variation/--var only for a browser signed in to the site's admin, so a headless --snapshot would show the saved design. Open the link in your browser, or `design set` and snapshot afterwards.".into(),
                    ));
                }
                render_to(ctx, &url, out)?;
            }
            if ctx.printer.quiet {
                println!("{url}");
                return Ok(());
            }
            if ctx.printer.format == Format::Table {
                ctx.printer.note(&format!(
                    "Preview of {} / {} (nothing saved):",
                    cell(&result["template"]),
                    cell(result.pointer("/styles/name").unwrap_or(&Value::Null))
                ));
                println!("{url}");
                return Ok(());
            }
            ctx.printer.raw(&result)
        }
        DesignSub::Revisions { limit } => super::revisions::list(ctx, PATH, limit),
        DesignSub::Revision { rev } => super::revisions::show(ctx, PATH, rev),
        DesignSub::Revert { rev } => {
            let result = super::revisions::revert_request(ctx, PATH, rev, "the design")?;
            ctx.printer.note(&format!(
                "Design is now {} / {}",
                cell(&result["template"]),
                cell(result.pointer("/styles/name").unwrap_or(&Value::Null))
            ));
            print_design(ctx, &result)
        }
    }
}

fn render_to(ctx: &Context, url: &str, out: &PathBuf) -> Result<()> {
    let browser = snapshot::find_browser(None)?;
    let capture = snapshot::Capture {
        url,
        width: 1440,
        height: 900,
        scale: 1,
        mobile: false,
        full: true,
        wait_ms: 1500,
    };
    let png = snapshot::render(&browser, &capture, None, 0)?;
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, png)?;
    ctx.printer
        .note(&format!("Rendered {url} to {}", out.display()));
    Ok(())
}

/// A catalogue entry with the variation list summarised for the table;
/// the JSON output keeps the full list.
fn template_row(template: &Value) -> Value {
    let mut row = template.clone();
    let names: Vec<String> = template
        .get("variations")
        .and_then(Value::as_array)
        .map(|v| v.iter().map(cell).collect())
        .unwrap_or_default();
    row["variations_count"] = Value::from(names.len());
    let sample: Vec<&str> = names.iter().take(4).map(String::as_str).collect();
    let more = names.len().saturating_sub(sample.len());
    row["variations_sample"] = Value::String(if more > 0 {
        format!("{}, +{more} more", sample.join(", "))
    } else {
        sample.join(", ")
    });
    row
}

/// The site builds the preview link on its public domain; put it on the
/// origin this CLI is actually talking to (local dev, staging).
pub fn rehome(client: &crate::client::Client, url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => {
            let mut rest = parsed.path().to_string();
            if let Some(q) = parsed.query() {
                rest.push('?');
                rest.push_str(q);
            }
            client.site_url(&rest)
        }
        Err(_) => url.to_string(),
    }
}

fn var_rows(vars: Option<&Value>) -> Vec<Value> {
    vars.and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, v)| json!({ "name": k, "value": v }))
                .collect()
        })
        .unwrap_or_default()
}

fn print_design(ctx: &Context, design: &Value) -> Result<()> {
    if ctx.printer.format != Format::Table {
        return ctx.printer.raw(design);
    }
    let mut summary = design.clone();
    if let Some(map) = summary.as_object_mut() {
        map.remove("styles");
        map.insert(
            "scheme".into(),
            design
                .pointer("/styles/name")
                .cloned()
                .unwrap_or(Value::Null),
        );
    }
    ctx.printer.raw(&summary)?;
    let rows = var_rows(design.pointer("/styles/vars"));
    if !rows.is_empty() {
        ctx.printer.list(&rows, None, VAR_COLUMNS)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_bodies() {
        let args = ChangeArgs {
            template: Some("anna-modern".into()),
            variation: None,
            vars: vec!["palette-brand-color=#0066A7".into()],
            snapshot: None,
        };
        assert_eq!(
            args.body().ok(),
            Some(
                json!({ "template": "anna-modern", "vars": { "palette-brand-color": "#0066A7" } })
            )
        );
        let empty = ChangeArgs {
            template: None,
            variation: None,
            vars: vec![],
            snapshot: None,
        };
        assert!(matches!(empty.body(), Err(Error::Usage(_))));
        let bad = ChangeArgs {
            template: None,
            variation: None,
            vars: vec!["nope".into()],
            snapshot: None,
        };
        assert!(matches!(bad.body(), Err(Error::Usage(_))));
    }
}
