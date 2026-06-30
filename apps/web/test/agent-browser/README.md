# Live UI flow tests for the Glia dashboard

Hard Boundry enforced by `helpers/client.ts`:

- **Refuses to run if `CI=true`, `GITHUB_ACTIONS=true`, or `NODE_ENV=production`**.
- **Refuses to run if `HUB_URL` or `WEB_URL` point anywhere other than
  `localhost` / `127.0.0.1`.**

Each flow is a standalone `tsx`-runnable file. The fixture setup lives
under `flows/01-login.ts` → `flows/09-401-recovery.ts`. Each maps to a
specific SPEC §V invariant (see comments at the top of each file).

## Pre-reqs

| | |
|---|---|
| 1. Docker | `glia-test/setup.sh` brings up `glia-hub` + `glia-web` + dependencies |
| 2. Node 18+ | runs the helpers via `tsx` |
| 3. agent-browser 0.26+ | the browser CLI we wrap; `npm i -g agent-browser && agent-browser install` |

`agent-browser install` once per machine; the helpers spawn the CLI
per invocation.

## Run

```bash
# 1. Copy and edit the env
cp .env.example .env

# 2. Bring up the Glia stack (Hub + Web)
bash ../../glia-test/setup.sh

# 3. Run the flows
cd apps/web/test/agent-browser
yarn test:live:setup       # polls /healthz + web /
yarn test:live             # 01 → 09 in sequence
yarn test:live:stop        # closes sessions + archives screenshots
```

Run a single flow:

```bash
yarn test:live:03          # only skills-toggle flow
```

Debug a flow:

```bash
yarn test:live:debug       # opens the first flow's URL, keeps the browser open
```

## What's here

```
test/agent-browser/
├── README.md                  # this file
├── package.json               # tsx + scripts
├── tsconfig.json
├── .env.example
├── .gitignore                 # /artifacts, /state, /screenshots, /node_modules
├── setup.ts                   # polls Hub + Web reachability
├── teardown.ts                # closes sessions + archives screenshots
├── helpers/
│   ├── client.ts              # agent-browser wrapper + hard-boundry guards
│   └── assertions.ts          # assertVisible, assertText, assertUrlMatches, …
└── flows/
    ├── 01-login.ts                       # Auth.js credentials login → /overview
    ├── 02-overview-kpis.ts               # /overview renders + SSE indicator
    ├── 03-skills-toggle.ts               # /skills toggle UI structural
    ├── 04-catalog-search.ts              # /catalog search + stack filter
    ├── 05-secrets-add-provider.ts        # /secrets providers list rendered
    ├── 06-settings-update.ts             # /settings theme/log-level
    ├── 07-sse-realtime.ts                # 2-tab SSE plumbing structural
    ├── 08-logout-flow.ts                 # sidebar logout → /login
    └── 09-401-recovery.ts                # Hub unauth returns 401 (interceptor wired)
```

## Cross-checks

Deep Rust tests for the same flows live in `glia-test/crates/`:

| Flow | Rust coverage |
|---|---|
| 01-login | `glia-test/crates/hub-flow/tests/login_journey.rs` |
| 03-skills-toggle | `glia-test/crates/hub-flow/tests/config_propagation.rs` (`SkillToggled` invalidation) |
| 04-catalog-search | `glia-test/crates/hub-flow/tests/catalog_install.rs` |
| 05-secrets-add-provider | `glia-test/crates/hub-flow/tests/secrets_oauth.rs` |
| 06-settings-update | `glia-test/crates/hub-flow/tests/config_propagation.rs` (`ConfigChanged`) |
| 07-sse-realtime | `glia-test/crates/hub-flow/tests/config_propagation.rs` (broadcast channel) |
| 08-logout-flow | `crates/glia-bridge` (auth wiring) + Rust tests in `crates/glia-e2e` |
| 09-401-recovery | `crates/glia-hub-api/src/auth.rs` + Rust tests in `glia-e2e` |

The TS flows are intentionally **structural** (verify the page
renders, the indicator appears). The Rust tests are **behavioral**
(verify state propagation). Together they lock in the spec from
both ends.
