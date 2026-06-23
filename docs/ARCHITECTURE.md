# Glia architecture

The deep dive behind [`README.md`](../README.md) and [`SPEC.md`](../SPEC.md).

## Trust model

Glia is built around three trust tiers, and the data plane is engineered so
that nothing in a higher tier ever needs to see data from a lower tier:

| Tier               | Components                              | Trusts                       |
| ------------------ | --------------------------------------- | ---------------------------- |
| Agent              | MCP client (Cursor, Cline, opencode…)   | nothing                      |
| Local (semi-trusted) | `glia` CLI, embedded SurrealKV         | the Hub, but not its secrets  |
| Hub (trusted)      | `glia-hub`, SurrealDB, OpenBao, Redis   | secrets (response-wrapped)    |
| Community (untrusted) | `community-catalog` GitHub repo       | only the static markdown      |

The **only** thing the agent ever calls is `glia_action(intent, params)`. The
Hub never sees plaintext secrets, the community catalog never sees the Hub,
and the CLI can run disconnected by falling back to its embedded SurrealKV.

## Component map

### `glia` CLI (the agent's only surface)

- `glia-bridge` — stdio ⇄ WebSocket translator. Implements the MCP
  transport so any MCP client can talk to the Hub without rewriting.
- `glia-action` — the orchestrator: parse intent → resolve skill →
  fetch chunks → compose prompt → LLM-synthesize → exec → result.
- `glia-init` — scans a repo, detects stack (Next.js, Supabase, Stripe, …),
  batches OAuth, and registers a pre-commit hook.
- `glia-save-skill` — `rust-embed` + `candle` author a new local skill
  using an OpenAI-compatible LLM (or a template fallback).
- `glia-use` — pulls a community skill from the catalog and registers it
  as `community::name` in the local SurrealKV.
- `glia-sync` — bidirectional LWW sync between local and Hub SurrealDB.
- `glia-auth` — local OAuth helper. When the Hub returns `AUTH_REQUIRED`,
  spins up a localhost callback server (≤ 120 s), opens the browser,
  writes the token, and replays the action.

### `glia-hub` (the trusted plane)

A single linux ELF (`debian:bookworm-slim` runtime) listening on
`0.0.0.0:3000`. Inside, eight services:

- `gateway` — `WS /gateway`, the only AI-exposed tool.
- `glia-sandbox` — UVX child process; on Linux adds seccomp (v1 cross-
  platform target, see `SPEC.md` §B for kernel sandbox backlog).
- `glia-hooks` — pre/post exec hooks (Slack notifiers, log sinks, …).
- `glia-synth` — OpenAI-compatible completion client, output ≤ 150 tokens.
- `glia-cache` — Redis client, target < 2 ms synthesis response.
- `glia-bao` — OpenBao client: Transit encrypt, KV store OAuth refresh
  tokens, response-wrap short-lived per-exec tokens, dynamic leases
  for DB / K8s credentials.
- `glia-action` — same as the CLI's, but server-side.
- `glia-db` — SurrealDB v2.6.5, multi-model: doc + graph + vector + SQL.
  Graph: `tool --needs_auth --> cred`. Vector: `mt_skills` HNSW.

### Storage sidecars

| Service     | Port (host) | What it's for                                |
| ----------- | ----------- | -------------------------------------------- |
| `surrealdb` | `8000`      | Doc + graph + vector. Vectors on MiniLM-6.   |
| `openbao`   | `8201`      | Dynamic DB / K8s creds, KV for SaaS tokens.  |
| `redis`     | `6379`      | Encrypted synthesis cache, ≤ 2 ms lookup.    |

All three are configured via the workspace-level `docker-compose.yml`.
T18's gotchas (rocksdb fails on Windows named volumes; OpenBao needs an
explicit `server -dev` command; OpenBao is IPv4-only) are recorded in
`SPEC.md` §T18.

## The one-tool surface

```text
tool: glia_action(intent: string, params: object)
  → result | AUTH_REQUIRED | AUTH_TIMEOUT
            | RULE_VIOLATION | HUB_UNREACHABLE
```

- `result` — opaque to the agent; payload may include URLs, file paths,
  IDs, but **never** secrets, prompts, embeddings, or chunks.
- `AUTH_REQUIRED` — emitted on a separate WS frame; the CLI opens a
  browser, the user consents, the Hub exchanges the code for a
  15-min access token stored in Redis (encrypted), and the action
  replays. The Hub never sees the user's interactive consent screen.
- `AUTH_TIMEOUT` — emitted after 120 s without consent. Idempotent.
- `RULE_VIOLATION` — the requested action hit `glia-bash` deny-list
  or path-boundary. The CLI surfaces a remediation hint.
- `HUB_UNREACHABLE` — local-only mode. The CLI re-runs the action
  against embedded SurrealKV; if no local rule matches, the action
  returns `NotApplicable`. The CLI never silently fabricates a result.

## Data flow: a `glia action` call

```
agent  ─── glia_action ──▶  glia CLI
                              │
                              ├── local SurrealKV hit?  ──▶ exec locally
                              │
                              └── miss  ──▶  glia-hub gateway
                                              │
                                              ├── resolve tool + skill
                                              ├── chk: tool.needs_auth?
                                              │     yes → AUTH_REQUIRED
                                              │           ↳ callback
                                              │           ↳ exchange
                                              │           ↳ 15min token
                                              │           ↳ resume action
                                              ├── compose prompt
                                              │   (chunked: tool desc, repo ctx, intent)
                                              ├── LLM synth (≤ 150 tok)
                                              ├── glia-sandbox exec
                                              │     env = wrapped token
                                              │     ↳ unwrap directly to OpenBao
                                              │     ↳ inject child process env
                                              │     ↳ purge on exit
                                              └── result ──▶ agent
```

## The secret plane (V18)

This is the bit that's actually novel. Glia's "zero-trust" claim is
implemented in `glia-bao` as a strict response-wrapping protocol:

1. CLI sends `glia_action` (or an `AUTH_REQUIRED` resume).
2. Hub needs a credential for the requested tool.
3. Hub calls `sys/wrapping/wrap` on OpenBao with a single-use token
   whose TTL is 30 s.
4. Hub returns the wrapped token to the CLI.
5. CLI passes it to `glia-sandbox` over the local socket.
6. `glia-sandbox` unwraps **directly against OpenBao** — the Hub is
   not in this round trip. OpenBao returns the plaintext to the
   sandbox only, never to the Hub.
7. The sandbox injects the credential into the child process env,
   execs the tool, and `memzero`s the env on exit (best-effort).
8. The wrapping token is consumed; replay is impossible.

What this guarantees:

- The Hub API process **never** holds plaintext credentials in memory.
- The wrapped token is useless without the OpenBao root token, which
  lives in the compose network — not in the Hub code or memory.
- A compromised Hub can replay, but cannot extract — the unwrap step
  happens in `glia-sandbox` against a one-time lease.

## Why SurrealDB

The single biggest design decision. The comparison:

- **SurrealDB** — multi-model, embeddable, graph+vector+SQL, no external
  embedding API needed. Cost: relatively young.
- **HelixDB** — strong graph, but vector support is still partial and
  no embedded mode.
- **CozoDB** — Datalog is great for graph, but no vector and no HTTP.
- **DuckDB** — vector support is excellent, but no native graph.
- **Qdrant / Weaviate** — vector-only, no graph or doc.
- **PostgreSQL + pgvector + Apache AGE** — proven, but the integration
  cost is high and no embedded mode.

Glia needs all four (doc + graph + vector + SQL) in **one** process
embedded in the CLI. SurrealDB is the only mature option. The
tradeoff (young project) is bounded by SPEC.md §T7's invariants
and §B's "if SurrealDB regresses, fall back to libsql + Qdrant" entry.

## Why `candle` (not ONNX runtime)

- **Pure Rust** — no C++ toolchain, no JS runtime, no external embedding
  API. Air-gap works.
- **`rust-embed`** — the model weights are bundled in the binary.
- **WASM-ready** — same crate compiles to the browser, which keeps
  the door open for a web admin UI.
- **MiniLM-6** — 384-dim, 22 MB, ~100 ms / query on Apple Silicon, fast
  enough to embed every skill at every `glia save-skill`. Reweight
  (`V19`) is `cosine × (1 + 0.1 × edges)`, cap 1.0.

## Sandbox posture

- v1 cross-platform: `glia-bash` allow-list + path boundary.
- Linux: seccomp (deferred — tracked in `SPEC.md` §B).
- macOS: `sandbox-exec` (deferred).
- Windows: AppContainer (deferred).

The `glia-bash` allow-list is the only thing in v1 that runs by default.
The kernel sandbox is opt-in for self-host; the cloud Hub runs it
unconditionally.

## Status

| Layer            | v0.1.0 status                         |
| ---------------- | ------------------------------------- |
| Bridge           | ✅ stdio ⇄ WS                         |
| Local DB         | ✅ embedded SurrealKV                  |
| Hub DB           | ✅ SurrealDB server                    |
| Embedding        | ✅ candle + MiniLM-6                   |
| LLM synth        | ✅ OpenAI-compatible, ≤ 150 tok        |
| Cache            | ✅ Redis, < 2 ms target                |
| OpenBao          | ✅ Transit + KV v2 + response-wrap     |
| AUTH_REQUIRED    | ✅ 120 s WS wait, localhost callback   |
| Sandbox          | 🟡 allow-list only; seccomp in §B      |
| CI               | ✅ ubuntu + windows + macos           |
| Release          | 🟡 cargo-dist config drafted           |
