# geekcli

The Real Geeks command line. `geekcli` manages a Real Geeks website: blog
posts and categories, content, area and agent pages, the home page,
navigation, sidebars, footers, settings, design and uploaded files. It is
built to be driven by scripts and AI agents as much as by people: JSON on
stdout when piped, JSON errors on stderr, stable exit codes, and a built-in
guide written for agents.

```bash
geekcli auth login --site www.example.com
geekcli me
geekcli posts create --title "Spring market update" --slug spring-market-update \
    --body-file post.md --category market-updates --create-categories
geekcli posts publish spring-market-update
```

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
[GitHub release](https://github.com/realgeeks/geekcli/releases), verify its
sha256, and install `geekcli` (`/usr/local/bin` or `~/.local/bin`;
`%LOCALAPPDATA%\Programs\geekcli` on Windows, added to your PATH).

`GEEKCLI_INSTALL_DIR` changes the destination and `GEEKCLI_VERSION` pins a
release. Prefer the environment variables when the script is piped: a flag
written after `| sh` or `| iex` goes to the shell, not to the script —
`| sh --version v0.3.0` makes `sh` print its own version and install nothing.
Run the script as a file and `--version v0.3.0` / `-Version v0.3.0` work as
usual; piped, use `| sh -s -- --version v0.3.0` or, in PowerShell,
`& ([scriptblock]::Create((irm $url))) -Version v0.3.0`.

Archives are built for macOS (Apple silicon, Intel), Linux (x86_64, arm64) and
Windows (x86_64), each with a `.sha256` beside it, so any download works too.

Or from source (needs a Rust toolchain):

```bash
cargo install --git https://github.com/realgeeks/geekcli   # or: cargo install --path .
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
geekcli auth login --site www.example.com --api-key rg_live_...
# or read it from stdin so it never lands in shell history
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

## Commands

```
geekcli auth        login | logout | status | sites | use
geekcli me
geekcli posts       list | get | create | update | delete | publish | unpublish
geekcli categories  list | get | create | update | delete
geekcli blog        get | update
geekcli pages       list | get | create | update | delete | search | revisions | revision | revert
geekcli area-pages  list | get | create | update | delete | search
geekcli agent-pages list | get | create | update | delete | revisions | revision | revert
geekcli agents
geekcli home-page   get | update | revisions | revision | revert
geekcli templates   list
geekcli search      fields | choices | check | run | url
geekcli nav         list | get | add | update | move | remove | set | clear
geekcli sidebars    list | get | create | rename | delete | item | add-html | add-links |
                      update-item | move-item | remove-item | set-items
geekcli footers     list | get | create | update | delete
geekcli featured    list | get | create | update | delete | add-tile | update-tile | remove-tile | set-tiles
geekcli settings    list | groups | get | set | clear
geekcli design      get | templates | variation | set | preview
geekcli files       list | get | url | upload | mkdir | move | delete
geekcli snapshot    [PATH] [--full|--mobile] [--selector CSS] [--out FILE]
geekcli inspect     [PATH] --text|--html|--attr|--count|--exists|--visible|--js|--assert …
geekcli api         METHOD PATH [-p k=v] [-d JSON]
geekcli guide
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
  and `-v` logs requests to stderr.
- **Stable exit codes**: 1 other failure, including a temporary outage behind
  the site (`crm_unavailable`, `design_catalogue_unavailable`,
  `files_unavailable`) worth retrying; 2 usage, 3 auth, which also covers a
  site whose API is switched off (`api_disabled`); 4 not found, 5 validation,
  6 conflict, 7 rate limited, 8 network. Errors are JSON on stderr in JSON
  mode and include the API's per-field messages.
- **Markdown in, HTML out**: `--body-file post.md` or `--markdown` converts
  before sending. `<!--read more-->` marks the summary break in posts. Content
  columns are Windows-1252: arrows, CJK text and emoji are rejected with a 422
  naming the character, so write those as entities (`&#8594;`).
- **Drafts by default**: `posts create` sends `status: draft` unless you pass
  `--status published`. The API itself defaults to published, which is the
  wrong default for automation.
- **Undo**: `pages revisions`, `pages revision <id>` and `pages revert <id>` (also
  on agent pages and the home page) list, preview and undo saves.
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
  actually understood and flag ignored keys (exit 5), `search url --save`
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
- **Files**: upload images and PDFs to the site's media bucket and get back
  the public URL to use in content, posts and settings.
- **Browser checks**: `geekcli snapshot / --full` renders the whole page with
  your installed Chrome (driven over DevTools), while `--selector '.hero'`
  captures one component. `geekcli inspect / --assert "document.querySelectorAll('h1').length === 1"`
  lets an agent verify rendered DOM behavior before moving on.
- **429s are retried** with `Retry-After`, up to `--max-retries` (default 3). The limit
  is 600 requests an hour per key.

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

## License

MIT. See [LICENSE](LICENSE).
