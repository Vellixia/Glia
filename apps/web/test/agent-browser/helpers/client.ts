/// Guarded wrapper around the `agent-browser` CLI. Every function
/// shells out (no in-process library binding) so the wrapper is
/// thin and matches the source of truth (agent-browser 0.26.x).
///
/// Hard Boundry (assert at construction):
///   - CI / GITHUB_ACTIONS / NODE_ENV=production → throw
///   - HUB_URL must be localhost / 127.0.0.1 → throw
///   - WEB_URL must be localhost / 127.0.0.1 → throw

import { spawnSync } from "node:child_process";

export interface ClientOptions {
  /** Per-flow session name; reuses cookies if the agent-browser CLI supports. */
  session?: string;
  /** Run id for artifact tagging; defaults to timestamp. */
  runId?: string;
}

export interface SnapshotResult {
  /** Parsed interactive-ref map keyed by `@eN` like "@e3" → "@e3 [button type=submit] \"Continue\"". */
  refs: Map<string, string>;
  /** Raw stdout from agent-browser snapshot. */
  raw: string;
}

export interface OpenResult {
  /** Final URL after redirects. */
  url: string;
  /** Page title. */
  title: string;
}

/** Read agent-browser env file (.env) into a process.env-shaped object. */
function readEnvFile(): Record<string, string> {
  const fs = require("node:fs") as typeof import("node:fs");
  const path = require("node:path") as typeof import("node:path");
  const envPath = path.join(process.cwd(), ".env");
  if (!fs.existsSync(envPath)) return {};
  const out: Record<string, string> = {};
  const text = fs.readFileSync(envPath, "utf8");
  for (const line of text.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const eq = trimmed.indexOf("=");
    if (eq < 0) continue;
    const k = trimmed.slice(0, eq).trim();
    const v = trimmed.slice(eq + 1).trim().replace(/^["']|["']$/g, "");
    out[k] = v;
  }
  return out;
}

function ensureHardBoundry(): void {
  const envFile = readEnvFile();
  const merged = { ...process.env, ...envFile };

  if (
    merged.CI === "true" ||
    merged.GITHUB_ACTIONS === "true" ||
    merged.NODE_ENV === "production"
  ) {
    throw new Error(
      "agent-browser flows are local-only by design (refusing CI=true / GITHUB_ACTIONS=true / NODE_ENV=production)",
    );
  }
  const hub = merged.HUB_URL ?? "http://127.0.0.1:3000";
  if (!/^https?:\/\/(localhost|127\.0\.0\.1)(:\d+)?(\/|$)/.test(hub)) {
    throw new Error(`Refusing to run agent-browser against non-localhost HUB_URL: ${hub}`);
  }
  const web = merged.WEB_URL ?? "http://127.0.0.1:3001";
  if (!/^https?:\/\/(localhost|127\.0\.0\.1)(:\d+)?(\/|$)/.test(web)) {
    throw new Error(`Refusing to run agent-browser against non-localhost WEB_URL: ${web}`);
  }
}

ensureHardBoundry();

function envVal(key: string): string | undefined {
  const envFile = readEnvFile();
  return envFile[key] ?? process.env[key];
}

function buildArgs(flowArgs: string[], opts?: ClientOptions): string[] {
  const args = [...flowArgs];
  if (opts?.session) {
    args.unshift("--session", opts.session);
  }
  return args;
}

function runAgentBrowser(args: string[], opts?: ClientOptions): string {
  const result = spawnSync("agent-browser", buildArgs(args, opts), {
    encoding: "utf8",
    env: {
      ...process.env,
      NO_COLOR: "1",
      CI: undefined,
    } as NodeJS.ProcessEnv,
  });
  if (result.status !== 0) {
    throw new Error(
      `agent-browser ${args.join(" ")} failed: ${result.stderr || result.stdout}`,
    );
  }
  return (result.stdout ?? "").trim();
}

/** Open a URL. Returns the final URL (after any redirects) and the page title. */
export function open(url: string, opts?: ClientOptions): OpenResult {
  const out = runAgentBrowser(["open", url], opts);
  // `agent-browser open` prints the resolved URL on stdout; we treat any
  // string starting with "URL:" as evidence it worked. Be defensive.
  const urlLine = out
    .split(/\r?\n/)
    .find((l) => l.startsWith("URL:"))
    ?.replace(/^URL:\s*/, "")
    .trim();
  const titleLine = out
    .split(/\r?\n/)
    .find((l) => l.startsWith("Title:"))
    ?.replace(/^Title:\s*/, "")
    .trim();
  return {
    url: urlLine ?? url,
    title: titleLine ?? "",
  };
}

/** Interactive-only snapshot. Returns a Map of @eN → ref descriptor. */
export function snapshotInteractive(opts?: ClientOptions): SnapshotResult {
  const out = runAgentBrowser(["snapshot", "-i"], opts);
  const refs = new Map<string, string>();
  for (const line of out.split(/\r?\n/)) {
    const m = /^(@e\d+)\s+\[(.+?)\]/.exec(line);
    if (m && m[1]) {
      refs.set(m[1], line);
    }
  }
  return { refs, raw: out };
}

/** Click an interactive ref. */
export function click(ref: string, opts?: ClientOptions): void {
  runAgentBrowser(["click", ref], opts);
}

/** Fill an input ref with text (clears first). */
export function fill(ref: string, value: string, opts?: ClientOptions): void {
  runAgentBrowser(["fill", ref, value], opts);
}

/** Wait for a URL pattern (glob, e.g. '**\/overview'). */
export function waitUrl(pattern: string, opts?: ClientOptions): void {
  runAgentBrowser(["wait", "--url", pattern], opts);
}

/** Wait until the given text appears on the page. */
export function waitText(text: string, opts?: ClientOptions): void {
  runAgentBrowser(["wait", "--text", text], opts);
}

/** Take a screenshot and write to ./screenshots/<flow-name>.png. */
export function screenshot(name: string, opts?: ClientOptions): string {
  const fs = require("node:fs") as typeof import("node:fs");
  const path = require("node:path") as typeof import("node:path");
  const dir = path.join(process.cwd(), "screenshots");
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `${name}.png`);
  runAgentBrowser(["screenshot", file], opts);
  return file;
}

/** Logout helper — runs `agent-browser open <web>/login` after signOut. */
export function openLoginPage(): OpenResult {
  const web = envVal("WEB_URL") ?? "http://127.0.0.1:3001";
  return open(`${web}/login`);
}

export function webUrl(): string {
  return envVal("WEB_URL") ?? "http://127.0.0.1:3001";
}

export function hubUrl(): string {
  return envVal("HUB_URL") ?? "http://127.0.0.1:3000";
}

export function hubAdminPassword(): string {
  return envVal("HUB_ADMIN_PASSWORD") ?? "glia";
}

/** Optional per-flow session name; reuse across flows for shared state. */
export function sessionName(flowName: string): ClientOptions {
  return { session: `glia-${flowName}` };
}
