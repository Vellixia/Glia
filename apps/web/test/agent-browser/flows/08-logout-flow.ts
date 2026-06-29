/// Flow 08 — sidebar logout redirects to /login.
///
/// Spec binding: SPEC §C5 (Auth.js v5 session termination), v0.4.0
/// phase 7 (SessionProvider + signOut).

import {
  open,
  click,
  snapshotInteractive,
  webUrl,
  sessionName,
  screenshot,
} from "../helpers/client.js";
import { assertUrlMatches, assertText } from "../helpers/assertions.js";

async function main(): Promise<void> {
  const opts = sessionName("08-logout");
  open(`${webUrl()}/overview`, opts);
  // Wait for the session to load.
  await new Promise((r) => setTimeout(r, 500));
  const snap = snapshotInteractive(opts);
  // Sidebar footer has a logout button (icon-only).
  const logout = [...snap.refs.keys()].find((k) =>
    /LogOut|Logout|sign.?out/i.test(snap.refs.get(k) ?? ""),
  );
  if (!logout) {
    // Try the icon-only button by aria-label.
    const ariaLogout = [...snap.refs.keys()].find((k) =>
      snap.refs.get(k)?.includes("aria-label") && /log.?out/i.test(snap.refs.get(k) ?? ""),
    );
    if (!ariaLogout) throw new Error("logout control not found in sidebar");
    click(ariaLogout, opts);
  } else {
    click(logout, opts);
  }
  assertUrlMatches("**/login", opts);
  assertText("Hub password", opts);
  screenshot("08-logout-redirect", opts);
  console.log("OK: sidebar logout → /login");
}

main().catch((err) => {
  console.error("FLOW 08 FAILED:", err.message ?? err);
  process.exit(1);
});
