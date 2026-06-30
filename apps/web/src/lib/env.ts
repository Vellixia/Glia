/**
 * Centralized runtime environment helpers for the Glia web dashboard.
 *
 * The canonical switch is `NODE_ENV` (matches Next.js / Node convention):
 *   - `production`  — live system behind a reverse proxy.
 *   - `development` — local dev (`next dev`).
 *   - `test`        — live UI tests (agent-browser, Playwright, MCP).
 *
 * `GLIA_APP_ENV` is accepted as an alias (preferred for compose config
 * because it doesn't conflict with Next.js's NODE_ENV interpretation).
 * When both are set, `NODE_ENV` wins — Next.js's own wiring relies on it.
 *
 * Defaults to "development" when neither is set.
 */

export type AppEnv = "production" | "development" | "test";

const VALID_ENVS: readonly AppEnv[] = ["production", "development", "test"];

function parseAppEnv(): AppEnv {
  // Prefer NODE_ENV (Next.js convention). Fall back to GLIA_APP_ENV, then development.
  const raw = (process.env.NODE_ENV ?? process.env.GLIA_APP_ENV ?? "development") as string;
  if (raw === "production" || raw === "prod") return "production";
  if (raw === "test") return "test";
  if (raw === "development" || raw === "dev" || raw === "") return "development";
  console.warn(`[env] unknown NODE_ENV/GLIA_APP_ENV="${raw}", defaulting to development`);
  return "development";
}

/** Resolved at module-load. Safe to read from anywhere (client + server). */
export const APP_ENV: AppEnv = parseAppEnv();

export const isProduction = APP_ENV === "production";
export const isTest      = APP_ENV === "test";
export const isDev       = APP_ENV === "development";

/**
 * Throws if the current environment is not in `allowed`.
 */
export function assertEnv(allowed: readonly AppEnv[], label = "test"): void {
  if (!allowed.includes(APP_ENV)) {
    throw new Error(
      `[env] ${label} requires ${allowed.join("|")} but APP_ENV="${APP_ENV}"`
    );
  }
}

/**
 * Hard Boundry guard. Live-UI tests must:
 *   - NOT be production
 *   - BE in test env (NODE_ENV=test OR GLIA_APP_ENV=test)
 *   - NOT run in CI (CI / GITHUB_ACTIONS unset)
 *   - WEB_URL / HUB_URL must be loopback or compose-internal
 */
export function assertHardBoundry(opts: {
  webUrl?: string;
  hubUrl?: string;
  label?: string;
} = {}): void {
  const label = opts.label ?? "live UI test";
  const issues: string[] = [];

  if (APP_ENV === "production") {
    issues.push("APP_ENV=production");
  } else if (APP_ENV !== "test") {
    issues.push(`APP_ENV=${APP_ENV} (must be "test" — set NODE_ENV=test or GLIA_APP_ENV=test)`);
  }
  if (process.env.CI) issues.push("CI is set");
  if (process.env.GITHUB_ACTIONS) issues.push("GITHUB_ACTIONS is set");

  const webUrl = opts.webUrl ?? process.env.WEB_URL ?? "";
  if (webUrl && !/^https?:\/\/(127\.0\.0\.1|localhost)(:\d+)?\//.test(webUrl)) {
    issues.push(`WEB_URL=${webUrl} is not loopback`);
  }
  const hubUrl = opts.hubUrl ?? process.env.HUB_URL ?? "";
  if (hubUrl && !/^https?:\/\/(127\.0\.0\.1|localhost|glia-hub)(:\d+)?/.test(hubUrl)) {
    issues.push(`HUB_URL=${hubUrl} is not loopback/compose-internal`);
  }

  if (issues.length > 0) {
    throw new Error(
      `[env] ${label} refuses to run: ${issues.join("; ")}. ` +
        `Live-UI tests must run against a loopback-only Hub + Web.`
    );
  }
}

/**
 * Hard-stop if the current env is not test. Used at the top of test
 * runners so a forgotten NODE_ENV can't accidentally fire off a
 * destructive flow against a dev or deployed system.
 */
export function requireTestEnv(label: string): void {
  if (APP_ENV !== "test") {
    throw new Error(
      `[env] ${label} requires NODE_ENV=test (currently "${APP_ENV}"). ` +
        `Set NODE_ENV=test or GLIA_APP_ENV=test in the test runner or compose overlay.`
    );
  }
}
