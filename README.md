<div align="center">

<img src="docs/assets/glia-logo.svg" alt="Glia" width="320">

**Cognitive control plane for AI agents.** One `glia action` call replaces
50 MCP installs. Local-first, Graph-RAG, zero-trust exec, air-gappable.

[![CI](https://github.com/Vellixia/Glia/actions/workflows/ci.yml/badge.svg)](https://github.com/Vellixia/Glia/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Rust 1.89+](https://img.shields.io/badge/rust-1.89%2B-orange.svg)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-167%20%E2%9C%93-brightgreen.svg)](#development)
[![Air-gap](https://img.shields.io/badge/air--gap-ready-success.svg)](#features)

</div>

---

## Why

Every agent today ships its own tool registry, OAuth dance, secret store,
per-vendor MCP. That's 50 installs, 50 trust boundaries, 50 places for a
credential to leak.

Glia collapses all of it into a single call:

```bash
glia action --intent "create a Linear issue for the login bug"
```

→ the agent gets a result, `AUTH_REQUIRED`, `AUTH_TIMEOUT`,
`RULE_VIOLATION`, or `HUB_UNREACHABLE` — and nothing else.

## Features

- **One tool, every intent** — intent → tool → credentials → exec → synthesis.
- **Local-first, zero-trust** — embedded HelixDB, OpenBao for secrets,
  response-wrapping so the Hub never sees plaintext. Offline? CLI keeps
  working from local state.
- **Air-gappable** — `candle` embeddings, model bundled via `rust-embed`,
  no external API, no JS runtime, no C++ toolchain at runtime.
- **Stack-aware** — auto-detect Next.js, Supabase, Stripe, … on `glia init`,
  pull only the skills that match.
- **Self-host in 2 min** — `docker compose up -d` brings the full stack.

## Quickstart

### Self-host the Hub

```bash
git clone https://github.com/Vellixia/Glia.git
cd Glia
docker compose up -d
docker compose ps
```

| Service    | Port | Healthcheck |
|------------|------|-------------|
| `glia-hub` | 3000 | up immediately |
| HelixDB  | 8000 | — |
| OpenBao    | 8201 | `wget /v1/sys/health` |
| Redis      | 6379 | `redis-cli ping` |

### Build the CLI

```bash
cargo build --release -p glia-cli
./target/release/glia --help
```

### First action

```bash
./target/release/glia action --intent "hello"
```

`NotApplicable` means the intent matched no rule locally — that's the
spec. Add skills via `glia save-skill` or `glia use <name>` and the same
intent will route to a tool.

## Docs

| Doc | What's in it |
|-----|--------------|
| [architecture.md](docs/architecture.md) | Trust tiers, data flow, secret plane, crate graph |
| [security.md](docs/security.md) | Threat model, invariants, sandbox, reporting |
| [cli.md](docs/cli.md) | All subcommands, config, state locations |
| [hub.md](docs/hub.md) | Self-host, API, `AUTH_REQUIRED` flow, ops |
| [catalog.md](docs/catalog.md) | Community skills: anatomy, lifecycle, trust |
| [development.md](docs/development.md) | Workspace, CI, release, style, common issues |

## Contributing

PRs welcome. The flow:

1. Fork.
2. `cargo test --workspace --all-targets -- --test-threads=1` must pass.
3. `cargo clippy --workspace --all-targets -- -D warnings` must pass.
4. `cargo fmt --all -- --check` must pass.
5. Open a PR with a clear description and a test for new behavior.

For community **skills** (not core code), contribute to
[`Vellixia/community-catalog`](https://github.com/Vellixia/community-catalog).

## License

[Apache-2.0](LICENSE). Copyright 2026 The Glia Authors.
