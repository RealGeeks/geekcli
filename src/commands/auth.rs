//! `geekcli auth …` — store, inspect and remove site credentials.

use std::io::{self, IsTerminal, Read};

use clap::{Args, Subcommand};
use serde_json::{json, Value};

use super::Context;
use crate::auth::browser::{self, LoginRequest};
use crate::client::{Client, Query};
use crate::config::{self, Config, Overrides, SiteConfig, Target};
use crate::error::{Error, Result};
use crate::output::{col, Column, Format, Printer};

pub const SITE_COLUMNS: &[Column] = &[
    col("site", "/site"),
    col("aliases", "/aliases"),
    col("default", "/default"),
    col("key", "/key_name"),
    col("prefix", "/prefix"),
    col("scopes", "/scopes"),
    col("expires_at", "/expires_at"),
    col("base_url", "/base_url"),
];

/// Every write scope the API offers; a write scope implies its read scope.
/// Narrow with `--scope` when a key should do less.
pub const DEFAULT_SCOPES: &[&str] = &[
    "blog:write",
    "pages:write",
    "area_pages:write",
    "home_page:write",
    "navigation:write",
    "sidebars:write",
    "settings:write",
    "files:write",
    "footers:write",
    "design:write",
];

#[derive(Debug, Args)]
pub struct AuthCommand {
    #[command(subcommand)]
    pub command: AuthSub,
}

#[derive(Debug, Subcommand)]
pub enum AuthSub {
    /// Approve the CLI in your browser (or store a key) and save it for a site
    #[command(after_help = "Notes:
  - Every key expires: a browser login mints a six-month key; keys made under Admin -> API keys live at most six months. `geekcli me` shows expires_at.
  - An expired key is a 401 token_expired (exit 3); run `auth login` again for a new one.
  - Only the site owner or a Real Geeks superuser can approve; the CLI never sees their password.
  - `--print-url` does not make sign-in browserless: open the printed URL in an already signed-in browser to finish approval.")]
    Login(LoginArgs),
    /// Forget the stored key for a site
    Logout,
    /// Show the current key and site (`GET /me/`)
    Status,
    /// List the sites with stored keys
    #[command(after_help = "Notes:
  - A site is stored under the name given to `auth login --site`; its live domain (from `/me`) is kept as an alias, and `--site` accepts either.
  - Sites stored before aliases existed have none; `--refresh` asks each site's `/me` for its live domain and records it.")]
    Sites {
        /// Ask each stored site's `/me` for its live domain and record it as an alias
        #[arg(long)]
        refresh: bool,
    },
    /// Make a stored site the default
    Use {
        /// Site domain (the login name or the live domain)
        site: String,
    },
}

#[derive(Debug, Args)]
pub struct LoginArgs {
    /// Store this API key (created under Admin → API keys) instead of signing in
    #[arg(long, value_name = "KEY", conflicts_with = "api_key_stdin")]
    pub api_key: Option<String>,
    /// Read the API key from stdin
    #[arg(long)]
    pub api_key_stdin: bool,
    /// Name for the new key (shown in the admin)
    #[arg(long)]
    pub name: Option<String>,
    /// Scopes for the new key; repeat or comma-separate
    #[arg(long = "scope", value_delimiter = ',', value_name = "SCOPE")]
    pub scopes: Option<Vec<String>>,
    /// Print the approval URL; open it yourself in an already signed-in browser
    #[arg(long, alias = "no-browser")]
    pub print_url: bool,
    /// Loopback port for the browser callback (default: any free port)
    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,
}

pub struct AuthEnv<'a> {
    pub config: &'a mut Config,
    pub overrides: &'a Overrides,
    pub printer: Printer,
    pub max_retries: u32,
    pub verbose: bool,
}

pub fn run(env: &mut AuthEnv<'_>, cmd: AuthCommand) -> Result<()> {
    match cmd.command {
        AuthSub::Login(args) => login(env, &args),
        AuthSub::Logout => logout(env),
        AuthSub::Status => {
            let target = config::resolve_target(env.config, env.overrides)?;
            let client = Client::new(&target, env.max_retries, env.verbose)?;
            status(&Context {
                client,
                printer: env.printer,
                yes: false,
            })
            .map_err(|err| crate::cli::with_refresh_hint(err, env.config, env.overrides))
        }
        AuthSub::Sites { refresh } => {
            if refresh {
                refresh_aliases(env)?;
            }
            sites(env)
        }
        AuthSub::Use { site } => {
            let Some((domain, _)) = env.config.lookup(&site)? else {
                let domain = config::normalize_domain(&site);
                return Err(Error::Config(
                    env.config.refresh_hint(&domain).unwrap_or_else(|| {
                        format!(
                            "no stored key for {domain}; run `geekcli auth login --site {domain}`"
                        )
                    }),
                ));
            };
            let domain = domain.to_string();
            env.config.default_site = Some(domain.clone());
            env.config.save()?;
            env.printer.note(&format!("Default site is now {domain}"));
            if env.printer.format != Format::Table {
                env.printer.raw(&json!({ "default_site": domain }))?;
            }
            Ok(())
        }
    }
}

pub fn status(ctx: &Context) -> Result<()> {
    let me = ctx.client.get("me/", &Query::new())?.body;
    ctx.printer.raw(&me)
}

fn login(env: &mut AuthEnv<'_>, args: &LoginArgs) -> Result<()> {
    let Some(site) = env
        .overrides
        .site
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    else {
        return Err(Error::Usage(
            "--site <domain> is required to log in (or set GEEKCLI_SITE)".into(),
        ));
    };
    let domain = config::normalize_domain(site);
    let base_url = env
        .overrides
        .base_url
        .clone()
        .unwrap_or_else(|| format!("https://{domain}"));
    let target = Target {
        domain: domain.clone(),
        base_url: base_url.clone(),
        api_key: None,
    };
    let anon = Client::new(&target, env.max_retries, env.verbose)?;

    let (api_key, minted) = if let Some(key) = args.api_key.as_deref() {
        (key.trim().to_string(), None)
    } else if args.api_key_stdin {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        (buf.trim().to_string(), None)
    } else {
        let request = LoginRequest {
            name: args.name.clone().unwrap_or_else(default_key_name),
            scopes: scopes(args),
            port: args.port,
            state: None,
        };
        let minted = browser::login(&anon, &request, !args.print_url)?;
        let key = minted
            .get("api_key")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("exchange response had no api_key".into()))?
            .to_string();
        (key, Some(minted))
    };
    if api_key.is_empty() {
        return Err(Error::Usage("empty API key".into()));
    }

    // Verify before storing so a typo never ends up in the config file.
    let authed = Client::new(
        &Target {
            api_key: Some(api_key.clone()),
            ..target
        },
        env.max_retries,
        env.verbose,
    )?;
    let me = authed.get("me/", &Query::new())?.body;
    let key_info = me.get("api_key").cloned().unwrap_or(Value::Null);
    // The live domain is kept as an alias so `--site` accepts either name.
    let aliases = config::aliases_from_me(&me, &domain);

    let stored_base = env.overrides.base_url.clone();
    env.config.put_site(
        &domain,
        SiteConfig {
            api_key,
            base_url: stored_base,
            key_name: key_info
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string),
            scopes: key_info
                .get("scopes")
                .and_then(Value::as_array)
                .map(|s| {
                    s.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            expires_at: key_info
                .get("expires_at")
                .and_then(Value::as_str)
                .map(str::to_string),
            aliases: aliases.clone(),
        },
    );
    let path = env.config.save()?;

    let live = me
        .pointer("/site/domain")
        .and_then(Value::as_str)
        .map(config::normalize_domain)
        .filter(|live| !live.is_empty() && *live != domain);
    let shown = live.map_or_else(
        || domain.clone(),
        |live| format!("{domain} (live domain: {live})"),
    );
    env.printer.note(&format!(
        "Logged in to {shown}; key stored in {}",
        path.display()
    ));
    if let Some(minted) = &minted {
        if let Some(name) = minted.pointer("/key/name").and_then(Value::as_str) {
            env.printer.note(&format!("Created API key \"{name}\""));
        }
    }
    let summary = json!({
        "site": me.get("site").cloned().unwrap_or(Value::Null),
        "api_key": key_info,
        "aliases": aliases,
        "config_path": path.display().to_string(),
        "default": env.config.default_site.as_deref() == Some(domain.as_str()),
    });
    env.printer.raw(&summary)
}

fn scopes(args: &LoginArgs) -> Vec<String> {
    args.scopes
        .clone()
        .unwrap_or_else(|| DEFAULT_SCOPES.iter().map(ToString::to_string).collect())
}

fn default_key_name() -> String {
    let host = std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
        })
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "unknown host".to_string());
    let name = format!("Real Geeks CLI on {host}");
    name.chars().take(100).collect()
}

fn logout(env: &mut AuthEnv<'_>) -> Result<()> {
    let target = config::resolve_target(env.config, env.overrides)?;
    if env.config.remove_site(&target.domain) {
        env.config.save()?;
        env.printer
            .note(&format!("Removed the stored key for {}", target.domain));
        if env.printer.format != Format::Table {
            env.printer
                .raw(&json!({ "logged_out": true, "site": target.domain }))?;
        }
        Ok(())
    } else {
        Err(Error::Config(format!(
            "no stored key for {}",
            target.domain
        )))
    }
}

fn sites(env: &AuthEnv<'_>) -> Result<()> {
    let rows: Vec<Value> = env
        .config
        .sites
        .iter()
        .map(|(domain, site)| {
            json!({
                "site": domain,
                "aliases": site.aliases,
                "default": env.config.default_site.as_deref() == Some(domain.as_str()),
                "key_name": site.key_name,
                "prefix": key_prefix(&site.api_key),
                "scopes": site.scopes,
                "expires_at": site.expires_at,
                "base_url": site.base_url,
            })
        })
        .collect();
    if rows.is_empty() && !io::stdout().is_terminal() {
        return env.printer.list(&rows, None, SITE_COLUMNS);
    }
    if rows.is_empty() {
        env.printer
            .note("No sites stored. Run `geekcli auth login --site <domain>`.");
        return Ok(());
    }
    env.printer.list(&rows, None, SITE_COLUMNS)
}

/// Fill each stored site's aliases from its `/me`. A site that cannot be
/// reached keeps what it had; the others are still saved.
fn refresh_aliases(env: &mut AuthEnv<'_>) -> Result<()> {
    let keys: Vec<String> = env.config.sites.keys().cloned().collect();
    let mut changed = false;
    for key in keys {
        let Some(site) = env.config.sites.get(&key) else {
            continue;
        };
        let target = Target {
            domain: key.clone(),
            base_url: env
                .overrides
                .base_url
                .clone()
                .or_else(|| site.base_url.clone())
                .unwrap_or_else(|| format!("https://{key}")),
            api_key: Some(site.api_key.clone()),
        };
        let me = Client::new(&target, env.max_retries, env.verbose)
            .and_then(|client| client.get("me/", &Query::new()));
        match me {
            Ok(me) => {
                let aliases = config::aliases_from_me(&me.body, &key);
                if let Some(site) = env.config.sites.get_mut(&key) {
                    changed |= site.aliases != aliases;
                    site.aliases = aliases;
                }
            }
            Err(err) => env.printer.note(&format!("Could not refresh {key}: {err}")),
        }
    }
    if changed {
        env.config.save()?;
    }
    Ok(())
}

/// `rg_live_abcdEFGH…` — enough to match the admin's key list, never the secret.
pub fn key_prefix(key: &str) -> String {
    let shown: String = key.chars().take(16).collect();
    format!("{shown}…")
}
