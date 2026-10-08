# AGENTS.md

Notes for AI agents. The first part is for agents that use `geekcli` to work
on a Real Geeks website; the second is for agents changing this repository.

## Using geekcli on a Real Geeks site

`geekcli` is the Real Geeks command line. Today it manages a site's website
content: blog posts and categories, content, area, agent and market report
pages, the home page, navigation, sidebars, footers, banners, settings,
design and uploaded files.

Install:

```bash
curl -fsSL https://raw.githubusercontent.com/realgeeks/geekcli/main/install.sh | sh
# Windows: irm https://raw.githubusercontent.com/realgeeks/geekcli/main/install.ps1 | iex
```

Log in. Browser login needs the site owner to click Approve, so ask your
human to run it, or to give you a key:

```bash
geekcli auth login --site www.example.com            # opens the site's approval page
GEEKCLI_SITE=www.example.com GEEKCLI_API_KEY=rg_live_... geekcli me   # a key from Admin → API keys
```

The API behind it is documented at
<https://developers.realgeeks.com/content-api/> (see its
[changelog](https://developers.realgeeks.com/content-api/changelog/) for what is
new). Then read the CLI's manual before writing anything:

```bash
geekcli guide            # the whole guide for scripts and agents (docs/GUIDE.md)
geekcli guide --list     # its topics
geekcli guide html       # one topic: what survives the sanitizer and theme
geekcli <command> --help # every flag, plus notes on what tends to go wrong
```

The rules that keep a live site safe:

1. `geekcli me` first, to confirm the site and the key's scopes.
2. Output is JSON when piped and errors are JSON on stderr with stable exit
   codes (3 auth, 4 not found, 5 validation, 7 rate limited, 9 geekcli too
   old). On exit 5 read `error.fields` and fix those fields; on exit 9 run
   `geekcli update` and repeat the command.
3. `posts create` makes drafts unless you pass `--status published`.
4. `update` only changes the flags you pass. Avoid `--replace`.
5. Read `warnings` on writes. A `warning:` line on stderr (and a
   `warnings` array in the result) means the write was applied but not as
   asked: HTML was stripped, or a search value matched nothing. Fix it and
   write again. `--fail-on-warnings` turns them into exit 5.
6. Note `revisions <ref> --limit 1` before a large rewrite of a page, area
   page, market report, post, footer, sidebar or navigation bar (and
   `settings revisions` / `design revisions` before those), so a bad result
   is one `revert` away. A deleted or overwritten file comes back with
   `files restore` for 90 days.
7. Check search criteria with `search check --count --strict` before putting
   them on a page; the site silently drops criteria it does not know, and a
   value in the wrong case (`McLean` for `Mclean`) matches nothing.
8. After a visible change, look at it: `geekcli snapshot <path> --full`
   (and `--mobile`), or assert on the DOM with `geekcli inspect`.
9. Anything the API cannot reach, report it to your human rather than
   working around it.

## Working on this repository

`geekcli` is a Rust CLI over a Real Geeks site's `/api/v3/` content API.
Its main users are scripts and AI agents, so stdout is machine-readable JSON
when piped, errors are JSON on stderr, and exit codes are stable and
documented in `docs/GUIDE.md` (printed by `geekcli guide`).

### Layout

```
src/
├── main.rs            # parse, run, print error, exit with the mapped code
├── cli.rs             # clap command tree and dispatch
├── config.rs          # ~/.config/geekcli/config.toml, site/key resolution
├── client.rs          # blocking reqwest client: bearer auth, envelope, `warnings`, pagination, 429 retry
├── error.rs           # Error enum → exit codes
├── output.rs          # json / jsonl / table printing, error printing
├── html.rs            # body input from flag/file/stdin, Markdown → HTML
├── auth/browser.rs    # browser-approval login: cli/start → loopback callback → cli/token
└── commands/
    ├── mod.rs         # Context, Payload builder, id/slug/path resolution, paging
    ├── auth.rs        # login / logout / status / sites / use
    ├── posts.rs       # blog posts (+ publish/unpublish, category add/remove)
    ├── blog_home.rs   # the blog landing page (title, meta, heading)
    ├── categories.rs  # blog categories (+ ensure_exist for --create-categories)
    ├── pages.rs       # content pages; TreeFields/TreeListArgs shared with area pages
    ├── area_pages.rs
    ├── agent_pages.rs # agent landing pages (+ `agents` CRM list); reuses pages helpers
    ├── market_reports.rs # market report pages (search, sold_within, header); reuses pages helpers
    ├── home_page.rs
    ├── nav.rs         # navigation bar links (bars are fixed)
    ├── sidebars.rs    # sidebars and html/links items
    ├── footers.rs     # shared footer HTML blocks
    ├── settings.rs    # typed site settings; coerce() maps NAME=value text to JSON
    ├── design.rs      # template + colour scheme; preview link rendered via snapshot::render
    ├── banners.rs     # banners; pages attach one with --banner (pages::AttachArgs)
    ├── featured.rs    # Featured Pages tile groups (anna-modern home page)
    ├── files.rs       # media bucket files; multipart upload via Client::post_multipart; versions/deleted/restore
    ├── snapshot.rs    # PNG of a page via an installed Chrome over DevTools (headless_chrome crate)
    ├── inspect.rs     # DOM queries and assertions on a rendered page, same browser
    ├── templates.rs
    ├── revisions.rs   # revision list/preview/revert shared by every resource with undo (pages … settings, design)
    ├── search.rs      # property search (/api/v2/search/): fields, check, run, url, saved-search lookup
    ├── guide.rs       # `guide [topic]` over docs/GUIDE.md
    ├── update.rs      # self-update from the GitHub release archives (self_update crate, its own client)
    └── api.rs         # raw METHOD PATH escape hatch
tests/                 # end-to-end tests against a mockito server; snapshot/inspect use a real Chrome when installed
docs/GUIDE.md          # agent-facing guide, embedded via include_str!; `guide <topic>` prints one `## N. Title` section
```

### Conventions

- Write bodies contain only the fields the caller gave (`Payload`), which is
  what makes `update` a true PATCH. Never add defaults on update.
- `posts create` defaults `status` to `draft` on purpose; the API defaults to
  published. Keep that.
- `area-pages create` refuses to run without `--search-criteria` or
  `--search` unless `--no-search` is passed: area pages have no draft state
  and `--area-name` does not filter listings. Keep that.
- Human-only text goes to stderr via `Printer::note`; stdout is reserved for
  the result document. API `warnings` are the exception: `Client` prints them
  with `eprintln!` in every mode, since agents run in JSON mode and need them.
- New commands: add a module under `commands/`, a variant in `cli::Command`,
  a section in `docs/GUIDE.md`, and an integration test under `tests/`.
- Field knowledge lives in three places, by depth: an `after_help` note on the
  command it applies to, a `## N. Title` section in `docs/GUIDE.md` (kept
  addressable by `guide <topic>`; add an alias in `guide.rs` if the title is
  not the obvious word), and the rebranding checklist for anything that spans
  commands. Write a lesson down everywhere an agent would meet it.
- The CLI holds no Real Geeks password or client secret. Login goes through
  the site's own `auth/cli/start` + admin approval + `auth/cli/token`
  endpoints, and the login POSTs only follow redirects within the same site.
- The API key never leaves the site's origin: no cross-origin redirects,
  no full URLs off the site, https only (plain http just for local dev
  hosts). Keep it that way in any new request path.
- `geekcli update` is the one request path that leaves the site: it talks to
  GitHub through `self_update`'s own client and must never be handed the
  site key. Nothing updates in the background; scripts and agents need the
  version to stay put between calls. `update::notify` mentions a newer
  release once a day, on a terminal only. Retiring a version is the API's
  job: it sees `User-Agent: geekcli/<version>` and answers `client_too_old`
  (or 426), which `error.rs` maps to exit 9 and a hint to update. Its archive and folder names come from
  `release.yml`, like the install scripts'.
- Search commands use the site's public `/api/v2/search/`, its field
  catalog `/search_forms/api/dump_uberform_fields.json` (falling back to
  `/search_forms/api/advanced_search_form.json` on sites without one) and
  the autocomplete index, through `Client::site_get`;
  everything else goes through `/api/v3/`. The site drops unknown criteria
  silently, so `understand()` in `search.rs` always diffs input keys against
  what `/metadata/` echoes back.
- Field and filter names mirror the API exactly. When the API grows a field,
  add a flag with the same name. The public reference is
  <https://developers.realgeeks.com/content-api/>, and its changelog lists
  what the API has added; every `docs/GUIDE.md` section ends with an
  `API reference:` link to the matching part of it.

### Build and test

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings   # pedantic, unwrap/expect/panic denied
cargo fmt
```

All must pass clean before a commit. Do not silence lints with `#[allow]`;
fix the code. CI runs them on every push and PR, plus `cargo deny check`
(advisories, licenses, sources; see `deny.toml`), a check on the MSRV
(`rust-version` in Cargo.toml), a static musl build run on old distros, and
zizmor over the workflows. Actions are pinned by commit SHA; Dependabot
updates the pins. `release.yml` uses
release-please: merging its release PR tags a version, builds the
macOS/Linux (static musl)/Windows archives with THIRD_PARTY_LICENSES.html
(cargo-about, `about.toml`), attests their build provenance, uploads them to
the GitHub release, then runs `install.sh` and `install.ps1` against it. Keep those scripts and `commands/update.rs` in step with the archive
names in `release.yml`.

### Commits

Conventional commits (`feat:`, `fix:`, `docs:`, `test:`, `chore:` …), written
plainly. Say what changed and why when the why is not obvious from the diff.
