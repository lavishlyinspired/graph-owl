#!/usr/bin/env node
/** Ported from `ui/scripts/check-budgets.mjs` verbatim (same logic, same
 *  budgets — `plans/122a-graphowl-app.md` A0 AC 3). Run after `npm run
 *  build`; wired into CI as a build-failing step. */

import { gzipSync } from "node:zlib";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { evaluateBudgets } from "./evaluate-budgets.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const DIST = join(ROOT, "dist");

function gzipSize(path) {
  return gzipSync(readFileSync(path)).length;
}

function findChunks() {
  const indexHtml = readFileSync(join(DIST, "index.html"), "utf8");
  const referenced = new Set([...indexHtml.matchAll(/src="([^"]+\.js)"/g)].map((m) => m[1].split("/").pop()));

  const staticDir = join(DIST, "static");
  const allJs = readdirSync(staticDir).filter((f) => f.endsWith(".js"));

  const initial = allJs.filter((f) => referenced.has(f));
  const routeChunks = allJs.filter((f) => !referenced.has(f));

  return {
    initialBytesGzip: initial.reduce((sum, f) => sum + gzipSize(join(staticDir, f)), 0),
    routeChunksGzip: routeChunks.map((f) => ({
      name: f,
      bytesGzip: gzipSize(join(staticDir, f)),
    })),
  };
}

function dependencyCount() {
  const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
  return Object.keys(pkg.dependencies ?? {}).length;
}

const { initialBytesGzip, routeChunksGzip } = findChunks();
const result = evaluateBudgets({
  initialBytesGzip,
  routeChunksGzip,
  dependencyCount: dependencyCount(),
});

console.log(`initial bundle: ${(initialBytesGzip / 1024).toFixed(1)}KB gzipped`);
for (const chunk of routeChunksGzip) {
  console.log(`route chunk ${chunk.name}: ${(chunk.bytesGzip / 1024).toFixed(1)}KB gzipped`);
}
console.log(`dependencies: ${dependencyCount()}`);

if (!result.ok) {
  console.error("\nBudget violations:");
  for (const v of result.violations) console.error(`  [${v.budget}] ${v.detail}`);
  process.exit(1);
}
console.log("\nAll budgets within bounds.");
