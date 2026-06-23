# SPEC.md — Glia

Cognitive control plane for AI agents. Rust CLI + Hub. Graph-RAG via SurrealDB.
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
C10: SurrealDB (multi-model: doc+graph+vector+SQL) — Rust-native, Apache 2.0, embeddable in CLI binary (local zero-network) + server mode for Hub
C11: OpenBao native dynamic engines where available (DB, K8s); OAuth refresh-token exchange (Glia-managed) for SaaS without engines

## §I — Interfaces

### CLI
cmd: `glia init` → scan repo, detect stack, batch auth, embed skills, install hooks
cmd: `glia_action(intent, params)` → unified tool discover + skill fetch + exec
cmd: `glia_save_skill(rule)` → embed rule into SurrealDB
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
hook: `PreToolUse` → ping Glia, check SurrealDB rules, block+inject correction
hook: `file-open` → auto-call `glia_action` background, inject stack-aware skills
hook: Git `pre-push` → chunk+embed `.glia/skills/*.md` into SurrealDB

### External services
svc: SurrealDB — multi-model (doc+graph+vector+SQL), embedded in CLI + server for Hub
svc: OpenBao — DB secrets engine (dynamic Postgres), K8s engine, KV v2 (OAuth refresh tokens), Cubbyhole (per-exec access tokens), Transit (encryption-at-rest)
svc: Redis — synthesis cache
svc: OpenAI-compatible LLM endpoint (configurable base_url)
svc: Rust `candle` crate — pure-Rust BERT forward, model `all-MiniLM-L6-v2` (~90MB safetensors, CPU)

## §V — Invariants

V1: ∀ `glia_action(local-intent)` → Rust CLI routes natively via embedded SurrealDB, ⊥ Hub network call
V2: ∀ `glia_action(remote-intent)` → CLI proxies via WS → Hub Gateway
V3: Hub API ⊥ read plaintext secrets — OpenBao DB/K8s engines issue dynamic leases → Sandbox; OAuth SaaS: OpenBao KV stores refresh tokens, Glia exchanges → 15min access token → Sandbox via Cubbyhole (per-token, never logged)
V4: ∀ synthesis output → cite source chunk (`[Source: file.md]`)
V5: synthesis ⊥ rewrite rules — extract & cite only
V6: ∀ skill embed → local ONNX-equivalent via Rust `candle` (`all-MiniLM-L6-v2`), ⊥ external embedding API, ⊥ JS runtime
V7: ∀ `PreToolUse` shell cmd → Glia checks SurrealDB rules → block+inject if violation
V8: DB/K8s lease TTL via OpenBao ≤ 15min, auto-revoke; OAuth SaaS access token TTL ≤ 15min, Glia-enforced (not OpenBao lease)
V9: ∀ `glia_action` dependency check (`which uvx/npx`) → fallback to Hub sandbox exec
V10: ∀ proactive hook injection → silent, stack-filtered via SurrealDB graph edges
V11: `glia_save_skill` → embed + team-shared (not per-dev)
V12: community catalog schema pull ≠ trust — exec in user private sandbox only
V13: ∀ `glia_action` → CLI checks intent registry (local cmd set | remote cmd set), unknown intent → query SurrealDB, cache result
V14: `AUTH_REQUIRED` WS wait ≤ 120s, timeout → return `AUTH_TIMEOUT` to AI, dev can retry
V15: Hub unreachable → local intents still serve via embedded SurrealDB, remote intents fail fast with `HUB_UNREACHABLE`, ⊥ silent hang
V16: SurrealDB embedded in CLI (local mode, persistent disk) ≠ Hub SurrealDB (server mode) — skills sync bidirectionally, Hub-authoritative LWW (Last-Write-Wins) for global skills, dev-local skills namespaced `local::skill_name` to prevent overwrite, local-first on disconnect
V17: ∀ OAuth SaaS access token → cache in Redis (encrypted) for TTL duration, ⊥ redundant OAuth exchange calls across parallel `glia_action` executions
V18: ∀ Hub Sandbox exec → Hub API issues 1-time OpenBao response-wrapping token (`X-Vault-Wrap-TTL`) to Sandbox. Sandbox unwraps via `sys/wrapping/unwrap` directly against OpenBao, injects secret into child process env, purges on exit. Hub API memory ⊥ plaintext secret
V19: ∀ synthesis output → ≤ 150 tokens. If raw chunks contain >150 tokens of relevant rules, prioritize by graph edge weight (stack match) then truncate

## §T — Tasks

id|status|task|cites
T1|x|build Rust CLI `bridge` cmd (tokio stdio<->WS)|V2,I.cli,I.hub
T2|x|impl native FS module in Rust CLI|V1
T3|x|impl `glia_bash` in Rust CLI via strict command allow-list (regex match against `.glia/config.toml`) + path boundary checks (⊥ kernel seccomp/sandbox-exec for v1 cross-platform compatibility)|V1,V9
T4|x|build Hub WS Gateway (Axum)|V2,I.hub
T5|x|impl basic Execution Sandbox (npx/uvx/docker): Sandbox receives 1-time OpenBao response-wrapping token, queries OpenBao directly via `sys/wrapping/unwrap`, injects secret into child process env, purges on exit|V9,V18
T6|x|impl Dependency Probe (`which uvx/npx` → fallback remote)|V9
T7|x|integrate SurrealDB: embedded mode in CLI (persistent disk) + server mode for Hub; define graph schema edges: `tool --REQUIRES--> auth`, `skill --APPLIES_TO--> stack`, `user --HAS_ACCESS_TO--> tool`; ingest MCP schemas into graph. Prereq: T7 before T9 (unified action needs graph schema)|V1,V4,V10,V16,C10
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
T18|x|package `docker-compose.yml` (Hub+SurrealDB server+OpenBao+Redis). SurrealDB uses `memory` storage (named volume mount triggers `There was a problem with the database` on Windows bind-mount; Linux prod should switch to `rocksdb` on a named volume). OpenBao runs `server -dev` (single-process, in-memory + dev root token). Healthcheck probes via `wget` against `127.0.0.1:8200` (not `localhost` — OpenBao binds IPv4 only, `localhost` resolves `::1` → connection refused). SurrealDB image is scratch+surreal binary, no shell/curl/wget → ⊥ healthcheck|C7,V16
T19|x|impl `glia init` repo scan + stack detect + batch auth|I.cli,V8
T20|x|impl `glia use <community-tool>` catalog pull + private sandbox exec|V12,C9
T21|x|build community catalog GitHub repo + contribution flow|C9
T22|x|impl local-remote SurrealDB bidirectional sync + disconnect fallback (`HUB_UNREACHABLE`)|V15,V16

## §R — Research

id|claim|source|date
R1|Xenova `@xenova/transformers` is JS-only (ONNX Runtime WASM), ⊥ Rust bindings — use Rust `ort` crate instead|github.com/huggingface/transformers.js README|2026-06-22
R2|OpenBao secrets engines: DB (dynamic Postgres/MySQL), K8s (dynamic), KV v2 (static), Cubbyhole (per-token), Transit (encryption), Identity/OIDC, SSH, LDAP, PKI, TOTP — no native GitHub/Linear/Supabase-Anago engine, but Supabase Postgres → DB engine works|openbao.org/docs/secrets/|2026-06-22
R3|OpenBao leases: all dynamic secrets get lease + TTL, auto-revoke on expiry, prefix-based revocation; KV v2 does NOT issue leases (returns lease_duration only)|openbao.org/docs/concepts/lease/|2026-06-22
R4|HelixDB OSS v1 uses LMDB (sequential writes, small data, in-memory default); HelixDB Cloud is different arch (object storage) — divergent license/API risk. SurrealDB: Rust, Apache 2.0, multi-model (doc+graph+vector+SQL), embeddable in-process + server mode, MCP server built-in, no LMDB limit|docs.helix-db.com, surrealdb.com|2026-06-22
R5|SurrealDB graph edges are first-class documents via `RELATE` statement — `RELATE tool:linear -> REQUIRES -> auth:linear_oauth` creates queryable edge with properties. Supports LWW via `updated_at` timestamp field + sync. Redis encrypted token cache: standard pattern, encrypt with Transit-derived key|surrealdb.com/docs/surrealql/statements/define|2026-06-22
R6|OpenBao response wrapping: `X-Vault-Wrap-TTL` header triggers single-use cubbyhole token containing the secret. Recipient unwraps via `sys/wrapping/unwrap` directly against OpenBao. Token is single-use, TTL-bound, intercept-detectable. Hub API never sees plaintext — only wrapping token|openbao.org/docs/concepts/response-wrapping/|2026-06-22
R7|`ort` crate: all 1.x versions yanked (incl. 1.16.0) on 2025-11-20; only `2.0.0-rc.12` available, depends on `ort-sys` which links C++ ONNX runtime → requires MSVC Build Tools on Windows. `candle` crate is pure-Rust (no C++ toolchain), 0.9.2, supports BERT forward via `candle-transformers::models::bert::BertModel`, Apache-2.0. Bundle `all-MiniLM-L6-v2` safetensors (~90MB) + tokenizer + config via `rust-embed`; write mean-pool + L2-normalize by hand|crates.io, github.com/huggingface/candle|2026-06-22

## §B — Bugs

id|date|cause|fix