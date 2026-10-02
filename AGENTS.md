# AGENTS.md

Notes for AI agents. The first part is for agents that use `geekcli` to work
on a Real Geeks website; the second is for agents changing this repository.

## Using geekcli on a Real Geeks site

`geekcli` is the Real Geeks command line. Today it manages a site's website
content: blog posts and categories, content, area and agent pages, the home
page, navigation, sidebars, footers, settings, design and uploaded files.

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
   codes (3 auth, 4 not found, 5 validation, 7 rate limited). On exit 5 read
   `error.fields` and fix those fields.
3. `posts create` makes drafts unless you pass `--status published`.
4. `update` only changes the flags you pass. Avoid `--replace`.
5. Note `revisions <ref> --limit 1` before a large rewrite of a page, area
   page, post or footer, so a bad result is one `revert` away.
6. Check search criteria with `search check` before putting them on a page;
   the site silently drops criteria it does not know.
7. After a visible change, look at it: `geekcli snapshot <path> --full`
   (and `--mobile`), or assert on the DOM with `geekcli inspect`.
8. Anything the API cannot reach, report it to your human rather than
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
├── client.rs          # blocking reqwest client: bearer auth, envelope, pagination, 429 retry
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
    ├── home_page.rs
    ├── nav.rs         # navigation bar links (bars are fixed)
    ├── sidebars.rs    # sidebars and html/links items
    ├── footers.rs     # shared footer HTML blocks
    ├── settings.rs    # typed site settings; coerce() maps NAME=value text to JSON
    ├── design.rs      # template + colour scheme; preview link rendered via snapshot::render
    ├── featured.rs    # Featured Pages tile groups (anna-modern home page)
    ├── files.rs       # media bucket files; multipart upload via Client::post_multipart
    ├── snapshot.rs    # PNG of a page via an installed Chrome over DevTools (headless_chrome crate)
    ├── inspect.rs     # DOM queries and assertions on a rendered page, same browser
    ├── templates.rs
    ├── revisions.rs   # revision list/preview/revert shared by pages, agent pages, home page
    ├── search.rs      # property search (/api/v2/search/): fields, check, run, url, saved-search lookup
    ├── guide.rs       # `guide [topic]` over docs/GUIDE.md
    └── api.rs         # raw METHOD PATH escape hatch
tests/                 # end-to-end tests against a mockito server; snapshot/inspect use a real Chrome when installed
docs/GUIDE.md          # agent-facing guide, embedded via include_str!; `guide <topic>` prints one `## N. Title` section
```

### Conventions

- Write bodies contain only the fields the caller gave (`Payload`), which is
  what makes `update` a true PATCH. Never add defaults on update.
- `posts create` defaults `status` to `draft` on purpose; the API defaults to
  published. Keep that.
- Human-only text goes to stderr via `Printer::note`; stdout is reserved for
  the result document.
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
- Search commands use the site's public `/api/v2/search/` and
  `/search_forms/api/advanced_search_form.json` through `Client::site_get`;
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
fix the code. CI runs them on every push and PR. `release.yml` uses
release-please: merging its release PR tags a version, builds the
macOS/Linux/Windows archives onto the GitHub release, then runs `install.sh`
and `install.ps1` against it. Keep those scripts in step with the archive
names in `release.yml`.

### Commits

Conventional commits (`feat:`, `fix:`, `docs:`, `test:`, `chore:` …), written
plainly. Say what changed and why when the why is not obvious from the diff.
