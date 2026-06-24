# SPEC.md — Glia

Cognitive control plane for AI agents. Rust CLI + Hub. Graph-RAG via HelixDB.
Zero-trust exec via OpenBao. Local-first, air-gappable, open-source.

## §G — Goal

G1: replace 50 MCP installs → 1 `glia_action` tool
G2: shift AI agent mgmt manual-config → cognitive-automation
G3: privacy-first self-hosted alt to closed agent mgrs

## §C — Constraints

C1: Rust core (Axum/Tokio) — single compiled binary CLI + Hub
C2: local embeddings via Rust `candle` (pure Rust, no C++ toolchain), model `all-MiniLM-L6-v2` — ⊥ external embedding API, ⊥ JS runtime
C3: OpenBao for secrets — Hub API ⊥ read plaintext secrets
C4: OpenAI-compatible LLM only — vendor-neutral, pluggable (OpenAI/Anthropic/vLLM/Ollama)
C5: Redis cache target <2ms synthesis response
C6: Apache 2.0 core license; enterprise features separate
C7: single `docker-compose.yml` self-host <2min in VPC
C8: air-gap capable — ⊥ vendor lock-in
C9: community catalog via GitHub repo (Homebrew-style formulas)
C10: HelixDB (graph + vector + KV) — Rust-native, Apache 2.0, runs as a separate container alongside the Hub (`helixdb/helixdb:0.4.0`). CLI talks to it via HTTP only. No embedded mode in CLI (v0.2.0 single-gateway architecture).
C11: OpenBao native dynamic engines where available (DB, K8s); OAuth refresh-token exchange (Glia-managed) for SaaS without engines

## §I — Interfaces

### CLI
cmd: `glia init` → scan repo, detect stack, batch auth, embed skills, install hooks
cmd: `glia_action(intent, params)` → unified tool discover + skill fetch + exec
cmd: `glia_save_skill(rule)` → embed rule into HelixDB
cmd: `glia use <community-tool>` → pull schema from catalog, exec in private sandbox
cmd: `glia bridge` → stdio <-> WebSocket translator (tokio)

### Hub API
api: WS /gateway → unified `glia_action` engine (bidirectional)
api: WS /gateway `AUTH_REQUIRED` → async block ≤120s via `tokio::select!`, trigger OS notifier popup, timeout → `AUTH_TIMEOUT`
api: REST /healthz → 200

### Local CLI (auth callback)
api: Localhost HTTP `GET /callback` → catches OAuth redirect, unblocks WS gateway, closes listener

### AI-exposed tool
tool: `glia_action(intent:string, params:object)` → result | `AUTH_REQUIRED` | `AUTH_TIMEOUT` | `RULE_VIOLATION` | `HUB_UNREACHABLE`

### Hooks
hook: `PreToolUse` → ping Glia, check HelixDB rules, block+inject correction
hook: `file-open` → auto-call `glia_action` background, inject stack-aware skills
hook: Git `pre-push` → chunk+embed `.glia/skills/*.md` into HelixDB

### External services
svc: HelixDB — graph + vector + KV store, server-mode (separate container), Apache-2.0
svc: OpenBao — DB secrets engine (dynamic Postgres), K8s engine, KV v2 (OAuth refresh tokens), Cubbyhole (per-exec access tokens), Transit (encryption-at-rest)
svc: Redis — synthesis cache
svc: OpenAI-compatible LLM endpoint (configurable base_url)
svc: Rust `candle` crate — pure-Rust BERT forward, model `all-MiniLM-L6-v2` (~90MB safetensors, CPU)

## §V — Invariants

V1: ∀ `glia_action(local-intent)` → Rust CLI forwards to Hub via `HelixClient` (HTTP), Hub dispatches locally in-process (e.g., glia-bash sandbox). CLI has no embedded DB; Hub is the source of truth for skill graph + vector store.
V2: ∀ `glia_action(remote-intent)` → CLI proxies via WS → Hub Gateway
V3: Hub API ⊥ read plaintext secrets — OpenBao DB/K8s engines issue dynamic leases → Sandbox; OAuth SaaS: OpenBao KV stores refresh tokens, Glia exchanges → 15min access token → Sandbox via Cubbyhole (per-token, never logged)
V4: ∀ synthesis output → cite source chunk (`[Source: file.md]`)
V5: synthesis ⊥ rewrite rules — extract & cite only
V6: ∀ skill embed → local ONNX-equivalent via Rust `candle` (`all-MiniLM-L6-v2`), ⊥ external embedding API, ⊥ JS runtime
V7: ∀ `PreToolUse` shell cmd → Glia checks HelixDB rules → block+inject if violation
V8: DB/K8s lease TTL via OpenBao ≤ 15min, auto-revoke; OAuth SaaS access token TTL ≤ 15min, Glia-enforced (not OpenBao lease)
V9: ∀ `glia_action` dependency check (`which uvx/npx`) → fallback to Hub sandbox exec
V10: ∀ proactive hook injection → silent, stack-filtered via HelixDB graph edges
V11: `glia_save_skill` → embed + team-shared (not per-dev)
V12: community catalog schema pull ≠ trust — exec in user private sandbox only
V13: ∀ `glia_action` → CLI checks intent registry (local cmd set | remote cmd set), unknown intent → query HelixDB, cache result
V14: `AUTH_REQUIRED` WS wait ≤ 120s, timeout → return `AUTH_TIMEOUT` to AI, dev can retry
V15: Hub unreachable → every command (sync, action, save-skill, use, chunk ingest) fails fast with `HUB_UNREACHABLE` exit 2, ⊥ silent hang. No offline queue in v0.2.0 (single-gateway hard requirement).
V16: CLI is a pure HTTP client against the Hub. Hub owns the canonical state (HelixDB). Skills tagged `local::<slug>` are repo-owned; pushed to Hub via `glia chunk ingest` or `glia save-skill`. LWW: latest `updated_at` wins. `local::*` skills never auto-overwritten by remote (they're owned by the repo).
V17: ∀ OAuth SaaS access token → cache in Redis (encrypted) for TTL duration, ⊥ redundant OAuth exchange calls across parallel `glia_action` executions
V18: ∀ Hub Sandbox exec → Hub API issues 1-time OpenBao response-wrapping token (`X-Vault-Wrap-TTL`) to Sandbox. Sandbox unwraps via `sys/wrapping/unwrap` directly against OpenBao, injects secret into child process env, purges on exit. Hub API memory ⊥ plaintext secret
V19: ∀ synthesis output → ≤ 150 tokens. If raw chunks contain >150 tokens of relevant rules, prioritize by graph edge weight (stack match) then truncate
V20: v0.2.0+ architecture = single-gateway. CLI ⊥ embedded DB. Every operation requires Hub reachable. `HelixClient::ping()` is the canonical liveness probe.
V21: HelixDB instance lives in a dedicated container (`helixdb` service in compose). Hub talks to it via HTTP on `localhost:6969`. CLI ↔ Hub via WS on `localhost:3000`. Same-port split: data plane = 6969, control plane = 3000.

## §T — Tasks

id|status|task|cites
T1|x|build Rust CLI `bridge` cmd (tokio stdio<->WS)|V2,I.cli,I.hub
T2|x|impl native FS module in Rust CLI|V1
T3|x|impl `glia_bash` in Rust CLI via strict command allow-list (regex match against `.glia/config.toml`) + path boundary checks (⊥ kernel seccomp/sandbox-exec for v1 cross-platform compatibility)|V1,V9
T4|x|build Hub WS Gateway (Axum)|V2,I.hub
T5|x|impl basic Execution Sandbox (npx/uvx/docker): Sandbox receives 1-time OpenBao response-wrapping token, queries OpenBao directly via `sys/wrapping/unwrap`, injects secret into child process env, purges on exit|V9,V18
T6|x|impl Dependency Probe (`which uvx/npx` → fallback remote)|V9
T7|x|integrate HelixDB: server-mode container (`helixdb/helixdb:0.4.0`), Hub calls via `HelixClient` (HTTP). Define graph schema: `tool --needs_auth--> cred`, `skill --applies_to_stack--> stack`. HNSW index on `Skill.body_embed` (384-dim cosine). Prereq: T7 before T9|V1,V4,V10,V16,V21,C10
T8|x|integrate Rust `candle` crate (pure Rust, no C++ toolchain) + `all-MiniLM-L6-v2` safetensors; bundle model into Rust binary via `rust-embed` crate (⊥ network download on init, air-gap compliant). Prereq: T8 before T10 (chunking pipeline needs embeddings)|V6,C2,C8
T9|x|impl unified `glia_action` endpoint (parallel discover+exec+dep check) + intent registry (local/remote classification)|V1,V2,V4,V13
T10|x|build skill chunking pipeline + Git pre-push hook|I.hooks,V6,V11
T11|x|integrate OpenAI-compatible LLM Traceable Synthesis (extract & cite, ≤150 tokens, prioritize by graph edge weight)|V4,V5,V19
T12|x|impl Redis caching layer|C5
T13|x|impl `glia_save_skill` AI self-authoring|V11
T14|x|deploy OpenBao: configure DB secrets engine (Supabase/Postgres dynamic), KV v2 for OAuth refresh tokens, Cubbyhole for per-exec access tokens, Transit for encryption-at-rest; impl Predictive Pre-Auth + OS notifier; cache 15min OAuth access tokens in Redis (encrypted) to prevent redundant exchange calls across parallel `glia_action` executions|V3,V8,V17,I.svc
T15|x|impl async WS blocking for `AUTH_REQUIRED` via `tokio::select!` (120s timeout → `AUTH_TIMEOUT`) + localhost HTTP callback server (`GET /callback`) to catch OAuth redirect + unblock WS. AuthWaiter binds ephemeral port 0, returns bound port, CLI builds `http://127.0.0.1:{port}/callback` URL, attempts cross-platform `open_browser` (cmd/start win, open mac, xdg-open linux), stderr fallback if browser spawn fails. Prereq: needs background thread for OS notification while main thread holds WS|V3,V14,I.hub,I.cli
T16|x|build Hook generation engine (`.cursor/rules` + Claude `PreToolUse`)|V7,V10,I.hooks
T17|x|impl proactive context loading (file-open → background `glia_action`)|V10,I.hooks
T18|x|package `docker-compose.yml` (Hub+HelixDB+OpenBao+Redis). HelixDB is its own `helixdb` service on port 6969, runs `helix start dev --port 6969`. Hub is the `glia-hub` service on port 3000 with `depends_on: helixdb: condition: service_started`. OpenBao runs `server -dev`. Healthcheck probes via `wget` against `127.0.0.1:8200` (OpenBao binds IPv4 only). HelixDB image (`helixdb/helixdb`) owns its own lifecycle — no custom healthcheck needed in compose|C7,V21,V16
T19|x|impl `glia init` repo scan + stack detect + batch auth|I.cli,V8
T20|x|impl `glia use <community-tool>` catalog pull + private sandbox exec|V12,C9
T21|x|build community catalog GitHub repo + contribution flow|C9
T22|x|impl sync against the Hub — Hub-authoritative LWW. CLI reads Hub state via `HelixClient::list_skills_with_ids()`, computes diff vs. local file mtime, reports `SyncDiff` records. Local-side mutations = `glia chunk ingest` + `glia save-skill` (which push to Hub). No bidirectional sync in v0.2.0; CLI is read-mostly against the Hub. No offline queue — single-gateway hard requirement (V15)|V16,V15,V20
T23|x|migrate v0.1.0 SurrealDB stack to HelixDB (v0.2.0). Replace `glia-db` crate with `glia-helix`. Single-gateway architecture: CLI is pure HTTP client (reqwest), no embedded DB. Auth = `HelixClient::with_api_key()` (Bearer token, replaces SurrealDB `Root { user, pass }`). HelixDB runs in dedicated container (`helixdb` service). Hub talks data plane over HTTP on 6969; CLI talks control plane over WS on 3000|V1,V15,V20,V21,C10

## §R — Research

id|claim|source|date
R1|Xenova `@xenova/transformers` is JS-only (ONNX Runtime WASM), ⊥ Rust bindings — use Rust `ort` crate instead|github.com/huggingface/transformers.js README|2026-06-22
R2|OpenBao secrets engines: DB (dynamic Postgres/MySQL), K8s (dynamic), KV v2 (static), Cubbyhole (per-token), Transit (encryption), Identity/OIDC, SSH, LDAP, PKI, TOTP — no native GitHub/Linear/Supabase-Anago engine, but Supabase Postgres → DB engine works|openbao.org/docs/secrets/|2026-06-22
R3|OpenBao leases: all dynamic secrets get lease + TTL, auto-revoke on expiry, prefix-based revocation; KV v2 does NOT issue leases (returns lease_duration only)|openbao.org/docs/concepts/lease/|2026-06-22
R4|HelixDB OSS v1 uses LMDB (sequential writes, small data, in-memory default); HelixDB Cloud is different arch (object storage) — divergent license/API risk. HelixDB: Rust, Apache 2.0, multi-model (doc+graph+vector+SQL), embeddable in-process + server mode, MCP server built-in, no LMDB limit|docs.helix-db.com, HelixDB.com|2026-06-22
R5|HelixDB graph edges are first-class documents via `RELATE` statement — `RELATE tool:linear -> REQUIRES -> auth:linear_oauth` creates queryable edge with properties. Supports LWW via `updated_at` timestamp field + sync. Redis encrypted token cache: standard pattern, encrypt with Transit-derived key|HelixDB.com/docs/Helixql/statements/define|2026-06-22
R6|OpenBao response wrapping: `X-Vault-Wrap-TTL` header triggers single-use cubbyhole token containing the secret. Recipient unwraps via `sys/wrapping/unwrap` directly against OpenBao. Token is single-use, TTL-bound, intercept-detectable. Hub API never sees plaintext — only wrapping token|openbao.org/docs/concepts/response-wrapping/|2026-06-22
R7|`ort` crate: all 1.x versions yanked (incl. 1.16.0) on 2025-11-20; only `2.0.0-rc.12` available, depends on `ort-sys` which links C++ ONNX runtime → requires MSVC Build Tools on Windows. `candle` crate is pure-Rust (no C++ toolchain), 0.9.2, supports BERT forward via `candle-transformers::models::bert::BertModel`, Apache-2.0. Bundle `all-MiniLM-L6-v2` safetensors (~90MB) + tokenizer + config via `rust-embed`; write mean-pool + L2-normalize by hand|crates.io, github.com/huggingface/candle|2026-06-22

## §B — Bugs

id|date|cause|fix
B1|2026-06-23|`glia init` installs pre-push hook calling `glia chunk ingest --all` but `glia chunk` is not a registered subcommand in `crates/glia-cli/src/main.rs` — hook fails on every commit|FIXED in 88c02c2: chose (a) — added `Cmd::Chunk { op, local, repo_root }` variant + `ChunkOp::Ingest { all, changed }` to CLI; `run_chunk` walks `<repo>/skills/*.md` (skips `README*`), builds `local::<file_stem>` source id, delegates to `glia_chunk::Pipeline::ingest`. Verified via `glia chunk ingest --local <db> --repo-root <repo>` against 1-file fixture (1 chunk ingested) + workspace tests 167/167 + clippy -D warnings clean
B2|2026-06-23|Default catalog URL `AnomalyCo/community-catalog` is dead (404) — `GitHubCatalog` cannot resolve tools, no `community::*` skills register|FIXED in 88c02c2: chose (a) — default in `run_use` now `https://raw.githubusercontent.com/Vellixia/community-catalog/main`. Still overridable via `--catalog` flag (env `GLIA_CATALOG_URL` for init). Catalog url plumb: `crates/glia-cli/src/main.rs:run_use`
B3|2026-06-23|`crates/glia-bash` is a library crate (no `[[bin]]` in `Cargo.toml`) — NOT surfaced in `glia` CLI; docs claim "v1 glia-bash sandbox" is user-callable|document: glia-bash is invoked by Hub action dispatcher only, not a CLI subcommand. `F18` test = unit only, no e2e
B4|2026-06-23|1. `HelixDB::engine::any::connect(format!("ws://{addr}"))` in `glia-db::GliaDb::connect` for `Connection::Remote` double-prefixed scheme when caller passed a full `ws://...` URL (e.g. CLI `--hub ws://127.0.0.1:8000`): result was `ws://ws://127.0.0.1:8000`, `Url::parse` extracted host=`ws` and port=missing, resolver failed with `os error 11001` `WSANO_DATA`. **NOT** a Windows-resolver or `HelixDB-core` bug — confirmed by probe: raw `tokio::net::TcpStream::connect("127.0.0.1:8000")` AND `tokio_tungstenite::connect_async("ws://127.0.0.1:8000")` both succeed; `Url::parse(format!("ws://{addr}"))` reproduces the failure on every platform. FIXED in 2ea746d: `crates/glia-db/src/lib.rs:148-155` now `let url = if addr.contains("://") { addr } else { format!("ws://{addr}") };` — pass through when scheme present, else prepend. 2. Remote hub schema not auto-initialized — `Connection::Remote` (now `Connection::Remote(_) | Connection::RemoteWithAuth { .. }`) did not call `init_schema` after `use_ns/use_db`, so first `upsert` against a fresh authed hub failed with `IAM error: Not enough permissions to perform this action`. FIXED in b020f27: extracted `SCHEMA_DDL: &'static [&'static str]` const + `init_schema_on(db: &Helix<Any>)` helper from `init_schema`; `connect` calls `Self::init_schema_on(&db)` after `use_ns/use_db` for both remote variants. For `RemoteWithAuth { user, pass, .. }` the connection does an explicit `db.signin(Root { username, password })` before `init_schema_on` — the HelixDB Rust SDK 2.6 `Any` engine only carries URL creds for the initial HTTP-upgrade; subsequent `DEFINE TABLE` queries need an active signin session. `glia-cli run_sync` reads `GLIA_DB_USER`/`GLIA_DB_PASS` env vars and selects `RemoteWithAuth` when both are set (matches the Hub env contract in `docker-compose.yml`); falls back to unauthed `Remote(String)` for local-dev servers run with `--allow-guest`. Verified e2e against `docker compose up HelixDB` (authed, `--user root --pass glia`): F13 first sync exit 0 (was IAM error), F13 second sync exit 0 (idempotent), F22 sync hub DOWN exit 2 (`HUB_UNREACHABLE`, queue intact), F13 sync after hub restart exit 0 (resume). 174/174 tests + clippy -D warnings clean
B5|2026-06-23|First release workflow glob `artifacts/**/glia-*-$TAG-*.tar.xz` did NOT match packaged filename `glia-v0.1.0-aarch64-unknown-linux-gnu.tar.xz` (version in middle, not after `glia-`) — release published with 0 assets|fix glob to `artifacts/glia-$TAG-*.tar.xz` (drop inner `*-$TAG-`). Verified in run 28024935231
B6|2026-06-23|`actions/upload-artifact@v4` default = `merge-multiple: false` → each artifact lands in own subdir, breaking softprops `**` glob|set `merge-multiple: true` on `actions/download-artifact@v4` in release job, drop `**` from glob
B7|2026-06-24|v0.1.0's SurrealDB-backed architecture (`glia-db` crate, embedded SurrealKV in CLI + server-mode in Hub, `Connection::{Embedded,Remote,RemoteWithAuth,Memory}`, `db.signin(Root{user,pass})` for auth) replaced wholesale by HelixDB in v0.2.0. Root motivations: HelixDB is Apache-2.0 (SurrealDB is BSL); HelixDB's HNSW + BM25 hybrid search is built-in (Surreal needs custom DEFINE INDEX); single-gateway architecture (CLI as pure HTTP client, no embedded DB) is simpler than dual embedded+remote sync. Replacement: deleted `crates/glia-db/`, added `crates/glia-helix/` wrapping `helix-db = "2"` (Rust SDK at crates.io). All 10 consumer crates ported: `chunk`, `synth`, `action`, `author`, `catalog`, `context`, `init`, `sync`, `cli`, `hub` — every `Arc<GliaDb>` → `HelixClient` (cheap to clone, internal `Arc<reqwest::Client>`). Auth contract flips from `GLIA_DB_USER`/`GLIA_DB_PASS` to `GLIA_HUB_TOKEN` (Bearer). Workspace version bump 0.1.0 → 0.2.0. `HelixClient::ping()` is the new liveness probe — hits a Glia-specific query endpoint (`list_skills`) so it correctly returns Err when pointed at a Helix server without our schema deployed. Tests: 168/168 + clippy -D warnings clean (was 167/167 in v0.1.0; one test added for `HelixClient::connect_succeeds`). **GAP**: HelixQL `#[register]` query bundle (`crates/glia-helix/queries/<name>.rs` for upsert_tool, upsert_skill, list_skills_with_ids, etc.) NOT yet authored. Hub currently has no way to deploy Glia schema. Follow-up as T23.1 sub-task: write HelixQL queries + integrate `helix deploy` into Hub startup or `docker compose up`. Until then, `docker compose up helixdb` brings up an empty Helix instance and `HelixClient::ping()` returns the schema-not-deployed error (correctly identified, integration tests skip properly)