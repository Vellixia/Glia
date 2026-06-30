# glia-test

SPEC-driven test infrastructure for Glia.

Two complementary layers:

| Layer | Tool | Coverage | Pre-reqs |
|---|---|---|---|
| `crates/hub-flow` (Rust) | `cargo test -p glia-test-hub-flow` | 18 tests: Hub auth flow, catalog, OAuth secrets, config broadcast | Rust toolchain + `cargo` |
| `crates/cli-flow` (Rust) | `cargo test -p glia-test-cli-flow` | 17 tests: CLI exit codes, subcommand shapes, WS protocol | Same + `cargo build -p glia-cli` |
| `crates/../glia-e2e` | `cargo test -p glia-e2e` | Live Docker/HelixDB/OpenBao/Redis tests | Docker |
| `apps/web/test/agent-browser/` | `yarn test:live` | 9 live UI flows | Docker + agent-browser |

Every test above is tagged with the SPEC §V invariant it locks in.
Search for `SPEC §V` in any test file to find the upstream invariant.

## Hub-flow tests (mock data, deterministic)

```bash
cargo test -p glia-test-hub-flow
```

Tests run against the in-process stubs (`StubOpenBao`,
`StubCatalog`) and the live `glia_hub::hub_router` mounted on an
ephemeral port. No network. No Docker. Sub-second end-to-end.

## CLI-flow tests (real binary + mocked Hub)

```bash
cargo build -p glia-cli
cargo test -p glia-test-cli-flow
```

Tests spawn the `glia` binary with `GLIA_HUB_URL=http://127.0.0.1:1`
to deterministically trigger `HUB_UNREACHABLE`. Tests that need a
real Hub spin one up via `glia_hub::hub_router` on an ephemeral port.

## Full Docker stack (live integration)

For tests that need real HelixDB, OpenBao, Redis — use the existing
`glia-e2e` crate:

```bash
docker compose -f docker-compose.yml up -d helixdb openbao redis
cargo test -p glia-e2e -- --ignored   # tests are `#[ignore]` by default
```

## Adding a new test

1. Find the SPEC §V invariant you want to lock in (`SPEC.md`).
2. Add a test file under `crates/hub-flow/tests/` (Hub-level) or
   `crates/cli-flow/tests/` (CLI-level).
3. Add a top-line comment quoting the SPEC §V line.
4. Use the existing `Stub*` types when possible — no Docker, no
   network.
5. Run `cargo fmt --all && cargo clippy -p glia-test-hub-flow -p
   glia-test-cli-flow -- -D warnings` before pushing.

## Tools

- `tools/hashgen/` — one-shot binary that prints an Argon2id hash for
  the admin password. Used to seed `.env.example`.
  ```bash
  cargo run -p glia-test-hashgen -- my-password
  ```

## compose.test.yml

Adds `GLIA_JWT_SECRET` + `GLIA_ADMIN_HASH` to the Hub service so the
Hub boots. Use as:

```bash
cp .env.example .env       # then edit
bash glia-test/setup.sh   # compose up + poll /healthz
bash glia-test/teardown.sh
```
