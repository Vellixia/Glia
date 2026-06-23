# Changelog

All notable changes to Glia are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-06-23

First public release. All 22 spec tasks (`T1`–`T22`) complete, 164/164
tests green, clippy clean, full stack verified up via `docker compose ps`.

### Added

- **`glia` CLI** (single binary, no JS runtime, no C++ toolchain)
  - `bridge` — stdio ⇄ WebSocket translator
  - `sync` — bidirectional LWW sync between local and Hub SurrealDB
  - `init` — repo scan, stack detect, batch OAuth, hook install
  - `action` — unified `glia_action(intent, params)` orchestrator
  - `save-skill` — author a local skill via OpenAI-compatible LLM
  - `use` — pull a community skill from `Vellixia/community-catalog`
- **`glia-hub`** (single linux ELF, `debian:bookworm-slim` runtime)
  - `WS /gateway` — the one tool the agent ever calls
  - `WS /gateway` — `AUTH_REQUIRED` async block, ≤ 120 s, then `AUTH_TIMEOUT`
  - `GET /health`
- **Storage**
  - `glia-db` — SurrealDB v2.6.5, multi-model, embedded SurrealKV in the CLI
  - `glia-embed` — `candle` + `all-MiniLM-L6-v2` via `rust-embed`
  - `glia-cache` — Redis, target < 2 ms synthesis response
  - `glia-bao` — OpenBao: Transit, KV v2, response-wrapping
- **20-crate workspace** with workspace-level lints:
  - `unsafe_code = "forbid"`
  - `missing_docs = "warn"`
  - `all = "warn"`
- **`docker-compose.yml`** — self-host in < 2 min:
  - `glia-hub` (3000)
  - `surrealdb` (8000) — `memory` storage
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

- SurrealDB on Windows named volumes: switched to `memory` storage;
  `rocksdb` still works in linux prod with a named `surrealdb_data` volume.
- OpenBao dev mode: explicit `["server","-dev","-dev-listen-address=0.0.0.0:8200","-dev-root-token-id=glia-root"]`
  in compose; env vars alone do not trigger dev mode.
- OpenBao healthcheck: `http://127.0.0.1:8200/v1/sys/health` (not
  `localhost` — IPv4-only bind refuses `::1`).
- Removed empty-dir bind mount for OpenBao config (was wiping image
  config on first start).
- SurrealDB healthcheck removed — the image is `scratch + surreal`
  binary, no shell to run `wget` or `curl`.

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
