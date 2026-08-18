import { describe, expect, it } from "vitest";
import { NAV, pageTitleForPath } from "./nav";
import { ROUTES } from "./routes";
import { strings } from "./strings";

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

describe("pageTitleForPath", () => {
  // WCAG's "page-has-heading-one" needs an h1 whose text names the current
  // screen — AppShell renders one (visually hidden) sourced from this, since
  // the console's own visual design has no on-screen page title.
  it("resolves a bare route to its nav label", () => {
    expect(pageTitleForPath("/lineage-view")).toBe("Lineage");
  });

  it("resolves a route carrying a deep-linked id segment", () => {
    expect(pageTitleForPath("/explore/some-entity-id")).toBe("Explore");
  });

  it("resolves a route carrying a query string", () => {
    expect(pageTitleForPath("/paths?from=a&to=b")).toBe("Paths");
  });

  it("resolves the bare root to the first route's label", () => {
    expect(pageTitleForPath("/")).toBe("Overview");
  });

  it("falls back to the brand name for a route reached contextually, not through nav", () => {
    expect(pageTitleForPath("/pipeline")).toBe(strings.brand);
  });
});
