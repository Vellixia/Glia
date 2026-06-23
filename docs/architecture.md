# Architecture

Glia splits a single tool call — `glia_action` — across four trust tiers.
Secrets never cross the trust boundary in plaintext.

## Trust tiers

```mermaid
flowchart TB
    subgraph Agent["Agent (untrusted)"]
        MCP["MCP client<br/>calls glia_action(intent, params)<br/>sees no secrets"]
    end

    subgraph Local["Local (semi-trusted)"]
        CLI["glia CLI<br/>embedded SurrealKV<br/>candle embeddings<br/>local skills"]
    end

    subgraph Hub["Hub (trusted)"]
        GATE["WS /gateway<br/>glia_action engine"]
        SDB[("SurrealDB<br/>server-mode")]
        BAO["OpenBao<br/>response-wrapping"]
        SAND["Sandbox<br/>seccomp / sandbox-exec"]
        CACHE["Redis<br/>synthesis cache"]
    end

    subgraph Catalog["Catalog (community)"]
        CC["Vellixia/community-catalog<br/>GitHub markdown"]
    end

    MCP <-->|"WS<br/>intent / result"| CLI
    CLI -->|"local: SurrealKV"| SDB
    CLI <-->|"WS /gateway"| GATE
    GATE --> SDB
    GATE --> BAO
    GATE --> SAND
    GATE --> CACHE
    SAND -->|"1-time wrapped token"| BAO
    CLI -->|"pull via glia use"| CC
    CC -.->|"markdown only,<br/>never trusted"| SAND
```

The Hub exposes a **single** AI-facing tool:

```text
tool: glia_action(intent:string, params:object)
  → result | AUTH_REQUIRED | AUTH_TIMEOUT | RULE_VIOLATION | HUB_UNREACHABLE
```

## Components

| Tier | Component | Role |
|------|-----------|------|
| Agent | MCP client | Calls `glia_action`. Sees no secrets. |
| Local | `glia` CLI | Embedded SurrealKV, candle embeddings, local skills. |
| Hub | `glia-hub` | SurrealDB server, OpenBao, Redis, sandbox dispatcher. |
| Catalog | `community-catalog` (GitHub) | Pulled into private sandbox, never trusted. |

## Data flow: a single action

```mermaid
sequenceDiagram
    participant A as Agent (MCP)
    participant C as glia CLI
    participant H as glia-hub
    participant O as OpenBao
    participant S as Sandbox

    A->>C: glia_action(intent, params)
    C->>C: match locally (SurrealKV + candle)
    alt local match
        C-->>A: result
    else remote
        C->>H: WS /gateway (intent)
        H->>H: tool graph lookup
        alt needs creds
            H->>O: issue 1-time wrapped token
            H-->>C: AUTH_REQUIRED
            C->>O: /callback (localhost, ≤120s)
            O-->>C: 15-min access token
            C-->>H: resume with token
        end
        H->>S: dispatch tool (wrapped token)
        S->>O: unwrap directly
        S->>S: exec, inject env, purge
        S-->>H: result
        H->>H: synthesize (≤150 tokens)
        H-->>C: result
    end
    C-->>A: result
```

## Secret plane (V3, V18)

Three rules govern secrets end to end:

1. **The Hub API never reads plaintext secrets.** OpenBao dynamic leases for
   DB/K8s, KV stores refresh tokens, Cubbyhole holds per-exec tokens.
2. **The Sandbox unwraps directly against OpenBao.** The Hub never sees
   plaintext — it only issues a response-wrapping token.
3. **Glia exchanges refresh tokens for 15-min OAuth access tokens** before
   handing them to the sandbox, so a stolen refresh token has a narrow
   blast radius.

## Storage

| Store | Backend | Holds |
|-------|---------|-------|
| Local DB | SurrealKV (embedded) | Skills, tools, stacks, edges |
| Hub DB | SurrealDB (server, in-memory for dev) | Same schema, Hub-authoritative |
| Cache | Redis | Synthesis responses (≤2 ms hot path) |
| Secrets | OpenBao | Refresh + access tokens, DB creds |
| Catalog | GitHub | Markdown skills, versioned |

## Sandbox (V17)

`glia-bash` enforces:

- **Allow-list** of binaries (`uvx`, `npx`, `cargo`, `git`, …) — anything else
  routes back to the Hub sandbox.
- **Path boundary** — every resolved path must be inside a workspace root;
  `..`, `~`, and absolute paths outside the root are rejected.

v1 is cross-platform. Kernel seccomp (Linux) and `sandbox-exec` (macOS) are
deferred — see `SPEC.md` §B.

## Synthesis (V19)

```text
score = min(1.0, cosine(query, skill) * (1.0 + 0.1 * edges(skill)))
output = top-k skills, ≤ 150 tokens
```

Edges boost — a skill that is structurally connected to the matched intent
outranks an isolated but cosinely-similar one. Synthesis is OpenAI-compatible
(OpenAI, Anthropic, vLLM, Ollama).

## Crate graph

```mermaid
flowchart LR
    CLI[glia-cli] --> ACT[glia-action]
    CLI --> INIT[glia-init]
    CLI --> SYN[glia-sync]
    CLI --> BR[glia-bridge]
    CLI --> AUT[glia-author]
    CLI --> CAT[glia-catalog]
    ACT --> DB[glia-db]
    ACT --> EMB[glia-embed]
    ACT --> SYT[glia-synth]
    CAT --> DB
    CAT --> EMB
    AUT --> DB
    AUT --> EMB
    INIT --> CTX[glia-context]
    INIT --> AUTH[glia-auth]
    INIT --> HK[glia-hooks]
    SYT --> CHK[glia-chunk]
    HUB[glia-hub] --> SDB[glia-sandbox]
    HUB --> HK
    HUB --> DB
    HUB --> CA[glia-cache]
    HUB --> EMB
    HUB --> SYT
    HUB --> BAO[glia-bao]
    BAO --> OP[(openbao)]
    CTX --> DB
    SYN --> DB
```

20 crates, no `unsafe` (workspace lint: `unsafe_code = "forbid"`).
