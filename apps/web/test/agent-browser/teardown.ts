/// teardown.ts — close all agent-browser sessions and archive
/// screenshots. Safe to run multiple times.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, renameSync } from "node:fs";
import { join } from "node:path";

function main(): void {
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const artifacts = join(process.cwd(), "artifacts", stamp);
  mkdirSync(artifacts, { recursive: true });

  const screenshots = join(process.cwd(), "screenshots");
  if (existsSync(screenshots)) {
    mkdirSync(artifacts, { recursive: true });
    for (const f of require("node:fs").readdirSync(screenshots)) {
      renameSync(join(screenshots, f), join(artifacts, f));
    }
    console.log(`archived screenshots → ${artifacts}`);
  } else {
    console.log("no screenshots to archive");
  }

  // Close all sessions.
  const r = spawnSync("agent-browser", ["close", "--all"], { encoding: "utf8" });
  if (r.status === 0) {
    console.log("agent-browser sessions closed");
  } else {
    console.error(`agent-browser close --all: ${r.stderr || r.stdout}`);
  }
}

main();
