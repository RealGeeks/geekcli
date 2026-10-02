# Changelog

## 0.6.0 (2026-10-02)

First public release of `geekcli`, the Real Geeks command line, previously
used internally as `realgeeks`.

### Features

* Blog posts and categories, content, area and agent pages, the home page,
  navigation, sidebars, footers, featured pages, settings, design and files
  over the Real Geeks Content API.
* Revisions with preview and revert for pages, agent pages, area pages, blog
  posts, footers and the home page.
* `files upload --from-url` has the site fetch a file itself.
* Browser-approval login (`auth login`), `snapshot` and `inspect` for checking
  rendered pages, property-search helpers, and a built-in guide for scripts
  and AI agents (`geekcli guide`).
* Static Linux binaries that run on any distribution, plus macOS and Windows
  builds, each with a checksum and a signed build-provenance attestation.

### Security

* The API key is only ever sent to the site's own origin over https:
  redirects to another origin are not followed, full URLs off the site are
  refused, and plain http is allowed only for local dev hosts.
* The config file is written atomically with owner-only permissions.
* Hardened browser login, path handling, terminal output and the install
  script.

### Upgrading from `realgeeks`

`geekcli` moves an existing `~/.config/realgeeks/` to `~/.config/geekcli/` the
first time it runs, and reads `GEEKCLI_*` environment variables in place of
`REALGEEKS_*`.
