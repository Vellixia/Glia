# Development

20 crates, no `unsafe`, single binary CLI + Hub. Workspace lints are
strict: warnings fail the build.

## Prerequisites

- **Rust 1.89+** (`rustup default stable`).
- **C/C++ toolchain** (Linux/macOS) — needed by `rocksdb` transitively
  pulled in by `HelixDB`. Windows users with MSVC are fine.
- **Docker** (optional) — for the self-host stack.
- **OpenSSL headers** (Linux) — `apt install libssl-dev`.

## Workspace layout

```
.
├── Cargo.toml                      # workspace root
├── SPEC.md                         # goal, constraints, invariants, tasks, bugs
├── Dockerfile.hub                  # multi-stage linux build for the Hub
├── docker-compose.yml              # self-host stack
├── crates/                         # 20-crate workspace
│   ├── glia-cli/                   # the `glia` binary
│   ├── glia-hub/                   # the `glia-hub` binary
│   ├── glia-{action,auth,bao,bash,bridge,catalog,cache,chunk,context,
│   │         db,embed,fs,hooks,init,sandbox,sync,synth,author}/
│   └── …
├── docs/                           # architecture, security, cli, hub, catalog, development
├── community-catalog/              # local clone of the catalog repo
│   ├── catalog.json
│   └── tools/                      # one .md per skill
├── .github/workflows/{ci,release}.yml
├── .dockerignore
├── .gitignore
└── LICENSE
```

## Commands

```bash
# Format
cargo fmt --all

# Lint (workspace-wide, warnings as errors)
cargo clippy --workspace --all-targets -- -D warnings

# Test (167 tests, including 2 auth-wiring tests)
cargo test --workspace --all-targets -- --test-threads=1

# Build the CLI
cargo build --release -p glia-cli

# Build the Hub image
docker build -f Dockerfile.hub -t glia-hub:dev .
```

The `--test-threads=1` flag dodges a Windows-only race in
`glia-init::tests::count_files_skips_node_modules` where parallel tests can
briefly see a sibling test's tmp dir.

## CI

GitHub Actions runs on `ubuntu-latest`, `windows-latest`, `macos-latest`
for every push and PR. Steps per OS:

1. `dtolnay/rust-toolchain@1.89` with `components: rustfmt, clippy`.
2. `Swatinem/rust-cache@v2` for cargo registry + build cache.
3. `cargo fmt --all -- --check`.
4. `cargo clippy --workspace --all-targets -- -D warnings`.
5. `cargo test --workspace --all-targets --no-fail-fast -- --test-threads=1`.
6. `cargo build --release -p glia-cli`.

The `unsafe_code = "forbid"` and `missing_docs = "warn"` lints are
enforced at the workspace level (`[workspace.lints]`).

## Embedding model assets

`glia-embed` bundles `all-MiniLM-L6-v2` via `rust-embed`. The model files
(`model.safetensors`, `tokenizer.json`, `config.json`) are gitignored
under `crates/glia-embed/embed/`. To run tests locally:

```bash
# Option A: use the bundled default
cargo run -p glia-embed -- --download   # one-time

# Option B: drop the model files manually
# curl -L https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/{config.json,tokenizer.json,model.safetensors} \
#   -o crates/glia-embed/embed/{config.json,tokenizer.json,model.safetensors}
```

CI skips embed-requiring tests when the model is absent — see
`Embedder::try_new()` in `crates/glia-embed/src/lib.rs`.

## Release process

Tag-driven, hand-rolled workflow (`.github/workflows/release.yml`):

1. Update `CHANGELOG.md` with the new version.
2. `git tag -a vX.Y.Z -m "vX.Y.Z" && git push origin vX.Y.Z`.
3. CI builds for 5 targets (linux + macos × x86_64 + aarch64, windows
   × x86_64), uploads artifacts, and attaches them to the GitHub
   Release.

aarch64-windows is intentionally dropped for v0.1.0 (no demand yet, long
cross-compile tail). Re-enable by adding a target to `release.yml`.

## Adding a new crate

1. `cargo new --lib crates/glia-foo`.
2. Add `glia-foo = { path = "crates/glia-foo" }` to root `Cargo.toml` `[workspace.dependencies]` (if shared) and to the consuming crate's `[dependencies]`.
3. Use `async-trait` if you expose a `dyn` trait, or `if-let-chains` for
   nested matches.
4. Add tests, run `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --all-targets -- --test-threads=1`.

## Style

- **No `unsafe`** — ever. The workspace lint forbids it.
- **No comments unless they add intent.** Code should be self-explanatory.
- **`#![deny(missing_docs)]` per crate where it matters.**
- **Error types** — use `thiserror` enums, not `String`. Add a variant,
  not a panic.
- **Async** — `tokio` everywhere. `#[async_trait]` for `dyn` traits,
  `Box::pin` for recursive futures.
- **No emoji in code or docs.** Stay terse.

## Common issues

| Symptom | Cause | Fix |
|---------|-------|-----|
| `error: could not find openssl-sys` | Missing OpenSSL headers | `apt install libssl-dev` (Linux), `brew install openssl` (macOS) |
| `LINK.exe not found` | No MSVC | Install Visual Studio Build Tools |
| CI fails with `manual implementation of ok` | clippy 1.89 new lint | Use `expr.ok()` not `match expr { Ok => Some, Err => None }` |
| CI fails with `if statement can be collapsed` | clippy 1.89 new lint | Use `if-let-chains` |
| `MissingAsset("model.safetensors")` | No embed model on disk | Download per above; tests will skip cleanly if absent |
