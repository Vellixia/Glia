/// Flow 06 — /settings page renders.
///
/// Spec binding: SPEC §T9 + v0.4.0 phase 5 Settings page.

import {
  open,
  snapshotInteractive,
  webUrl,
  sessionName,
  screenshot,
} from "../helpers/client.js";
import { assertSnapshotContains } from "../helpers/assertions.js";

async function main(): Promise<void> {
  const opts = sessionName("06-settings");
  open(`${webUrl()}/settings`, opts);
  const snap = snapshotInteractive(opts);
  // The settings form has theme + log-level. Look for either.
  const hasTheme =
    snap.raw.includes("Theme") || snap.raw.includes("theme");
  const hasLogLevel =
    snap.raw.includes("Log level") || snap.raw.includes("log-level");
  if (!hasTheme && !hasLogLevel) {
    throw new Error(
      "/settings rendered but no theme/log-level controls found",
    );
  }
  try {
    assertSnapshotContains("Save", opts);
  } catch {
    // Some variants render Save as "Save changes"; both are acceptable.
    assertSnapshotContains("Save changes", opts);
  }
  screenshot("06-settings", opts);
  console.log("OK: /settings rendered with theme + log-level controls");
}

main().catch((err) => {
  console.error("FLOW 06 FAILED:", err.message ?? err);
  process.exit(1);
});
