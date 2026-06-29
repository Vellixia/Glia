# Glia Web Dashboard

Next.js 16 + React 19 dashboard for the Glia Hub. Signs in via Auth.js
v5, talks to the Hub GraphQL endpoint via a same-origin proxy (JWT
never reaches the browser), and subscribes to `/api/events` for
real-time SSE updates.

## Quickstart

```bash
# From the repo root — installs everything in the workspace
yarn install

# Start the Hub (Rust) on :3000
yarn dev:hub

# Start this dashboard on :3000
yarn dev
```

Then open <http://localhost:3000>. Use `/login` with the admin password
configured via `GLIA_ADMIN_HASH` on the Hub.

## Architecture

- **Auth.js v5** — httpOnly cookie session; Credentials provider calls
  Hub `/graphql` `login` mutation server-side and stores the issued JWT
  in the session.
- **GraphQL proxy** — `app/api/graphql/route.ts` reads `session.hubToken`,
  forwards requests to `HUB_URL/graphql` with `Authorization: Bearer`,
  passes through responses.
- **SSE proxy** — `app/api/events/route.ts` does the same for the
  `/api/events` stream. The browser's `EventSource` cannot set custom
  headers, so the same-origin proxy attaches the JWT server-side.
- **Event bus** — `services/event-bus.ts` opens the `EventSource`,
  listens for named events, and calls `queryClient.invalidateQueries()`
  on the affected query-key prefixes. State (`connecting`/`connected`/
  `reconnecting`/`disconnected`) in `stores/event-store.ts` (Zustand).
- **Connection indicator** — `components/connection-indicator.tsx`
  renders in the sidebar footer.

## Scripts

| Script        | What it does                             |
|---------------|------------------------------------------|
| `yarn dev`    | Start Next.js dev server on :3000        |
| `yarn build`  | Production build                         |
| `yarn start`  | Run production build                     |
| `yarn lint`   | Lint (Next.js)                           |
| `yarn typecheck` | `tsc --noEmit` (zero errors required) |
| `yarn test`   | Vitest one-shot (53 tests)               |
| `yarn test:watch` | Vitest watch mode                    |

## Tests

53 unit tests across 6 files:
- `stores/event-store.test.ts` (6) — Zustand connection-state lifecycle
- `stores/ui-store.test.ts` (5)
- `stores/log-store.test.ts` (9)
- `lib/schemas/settings.test.ts` (9)
- `lib/query-keys.test.ts` (18)
- `components/error-state.test.tsx` (6)

## Routes

```
/login                 — Auth.js Credentials sign-in
/overview              — KPI grid (active agents, skills, etc.)
/skills                — DataTable + optimistic toggle
/catalog               — Grid/list, install dialog
/agents                — Card grid
/secrets               — OAuth providers + credentials
/sync                  — Adaptive polling
/logs                  — Live event stream (own SSE proxy)
/settings              — react-hook-form + zod
/api/graphql           — Proxy → HUB_URL/graphql
/api/events            — Proxy → HUB_URL/api/events (SSE)
/api/auth/[...nextauth] — Auth.js handlers
```
