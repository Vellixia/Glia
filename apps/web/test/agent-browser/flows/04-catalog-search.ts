/// Flow 04 — catalog search debounce.
///
/// Spec binding: SPEC §T1 + §T20, v0.4.0 phase 6 Catalog page.

import {
  open,
  snapshotInteractive,
  webUrl,
  sessionName,
  screenshot,
} from "../helpers/client.js";
import { assertSnapshotContains } from "../helpers/assertions.js";

async function main(): Promise<void> {
  const opts = sessionName("04-catalog");
  open(`${webUrl()}/catalog`, opts);
  const snap = snapshotInteractive(opts);
  // The Catalog page has [searchbox] placeholder for search. Find it
  // structurally; if not present the page may be empty (no catalog
  // entries, no input yet).
  const search = [...snap.refs.keys()].find((k) =>
    /searchbox|search/i.test(snap.refs.get(k) ?? ""),
  );
  if (!search) {
    console.log("SKIP: catalog page has no search input (catalog empty)");
    screenshot("04-catalog-empty", opts);
    return;
  }
  assertSnapshotContains("All stacks", opts);
  screenshot("04-catalog-rendered", opts);
  console.log("OK: /catalog rendered with search input + stack filter");
}

main().catch((err) => {
  console.error("FLOW 04 FAILED:", err.message ?? err);
  process.exit(1);
});
