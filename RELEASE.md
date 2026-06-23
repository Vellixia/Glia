# Release process

Glia releases are tag-driven and cross-platform.

## TL;DR

```bash
# 1. Bump version (workspace at root, members tracked by release-plz)
git tag -a v0.1.0 -m "v0.1.0"
git push origin v0.1.0
```

That's it. The release workflow at
[`.github/workflows/release.yml`](.github/workflows/release.yml):

1. Builds the `glia` CLI for 5 targets in parallel:
   - `x86_64-unknown-linux-gnu`
   - `aarch64-unknown-linux-gnu`
   - `x86_64-apple-darwin`
   - `aarch64-apple-darwin`
   - `x86_64-pc-windows-msvc`
2. Bundles each binary with `README.md` + `LICENSE` into
   `glia-<tag>-<target>.{tar.xz,zip}`.
3. Uploads artifacts and creates a GitHub Release with auto-generated
   notes.

Windows `aarch64` is intentionally out of scope for v0.1.0 —
GitHub's `windows-latest` runner is x86_64-only and MSVC ARM64
cross-toolchain is not pre-installed.

## How a version is decided

We follow [SemVer](https://semver.org/spec/v2.0.0.html):

- **MAJOR** — breaking change to the `glia_action` contract, the WS
  gateway protocol, or the on-disk SurrealDB schema.
- **MINOR** — new CLI subcommand, new `glia-hub` endpoint, new
  community-catalog API, new optional sidecar.
- **PATCH** — bug fix, perf gain, doc fix, or any change that does
  not alter a public contract.

## Changelog

Every PR must update [`CHANGELOG.md`](CHANGELOG.md) under one of:

- `### Added`
- `### Changed`
- `### Deprecated`
- `### Removed`
- `### Fixed`
- `### Security`

This is enforced by review, not by a CI check yet.

## Signing

Binaries are **not** signed at the binary level in v0.1.0. SHA-256
sums are published with each release; downstream packagers (homebrew,
winget, AUR) can pin to those sums.

The plan for v0.2.0 is to add:

- `cosign` keyless signing of the `glia` linux binary
- `minisign` for macOS
- Authenticode for Windows MSI

## Pre-release checklist

- [ ] `cargo fmt --all -- --check` is clean
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is clean
- [ ] `cargo test --workspace --all-targets` is green (167 tests)
- [ ] `docker compose up -d` brings all four services up
- [ ] `docker compose ps` reports all four as `Up` (or `healthy`)
- [ ] `cargo run -p glia-cli -- action --intent "hello"` returns
      `NotApplicable` against local mode
- [ ] `cargo run -p glia-cli -- use linear-create-issue` pulls from
      the local catalog clone and registers `community::linear-create-issue`
- [ ] `CHANGELOG.md` is updated and the link at the bottom points
      to the new release tag
- [ ] `SPEC.md` §T is in sync with the released code (run
      `cargo install spec-build` + `spec check` if you have it locally)

## Post-release

- Announce in GitHub Discussions → `Announcements`
- Bump `workspace.package.version` in `Cargo.toml` to `0.2.0-dev`
  so subsequent PRs show up as unreleased

## Out of scope for v0.1.0

- Crates.io publish (workspace is `publish = false` for v0.1.0)
- Homebrew tap
- Winget PR
- Docker Hub image

These are tracked as issues in
[`Vellixia/Glia`](https://github.com/Vellixia/Glia/issues).
