//! The command tree and top-level dispatch.

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crate::client::Client;
use crate::commands::{
    agent_pages, api, area_pages, auth, blog_home, categories, design, featured, files, footers,
    guide, home_page, inspect, nav, pages, posts, search, settings, sidebars, snapshot, templates,
    Context,
};
use crate::config::{self, Config, Overrides};
use crate::error::Result;
use crate::output::{Format, Printer};

const ABOUT: &str = "The Real Geeks command line: manage a Real Geeks website's content.";
const AFTER_HELP: &str = "\
Start with `geekcli auth login --site www.example.com`, then `geekcli me`.

`geekcli guide` is the walkthrough for scripts and AI agents; `guide --list`
shows its topics and `guide <topic>` prints one, e.g. `guide html` (what the
sanitizer keeps and what breaks on a page), `guide search` (criteria the site
accepts), `guide rebrand` (every place a site's identity lives), `guide exit`.
After changing anything visible, `geekcli snapshot <path> --full` and look.

API reference: https://developers.realgeeks.com/content-api/";

#[derive(Debug, Parser)]
#[command(name = "geekcli", version, about = ABOUT, after_help = AFTER_HELP, propagate_version = true)]
pub struct Cli {
    #[command(flatten)]
    pub global: Global,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, clap::Args)]
#[command(next_help_heading = "Global options")]
pub struct Global {
    /// Site domain, e.g. www.example.com
    #[arg(long, global = true, env = "GEEKCLI_SITE", value_name = "DOMAIN")]
    pub site: Option<String>,
    /// API key; overrides the stored key for the site
    #[arg(
        long,
        global = true,
        env = "GEEKCLI_API_KEY",
        hide_env_values = true,
        value_name = "KEY"
    )]
    pub api_key: Option<String>,
    /// Reach the API at this origin instead of https://<site> (local dev)
    #[arg(long, global = true, env = "GEEKCLI_BASE_URL", value_name = "URL")]
    pub base_url: Option<String>,
    /// Output format (default: table on a terminal, json when piped)
    #[arg(short = 'o', long, global = true, value_enum, value_name = "FORMAT")]
    pub output: Option<Format>,
    /// Shorthand for --output json
    #[arg(long, global = true, conflicts_with = "output")]
    pub json: bool,
    /// Print only ids
    #[arg(short = 'q', long, global = true)]
    pub quiet: bool,
    /// Skip confirmation prompts
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,
    /// Log requests to stderr
    #[arg(short = 'v', long, global = true)]
    pub verbose: bool,
    /// Retries after a 429 rate limit (uploads included). Each wait prints
    /// `rate limited; retrying in Ns (attempt i of n)` on stderr
    #[arg(long, global = true, default_value_t = 3, value_name = "N")]
    pub max_retries: u32,
}

impl Global {
    pub fn format(&self) -> Option<Format> {
        if self.json {
            Some(Format::Json)
        } else {
            self.output
        }
    }

    pub fn overrides(&self) -> Overrides {
        Overrides {
            site: self.site.clone(),
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Sign in and manage stored site keys
    Auth(auth::AuthCommand),
    /// Show the current key and site (same as `auth status`)
    Me,
    /// Blog posts
    Posts(posts::PostsCommand),
    /// Blog categories
    Categories(categories::CategoriesCommand),
    /// The blog's landing page (title, meta, heading)
    Blog(blog_home::BlogCommand),
    /// Content pages
    Pages(pages::PagesCommand),
    /// Area (community) pages
    #[command(name = "area-pages")]
    AreaPages(area_pages::AreaPagesCommand),
    /// Agent landing pages (leads route to a CRM agent)
    #[command(name = "agent-pages")]
    AgentPages(agent_pages::AgentPagesCommand),
    /// The site's CRM agents, for --agent-id
    Agents,
    /// The site's home page
    #[command(name = "home-page")]
    HomePage(home_page::HomePageCommand),
    /// Page templates
    Templates(templates::TemplatesCommand),
    /// Navigation bar links
    Nav(nav::NavCommand),
    /// Sidebars and their items
    Sidebars(sidebars::SidebarsCommand),
    /// Footers (shared HTML blocks)
    Footers(footers::FootersCommand),
    /// Featured Pages tile groups for the anna-modern home page
    Featured(featured::FeaturedCommand),
    /// Site settings
    Settings(settings::SettingsCommand),
    /// Template and colour scheme, with unsaved previews
    Design(design::DesignCommand),
    /// Uploaded files (images, PDFs) on the site's media bucket
    Files(files::FilesCommand),
    /// Render a page with a local Chrome and save a PNG to look at
    Snapshot(snapshot::SnapshotArgs),
    /// Query a rendered public page through a local Chrome
    Inspect(inspect::InspectArgs),
    /// Test property-search criteria and build search links
    Search(search::SearchCommand),
    /// Call any /api/v3/ endpoint directly
    Api(api::ApiArgs),
    /// The usage guide for scripts and AI agents, whole or one topic (`guide html`)
    Guide(guide::GuideArgs),
    /// Generate shell completions
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
}

pub fn run(cli: Cli) -> Result<()> {
    let printer = Printer::new(cli.global.format(), cli.global.quiet);
    let overrides = cli.global.overrides();

    match cli.command {
        Command::Guide(args) => guide::run(&args),
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "geekcli", &mut std::io::stdout());
            Ok(())
        }
        Command::Auth(cmd) => {
            let mut config = Config::load()?;
            let mut env = auth::AuthEnv {
                config: &mut config,
                overrides: &overrides,
                printer,
                max_retries: cli.global.max_retries,
                verbose: cli.global.verbose,
            };
            auth::run(&mut env, cmd)
        }
        command => {
            let config = Config::load()?;
            let target = config::resolve_target(&config, &overrides)?;
            let client = Client::new(&target, cli.global.max_retries, cli.global.verbose)?;
            let ctx = Context {
                client,
                printer,
                yes: cli.global.yes,
            };
            match command {
                Command::Me => auth::status(&ctx),
                Command::Posts(cmd) => posts::run(&ctx, cmd),
                Command::Categories(cmd) => categories::run(&ctx, cmd),
                Command::Blog(cmd) => blog_home::run(&ctx, cmd),
                Command::Pages(cmd) => pages::run(&ctx, cmd),
                Command::AreaPages(cmd) => area_pages::run(&ctx, cmd),
                Command::AgentPages(cmd) => agent_pages::run(&ctx, cmd),
                Command::Agents => agent_pages::agents(&ctx),
                Command::HomePage(cmd) => home_page::run(&ctx, cmd),
                Command::Templates(cmd) => templates::run(&ctx, &cmd),
                Command::Search(cmd) => search::run(&ctx, cmd),
                Command::Nav(cmd) => nav::run(&ctx, cmd),
                Command::Sidebars(cmd) => sidebars::run(&ctx, cmd),
                Command::Footers(cmd) => footers::run(&ctx, cmd),
                Command::Featured(cmd) => featured::run(&ctx, cmd),
                Command::Settings(cmd) => settings::run(&ctx, cmd),
                Command::Design(cmd) => design::run(&ctx, cmd),
                Command::Files(cmd) => files::run(&ctx, cmd),
                Command::Snapshot(args) => snapshot::run(&ctx, &args),
                Command::Inspect(args) => inspect::run(&ctx, &args),
                Command::Api(args) => api::run(&ctx, &args),
                Command::Guide(_) | Command::Completions { .. } | Command::Auth(_) => Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_tree_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn legacy_no_browser_flag_is_accepted() {
        assert!(Cli::try_parse_from(["geekcli", "auth", "login", "--no-browser"]).is_ok());
    }
}
