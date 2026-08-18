import { describe, expect, it } from "vitest";
import { NAV } from "./nav";
import { ROUTES } from "./routes";

describe("NAV", () => {
  it("references only routes that exist in the route budget", () => {
    const known = new Set<string>(ROUTES);
    for (const group of NAV) {
      for (const item of group.items) {
        expect(known.has(item.route), `"${item.route}" is not in ROUTES`).toBe(true);
      }
    }
  });

  it("every route appears exactly once across the nav", () => {
    const seen = NAV.flatMap((g) => g.items.map((i) => i.route));
    expect(new Set(seen).size).toBe(seen.length);
    expect(seen.length).toBe(ROUTES.length);
  });

  it("covers all 28 destinations from the delivered mockup", () => {
    const navRoutes = new Set(NAV.flatMap((g) => g.items.map((i) => i.route)));
    for (const route of ROUTES) {
      expect(navRoutes.has(route), `"${route}" is missing from NAV`).toBe(true);
    }
    expect(ROUTES.length).toBe(28);
  });
});
