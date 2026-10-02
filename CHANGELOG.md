# Changelog

## [0.5.0](https://github.com/RealGeeks/geekcli/compare/v0.4.0...v0.5.0) (2026-10-02)


### Features

* catch up with the Content API and link its reference ([#3](https://github.com/RealGeeks/geekcli/issues/3)) ([aec5f77](https://github.com/RealGeeks/geekcli/commit/aec5f77b5973edb87af17cc9164876025c21845a))

## 0.4.0 (2026-10-02)


### Features

* geekcli, the Real Geeks command line ([873dce5](https://github.com/RealGeeks/geekcli/commit/873dce526d3b3fc8e5c8e1a47a3655f888b942cc))


### Bug Fixes

* **deps:** update rustls to 0.23.45 for RUSTSEC-2026-0285 ([4439918](https://github.com/RealGeeks/geekcli/commit/44399187318a68ed48267b92976b9c0c41cfe740))

## Changelog

Versions before 0.4.0 were released internally under the name `realgeeks`.
`geekcli` moves an existing `~/.config/realgeeks/` to `~/.config/geekcli/` the
first time it runs, and reads `GEEKCLI_*` environment variables in place of
`REALGEEKS_*`.
