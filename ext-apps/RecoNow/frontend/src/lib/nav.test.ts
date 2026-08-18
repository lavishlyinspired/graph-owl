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

describe("the five-stage shape", () => {
  it("groups the work by stage, not by noun", () => {
    // Plan 123 Slice D. The old grouping named *things* — ITC, SUPPLIERS,
    // COMPLIANCE, DELIVER — so a reviewer had to know which noun held the
    // screen they wanted. The five stages name *what you are doing*, in the
    // order a period is actually worked: get the data in, reconcile it, work
    // the cases, understand the position, act on it.
    expect(NAV.map((g) => g.label)).toEqual([
      "HOME",
      "DATA",
      "RECONCILE",
      "CASES",
      "INTELLIGENCE",
      "ACT",
      "SETTINGS",
    ]);
  });

  it("puts every screen a period's work touches into exactly one stage", () => {
    const staged = NAV.filter((g) => !["HOME", "SETTINGS"].includes(g.label)).flatMap(
      (g) => g.items.map((i) => i.route),
    );

    expect(new Set(staged).size).toBe(staged.length);
  });

  it("orders the stages so each depends only on the ones before it", () => {
    // Reconciling needs data; a case needs a reconciliation to have produced
    // it; the ITC position needs the cases resolved; acting needs the
    // position. A nav that lists them in another order teaches the wrong
    // sequence to whoever is learning the product from it.
    const order = NAV.map((g) => g.label);

    expect(order.indexOf("DATA")).toBeLessThan(order.indexOf("RECONCILE"));
    expect(order.indexOf("RECONCILE")).toBeLessThan(order.indexOf("CASES"));
    expect(order.indexOf("CASES")).toBeLessThan(order.indexOf("INTELLIGENCE"));
    expect(order.indexOf("INTELLIGENCE")).toBeLessThan(order.indexOf("ACT"));
  });

  it("keeps upload at the head of DATA, because nothing else works without it", () => {
    const data = NAV.find((g) => g.label === "DATA");

    expect(data?.items[0]?.route).toBe("pipeline");
  });

  it("carries the GSTR-3B working paper under INTELLIGENCE", () => {
    // The plan's own words: gross -> reversals -> net Table 4, every figure
    // traced. It is an understanding screen, not an action one.
    const intelligence = NAV.find((g) => g.label === "INTELLIGENCE");

    expect(intelligence?.items.map((i) => i.route)).toContain("workingpaper");
  });

  it("loses no screen in the regrouping", () => {
    // The regrouping is an information-architecture change, not a deletion.
    // A screen that silently vanished would take its functionality with it.
    const reachable = new Set(NAV.flatMap((g) => g.items.map((i) => i.route)));

    for (const route of ROUTES) {
      expect(reachable.has(route)).toBe(true);
    }
  });
});
