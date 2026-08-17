import { describe, expect, it } from "vitest";
import source from "./router.tsx?raw";
import { ROUTES } from "./lib/routes";

/** `ui/src/routes.structural.test.ts`'s pattern, adapted: `ROUTES` there was
 *  a hand-maintained mirror of a text-based `?section=` switch. Here the
 *  router is generated *from* `ROUTES` (`router.tsx`'s `ROUTES.map(...)`),
 *  so drift is structurally impossible for the mapped routes — this test
 *  instead guards the thing that actually can drift: that every route file
 *  `ROUTES` expects really exists on disk, and that the router is still
 *  built from `ROUTES` rather than a second, hand-written list. */
describe("router.tsx stays derived from ROUTES, not a parallel list", () => {
  it("builds its route table from ROUTES.map, not a literal array", () => {
    expect(source).toMatch(/ROUTES\.map\(/);
  });

  it.each(ROUTES)('a route module exists for "%s"', async (route) => {
    const routeFiles = import.meta.glob("./routes/*.tsx");
    expect(Object.keys(routeFiles), `no route file for "${route}"`).toContain(`./routes/${route}.tsx`);
  });
});
