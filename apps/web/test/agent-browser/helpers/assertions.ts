/**
 * Tiny assertion library for agent-browser flows. Each function
 * throws on failure with a clear message + the offending state.
 */

import * as client from "./client.js";

export function assertEqual<T>(actual: T, expected: T, label: string): void {
  if (actual !== expected) {
    throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

export function assertTrue(cond: boolean, label: string): void {
  if (!cond) throw new Error(`${label}: condition false`);
}

export function assertVisible(ref: string, opts?: client.ClientOptions): void {
  const snap = client.snapshotInteractive(opts);
  if (!snap.refs.has(ref)) {
    throw new Error(
      `assertVisible(${ref}): ref not in snapshot\nrefs found: ${[...snap.refs.keys()].join(", ")}`,
    );
  }
}

export function assertText(text: string, opts?: client.ClientOptions): true {
  const snap = client.snapshotInteractive(opts);
  const haystack = snap.raw;
  if (!haystack.includes(text)) {
    throw new Error(
      `assertText(${JSON.stringify(text)}): text not in snapshot\nsnapshot: ${haystack}`,
    );
  }
  return true;
}

export function assertUrlMatches(pattern: string, opts?: client.ClientOptions): void {
  // Wait up to ~5s for the URL to settle, then assert.
  client.waitUrl(pattern, opts);
  // Re-fetch current URL via a snapshot (URL: line is in the header).
  const snap = client.snapshotInteractive(opts);
  const urlLine = snap.raw
    .split(/\r?\n/)
    .find((l) => l.startsWith("URL:"))
    ?.replace(/^URL:\s*/, "");
  if (!urlLine) {
    throw new Error(`assertUrlMatches: no URL line in snapshot`);
  }
  // Convert the glob `**/overview` to a regex.
  const regexBody =
    "^" +
    pattern
      .split(/(\*\*|\*)/)
      .map((seg) => {
        if (seg === "**") return ".*";
        if (seg === "*") return "[^/]+";
        return seg.replace(/[.+^${}()|[\]\\]/g, "\\$&");
      })
      .join("");
  const regex = new RegExp(regexBody + "$");
  if (!regex.test(urlLine)) {
    throw new Error(`assertUrlMatches(${pattern}): current URL ${urlLine} did not match`);
  }
}

export function assertSnapshotContains(needle: string, opts?: client.ClientOptions): void {
  const snap = client.snapshotInteractive(opts);
  if (!snap.raw.includes(needle)) {
    throw new Error(`assertSnapshotContains(${JSON.stringify(needle)}): not in snapshot`);
  }
}
