# CLI

The `glia` binary is the agent-facing surface. It runs locally, holds no
secrets of its own, and talks to the Hub over a single WebSocket.

## Build

```bash
cargo build --release -p glia-cli
./target/release/glia --help
```

Requires Rust 1.89+ and a C/C++ toolchain (for `rocksdb` on the Hub side;
the CLI itself uses `HelixDB`, which is pure Rust).

## Subcommands

```text
$ glia --help
Cognitive control plane for AI agents

Usage: glia <COMMAND>

Commands:
  bridge      stdio <-> WebSocket translator. Connects to the Glia Hub /gateway
  sync        Bidirectional sync between local and Hub HelixDB
  init        Scan repo, detect stacks, batch auth
  action      Unified tool discover + skill fetch + exec
  save-skill  Author and register a new local skill
  use         Pull a community tool from the catalog and register it
```

### `glia bridge`

```bash
glia bridge                              # default: ws://127.0.0.1:6969/gateway
glia bridge --hub ws://hub:3000/gateway
```

Reads JSON-RPC 2.0 from stdin, frames it as `glia_action` over WS, writes
results to stdout. Use this from any agent that already speaks stdio JSON.

### `glia init`

```bash
glia init                               # scan current dir
glia init --path ./apps/web
```

1. Walks the repo (skipping `node_modules`, `target`, `.git`).
2. Detects stacks: `nextjs`, `supabase`, `stripe`, `rust-axum`, …
3. Batches OAuth: any detected stack that needs a credential triggers
   `AUTH_REQUIRED` against the Hub.
4. Installs hooks (`.glia/hooks/pre-commit.sh`).

### `glia action`

```bash
glia action --intent "create a Linear issue for the login bug"
```

The one call. The CLI:

1. Embeds the intent locally (`candle`).
2. Looks up matching skills + tools in local HelixDB (V1: ⊥ Hub network).
3. If a tool needs creds, opens `AUTH_REQUIRED` flow against the Hub.
4. Hands off to Hub sandbox, gets result, synthesizes (≤150 tokens).
5. Returns JSON.

Output:

```json
{
  "intent":   { "query": "create a Linear issue for the login bug", "stack": null },
  "kind":     "Local" | "Remote" | "Mixed",
  "skills":   ["use-linear-api", "create-issue"],
  "tools":    ["linear"],
  "missing":  [],
  "outcome":  "Ok" | "AuthRequired" | "AuthTimeout" | "RuleViolation"
              | "HubUnreachable" | "NotApplicable",
  "finished_at": "2026-06-23T03:39:55.246Z"
}
```

### `glia save-skill`

```bash
glia save-skill --name use-zustand --content "Use zustand for React state."
glia save-skill --interactive       # opens $EDITOR
```

Calls an OpenAI-compatible LLM (configured via `OPENAI_API_KEY`,
`ANTHROPIC_API_KEY`, or local `OLLAMA_HOST`) to author the markdown,
embeds it locally, and stores it under `local::use-zustand` in HelixDB.
Falls back to a template if no LLM is reachable.

### `glia use <name>`

```bash
glia use linear                       # pulls from community-catalog
```

Fetches the markdown skill from `Vellixia/community-catalog`, runs it
in a private sandbox to register, stores it under `community::linear` in
local HelixDB, and syncs to the Hub on the next `glia sync`.

### `glia sync`

```bash
glia sync                              # bidirectional LWW
glia sync --push-only
glia sync --pull-only
```

Bidirectional, Hub-authoritative last-writer-wins. Conflict resolution is
`updated_at` lex order (RFC-3339). If the Hub is unreachable, the CLI
queues writes locally and replays on the next successful sync. Local-only
skills (`local::*`) never sync to the Hub.

## Configuration

| Env var | Default | Purpose |
|---------|---------|---------|
| `GLIA_HUB_URL` | `ws://127.0.0.1:6969/gateway` | Hub WebSocket |
| `GLIA_SYNC_URL` | `ws://127.0.0.1:8000` | HelixDB sync |
| `GLIA_CATALOG` | `Vellixia/community-catalog/main` | Catalog repo + ref |
| `GLIA_AUTH_TIMEOUT` | `120` | seconds |
| `OPENAI_API_KEY` | — | Synthesis LLM (any OpenAI-compat) |
| `ANTHROPIC_API_KEY` | — | Synthesis LLM |
| `OLLAMA_HOST` | — | Local synthesis LLM |

## State locations

| Path | Holds |
|------|-------|
| `~/.glia/local.db/` | Embedded HelixDB |
| `~/.glia/skills/` | Authored skill markdown |
| `~/.glia/hooks/` | Per-repo hooks |
| `~/.glia/whitelist.toml` | `glia-bash` allow-list |
