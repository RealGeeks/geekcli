# geekcli

[![CI](https://github.com/RealGeeks/geekcli/actions/workflows/ci.yml/badge.svg)](https://github.com/RealGeeks/geekcli/actions/workflows/ci.yml)
[![Quality](https://github.com/RealGeeks/geekcli/actions/workflows/quality.yml/badge.svg)](https://github.com/RealGeeks/geekcli/actions/workflows/quality.yml)
[![Release](https://img.shields.io/github/v/release/RealGeeks/geekcli)](https://github.com/RealGeeks/geekcli/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![MSRV 1.88](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](Cargo.toml)

The Real Geeks command line. `geekcli` manages a Real Geeks website: blog
posts and categories, content, area, agent and market report pages, the home
page, navigation, sidebars, footers, banners, settings, design and uploaded
files. It is
built to be driven by scripts and AI agents as much as by people: JSON on
stdout when piped, JSON errors on stderr, stable exit codes, and a built-in
guide written for agents.

It is a client for the Real Geeks Content API, documented at
<https://developers.realgeeks.com/content-api/>. The API is switched on per
site; site owners can [request access](https://developers.realgeeks.com/request-api-access/).

**Status:** pre-1.0 and under active development alongside the Content API
beta. Breaking changes are called out in the [changelog](CHANGELOG.md).

```bash
geekcli auth login --site www.example.com
geekcli me
geekcli posts create --title "Spring market update" --slug spring-market-update \
    --content-file post.md --category market-updates --create-categories
geekcli posts publish spring-market-update
```

## Why Real Geeks works well with AI agents

Most real estate websites can only be changed by clicking through an admin.
A Real Geeks website can be run by the AI tools you already use:

- **The whole site is an open, documented API.** Blog posts, pages,
  neighborhood (area) pages, market report pages, the home page, navigation,
  sidebars, footers, banners, settings, design and files, all described in the
  [Content API documentation](https://developers.realgeeks.com/content-api/).
- **Two ways in.** Connect Claude or ChatGPT to the site and ask in plain
  English, or let a coding agent such as Claude Code or Codex drive geekcli.
  Either way the site owner approves access in the site's own admin, chooses
  what it may change, and can revoke it at any time.
- **Safety rails built in.** New posts start as drafts, almost every change
  (pages, posts, navigation, sidebars, settings, design) keeps a revision
  history with one-command undo, deleted or overwritten files can be
  restored for 90 days, design changes can be previewed
  before they go live, and every change is recorded with the key that made it.
  When something is saved but probably isn't what was meant, the response
  says so instead of failing silently.
- **Listings an agent can get right.** Agents read the site's own MLS search
  fields and check values against the MLS's spelling, so a neighborhood page
  shows that neighborhood's listings instead of an empty or city-wide feed.
- **Built to be found, including by AI search.** SEO Fast Track area pages,
  sitemaps that pick up new pages within about 15 minutes, canonical tags,
  structured data, and a robots.txt that welcomes AI assistants and AI search
  crawlers.

## New to Real Geeks?

Real Geeks is an all-in-one real estate platform: an IDX website with your
MLS's listings, a built-in CRM, and lead generation, for agents and teams. If
you want a website your AI tools can actually run, from writing the market
update to building the next neighborhood page,
[book a demo](https://www.realgeeks.com/demo/) or
[see pricing](https://www.realgeeks.com/real-geeks-pricing).

Already a customer? Read on.

## What you need

- **The Content API switched on for the website.** It is in beta and enabled
  per site: [request access](https://developers.realgeeks.com/request-api-access/)
  or ask Real Geeks support. Until then every command fails with
  `api_disabled`.
- **The site owner's approval.** Only the owner (or a Real Geeks superuser)
  can approve geekcli for a site, in the site's own admin. Anyone else can use
  a key the owner creates for them under **Admin → API keys**.
- A Mac, Linux or Windows terminal. If you would rather just ask in plain
  English, connect the site to Claude or ChatGPT instead (see
  [AI assistants](https://developers.realgeeks.com/content-api/#ai-assistants));
  it uses the same API and permissions with nothing to install.

Changes are live on the site as soon as a command succeeds, exactly like
edits in the admin. Blog posts are the exception: geekcli creates them as
drafts until you publish.

## For AI agents

Start with `geekcli guide`: it is the complete manual for driving the CLI
(also at [docs/GUIDE.md](docs/GUIDE.md)), and `geekcli guide <topic>` prints
one section. [AGENTS.md](AGENTS.md) has the short version: install, log in,
and the rules that keep a live site safe.

## Install

macOS and Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/realgeeks/geekcli/main/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/realgeeks/geekcli/main/install.ps1 | iex
```

The scripts pick the archive for your platform from the latest
[GitHub release](https://github.com/RealGeeks/geekcli/releases), verify its
sha256, and install `geekcli` (`/usr/local/bin` or `~/.local/bin`;
`%LOCALAPPDATA%\Programs\geekcli` on Windows, added to your PATH).

`GEEKCLI_INSTALL_DIR` changes the destination and `GEEKCLI_VERSION` pins a
release. Prefer the environment variables when the script is piped: a flag
written after `| sh` or `| iex` goes to the shell, not to the script —
`| sh --version v0.3.0` makes `sh` print its own version and install nothing.
Run the script as a file and `--version v0.3.0` / `-Version v0.3.0` work as
usual; piped, use `| sh -s -- --version v0.3.0` or, in PowerShell,
`& ([scriptblock]::Create((irm $url))) -Version v0.3.0`.

Archives are built for macOS (Apple silicon, Intel), Linux (x86_64, arm64;
static binaries that run on any distribution, Alpine included) and Windows
(x86_64), each with a `.sha256` beside it, so any download works too. Every
archive also carries a signed build-provenance attestation; check that a
download was built from this repository with
`gh attestation verify <archive> --repo RealGeeks/geekcli`.

To update, run `geekcli update`: it installs the latest release over the
binary you have, after the same sha256 check (`geekcli update --check` only
reports whether there is one). geekcli never updates on its own; on a
terminal it mentions a newer release once a day
(`GEEKCLI_NO_UPDATE_CHECK=1` turns that off), and scripts never see that.

To uninstall, delete the `geekcli` binary and, if you no longer want the
stored keys, `~/.config/geekcli/` (revoke the keys under **Admin → API keys**
on each site too).

Or from source (needs a Rust toolchain):

```bash
cargo install --locked --git https://github.com/RealGeeks/geekcli   # or: cargo install --locked --path .
```

## Authentication

Every site has its own API keys. The easiest way to get one is to let the
CLI ask the site for it, `gh auth login` style:

```bash
geekcli auth login --site www.example.com
```

The CLI opens an approval page on the site's admin. The site's normal login
runs (including 2FA), you see the key name and scopes and click Approve, and
the browser hands a one-time code back to the CLI on a loopback port. The CLI
redeems it with a PKCE-style verifier and stores the key. The CLI never sees
your password and holds no client secret. Only the site owner or a Real
Geeks superuser can approve. `--print-url` prints the URL instead of opening
it; it still needs to be opened in an already signed-in browser to approve the
CLI. `--no-browser` remains as a legacy alias. `--name` and `--scope` set what is minted (default: every write scope,
which implies the read scopes; pass `--scope blog:write` to narrow it).

A key created by hand under **Admin → API keys** works too:

```bash
# read from stdin so the key stays out of shell history and the process list
pbpaste | geekcli auth login --site www.example.com --api-key-stdin
```

Every key expires. A browser login mints a six-month key; keys created by
hand live at most six months. `geekcli me` shows `expires_at`, and an
expired key fails with `token_expired` (exit 3) until you log in again.

Every key is verified against `GET /api/v3/me/` before it is saved to
`~/.config/geekcli/config.toml` (mode 600; `XDG_CONFIG_HOME` is honoured). The first site becomes the default;
`geekcli auth use <domain>` switches, `geekcli auth sites` lists, `geekcli auth logout`
forgets.

Environment variables cover CI and agent sandboxes without a config file:

| Variable             | Same as                                                           |
| -------------------- | ----------------------------------------------------------------- |
| `GEEKCLI_SITE`       | `--site`                                                          |
| `GEEKCLI_API_KEY`    | `--api-key`                                                       |
| `GEEKCLI_BASE_URL`   | `--base-url` (point at `http://localhost:8000` for local dev)     |
| `GEEKCLI_CONFIG_DIR` | where the config file lives                                       |
| `GEEKCLI_BROWSER`    | Chrome/Chromium binary for `snapshot` and `inspect` (`--browser`) |

To revoke access, delete the key under **Admin → API keys** on the site; it
stops working immediately. `geekcli auth logout --site www.example.com`
forgets the stored key on your computer.

## A quick tour

Every command explains itself with `--help`, for example
`geekcli posts create --help`.

Write a blog post in Markdown, then publish it or schedule it:

```bash
geekcli posts create --title "Spring market update" --slug spring-market-update \
    --content-file post.md --category market-updates --create-categories   # saved as a draft
geekcli posts publish spring-market-update
geekcli posts publish spring-market-update --at 2026-11-01T09:00:00-05:00
```

Update a page; only what you pass changes:

```bash
geekcli pages update /about/ --content-file about.md
geekcli pages update /buying/ --meta-description "Everything you need to know about buying in Riverside."
```

Build a neighborhood (area) page. Its listings come from its search, not its
name, so check the search first:

```bash
geekcli search fields                                  # every field this site's searches accept
geekcli search check subdivision=Downtown --count      # does the MLS know the value? how many listings?
geekcli area-pages create --slug downtown --area-name Downtown --anchor-text "Downtown Homes" \
    --search-criteria subdivision=Downtown --content-file downtown.md
```

Upload a photo and get back the URL to use in posts and pages, then look at
the result (snapshots use your installed Chrome or Chromium):

```bash
geekcli files upload hero.jpg --to images
geekcli snapshot / --full
geekcli snapshot /blog/spring-market-update/ --mobile --full
```

[docs/GUIDE.md](docs/GUIDE.md) (also `geekcli guide`) covers every task,
including navigation, sidebars, footers, banners, featured pages, market
report pages, settings and design.

## Using it with an AI coding agent

Coding agents such as Claude Code or Codex run commands, so they can drive
geekcli directly, and `geekcli guide` is written for them.

1. Install geekcli and run `geekcli auth login` yourself, so the agent never
   handles the site owner's sign-in.
2. Ask for what you want and point it at the guide, for example: *"Use
   geekcli to write a draft blog post about this spring's market in
   Riverside, with a header image. Run `geekcli guide` first."*
3. The agent checks the site with `geekcli me`, does the work, and can take
   snapshots to check the result.

Ask it to keep posts as drafts and to show you a snapshot before changing
anything visible on the home page or navigation; the guide already tells
agents to work that way.

## Staying safe on a live site

- **Drafts first**: new posts stay drafts until `geekcli posts publish`.
  Drafts and scheduled posts are not visible to visitors or search engines.
- **Undo**: pages, area and agent pages, market reports, posts, footers, the
  home page, sidebars, navigation bars, Featured Pages groups, banners,
  settings and design all keep a history. `geekcli pages revisions /buying/`
  lists versions and `geekcli pages revert /buying/ <id>` goes back to one;
  the same three subcommands work in every one of those groups
  (`geekcli settings revert <id>`, `geekcli nav revert top_primary <id>`).
- **Files come back**: a file deleted or overwritten in the last 90 days is
  restored at the same URL with `geekcli files restore images/logo.png`;
  `geekcli files deleted` lists what can be. Deleting anything other than a
  file is permanent, and blog categories and the blog landing page keep no
  history.
- **Preview design changes**: `geekcli design preview` returns a link that
  shows a new template or color scheme without saving it.
- **Warnings**: when a write is saved but probably not what you meant (a city
  the MLS doesn't know, markup the site can't keep), geekcli prints a
  `warning:` line, with suggestions where it can.
- **History**: every change is recorded in the site's change history, labelled
  with the key that made it.


## Commands

```
geekcli auth        login | logout | status | sites | use
geekcli me
geekcli posts       list | get | create | update | delete | publish | unpublish |
                      revisions | revision | revert
geekcli categories  list | get | create | update | delete
geekcli blog        get | update
geekcli pages       list | get | create | update | delete | search | revisions | revision | revert
geekcli area-pages  list | get | create | update | delete | search | revisions | revision | revert
geekcli agent-pages list | get | create | update | delete | revisions | revision | revert
geekcli agents
geekcli market-reports list | get | create | update | delete | revisions | revision | revert
geekcli home-page   get | update | revisions | revision | revert
geekcli templates   list
geekcli search      fields | choices | check | run | url
geekcli nav         list | get | add | update | move | remove | set | clear |
                      revisions | revision | revert
geekcli sidebars    list | get | create | rename | delete | item | add-html | add-links |
                      update-item | move-item | remove-item | set-items |
                      revisions | revision | revert
geekcli footers     list | get | create | update | delete | revisions | revision | revert
geekcli banners     list | get | create | update | delete | revisions | revision | revert
geekcli featured    list | get | create | update | delete | add-tile | update-tile | remove-tile | set-tiles |
                      revisions | revision | revert
geekcli settings    list | groups | get | set | clear | revisions | revision | revert
geekcli design      get | templates | variation | set | preview | revisions | revision | revert
geekcli files       list | get | url | upload | mkdir | move | delete |
                      versions | deleted | restore
geekcli snapshot    [PATH] [--full|--mobile] [--selector CSS] [--out FILE]
geekcli inspect     [PATH] --text|--html|--attr|--count|--exists|--visible|--js|--assert …
geekcli api         METHOD PATH [-p k=v] [-d JSON]
geekcli guide
geekcli update      [--check] [--tag vX.Y.Z]
geekcli completions <shell>
```

`geekcli <command> --help` lists every flag, with notes on what tends to go
wrong for that command. `geekcli guide` is the full walkthrough for scripts
and AI agents; `guide --list` shows its topics and `guide <topic>` prints one,
such as `guide html` (what the sanitizer keeps and what breaks a page),
`guide search` (criteria a site accepts) or `guide rebrand` (every place a
site's identity lives).

Highlights:

- **Output is JSON when piped**, a table on a terminal. `--json`, `-o jsonl`,
  `-o table` override; `-q` prints only ids, `-y` skips confirmation prompts
  and `-v` logs requests to stderr. Warnings the API returns with a
  successful write print to stderr as `warning: …` in every mode;
  `--fail-on-warnings` makes them exit 5 (after the write).
- **Stable exit codes**: 1 other failure, including a temporary outage behind
  the site (`crm_unavailable`, `design_catalogue_unavailable`,
  `files_unavailable`) worth retrying; 2 usage, 3 auth, which also covers a
  site whose API is switched off (`api_disabled`); 4 not found, 5 validation,
  6 conflict, 7 rate limited, 8 network, 9 this geekcli is too old for the
  API (run `geekcli update`). Errors are JSON on stderr in JSON
  mode and include the API's per-field messages.
- **Markdown in, HTML out**: `--content-file post.md` or `--markdown` converts
  before sending. `<!--read more-->` marks the summary break in posts. Content
  columns are Windows-1252: arrows, CJK text and emoji are rejected with a 422
  naming the character, so write those as entities (`&#8594;`).
- **Your own CSS and schema markup**: the API keeps `<style>` blocks, inline
  `style`, JSON-LD (`<script type="application/ld+json">`), microdata
  attributes and inline `<svg>` in content, so a designed page can be edited
  without losing them. Other scripts, event handlers and forms are still
  removed and reported as warnings; `geekcli guide html` has the rules.
- **Drafts by default**: `posts create` sends `status: draft` unless you pass
  `--status published`. The API itself defaults to published, which is the
  wrong default for automation.
- **Undo**: `revisions`, `revision <ref> <id>` and `revert <ref> <id>` list,
  preview and undo saves on pages, agent pages, area pages, market reports,
  blog posts, footers, the home page, sidebars, navigation bars, Featured
  Pages groups and banners; `settings` and `design` have the same three
  without a `<ref>`. `files versions`, `files deleted` and `files restore`
  undo a file delete or overwrite from the last 90 days. A revert that can
  no longer be applied (the URL or name is taken, the template is gone)
  exits 6.
- **Partial updates**: `update` sends a PATCH with only the flags you passed.
  `--replace` sends a PUT. `--data '{...}'`, `--data @file.json` or
  `--data -` merges arbitrary fields.
- **Human references**: posts and categories by slug, pages by `/path/` or
  slug, `--parent /resources/` on page creation.
- **Category helpers**: `--create-categories` makes missing slugs on the fly;
  `--add-category`, `--remove-category` and `--clear-categories` edit the set
  without restating it.
- **Search criteria you can trust**: `search fields` and `search choices` list
  what a site accepts, `search check` and `search run` show what the site
  actually understood and flag ignored keys (exit 5) and values that are
  not the site's (wrong case, typos, with suggestions), `search url --save`
  stores a search and returns the id pages use. `pages search <ref>` shows
  the saved search behind a page, and `pages update --search-criteria …`
  or `--sidebar <name>` attaches a search or sidebar to it.
- **Navigation, sidebars and settings**: edit nav links by position name,
  build sidebar items from Markdown or `--link Text=/url/` flags, and change
  site settings with `NAME=value` pairs that are typed from the setting's
  definition before they are sent.
- **Design**: `design templates`, `design preview --snapshot` and `design set`
  change the template and colour scheme, with an unsaved preview first.
- **Blog landing page**: `blog get|update` for the blog's own title, meta and heading.
- **Files**: upload images and PDFs to the site's media bucket, or have the
  site fetch one from a URL (`--from-url`), and get back the public URL to
  use in content, posts and settings.
- **Browser checks**: `geekcli snapshot / --full` renders the whole page with
  your installed Chrome (driven over DevTools), while `--selector '.hero'`
  captures one component. `geekcli inspect / --assert "document.querySelectorAll('h1').length === 1"`
  lets an agent verify rendered DOM behavior before moving on.
- **429s are retried** with `Retry-After`, up to `--max-retries` (default 3). The limit
  is 600 requests an hour per key.

## Troubleshooting

- **`api_disabled`** (exit 3): the Content API is off for that site.
  [Request access](https://developers.realgeeks.com/request-api-access/) or
  contact Real Geeks support.
- **`token_expired`** (exit 3): keys last six months. Run
  `geekcli auth login --site www.example.com` again.
- **`command not found: geekcli`**: open a new terminal. If it is still
  missing, run the installer again: its `Installed …` line says where geekcli
  went, and the next line shows how to add that folder to your PATH if it
  isn't already.
- **Not the site owner?** Ask the owner to run the login, or to create a key
  under **Admin → API keys** for you, then run
  `geekcli auth login --site www.example.com --api-key-stdin`, paste the key,
  press Enter and finish with Ctrl-D (Ctrl-Z then Enter on Windows).
- **A character is rejected** (`422` naming it): site text is stored as
  Windows-1252, so emoji, arrows and CJK text can't be saved. Remove them or
  write them as HTML entities (`&#8594;`).
- **Anything else**: add `-v` to see each request and response, and send the
  output to Real Geeks support or open a
  [GitHub issue](https://github.com/RealGeeks/geekcli/issues).

## FAQ

**Does it cost anything?** No. geekcli is free and open source (MIT).

**How is it different from connecting an AI assistant?** Both use the same
Content API and permissions. Connecting Claude or ChatGPT to the site lets you
manage it by chatting, with nothing to install. geekcli runs on your computer,
for people and coding agents who work in a terminal or want to script
repeated tasks.

**Can it do everything the admin can?** It covers blog posts and categories,
pages, area, agent and market report pages, the home page, navigation,
sidebars, footers, banners, featured pages, editable settings, design and
files. Leads, the CRM and
billing stay in the admin and the CRM.

**More than one website?** Log in to each once; `geekcli auth sites` lists
them, `geekcli auth use <domain>` switches the default, and `--site` picks one
for a single command.

**Where is the full reference?** The
[Content API documentation](https://developers.realgeeks.com/content-api/)
for every field and rule, and [docs/GUIDE.md](docs/GUIDE.md) for the command
behind each.

## Development

```bash
cargo build
cargo test                                   # unit + mock-server integration tests
cargo clippy --all-targets -- -D warnings    # must be clean
cargo fmt
```

CI runs the same three on every push and PR (`.github/workflows/`), plus
`cargo audit` nightly. PR titles must be conventional commits.

Releases are cut by release-please from the conventional commit history:
merging the open "chore(main): release" PR tags a version, and the Release
workflow builds the archives and attaches them to the GitHub release.

## Support

- Questions about a Real Geeks website or account:
  [Real Geeks support](https://support.realgeeks.com/).
- Bugs and feature requests for the CLI:
  [GitHub issues](https://github.com/RealGeeks/geekcli/issues).
- Security problems: [private reporting](SECURITY.md), never a public issue.
- Contributing: see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT. See [LICENSE](LICENSE). Release archives include
`THIRD_PARTY_LICENSES.html` for the open-source crates built into the binary.
