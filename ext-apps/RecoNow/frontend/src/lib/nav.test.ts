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

  it("reaches every route from the nav — no route is unreachable", () => {
    // The count is deliberately not pinned. It was 28, matching the delivered
    // mockup exactly; plan 123 both adds screens a CA needs (Reconcile, and
    // later the 3B working paper) and consolidates ones that were the same
    // list under different headings. Asserting a number would fail on every
    // such change while proving nothing — what matters is that nothing in
    // ROUTES is unreachable from the navigation.
    const navRoutes = new Set(NAV.flatMap((g) => g.items.map((i) => i.route)));
    for (const route of ROUTES) {
      expect(navRoutes.has(route), `"${route}" is missing from NAV`).toBe(true);
    }
    expect(ROUTES.length).toBeGreaterThan(0);
  });
});
