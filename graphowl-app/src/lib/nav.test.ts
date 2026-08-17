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

  it("every nav-reachable route appears exactly once", () => {
    const seen = NAV.flatMap((g) => g.items.map((i) => i.route));
    expect(new Set(seen).size).toBe(seen.length);
  });

  it("covers every route except the ones reached contextually (not nav items)", () => {
    const navRoutes = new Set(NAV.flatMap((g) => g.items.map((i) => i.route)));
    const contextual = new Set(["pipeline"]);
    for (const route of ROUTES) {
      if (contextual.has(route)) continue;
      expect(navRoutes.has(route), `"${route}" is missing from NAV`).toBe(true);
    }
  });
});
