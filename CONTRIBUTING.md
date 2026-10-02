# Contributing to geekcli

Thanks for helping. Bug reports, fixes and new flags for Content API fields
are all welcome.

- **Questions about a Real Geeks website or account** go to
  [Real Geeks support](https://support.realgeeks.com/), not this repository.
- **Security problems** go through [private reporting](SECURITY.md), never a
  public issue.

## Development

You need a Rust toolchain (1.88 or newer).

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings   # pedantic; unwrap/expect/panic are denied
cargo fmt
```

All four must pass before a pull request is merged; CI runs them on Linux,
macOS and Windows, along with `cargo deny` (advisories and licenses), an MSRV
check and a lint of the GitHub Actions workflows. Tests run against mock
servers; nothing in the test suite talks to a real site. Point the CLI at a
local server with `GEEKCLI_BASE_URL=http://localhost:8000`.

[AGENTS.md](AGENTS.md) describes the layout and the conventions the code
follows, for people and coding agents alike. In short:

- Flags mirror the Content API's field names exactly.
- `update` sends only the fields you passed; never add defaults on update.
- stdout is the result document; anything for humans goes to stderr.
- A new command gets a section in `docs/GUIDE.md` and an integration test.

## Pull requests

- Use a [conventional commit](https://www.conventionalcommits.org/) title
  (`feat: …`, `fix: …`, `docs: …`): release notes are generated from them.
- Keep API keys, customer data and private site content out of code, tests,
  fixtures and screenshots.
- By contributing you agree that your contribution is licensed under the
  project's [MIT license](LICENSE).
