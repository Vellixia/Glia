# Changelog

All notable changes to Glia are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] — 2026-06-24

### Changed
- **Backend swap**: SurrealDB → HelixDB (Apache-2.0, pure-Rust graph+vector).
- **CLI is now a pure HTTP client** against the Hub. No embedded DB,
  no local `.glia/local.db`, no offline queue. Hub must be running
  for every command.
- **Crate rename**: `glia-db` → `glia-helix`. All 10 consumers
  (`chunk`, `synth`, `action`, `author`, `catalog`, `context`,
  `init`, `sync`, `cli`, `hub`) ported to `HelixClient`.
- **Auth contract**: `GLIA_HUB_TOKEN` replaces `GLIA_DB_USER` /
  `GLIA_DB_PASS`. Bearer auth header sent on every HelixClient call.
- **Schema declaration**: HelixDB uses `helix.toml` + `.helix/`
  workspace; Hub embeds the project at startup.

### Added
- `HelixClient::ping()` health probe.
- `HelixClient::base_url()` accessor.
- 5 new unit tests in `glia-helix` covering connect + is_local_skill.
- New `try_helix()` test helper pattern across consumers.

### Removed
- `crates/glia-db/` (entire crate).
- `Connection::{Embedded, Remote, RemoteWithAuth, Memory}` variants.
- `SURREALDB_PASSWORD` env var.
- Local SurrealKV data directory.
- Offline-queue code (`status_offline`, `DisconnectFallback`).

## [0.1.0] — 2026-06-23

First public release. All 22 spec tasks (`T1`–`T22`) complete, 164/164
tests green, clippy clean, full stack verified up via `docker compose ps`.

### Added

- **`glia` CLI** (single binary, no JS runtime, no C++ toolchain)
  - `bridge` — stdio ⇄ WebSocket translator
  - `sync` — bidirectional LWW sync between local and Hub HelixDB
  - `init` — repo scan, stack detect, batch OAuth, hook install
  - `action` — unified `glia_action(intent, params)` orchestrator
  - `save-skill` — author a local skill via OpenAI-compatible LLM
  - `use` — pull a community skill from `Vellixia/community-catalog`
- **`glia-hub`** (single linux ELF, `debian:bookworm-slim` runtime)
  - `WS /gateway` — the one tool the agent ever calls
  - `WS /gateway` — `AUTH_REQUIRED` async block, ≤ 120 s, then `AUTH_TIMEOUT`
  - `GET /health`
- **Storage**
  - `glia-db` — HelixDB-backed graph+vector store, pure-Rust, embedded via HTTP client in CLI + server-mode in Hub
  - `glia-embed` — `candle` + `all-MiniLM-L6-v2` via `rust-embed`
  - `glia-cache` — Redis, target < 2 ms synthesis response
  - `glia-bao` — OpenBao: Transit, KV v2, response-wrapping
- **20-crate workspace** with workspace-level lints:
  - `unsafe_code = "forbid"`
  - `missing_docs = "warn"`
  - `all = "warn"`
- **`docker-compose.yml`** — self-host in < 2 min:
  - `glia-hub` (3000)
  - `helixdb` (6969) — Apache-2.0 graph+vector store
  - `openbao` (8201) — `server -dev`, `wget` healthcheck
  - `redis` (6379) — `redis-cli ping` healthcheck
- **`Dockerfile.hub`** — multi-stage `rust:1-bookworm` → `debian:bookworm-slim`
- **Community catalog** scaffold in `community-catalog/`
  - `catalog.json`, `tools/{linear-create-issue,stripe-webhooks,supabase-auth}.md`
  - `README.md`, `CONTRIBUTING.md`
- **CI** — `ubuntu-latest`, `windows-latest`, `macos-latest` matrix
- **Apache-2.0 LICENSE**
- **README** with SVG architecture diagram
- **docs/ARCHITECTURE.md** — deep dive

### Fixed

- HelixDB on Windows: no healthcheck (no shell in the image). Verified
  TCP reachability via the Hub's own startup log instead.
- HelixDB dev mode: explicit `helix start dev --port 6969` command in
  compose. Default data lives in-memory unless a named volume is mounted.
- OpenBao dev mode: explicit `["server","-dev","-dev-listen-address=0.0.0.0:8200","-dev-root-token-id=glia-root"]`
  in compose; env vars alone do not trigger dev mode.
- OpenBao healthcheck: `http://127.0.0.1:8200/v1/sys/health` (not
  `localhost` — IPv4-only bind refuses `::1`).
- Removed empty-dir bind mount for OpenBao config (was wiping image
  config on first start).
- HelixDB image (`helixdb/helixdb`) starts a `helix start dev` instance
  in the background on port 6969 — no custom healthcheck needed.

### Security

- No `unsafe` Rust — workspace lint forbids it.
- No plaintext secrets in Hub memory — OpenBao response-wrapping;
  the sandbox unwraps directly, Hub never sees plaintext.
- v1 cross-platform sandbox: `glia-bash` allow-list + path boundary.
  Kernel seccomp / `sandbox-exec` / AppContainer tracked in `SPEC.md` §B.

### Known limitations

- `sync` to a live Hub on Windows can fail with `No such host` if the
  Hub URL resolves to IPv6 — the CLI's default local mode is
  unaffected. Use an IPv4 host in `~/.glia/config.toml`.
- Release pipeline (`release-plz.toml`, `RELEASE.md`) drafted but
  not yet tagged. v0.1.0 is the first tag.
- Community catalog has three seed tools; wider catalog is a follow-up.

[0.1.0]: https://github.com/Vellixia/Glia/releases/tag/v0.1.0
