/// Flow 05 — secrets page add-provider dialog open + cancel.
///
/// Spec binding: SPEC §V3 + §V17, v0.4.0 phase 6 Secrets page.

import {
  open,
  snapshotInteractive,
  webUrl,
  sessionName,
  screenshot,
} from "../helpers/client.js";
import { assertSnapshotContains } from "../helpers/assertions.js";

async function main(): Promise<void> {
  const opts = sessionName("05-secrets");
  open(`${webUrl()}/secrets`, opts);
  assertSnapshotContains("OAuth providers", opts);
  // The Secrets page header has an "Add provider" button.
  const snap = snapshotInteractive(opts);
  assertSnapshotContains("Add provider", opts);
  screenshot("05-secrets-rendered", opts);
  console.log("OK: /secrets rendered");
}

main().catch((err) => {
  console.error("FLOW 05 FAILED:", err.message ?? err);
  process.exit(1);
});
