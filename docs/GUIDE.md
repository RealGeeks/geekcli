# geekcli — guide for scripts and AI agents

`geekcli` is the Real Geeks command line. It works on one Real Geeks website at
a time through the site's API (`/api/v3/`): blog posts and categories, content,
area, agent and market report pages, the home page, navigation, sidebars,
footers, banners, settings, design and files. This guide is the contract an
automated caller can rely on.

The API itself is documented at <https://developers.realgeeks.com/content-api/>,
with a [changelog](https://developers.realgeeks.com/content-api/changelog/) of
what it has gained. Each section below links to the matching part of that
reference. The API is switched off until Real Geeks enables it for a site
(`api_disabled`, exit 3); site owners can
[request access](https://developers.realgeeks.com/request-api-access/).

## 1. Setup

Install:

```bash
curl -fsSL https://raw.githubusercontent.com/realgeeks/geekcli/main/install.sh | sh   # macOS, Linux
irm https://raw.githubusercontent.com/realgeeks/geekcli/main/install.ps1 | iex        # Windows PowerShell
```

The scripts verify the release checksum; `geekcli --version` confirms it.
`geekcli update` installs a newer release later (§27); nothing updates on
its own.

```bash
# Approve the CLI in your browser: the site's admin login runs, you click
# Approve, and the key is minted and stored. No secrets in the CLI.
geekcli auth login --site www.example.com

# Or store a key that a site owner created under Admin → API keys, read
# from stdin so it stays out of shell history and the process list
pbpaste | geekcli auth login --site www.example.com --api-key-stdin

# Confirm the key, its scopes and the site
geekcli me
```

Browser login opens `https://<site>/admin/api_keys/apikey/authorize-cli/…`
and waits on a loopback port for the approval. `--print-url` prints the
URL instead of opening it, but does **not** make login browserless: open that
URL in an already signed-in browser to approve the CLI. `--no-browser` is a
legacy alias. `--port` pins the loopback port. Only the site
owner or a Real Geeks superuser can approve. By default the key gets every
write scope (`blog`, `pages`, `area_pages`, `home_page`, `navigation`,
`sidebars`, `settings`, `files`, `footers`, `design`); `--scope` narrows it.

Every key expires. A browser login mints a six-month key; a key made under
Admin → API keys lives at most six months. `geekcli me` shows `expires_at`.
An expired key fails every call with `token_expired` (exit 3, with a hint);
`auth login` again mints a new one.

Keys are stored per site in `~/.config/geekcli/config.toml` (mode 600; `XDG_CONFIG_HOME` is honoured). The
first stored site becomes the default; `geekcli auth use <domain>` changes it.
`geekcli auth sites` lists what is stored.

A site can have two names: the one you logged in with (often
`example.realgeeks.com`) and its live domain (say `www.example.com`), which is
what `geekcli me` reports as `site.domain`. The key is stored under the login
name, and login also records the live domain and current URL from `/me` as
aliases, so `--site`, `GEEKCLI_SITE`, `auth use` and `auth logout` accept
either name; both refer to the same site and key. Login prints
`Logged in to example.realgeeks.com (live domain: www.example.com)` and
`auth sites` shows an `aliases` column. A site stored by an older version has
no aliases yet: `geekcli auth sites --refresh` asks each stored site's `/me`
for its live domain and records it. If one alias belongs to more than one
stored site, `--site` with it is a usage error (exit 2) that names them; pass
a login name instead.

Environment variables work without a config file and are the easiest way to
run in CI or inside an agent sandbox:

| Variable             | Meaning                                           |
| -------------------- | ------------------------------------------------- |
| `GEEKCLI_SITE`       | site domain (same as `--site`)                    |
| `GEEKCLI_API_KEY`    | API key (same as `--api-key`)                     |
| `GEEKCLI_BASE_URL`   | API origin override, e.g. `http://localhost:8000` |
| `GEEKCLI_CONFIG_DIR` | where `config.toml` lives                         |

Precedence: flags, then environment, then the config file.

Keys only travel over https. Plain `http://` base URLs are refused except
for local dev hosts (`localhost`, loopback addresses, `*.localhost`,
`*.local`, `*.test`). A stored key is only sent to the server it was stored
for: with `--base-url` (or `GEEKCLI_BASE_URL`) pointing anywhere else, pass
that server's key with `--api-key-stdin`, `--api-key` or `GEEKCLI_API_KEY`.
The CLI never follows a redirect to another origin with the key, and never
sends it to a full URL off the site (`geekcli api GET https://elsewhere/`
is refused).

Shell completion: `geekcli completions zsh` (or bash, fish, powershell)
prints a script to source from your shell profile.

API reference: [Enabling](https://developers.realgeeks.com/content-api/#enabling), [Authentication](https://developers.realgeeks.com/content-api/#authentication), [Who am I](https://developers.realgeeks.com/content-api/#who-am-i).

## 2. Output

- **When stdout is not a terminal, output is JSON.** Force it with `--json`
  or `-o json`; `-o jsonl` prints one object per line for lists; `-o table`
  is the human view.
- A single resource prints as one JSON object. Lists print
  `{"results": [...], "pagination": {...}}`; with `--all` every page is
  fetched and `pagination` is omitted.
- `-q` / `--quiet` prints only ids, one per line. Handy after `create`.
- Human-only notes go to **stderr**, never stdout.
- **API warnings.** A successful response can carry a `warnings` array: the
  request worked, but not quite as asked. Each entry has a `code` and a
  `message`; known codes are `unknown_value` / `ambiguous_value` (a search
  value the site could not match, with `field`, `criterion`, `value` and
  `suggestions`) and `sanitized` (HTML the sanitizer removed, listed in
  `removed`). Every command prints each one to stderr as
  `warning: <message>`, in every output mode including JSON, with
  ` (did you mean: …?)` added for suggestions. The `warnings` array stays in
  the JSON on stdout. **By the time a warning prints, the write has already
  been applied**: read the warning, then fix the input and run the command
  again, or undo it with `revisions` / `revert`. Pass `--fail-on-warnings` (or
  set `GEEKCLI_FAIL_ON_WARNINGS=1`) to exit 5 after printing the result when
  any warning came back.
- Errors go to stderr as `{"error": {"code", "message", "status", "fields",
  "exit_code"}}` in JSON mode. `fields` maps request field names to messages
  when the API rejected input.

## 3. Exit codes

| Code | Meaning                                                 |
| ---- | ------------------------------------------------------- |
| 0    | success                                                 |
| 1    | other failure, including a temporary outage behind the site (`crm_unavailable`, `design_catalogue_unavailable`, `files_unavailable`): retry later |
| 2    | usage error (bad flags, missing required field)         |
| 3    | not logged in, invalid key, key lacks the scope, or the site's API is off (`api_disabled`: Real Geeks enables it per site) |
| 4    | not found                                               |
| 5    | validation error (see `fields`), bad request, or a body too large (413); with `--fail-on-warnings`, the API returned warnings (code `warnings`; the request succeeded and any write was applied) |
| 6    | conflict: a guarded delete (the message names the flag), or two writes raced on the same slug or name (retry) |
| 7    | rate limited after retries (`retry_after` seconds)      |
| 8    | network error                                           |
| 9    | this geekcli is too old for the site's API (`client_too_old`, or HTTP 426): run `geekcli update`, then run the command again (§27) |

429 responses are retried automatically up to `--max-retries` (default 3),
file uploads included, honouring `Retry-After` (seconds or an HTTP date;
5s when absent, each wait capped at 30s). Every wait prints one line on
stderr, with or without `-v`, so a long run never looks hung:

```
rate limited; retrying in 30s (attempt 2 of 3)
```

stdout is untouched. When retries run out the command exits 7. The limit is
600 requests per hour per key. If a response carries `X-RateLimit-Remaining`
and `X-RateLimit-Limit` and fewer than 10% of requests are left, a single
`warning: rate limit nearly used: …` line goes to stderr (once per run); slow
a batch down or pause until the window resets. `--max-retries 0` fails fast
on the first 429.
Errors that have an obvious next step carry a `hint` (in the JSON envelope and after the table message).

API reference: [Conventions](https://developers.realgeeks.com/content-api/#conventions).

## 4. Resources and references

Every resource has a numeric `id`. Commands also accept human references:

| Resource     | `<reference>` accepts                          |
| ------------ | ---------------------------------------------- |
| posts        | id or slug                                     |
| categories   | id or slug                                     |
| pages        | id, `/site/relative/path/`, or slug            |
| area-pages   | id, `/path/`, or slug                          |
| agent-pages  | id, `/path/`, or slug                          |
| market-reports | id, `/path/`, or slug                        |
| sidebars     | id or name                                     |
| footers      | id or name                                     |
| banners      | id or name                                     |
| nav bars     | id or position (`top_primary`, …); links by id, text or URL |

A slug that matches more than one page is an error; use the id or path.

**Body content.** Every command that takes HTML (posts, pages, area-pages,
agent-pages, home-page, blog, footers, sidebars `add-html`/`update-item`)
accepts `--content` / `--content-file`; use those everywhere.
`--body`/`--body-file` (the API's field name for posts) and
`--html`/`--html-file` (its name for sidebar items) work too, as aliases. A file
may be `-` for stdin; add `--markdown` (or use a `.md` file) to convert
Markdown to HTML.

## 5. Blog posts

```bash
geekcli posts list --state published --no-body
geekcli posts list --search "market" --category market-updates --all
geekcli posts get spring-market-update

# Create a DRAFT (the CLI defaults to draft; the API itself defaults to published)
geekcli posts create \
  --title "Spring market update" \
  --slug spring-market-update \
  --content-file post.md \
  --category market-updates --create-categories \
  --meta-description "Inventory is up across the metro."

# Edit: only the flags you pass change (PATCH)
geekcli posts update spring-market-update --title "Spring 2026 market update"
geekcli posts update 42 --add-category buyers --remove-category sellers

# Publish now, schedule, or revert to draft
geekcli posts publish spring-market-update
geekcli posts publish 42 --at 2026-10-01T09:00:00-05:00
geekcli posts unpublish 42

geekcli posts delete 42
```

Field flags: `--title --slug --content --content-file --markdown --status
--publish --category --create-categories --page-title --meta-description
--meta-keywords --facebook-image --allow-comments --nofollow-comments
--data`. `--data` takes a JSON object (inline, `@file`, or `-` for stdin)
for anything without a flag; explicit flags win over `--data`.

Notes:

- `--content-file x.md` (or `--markdown`) converts Markdown to HTML before
  sending. Raw HTML inside Markdown passes through.
- Put `<!--read more-->` in the body where the summary should end. It
  survives Markdown conversion and server-side sanitization.
- `--category` replaces the whole set; `--add-category` / `--remove-category`
  adjust it; `--clear-categories` empties it. Unknown slugs fail with a
  validation error unless `--create-categories` is given.
- `--publish` in the future makes the post `scheduled`; `state` in the
  response is `draft`, `scheduled` or `published`.
- `state` is the field that reflects visibility; `status` alone does not
  (a `published` post with a future publish date is `scheduled`). Only
  `state: published` posts are public. Drafts and scheduled posts return
  404 to visitors and search engines and stay out of the blog home,
  categories, archives, RSS feed and sitemap; a site admin logged into the
  site who can edit posts sees them at their URL as a preview marked
  `noindex`.
- `--replace` on `update` sends PUT: omitted optional fields reset.

API reference: [Blog posts](https://developers.realgeeks.com/content-api/blog-posts/#blog-posts).

## 6. Blog landing page

`geekcli blog get` and `geekcli blog update` manage the blog's own
page: `--title`, `--meta-description`, `--meta-keywords`, and `--content`
(the heading shown above the post list, usually one `<h2>`). The site
creates the page on the first update if it has none. When rebranding, this
heading is easy to miss: it is not a post and not a content page.

API reference: [Blog home page](https://developers.realgeeks.com/content-api/blog-posts/#blog-home-page).

## 7. Categories

```bash
geekcli categories list
geekcli categories create --name "Market Updates"           # slug derived
geekcli categories update market-updates --name "Market News"
geekcli categories delete market-updates --force            # also untags posts
```

API reference: [Blog categories](https://developers.realgeeks.com/content-api/blog-posts/#blog-categories).

## 8. Content pages

```bash
geekcli templates list
geekcli pages list --parent null                 # top-level pages
geekcli pages list --path /resources/buyers/
geekcli pages get /resources/buyers/

geekcli pages create \
  --slug sellers --parent /resources/ \
  --anchor-text "Sellers" --title "Sell your home with us" \
  --template "Page without Search" \
  --content-file sellers.md

geekcli pages update /resources/sellers/ --meta-description "..."
geekcli pages update 17 --parent null            # move to top level
geekcli pages delete 17 --orphan-children        # children become top-level
```

Lists (`pages list`, `area-pages list`, `agent-pages list`) never include
`content`, `extra_content` or `agents`, which keeps them fast on sites with
tens of thousands of pages. Use `get` for a page's content: it returns the
full record whether the page is given by id, path or slug. The table view
cuts `content` short, so read it with `-o json`.

Templates can add areas (`geekcli templates list` shows them). Set
them with `--area "Name=value"`, repeatable; `Name=null` clears one. The
**Agent Detail Page** template is how a team is built: one page per agent
with `Agent Name`, `Agent Photo` (a file URL from `files upload -q`),
`Cell Phone Number`, `Email`, `Company`, `Address` and `Testimonials`, and
the page's `content` as the bio. An **About Page** then lists every agent
automatically (its `agents` field shows them; `Agent Ordering` takes ids),
so build agents as pages rather than as cards in content.

```bash
geekcli pages create --slug dana-whitfield --anchor-text "Dana Whitfield" \
  --template "Agent Detail Page" --area "Agent Name=Dana Whitfield" \
  --area "Agent Photo=https://u.realgeeks.media/<site>/team/dana.jpg" \
  --area "Email=dana@example.com" --content-file dana.md
geekcli pages update /about/ --template "About Page"
```

**Agent landing pages** (`geekcli agent-pages …`) are content pages
tied to a CRM agent, so leads from the page go to that agent instead of the
site's round robin. Same flags as `pages` plus `--agent-id` from
`geekcli agents` (`null` to untie). They live in their own list; `pages`
does not show them and vice versa, but an About Page lists Agent Detail
pages of both kinds. When the CRM cannot be reached, `agents` returns
`crm_unavailable` and writing `--agent-id` is refused with the same code
until it is back.

`--sidebar` takes a sidebar id or name; `--search` a saved search's short id,
`--search-criteria key=value` builds one (repeat for several); `null`
detaches either. `--parent` accepts an id, `null`, a path or a slug. Pages nest at most 10 levels deep and a page URL is at most 200 characters. Required on create:
`--slug` and `--anchor-text`. Slugs may contain letters, numbers, `-` and
`_`. The resulting URL must be unique and must not be a reserved path
(search, market reports, the blog root); those come back as validation
errors under `fields.__all__`.

**Area pages** are the same with `--area-name` (required) and `--featured
true|false`. Three things differ from what the flags suggest:

- `--area-name` is display text only: it fills the listing header and
  titles. It does not decide which listings the page shows.
- The page's search is what scopes its listings. Give it with
  `--search-criteria key=value` (repeatable) or `--search <saved search id>`.
  Common keys are `city`, `county`, `subdivision` and `zip`, but names and
  values are site specific; confirm them with `search fields` and
  `search choices <field>`, and the result with `search check` and
  `search run` before creating (§11). `area-pages create` fails with exit
  code 2 when no search is given; `--no-search` creates the page anyway,
  with listings that are not scoped to the area.
- There is no draft state. An area page is public as soon as it is
  created, and every update is live immediately.

```bash
geekcli search run subdivision=Downtown --per-page 3      # listings come back?
geekcli area-pages create --slug downtown --area-name Downtown \
  --anchor-text Downtown --search-criteria subdivision=Downtown \
  --content-file downtown.md
```

API reference: [Content pages](https://developers.realgeeks.com/content-api/site-pages/#content-pages), [Agent landing pages](https://developers.realgeeks.com/content-api/site-pages/#agent-landing-pages), [Area pages](https://developers.realgeeks.com/content-api/site-pages/#area-pages), [Page templates](https://developers.realgeeks.com/content-api/site-pages/#page-templates).

## 9. Revisions and undo

Content pages, agent landing pages, market report pages, area pages, blog
posts, footers and the home page keep a revision for every save that
changes a tracked field. It is the same history as the admin's Versions
page, and each revision is attributed to the API key that made it.

| Resource | Tracked fields |
| --- | --- |
| content and agent pages | content, template, anchor text, sidebar, footer, search, search field defaults, search and listing headers, number and placement of listings, template areas (`--area`) |
| area pages | the same as content pages except template and areas, plus `area_name` and `featured` |
| market report pages | content, anchor text, footer, search, search field defaults, number of properties |
| blog posts | title, slug, body, status, publish, page title, meta fields, Facebook image |
| footers | content |
| home page | title, meta description and keywords, content, sidebar, footer, search and listing fields, featured agents, listing display type |

Changes to untracked fields (a page's slug, parent, title, meta fields,
landscape image, banner and search form type; a market report's `--header`
and `--sold-within`; the home page's landscape image; a post's categories)
go into the
site's change log but cannot be undone here; re-read before overwriting
those. Because a post's slug, status and publish date are tracked, reverting
a post can change its URL or publish or unpublish it: preview first.

```bash
geekcli pages revisions /buying/ --limit 5     # newest first: id, when, who, fields
geekcli pages revision /buying/ 318            # what a revert would restore, per field
geekcli pages revert /buying/ 318              # undo 318 and everything after it
geekcli agent-pages revisions /jordan-avery/
geekcli area-pages revert /riverside/ 402
geekcli posts revisions spring-market-update
geekcli posts revision spring-market-update 512
geekcli footers revisions "Default Footer"
geekcli footers revert 1 530
geekcli home-page revisions
geekcli home-page revert 325
geekcli market-reports revisions /riverside-market/
```

A revert is itself a revision, so a mistaken revert is undone by reverting
the revert. The creation revision cannot be reverted (exit 6, `conflict`);
delete the object instead. Revision lists are not paginated; `--limit`
trims them. Sidebars, nav bars, banners, featured pages, categories, the
blog landing page, settings, design and files have change logs but no undo.

API reference: [Revisions and undo](https://developers.realgeeks.com/content-api/site-pages/#revisions-and-undo).

## 10. Home page

```bash
geekcli home-page get
geekcli home-page update --search-header "Find your next home" --content-file home.html
geekcli home-page update --page-heading "Riverside Homes for Sale"   # same field, alias
geekcli home-page update --search-image https://u.realgeeks.media/<site>/images/badge.png
geekcli home-page update --featured-agents /dana-whitfield/,/sam-ortiz/   # the agent cards
geekcli home-page update --featured-agents null                          # no agent cards
```

- `--search-header` (alias `--page-heading`, also on content and area
  pages) is the page's main hero heading, not a small label over the form.
  When it is unset the site shows the `BIG_SEARCH_TITLE` setting ("Real
  Estate Search"), so set it, ideally to the page's target keyword.
- The hero background is the page's `--landscape` (§15), or the sitewide
  `HEADER_IMAGE` setting when the page has none. `--search-image` is not a
  background: it renders as an image, logo-style, inside the hero.
- `--featured-agents` sets the agent cards (photo and bio) on the home
  page, and is always the full list: read it with `home-page get`, then
  send it back with the agent added or left out. It takes Agent Detail
  pages (§8) by id, path or slug; these are content pages (`pages list
  --template "Agent Detail Page"`), not the CRM agents `geekcli agents`
  lists, so an agent without such a page needs one first. The list picks
  who is shown, not the order.
- A card is built from the agent's page: the name is its `--title`, the bio
  its content as plain text, the photo its `Agent Photo` area and the link
  its slug. Fix a placeholder bio or a missing photo with `pages update`,
  not here. Cards show on the anna-modern home page, and in the anna
  sidebar when the `ENABLE_FEATURED_AGENTS_IN_SIDEBAR` setting is on.

API reference: [Home page](https://developers.realgeeks.com/content-api/site-pages/#home-page).

## 11. Property-search criteria and links

Pages and posts often link to property searches (`/search/results/?…`).
The site silently ignores any criterion that is not one of its search
fields, so a wrong key does not fail; it just returns everything.
These commands, which need no API key, make that visible:

```bash
geekcli search fields                 # every field this site accepts, with defaults
geekcli search choices type           # valid values for a field
geekcli search choices city -s beach  # filter the values
geekcli search choices subdivision --all -s oak   # every value the site knows, all cities
geekcli search choices city --all --fuzzy mclean     # the 10 closest values, ranked

# What the site understands; ignored keys are listed and exit code 5 is returned
geekcli search check list_price_min=1000000 type=res type=con city="Key Biscayne"
geekcli search check "/search/results/?list_price_min=1000000&q=Buckhead"   # paste a link
geekcli search check city=McLean --count       # also check values and count the matches
geekcli search check city=McLean --strict      # exit 5 on a value warning

# Run it: matches, total count and the site's own description of the search
geekcli search run list_price_min=1000000 city=Miami --sort highest --per-page 5
geekcli search run city=Miami --strict         # fail instead of warn on ignored keys

# Links to put in content, or a saved search id to reference
geekcli search url list_price_min=1000000 city=Miami
geekcli search url list_price_min=1000000 city=Miami --save
```

`search fields` reads the site's field catalog: every field its search
accepts for the board, including ones the advanced search form leaves out,
such as `subdivision`, `zip` or `school_district`. Each row has the field
name (`attr`), `label`, `widget`, `default`, `choices` and:

- `params`: the keys to search with. A range field is searched by its
  bounds, never the bare name: `list_price` takes `list_price_min` and
  `list_price_max` (`list_price=…` is ignored).
- `dynamic`: `true` when the values come from the listings (city,
  subdivision, zip …) rather than a fixed list.
- `section`: `catalog`; `builtin` for `polygon`, which the CLI adds (see
  Custom areas below).

Sites that do not serve the catalog fall back to the advanced search form
(fewer fields; `section` is `primary` or `secondary`), with a note on
stderr.

`search choices <field>` takes its values from:

- the catalog, for fixed lists (`type`, price presets, `frontage` …);
  `list_price_min` finds `list_price`'s presets;
- the advanced form's list, for `dynamic` fields, which the catalog leaves
  empty: the default county's cities, areas and so on;
- with `--all`, the site's autocomplete index: every value it knows for the
  field, across all counties. Use it for city and subdivision.

A field with no list at all (free text, or a dynamic field the form does
not show) prints nothing and a note on stderr pointing at `--all`.

Criteria are `key=value` items; repeat a key for several values
(`type=res type=con`). A whole query string or URL copied from the site is
accepted as one item. `--save` stores the search on the site and returns
its short id, which is what `/search/results/<id>/`, the map search and a
page's `search_id` refer to.

`search check` also compares each value with the field's choices, for
fields that have a choice list (city, county, subdivision, zip, type …;
price, beds and other numeric fields are skipped). Values are case
sensitive on the site: `city=McLean` is understood but matches nothing when
the site stores `Mclean`. The result carries:

- `values_checked`: the fields whose values were compared.
- `value_warnings`: one entry per bad value, `{field, value, kind,
  suggestions, message}`. `kind` is `case_mismatch` (the suggestion is the
  site's spelling) or `unknown_value` (up to 3 closest values by edit
  distance).
- `count` (with `--count`): how many listings the search matches. 0 is a
  warning.
- `warnings`: every warning as text, also printed to stderr.

Warnings exit 0; `--strict` turns them into exit 5 (`value_mismatch`, or
`no_matches` for a zero count) with the messages in `error.fields`. If the
site's choice lists cannot be read, values go unchecked (reported in
`value_check_error`); `--strict` then exits 1 (`value_check_unavailable`)
rather than pass a check it could not make. The
field's list in the catalog (the form's on sites without one; reported in
`choices_source`) is tried first and the site's autocomplete index (every
county) only when that does not settle it, so a value valid in another
county is not flagged. Yes-no fields (`pool=true`, `1` or `yes`) are not
value-checked. A clean check without `--count` still does not prove
the page will show listings; use `--count` or `search run`.

Every content page, area page and the home page can show a saved search.
Inspect it with `geekcli pages search /buying/` or `geekcli area-pages
search /lakeside/`: the description, live match count and URL. A
description mentioning a field that `search fields` does not list is a
search that filters nothing. Fix it by attaching a new search:

```bash
geekcli area-pages update /lakeside/ --search-criteria subdivision=Lakeside
geekcli pages update /featured/ --search-criteria county="Lake" \
    --search-criteria list_price_min=5000000 --search-criteria type=res
geekcli pages update /featured/ --search a        # an existing search's short id
geekcli home-page update --search null            # detach
```

The site validates criteria and rejects unknown fields with exit code 5
naming them, so a page can no longer end up with a search that matches
everything. Identical criteria reuse the same saved search.

Values are site specific. Before writing a search link, run
`search check --count --strict` on it: that confirms the fields, the values
and that it matches listings. `search choices <field> --all --fuzzy <text>`
finds the right spelling of a value.

### Custom areas (polygon)

A school attendance zone, or a neighbourhood with no MLS subdivision value,
can still be a search: `polygon` takes a boundary drawn as points, the way
the site's map drawing tool saves it. It is a normal criterion, so it works
in `search check`, `search run`, `search url` and `--search-criteria`. The
site's field lists do not include it; `search fields` adds it as a built-in
row.

```
polygon=lat,lng;lat,lng;lat,lng;…;lat,lng
```

- Decimal degrees, **latitude first**, a comma inside a point and `;`
  between points. About 5 decimals (roughly 1 m) is plenty.
- Close the ring: repeat the first point at the end, as the map tool does.
- At least 3 distinct points. Keep it to about 100 or fewer; simplify a
  traced boundary rather than sending every vertex.
- A polygon replaces the site's default location limits (default county or
  cities), and combines with other criteria such as type, price and beds
  (use the names `search fields` lists).

```bash
P="polygon=38.78512,-77.24901;38.79870,-77.21544;38.76632,-77.19902;38.75421,-77.23718;38.78512,-77.24901"
geekcli search check "$P" type=res           # polygon and type both understood?
geekcli search run "$P" type=res --per-page 5  # listings come back, total looks right?
geekcli area-pages create --slug my-zone --area-name "My Zone" --anchor-text "My Zone" \
    --search-criteria "$P" --search-criteria type=res
```

Quote the value: `;` ends a command in the shell.

The CLI checks the value before sending it. A point that is not two numbers,
a latitude outside -90..90 or a longitude outside -180..180, or fewer than 3
distinct points is a usage error (exit code 2) and nothing is sent. It warns
on stderr, and still sends the value unchanged, when the ring is not closed,
when a latitude is below -60 (no listings are there; it almost always means
the points are lng,lat), or when there are more than 100 points.

The boundary is yours to get right. Take it from an authoritative source
(the school district's or city's official boundary map, a published GIS
layer), not from memory, and check the `search run` total and a few result
addresses against that map before attaching it to a page.

API reference: [Searches](https://developers.realgeeks.com/content-api/searches/).

## 12. Navigation bars

A site has one bar per position (`top_primary`, `bottom_primary`,
`top_secondary`, `bottom_secondary`, `seller_leads`). Bars are fixed; you
edit their links. A bar can be named by id or position.

```bash
geekcli nav list
geekcli nav get top_primary
geekcli nav add top_primary --url /luxury/ --text "Luxury"        # append
geekcli nav add top_primary --url /luxury/ --text "Luxury" --at 1 # insert
geekcli nav add top_primary --contact                             # the site's contact link
geekcli nav update top_primary 16 --text "Meet Jordan" --url /jordan-avery/
geekcli nav move top_primary 16 --to 0
geekcli nav remove top_primary 27
geekcli nav set top_primary --data @links.json                    # replace all, in order
geekcli nav clear seller_leads
```

A link is `{"type": "custom"|"contact", "url", "anchor_text", "nofollow"}`.
`update`, `move` and `remove` take a link by id, or by its anchor text or
URL as a convenience. Ids are stable: `add --at` and `move` send a
position, and `set` keeps the rows whose ids you include, so
`--data '[{"id": 12}, {"id": 10}]'` is a pure reorder.

API reference: [Navigation bars](https://developers.realgeeks.com/content-api/navigation-sidebars-footers/#navigation-bars).

## 13. Sidebars

A sidebar is a named list of items. An item is either sanitized HTML or a
links block with an optional header and one or two columns. Refer to a
sidebar by id or name. The two built-in sidebars (`special: true`) cannot
be renamed or deleted. Detail output includes `used_by`, the slugs of up
to 500 pages using the sidebar, and `used_by_count`, the real total.

```bash
geekcli sidebars list
geekcli sidebars get "Blog Sidebar"
geekcli sidebars create --name "Luxury Sidebar"
geekcli sidebars add-html "Luxury Sidebar" --content-file card.md         # Markdown converts
geekcli sidebars add-links "Luxury Sidebar" --header "Featured Areas" --header-url /areas/ \
    --link "Riverside=/riverside/" --link "Fairview=/fairview/" --columns 2
geekcli sidebars update-item 5 51 --link "Riverside=/riverside/" --link "Millbrook=/millbrook/"
geekcli sidebars update-item 5 50 --content "<p>New card</p>"
geekcli sidebars move-item 5 51 --to 0
geekcli sidebars remove-item 5 50
geekcli sidebars set-items 5 --data @items.json                         # replace all
geekcli sidebars delete 5 --force                                       # even if pages use it
```

Item JSON shapes, for `--data`:

```json
{"type": "html", "html": "<p>Sanitized HTML</p>"}
{"type": "links", "header": {"text": "Featured Areas", "url": "/areas/"},
 "links": [{"url": "/riverside/", "anchor": "Riverside"}, {"anchor": "plain text"}], "columns": 2}
```

Attach a sidebar to a page with `geekcli pages update <ref> --sidebar
<id or name>` (`--sidebar null` detaches). `set-items` keeps items whose
ids you include; ids are stable across `move-item` and `add-* --at`.

API reference: [Sidebars](https://developers.realgeeks.com/content-api/navigation-sidebars-footers/#sidebars).

## 14. Footers

Footers are shared HTML blocks shown in the footer's third column. Footer 1
is the site default and cannot be deleted. Refer to one by id or name.

```bash
geekcli footers list
geekcli footers get "Default Footer"
geekcli footers update 1 --content-file footer.html
geekcli footers create --content-file alt-footer.md          # Markdown converts
geekcli pages update /buying/ --footer 2                     # attach; `--footer null` for the default
geekcli footers delete 2 --force
geekcli footers revisions 1 --limit 5                          # and `revision`, `revert` (§9)
```

An agent block is the usual footer: name, address, phone, licence and
social icons (HTML rules in §21):

```html
<p><strong>Jordan Avery</strong><br>Example Realty<br>
123 Main St, Springfield, ST 00000<br>
<a href="tel:+15555550123">(555) 555-0123</a><br>License #0000000</p>
<p><a href="https://facebook.com/example"><em class="fa-brands fa-facebook"></em></a>
<a href="https://instagram.com/example"><em class="fa-brands fa-instagram"></em></a>
<a href="https://linkedin.com/in/example"><em class="fa-brands fa-linkedin"></em></a></p>
```

`footers get` shows `used_by`, the slugs of up to 500 pages that use the
footer, and `used_by_count`, the real total.

API reference: [Footers](https://developers.realgeeks.com/content-api/navigation-sidebars-footers/#footers).

## 15. Featured Pages tiles and landscape images

On anna-modern the home page can show a **Featured Pages** block: a titled
group of up to twelve tiles, each with a background image, the area name,
button text and a page link. This is the native version of an area grid.

```bash
geekcli featured create --title "Where We Live" --blurb "The neighborhoods we know best"
geekcli featured add-tile "Where We Live" --title Riverside --link /riverside/ \
    --cta "View Homes" --image https://u.realgeeks.media/<site>/areas/riverside.jpg
geekcli featured get "Where We Live"
geekcli featured update-tile 3 9 --cta "Explore Riverside"
geekcli featured set-tiles 3 --data @tiles.json         # entries with an id are kept
geekcli home-page update --tile-group "Where We Live"
geekcli home-page update --tile-group null              # detach
```

Every page (content, area, agent and home) can also carry its own
**landscape image or video**, the big picture at the top:

```bash
geekcli area-pages update /riverside/ --landscape https://u.realgeeks.media/<site>/areas/riverside.jpg --landscape-alt "Riverside Harbor"
geekcli pages update /buying/ --landscape none          # hide the section on this page
geekcli pages update /buying/ --landscape null          # back to the site's header image
```

The content type comes from the file extension (an `.mp4` becomes a video
override); pass `--landscape-content-type` only when the URL does not say.

Things learned on a real site:

- Group titles are unique per site: a second `create` with the same title
  is a 422. `featured get <title>` finds the existing one, and `featured
  get` shows `used_by_home_page`.
- A tile link is a site path, an area page (`/riverside/`) or a search URL
  (`/search/results/?city=Riverside&list_price_min=1000000`), or an
  http(s)/mailto/tel URL. The image URL may not contain spaces, quotes or
  parentheses. Keep tile titles to a word or two; "North Riverside Heights"
  truncates on phones.
- Deleting the group the home page shows is a 409. Detach it first
  (`home-page update --tile-group null`) or pass `--force`.
- Once the tiles are attached, take any hand-built area grid out of the
  home content and move the photo credits into the footer so they survive.
- The tile block sits between the home content and the listings strip; the
  home content above it should be short.

API reference: [Featured pages](https://developers.realgeeks.com/content-api/navigation-sidebars-footers/#featured-pages).

## 16. Search form and listings options on pages

Content pages, area pages and the home page all take:

```bash
geekcli pages update /buying/ --search-form-type typeahead
geekcli pages update /buying/ --search-field-defaults-criteria city=Riverside   # pre-fills the form
geekcli pages update /buying/ --search-field-defaults null
geekcli pages update /buying/ --listing-header "Homes in Riverside" --number-of-properties 24
```

`--number-of-properties` must be one of 0, 3, 5, 6, 10, 12, 15, 20, 24, 25,
30, 40, 50, 60, 75, 100 or 120. The home page adds
`--property-display-type carousel|grid|null`, `--search-form-tabs true|false`
and `--search-image <file URL>|null` (an image in the hero, not its
background; §10). Some of these render only on certain
designs (display type and tabs are anna-modern features); the API accepts
them everywhere, as the admin does.

## 17. Site settings

The settings a site owner can change under Website Settings, with typed
values. Names are upper-case; the CLI upper-cases what you type.

```bash
geekcli settings groups
geekcli settings list --group common
geekcli settings list --search email --overridden
geekcli settings get EMAIL_FROM_NAME                 # description, type, choices, depends
geekcli settings set EMAIL_FROM_NAME="Jordan Avery" LEAD_CAPTURE_ON_PROPERTY=2
geekcli settings set GA4_MEASUREMENT_ID="G-AAA,G-BBB" # lists take comma-separated values
geekcli settings set --data '{"SOME_OBJECT_SETTING": {"k": 1}}'
geekcli settings clear GOOGLE_ANALYTICS_KEY           # back to the inherited default
geekcli settings set HEADER_LOGO="$(geekcli files upload logo.png --to images -q)"
```

`HEADER_LOGO` is the header logo. Set it, like every file setting, to a
URL from `files upload -q`; an external URL may not render.

`set` reads each setting's definition first and converts your text to its
type (`boolean`, `integer`, `integer_list`, `list`, `choice`, `object`,
`yaml`, or a string kind). A value outside a setting's `choices` is
rejected locally with exit code 2 before anything is sent. `depends`
names other settings whose values must allow the change; the API rejects
a violating change with exit code 5 and names the field. Changes take
effect on the live site within a few seconds and are recorded in the
site's settings history.

API reference: [Site settings](https://developers.realgeeks.com/content-api/design-settings-files/#site-settings).

## 18. Design: template and colours

A site's look is a **template** (design family: `miranda`, `miranda-thin`,
`molly`, `anna`, `anna-modern`) and a **colour scheme**: a named variation
of that template plus variable values. CSS is generated from these, so a
change is live immediately.

```bash
geekcli design get                                   # template, scheme, variables, consistent?
geekcli design templates                             # catalogue with variation names
geekcli design variation anna-modern coastal         # that variation's variables
geekcli design preview --template anna-modern --snapshot preview.png   # nothing saved
geekcli design set --template anna-modern --variation coastal
geekcli design set --var palette-brand-color=#0066A7 --snapshot after.png
```

Changing the template alone applies that template's default variation,
since variables belong to a template family. Variable values are plain CSS
(up to 512 characters, none of `; { } /* */ \ @ $ ! < >`); on the
SCSS-compiled templates (miranda, miranda-thin, molly) they are compiled
before saving, so a value that would break the stylesheet is a 422.

`preview` validates the same flags and returns a link that renders the
change without saving. Unsaved variables are applied only for a browser
signed in to the site's admin, so open the link there. `--snapshot FILE`
on `preview` is allowed for a template-only change (the template switch
renders for everyone) and refused when `--variation` or `--var` is present;
for those, `design set --snapshot after.png` and revert with another `set`
if it disappoints. The link is re-homed onto the origin the CLI is talking
to, so it works against a local or staging site as well as the live domain. Content features differ by design (tiles and featured agents
on anna-modern, a right-hand sidebar on molly), so after a real change
snapshot the home page, a content page and a post.

API reference: [Design](https://developers.realgeeks.com/content-api/design-settings-files/#design).

## 19. Files

The site's uploaded files (the admin's Manage Files page), served from
`https://u.realgeeks.media/`. Paths are relative to the site's folder.

```bash
geekcli files list                       # root, folders first
geekcli files list images --all          # every page of a folder
geekcli files list --search logo         # by name, across folders
geekcli files get images/logo.png
geekcli files url images/logo.png        # just the public URL
geekcli files upload hero.jpg team.jpg --to images
geekcli files upload hero.jpg --to images --name hero-2026.jpg --overwrite
geekcli files upload --from-url https://u.realgeeks.media/<site>/images/logo.png --to images/2026
geekcli files mkdir images/2026
geekcli files move images/hero.jpg images/2026/hero.jpg
geekcli files delete images/2026         # a folder goes with everything in it
```

Uploads are limited to 8,000,000 bytes and to jpg/jpeg, png, gif, ico,
mp4, pdf, txt and css. HTML, SVG, XML and scripts are refused because files
are served inline from a domain shared by every site; use the admin for
those. The stored content type follows the extension (of `--name` when
given). The CLI checks every file's extension and size before sending
any of them, so one bad file in a multi-file upload refuses the whole run
with exit code 2 and a message naming each problem; nothing is half
uploaded. Whitespace and
slashes in names become `_`, and names ending in `_thumbnail`, `_small`,
`_medium`, `_big`, `_agent` or `_fb_thumb` are reserved. An existing file
name is a conflict (exit 6) unless `--overwrite`; a folder of that name is
always a conflict. The destination folder must already exist.

Uploading with `--overwrite` keeps the same URL, and the CDN may keep
serving the old file for a long time; to replace a live image, upload it
under a new name and repoint whatever uses it.

`--from-url` has the site fetch the file itself instead of uploading a local
one. It takes https URLs on the hosts the API allows, such as
`u.realgeeks.media` to copy a file that is already uploaded; a host that
is not allowed is a validation error (exit 5) whose message lists the ones
that are. The stored name defaults to the URL's last segment; when the URL
does not end in a file name, pass `--name` with the right extension.

Deleting a folder removes its files from storage first; if storage refuses
some of them the call fails (exit 1) and those files stay listed, so run the
same delete again.

`-q` on `upload`, `get` and `url` prints only the public URL, which is
what to put in `--facebook-image`, in `<img src>` inside content, and in
file-typed settings such as `HEADER_LOGO` and `FAVICON`. Listings include
`dimensions` for images and a `thumbnail_url`.

API reference: [Files](https://developers.realgeeks.com/content-api/design-settings-files/#files).

## 20. Snapshots (visual check)

`geekcli snapshot` renders a page with a Chrome or Chromium already on
the machine and saves a PNG you can open or, as an agent, read as an
image. Nothing is bundled; the command finds Google Chrome, Chromium,
Brave, Edge or Playwright's Chromium, or use `--browser` /
`GEEKCLI_BROWSER`. Chrome is driven over DevTools, so `--full` captures
the whole document while the viewport stays a normal size (a hero sized
to the viewport stays one screen tall), and the page is scrolled once
before capture so lazy-loaded images render.

Each run starts a fresh, sandboxed browser profile with no cookies or
logins, and certificate errors fail the load. While it runs, Chrome's
DevTools port listens on localhost, so on a machine shared with untrusted
local users prefer running `snapshot` and `inspect` in your own container
or VM.

```bash
geekcli snapshot                                 # first screen of the home page → snapshot-home.png
geekcli snapshot / --full                        # the whole page
geekcli snapshot /blog/my-post/ --full --out post.png
geekcli snapshot / --mobile --full               # phone emulation: 390x844, touch, 2x
geekcli snapshot / --width 1024 --height 768 --scale 2
geekcli snapshot https://www.example.com/buying/ # any URL
geekcli snapshot / --selector ".featured-listing" --out listing.png
geekcli snapshot / --selector ".card" --nth 2 --out third-card.png
```

Output is `{"path", "url", "width", "height", "full", "bytes", "browser"}`;
`-q` prints just the file path. Only public pages render: the browser
runs without a login, so a draft or scheduled post is a 404 (§5); snapshot
it after publishing. After a content
change, snapshot the page and look at it before moving on; unstyled
markup (a class the theme does not know), overflow on a phone, or a
character the database could not store are invisible in the API response
and obvious in the picture.

### Rendered DOM checks

`geekcli inspect` opens the same public page in Chrome, waits and walks it
as `snapshot` does, then makes one read-only DOM query. It is for checks the
API cannot make: what the theme rendered, which breakpoint is active and
whether old content is still visible.

```bash
geekcli inspect / --text h1
geekcli inspect / --html "main .hero"
geekcli inspect / --attr "a.cta" href
geekcli inspect / --count ".featured-listing"
geekcli inspect / --exists "#lead-form"
geekcli inspect / --visible ".mobile-menu"
geekcli inspect / --js "document.title"
geekcli inspect / --assert "document.querySelectorAll('h1').length === 1"
geekcli inspect / --mobile --assert "document.querySelector('.mobile-menu')?.checkVisibility()"
```

Every successful query prints JSON with the URL, viewport and value.
`--assert` exits 1 if its expression is false, which makes it suitable for
agent and CI checks. `--js` is a browser-side escape hatch; prefer selector
queries and assertions when they express the check.

## 21. Writing content that renders

Everything an agent writes into `content`, `body`, a sidebar item or a
footer goes through the same sanitizer and then a theme it cannot see.
These rules come from watching real pages break.

**The sanitizer keeps** `p`, `br`, `hr`, `h1`–`h6`, lists, `a`, `img`,
`strong`/`em`/`u`/`s`, `sub`/`sup`, `blockquote`, `pre`/`code`, tables,
`div`, `span`, `figure`/`figcaption`, and `iframe` only for YouTube or
Vimeo over https. Attributes: `class`, `id`, `style`, `title`, `href`,
`target`, `rel`, `src`, `alt`, `width`, `height`, `loading`, table
spans. **It drops** `script`, `style` blocks, `button`, forms and inputs,
`video`/`audio`, `svg`, event handlers, `javascript:` links and any other
attribute (a `number-of-tiles="three"` hook vanishes silently). Inline
`style` keeps only: text-align, color, background-color, font-weight,
font-style, font-size, text-decoration, width, height, max-width, margin*,
padding*, float, display, border*, border-collapse, list-style-type,
vertical-align, line-height. `min-width`, `letter-spacing`, `flex`, `grid`
and `url(...)` are removed. Trailing semicolons and entities get
normalised; that is not a change.

**Characters.** The content columns are Windows-1252. Accented Latin
letters, curly quotes, en and em dashes, `«»`, `·`, `©` and `€` are fine;
arrows, CJK text and emoji are not, and any field containing one is
rejected with a 422 that names the characters (nothing is silently saved
as `?`). Entities are decoded before the check, so `&#8594;` is rejected
too, as are control characters. Use what the charset has: `&raquo;` (»)
for an arrow, `&middot;` (·) for a separator, `--` for a dash.

**Contact links.** Link to the site's contact form as
`<a class="popup" href="/member/contact/">Contact Jordan</a>`. The `popup`
class opens the form as an overlay on the Miranda, Miranda-Thin and Molly
designs instead of leaving the page; where a design has no overlay the
link still works as a normal page, so use the class everywhere
(support: Add a Contact Us link or button).

**Buttons.** `<button>` is stripped. A button is an `<a>` with
`display: inline-block; padding: 12px 24px; background-color: #111;
color: #fff; text-decoration: none`.

**Links.** Leave link colour to the theme, or they look like plain text.
Only override colour on things that are deliberately not links.

**Layout.** Themes have no grid helpers you can rely on and no
`@media` rules apply to your inline styles. What survives: a centred
container with `display: inline-block; width: 210px; margin: 6px;
vertical-align: top` cards, which sit three across on desktop and stack on
a phone. Tables do not wrap and overflow narrow screens. Give a row of
cards the same `height` when their text lengths differ.

**Classes.** A class from another site (`icon-tiles`, `two_column`) does
nothing unless this theme's stylesheet knows it. Assume it does not;
style inline, then snapshot.

**Icons.** Font Awesome 6 is loaded on the themes seen so far: `<em
class="fa-solid fa-house"></em>` works; nothing else needs to be loaded.
Verify with a snapshot, since this is a theme choice.

**Images in content.** MLS listing photos may only appear with their
listing, never as decoration. For area or lifestyle images use public
domain or CC0 photos, or CC BY / CC BY-SA with a visible credit line
(Wikimedia Commons' API exposes the licence per file). Crop to one aspect
ratio before uploading (a 3:2 600x400 JPEG suits a card; trim the sides of
a wide photo and the top/bottom of a tall one, never pad), upload with
`files upload -q`, and give a replaced image a new file name since the CDN
caches by path. Headshots: flatten to white and resize to the same width.

**Read-back rule.** Do not resend HTML you read unless you changed it;
admin-authored markup the sanitizer would strip is left alone when the
field is omitted. Appending to existing content re-sends the whole
field, so anything the admin authored with `<button>`, `<script>` or an
iframe is stripped in the same save and can leave a dangling sentence
("… today to find out how we can help!" minus its button). After such a
save, read the content back and look for orphaned fragments.

**Themes differ.** On molly the sidebar is a right column; on
anna-modern it stacks under the content as full-width sections and some
iframes render as blank space. After a template change, snapshot the
home page, a content page and a post.

## 22. Rebranding a site

Where a site's identity lives, in the order to change it. Each line is one
CLI call; snapshot when done.

1. **Settings**: `HEADER_LOGO` (on designs whose header sits over the hero
   photo, use a white wordmark with a soft shadow and no small tagline; keep
   the full dark logo for the footer and `DETAIL_PAGE_PHOTO_OR_LOGO`),
   `FAVICON`, `HEADER_IMAGE`,
   `DETAIL_PAGE_PHOTO_OR_LOGO` (file URLs; upload with `files upload -q`
   first), `HEADER_LOGO_ALT`, `EMAIL_FROM_NAME`, `FORWARD_EMAIL_TO`,
   `HEADER_PHONE`, `DETAIL_PHONE`, `BIG_SEARCH_TITLE`. A batch with one
   invalid entry is rejected whole; template-only settings (`TAGLINE`,
   `HEADER_IMAGE_ALT`, `MOBILE_HEADER_LOGO`) fail on other templates.
   A file value with a server path in front of `https://…` is corrupt; the bare URL
   fixes it.
2. **Home page**: `home-page update` for title, meta, search header and
   subheader, content, `--search-criteria` for the listings strip,
   `--search-field-defaults-criteria` for the form's defaults and
   `--featured-agents` for the agent cards.
3. **Navigation**: `nav get top_primary` and `bottom_primary`; update the
   agent link (`nav update top_primary "Meet Riley" --text … --url …`).
4. **Sidebars**: `sidebars get "Default Sidebar"`. The agent card is
   usually item 0 (photo, phone, email, contact button); a social-links
   block near the end links to the old agent's accounts. Widgets with a
   `data-domain` are system items; leave them and report.
5. **Footer**: `footers get "Default Footer"`, then `footers update 1`.
6. **Pages**: `pages list --search <old name>`; the "Meet …" page and the
   About page. Create the new agent's page and point nav at it.
7. **Blog**: `posts list --all --no-body`; rewrite or unpublish posts that
   name the old agent or market, and `blog update --title … --content …`
   for the blog's own heading and meta.
8. **Area pages**: `area-pages search <ref>` for each; a description
   naming a field `search fields` does not list matches everything. Fix
   with `--search-criteria subdivision=<name>` (repeat for several) after
   `search run` shows listings; find the real names with
   `search choices subdivision --all -s <area>`. Then give every page a
   title, meta description (its own first sentences work) and keywords,
   and a call to action naming the agent.
9. **Contact links**: every `href="/member/contact/"` should carry
   `class="popup"`; grep the content of pages, area pages, posts,
   sidebars and footers for ones without it.
10. **Snapshot** the home page (`--full` and `--mobile`), the agent page
   and a post. Look for the old name, old phone, broken images, overflow
   and `?` characters.

The template and colour scheme are `design set`; preview first with
`design preview --snapshot`. Not reachable through the API: the MLS/board
and widget domains. Report those rather than working around them.

## 23. Design guidance from Real Geeks support

Rules the support articles state that affect what the CLI should send.

- **Header logo**: 400 x 86 pixels, PNG preferred, horizontal or text-based;
  tall or detailed logos crop or blur. Upload at 2x (800 x 172) for sharp
  screens; keep the same ratio. Square logos belong in the sidebar or footer.
- **Navigation**: 5 to 6 links in the top bar, 6 to 8 in the bottom bar;
  short anchor text; slug-only URLs for your own pages (`/buying/`); external
  links discouraged, and `nofollow` when you must. The `contact` link type
  opens a contact menu only on Miranda, Miranda-Thin and Molly.
- **Home page**: it is a directory, not a brochure. A short introduction, the
  areas served, links to key resources, and the search. The landscape image
  or video above the fold is the per-page "Override landscape image"
  (`HEADER_IMAGE` sitewide); use local photography.
- **Featured Areas (anna-modern)**: the home page has a native image-tile
  section for featured areas: up to 12 tiles with a background image, the
  area name, button text and a page link, edited under Home Page in the
  admin. The sidebar's Featured Areas list (up to 30 MLS locations) sits at
  the top of the sidebar. Use `geekcli featured` for the tiles rather than
  a hand-built card grid.
- **Area pages**: educate rather than sell. Include what living there is
  like, market insight, local highlights (schools, dining, amenities), the
  matching property search, a downloadable guide if you have one, and a
  clear call to action (listing alerts, home valuation). Niche pages
  (a neighbourhood or a condo building) tend to earn more than a whole city.
  Each area page can carry its own landscape image.
- **Sidebars and footers**: a sidebar holds links and resources next to the
  content (a right column on older designs, stacked below on anna-modern);
  the footer is for contact details, disclaimers and brokerage compliance.
  Both can be page-specific.
- **Blog landing page**: title, meta and an introduction at the top; the
  read-more break after the first paragraph or two controls how much of a
  post the feed shows.
- **PDF guides**: upload to the file browser (`files upload`), then link
  from a page as a text link or an image; they generate leads and help SEO.

## 24. Real Geeks support articles

The support site (support.realgeeks.com) documents the same features from
the admin's side. When a command's behaviour seems odd, the article for
that feature usually explains the design or the rule behind it.

| Topic | Article |
| --- | --- |
| Contact links and buttons | https://support.realgeeks.com/add-a-contact-us-link-or-button-to-your-website |
| Blog posts | https://support.realgeeks.com/blog-posts |
| Blog, complete guide | https://support.realgeeks.com/complete-guide-to-building-a-blog-on-your-real-geeks-website |
| Blog categories | https://support.realgeeks.com/how-to-use-blog-categories-on-your-real-geeks-website |
| Blog landing page | https://support.realgeeks.com/how-to-edit-your-blog-homepage-on-your-real-geeks-website |
| Posts that rank on Google | https://support.realgeeks.com/how-to-create-blog-posts-that-rank-on-google-using-chatgpt |
| Home page | https://support.realgeeks.com/home-page |
| Agent photo on the home page | https://support.realgeeks.com/adding-agent-photo-to-the-home-page |
| Assigned agent's photo | https://support.realgeeks.com/using-the-assigned-agents-photo-on-the-website |
| Area pages | https://support.realgeeks.com/area-page |
| Dynamic area pages | https://support.realgeeks.com/dynamic-area-pages |
| Featured areas | https://support.realgeeks.com/featured-areas |
| Market report pages | https://support.realgeeks.com/market-report-pages |
| Market reports | https://support.realgeeks.com/market-reports |
| Saved searches | https://support.realgeeks.com/saved-searches |
| Sidebars | https://support.realgeeks.com/sidebars |
| Footers | https://support.realgeeks.com/footers |
| Navigation bars | https://support.realgeeks.com/navigation-bars |
| Uploading a logo | https://support.realgeeks.com/uploading-your-logo |
| Embedding PDFs | https://support.realgeeks.com/embedding-pdf-files |
| Website designs (Anna) | https://support.realgeeks.com/migrating-to-our-new-website-design-anna |
| Design services | https://support.realgeeks.com/website-design-services |
| Google Search Console and SEO | https://support.realgeeks.com/google-search-console-seo |

## 25. Anything else

```bash
geekcli api GET blog/posts/ -p page_size=5 -p ordering=-updated_at
geekcli api PATCH blog/posts/42/ -d '{"meta_keywords": "homes, spring"}'
geekcli api DELETE blog/categories/3/ -p force=true
```

`geekcli api` sends the request as-is and prints the JSON response, so any
endpoint the typed commands do not cover is still reachable.

API reference: [the full Content API](https://developers.realgeeks.com/content-api/), [changelog](https://developers.realgeeks.com/content-api/changelog/).

## 26. Recommended agent workflow

1. `geekcli me --json` to confirm the site and scopes before writing.
2. `geekcli search fields` before writing any search link or page
   search; verify each with `search check --count --strict` (keys, values
   and a non-zero match count). An area page's listings come from its
   `--search-criteria`, not its `--area-name`, and it is public the moment
   it is created (§8).
3. Create posts as drafts; publish with `geekcli posts publish` once
   reviewed. A post created with `--status published` can read
   `scheduled` for the first second; it is published.
4. Use `-q` to capture the id or URL of what you created.
5. On exit code 5, read `error.fields` and fix the named fields. On a
   settings batch, fix or drop the named setting and resend the rest.
6. Prefer `update` with specific flags over `--replace`.
7. Before a large rewrite of a page, area page, post or footer, note
   `<command> revisions <ref> --limit 1`; a bad result is one `revert` away.
8. After changing anything visible, `geekcli snapshot <path> --full`
   and look at the image. Check `--mobile` too when layout changed.
9. `geekcli guide <topic>` pulls one section of this guide (for
   example `guide html`, `guide rebrand`, `guide search`); `guide --list`
   shows the topics.
10. When the API can do something the CLI has no flag for, check the
    [API changelog](https://developers.realgeeks.com/content-api/changelog/)
    and reach it with `geekcli api` (§25).

## 27. Updating geekcli

geekcli never updates itself in the background: a script keeps the version
it was written against until something runs `geekcli update` (also spelled
`geekcli upgrade`, and unrelated to `posts update` and the other resource
updates).

```bash
geekcli update --check        # is there a newer release? installs nothing
geekcli update                # install the latest release over this binary
geekcli update --tag v0.7.0   # install one release; an older tag downgrades
```

Every form prints
`{"current_version", "release_version", "update_available", "updated", "target"}`,
plus `path` after an install. `update_available` compares the release with
the binary that ran the command, so it is still `true` in the result of the
run that installed it; `updated` says whether the binary was replaced.
`--check` exits 0 either way: branch on `update_available`.

The archive is the one `install.sh` and `install.ps1` use, from the GitHub
release for this platform (Linux always gets the static build). It is
verified against the `.sha256` published beside it and against the digest
GitHub records for the asset, and the binary is replaced only after both
pass. A failed update leaves the installed version in place. These requests
go to GitHub and never carry the site's API key.

On a terminal, any command says once a day (on stderr) when a newer release
exists. Scripts, agents and CI never get that notice or the lookup behind
it: it needs a table-format run with stderr on a terminal and no `CI`
variable, and `GEEKCLI_NO_UPDATE_CHECK=1` turns it off everywhere. The time
of the last look is kept in `update-check` beside `config.toml`.

What a script or agent gets instead is the API's word on old versions.
Every request names the CLI's version (`User-Agent: geekcli/<version>`), so
the API can tell an old one apart. If it answers with a warning about the
version, that arrives like any other API warning: a `warning:` line on
stderr and an entry in `warnings` (§2). If it refuses the version
(`client_too_old`, or HTTP 426), the command fails with **exit 9**: run
`geekcli update` and repeat the command.

| Problem | What to do |
| ------- | ---------- |
| `client_too_old` (exit 9) | The site's API no longer accepts this version. `geekcli update`, then run the command again. |
| `cannot write to <dir>` (exit 1) | The binary is in a directory you cannot write, such as a root-owned `/usr/local/bin`. Re-run with `sudo`, or reinstall with `GEEKCLI_INSTALL_DIR` set to a directory you own. |
| `rate_limited` (exit 7) | GitHub limits anonymous lookups per address. Set `GH_TOKEN` or `GITHUB_TOKEN`, or retry later. |
| `not_found` (exit 4) | No release has that `--tag`. |
| `network` (exit 8) | GitHub is unreachable. The update trusts the operating system's certificate store, so a corporate proxy's certificate has to be installed there. |

`GEEKCLI_REPO=owner/name` updates from a fork, as it does for the install
scripts. A binary built from source (`cargo install`) is replaced by the
release build.

## 28. Banners

A banner is a one-line message with a button, shown across the top of the
pages it is attached to: an open house, a new-listings alert, a seasonal
offer. It is not the header image (that is `--landscape`, or the
`HEADER_IMAGE` setting).

```bash
geekcli banners create --name "Open house" --message "Open house this Saturday, 1 to 4" \
    --call-to-action "See details" --url /open-house/
geekcli banners list
geekcli pages update /buying/ --banner "Open house"       # show it on a page
geekcli home-page update --banner "Open house"
geekcli area-pages update /riverside/ --banner null       # take it off a page
geekcli banners update "Open house" --message "Open house this Sunday, 1 to 4"
geekcli banners get "Open house"                          # used_by: the pages showing it
geekcli banners delete "Open house" --force
```

- A banner shows nowhere until a page uses it, and there is no site-wide
  banner: set `--banner` on each content page, agent page, area page or the
  home page that should show it. Market report pages have no banner.
- Editing a banner changes it on every page that shows it. `--message` is
  150 characters at most and `--call-to-action` (alias `--cta`) 50; keep
  both short, the strip is one line on a phone.
- `--url` is a site path or an http(s), mailto or tel URL.
- Deleting a banner pages still show is a 409; `--force` takes it off them.
- Snapshot a page after attaching one (`geekcli snapshot /buying/`).

API reference: [Banners](https://developers.realgeeks.com/content-api/navigation-sidebars-footers/#banners).

## 29. Market report pages

A market report page shows market statistics and tables of active, pending
and sold listings for one area, all generated from a search. They are
separate from `pages`: neither list shows the other.

```bash
geekcli search check --count --strict city=Riverside
geekcli market-reports create --slug riverside-market --anchor-text "Riverside Market Report" \
    --search-criteria city=Riverside --header "Riverside this month" \
    --title "Riverside Real Estate Market Report" --content-file intro.html
geekcli market-reports list
geekcli market-reports update /riverside-market/ --sold-within 12 --number-of-properties 10
geekcli market-reports revisions /riverside-market/ --limit 1
geekcli market-reports delete /riverside-market/
```

- A search is required and cannot be removed: without one there is no
  report. Build it with `--search-criteria` and check the count first; a
  search that matches nothing makes an empty report.
- `--sold-within` is how many months of sold listings the Sold table
  covers: 1 to 6, 12 or 18 (default 6). `--number-of-properties` is the
  rows in each table, 1 to 50 (default 6).
- `--header` is a line of text at the top of the report, above the sign-up
  call to action. The page's own text is `--content`.
- There are no template, sidebar, banner or landscape options. The page is
  live as soon as it is created; link it from the navigation or an area
  page so visitors find it.

API reference: [Market report pages](https://developers.realgeeks.com/content-api/site-pages/#market-report-pages).
