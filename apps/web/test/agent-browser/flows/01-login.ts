/// Flow 01 — login.
/// Spec binding: SPEC §C5 (Auth.js v5 Credentials provider), v0.4.0 phase 7.

import {
  openLoginPage,
  open,
  snapshotInteractive,
  fill,
  click,
  waitUrl,
  webUrl,
  hubAdminPassword,
  sessionName,
  screenshot,
} from "../helpers/client.js";
import {
  assertEqual,
  assertText,
  assertUrlMatches,
} from "../helpers/assertions.js";

async function main(): Promise<void> {
  const opts = sessionName("01-login");
  const { url } = openLoginPage();
  screenshot("01-login-form", opts);

  const snap = snapshotInteractive(opts);
  // The login form has: [input type=password] placeholder="Hub password"
  // and [button] "Sign in".
  const passwordInput = [...snap.refs.keys()].find((k) =>
    snap.refs.get(k)?.includes(`type="password"`),
  );
  const submitButton = [...snap.refs.keys()].find((k) =>
    snap.refs.get(k)?.includes(`"Sign in"`),
  );
  if (!passwordInput) throw new Error("password input not in snapshot");
  if (!submitButton) throw new Error("Sign in button not in snapshot");

  fill(passwordInput, hubAdminPassword(), opts);
  click(submitButton, opts);

  // Login redirects to /overview on success.
  assertUrlMatches("**/overview", opts);
  assertText("Glia", opts);

  console.log(`OK: ${url} → ${webUrl()}/overview`);
}

main().catch((err) => {
  console.error("FLOW 01 FAILED:", err.message ?? err);
  process.exit(1);
});
