/// setup.ts — boot the test stack before running flows.
///
/// 1. Read .env (optional) for WEB_URL/HUB_URL/HUB_ADMIN_PASSWORD.
/// 2. Poll http://HUB_URL/healthz until 200 (Hub up).
/// 3. Poll http://WEB_URL/ until 200 (web dev server up).
/// 4. Print the resolved env so flows know what's expected.
///
/// Idempotent: safe to run before every test:live invocation. Exits
/// 0 once both endpoints respond, exit 1 if timed out.

import { setTimeout as wait } from "node:timers/promises";
import * as client from "./helpers/client.js";

const HUB = client.hubUrl();
const WEB = client.webUrl();
const TIMEOUT_MS = 90_000;

async function poll(
  label: string,
  url: string,
  ok: (resp: Response) => boolean,
): Promise<void> {
  const deadline = Date.now() + TIMEOUT_MS;
  let last = "?";
  while (Date.now() < deadline) {
    try {
      const resp = await fetch(url, { method: "GET" });
      last = `${resp.status}`;
      if (ok(resp)) {
        console.log(`✓ ${label}: ${url} (HTTP ${resp.status})`);
        return;
      }
    } catch (e) {
      last = (e as Error).message;
    }
    await wait(500);
  }
  throw new Error(`${label} (${url}) did not become reachable in ${TIMEOUT_MS / 1000}s; last=${last}`);
}

async function main(): Promise<void> {
  console.log(`agent-browser setup — HUB=${HUB} WEB=${WEB}`);
  await poll("hub /healthz", `${HUB}/healthz`, (r) => r.status === 200);
  await poll("web /", `${WEB}/`, (r) => r.status === 200 || r.status === 307);
  console.log("agent-browser setup complete.");
}

main().catch((err) => {
  console.error(err.message ?? err);
  process.exit(1);
});
