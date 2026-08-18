/** The single-source guard for Epic 39 Slice E's shared trust components.
 *
 *  "One shared component set" is not a design intention unless something
 *  fails when a second one appears — Epic 40's canvas or Epic 42's queues
 *  growing their own confidence styling is exactly how a user learns to
 *  distrust the indicator (the plan's own words). This is the frontend
 *  version of the structural bans already used on the Rust side
 *  (`no_query_crate_references_a_projection_target` in
 *  `graph-owl-lpg-io/src/projection.rs`): read the source tree as text and
 *  assert the invariant directly, rather than trust that reviewers notice a
 *  hand-rolled badge three files away.
 *
 *  `import.meta.glob` rather than `node:fs`, deliberately: this is a browser
 *  app whose typecheck scope excludes Node globals everywhere else
 *  (`tsconfig.json`'s note on `src/generated`), and Vite's own eager-raw
 *  glob reads the same source tree without reopening that door for one
 *  test. */

import { describe, expect, it } from "vitest";

const files = import.meta.glob<string>("/src/**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
});

function sourceFiles(): [path: string, contents: string][] {
  return Object.entries(files).filter(
    ([path]) => !path.endsWith(".test.ts") && !path.endsWith(".test.tsx"),
  );
}

const outsideTrust = ([path]: [string, string]) => !path.includes("/src/trust/");

describe("confidence, derivation, certification and provenance render through one component set", () => {
  it("nothing outside trust/ imports the raw descriptors directly", () => {
    // Importing `describeConfidence` et al. anywhere else is exactly how a
    // second, drifting rendering of "what does 0.62 mean" gets built —
    // going through `ConfidenceBadge`/`DerivationBadge`/`CertificationBadge`
    // instead is the whole point of Slice E.
    const offenders = sourceFiles()
      .filter(outsideTrust)
      .filter(([, contents]) => /from\s+["'][^"']*\/confidence["']/.test(contents))
      .map(([path]) => path);
    expect(offenders).toEqual([]);
  });

  it('nothing hard-codes dir="ltr" — base direction always goes through userTextDir', () => {
    const offenders = sourceFiles()
      .filter(outsideTrust)
      .filter(([, contents]) => /dir=["']ltr["']/.test(contents))
      .map(([path]) => path);
    expect(offenders).toEqual([]);
  });
});
