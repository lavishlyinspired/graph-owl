import { describe, expect, it } from "vitest";
import source from "./App.tsx?raw";
import { ROUTES } from "./routes";

/** `ROUTES` is a hand-maintained mirror of `App.tsx`'s own
 *  `section === "..."` switch, not derived from it — so nothing stops the
 *  two from drifting apart the moment a new section is added to one and not
 *  the other. This reads `App.tsx`'s raw source (the same `?raw` structural
 *  pattern `VocabularyBrowser.structural.test.ts` already established) and
 *  fails if they ever disagree, in either direction: a route in `App.tsx`
 *  the budget never counted, or a route in the budget that no longer exists
 *  to inflate the count. */

describe("routes.ts stays in sync with App.tsx's real section switch", () => {
  it.each(ROUTES)('every budgeted route ("%s") is really checked in App.tsx', (route) => {
    expect(source).toMatch(new RegExp(`section === "${route}"`));
  });

  it("every section App.tsx checks is accounted for in the budget", () => {
    const matches = [...source.matchAll(/section === "([a-z-]+)"/g)].map((m) => m[1]);
    const real = new Set(matches);
    for (const found of real) {
      expect(ROUTES as readonly string[], `App.tsx checks "${found}" but routes.ts does not list it`).toContain(
        found,
      );
    }
  });
});
