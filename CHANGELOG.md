# Changelog

## [0.7.1](https://github.com/RealGeeks/geekcli/compare/v0.7.0...v0.7.1) (2026-10-02)


### Bug Fixes

* exit quietly when the output pipe closes early ([#57](https://github.com/RealGeeks/geekcli/issues/57)) ([d76c237](https://github.com/RealGeeks/geekcli/commit/d76c2371531141913217945576393743c0f2512e))

## [0.7.0](https://github.com/RealGeeks/geekcli/compare/v0.6.0...v0.7.0) (2026-10-02)


### Features

* accept --content/--body/--html spellings on every command that takes a body ([#52](https://github.com/RealGeeks/geekcli/issues/52)) ([ec4d0b9](https://github.com/RealGeeks/geekcli/commit/ec4d0b9238d514b2f0a1b2ac45c499c194376d96))
* **area-pages:** require a search on create and explain area-name ([#45](https://github.com/RealGeeks/geekcli/issues/45)) ([1855927](https://github.com/RealGeeks/geekcli/commit/1855927e9919b78095e678d70c1cf17860608102)), closes [#6](https://github.com/RealGeeks/geekcli/issues/6)
* group flags in --help by purpose ([#53](https://github.com/RealGeeks/geekcli/issues/53)) ([d1519b4](https://github.com/RealGeeks/geekcli/commit/d1519b406701467d16d64b8ade9cdd2f57b56b10)), closes [#14](https://github.com/RealGeeks/geekcli/issues/14)
* print API warnings to stderr ([#51](https://github.com/RealGeeks/geekcli/issues/51)) ([37e49dd](https://github.com/RealGeeks/geekcli/commit/37e49ddad2c8ca5df28f39830081f8e900022da8)), closes [#28](https://github.com/RealGeeks/geekcli/issues/28)
* **search:** check values against the site's choices ([#47](https://github.com/RealGeeks/geekcli/issues/47)) ([9cf8b28](https://github.com/RealGeeks/geekcli/commit/9cf8b285247b4f9896b7d88e043c05e2096f6d67))
* **search:** document and validate the polygon criterion ([#48](https://github.com/RealGeeks/geekcli/issues/48)) ([293c00e](https://github.com/RealGeeks/geekcli/commit/293c00e3e1d864491f5ff52d7012779757126ebe)), closes [#18](https://github.com/RealGeeks/geekcli/issues/18)
* **search:** list every field the site accepts from its field catalog ([#55](https://github.com/RealGeeks/geekcli/issues/55)) ([71d7581](https://github.com/RealGeeks/geekcli/commit/71d75817861bcacc96664d9b0dcebf9841390e7b))


### Bug Fixes

* return the full record when getting by slug or path ([#43](https://github.com/RealGeeks/geekcli/issues/43)) ([a171a61](https://github.com/RealGeeks/geekcli/commit/a171a6139e8ef86b048995642eed9963e9858dff)), closes [#8](https://github.com/RealGeeks/geekcli/issues/8)

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
