import { describe, expect, it } from "vitest";
import { CAPABILITIES, SECTIONS, hasTabs, sectionsFor } from "./sections";
import { ROUTES, type RouteName } from "./routes";
import { NAV } from "./nav";

describe("consolidating 30 screens into the five stages", () => {
  it("keeps every capability the 30-route console had", () => {
    // Plan 123 Slice D's binding constraint: the count comes down, the
    // capabilities do not. A screen that vanished would take its
    // functionality with it, and nobody would notice until a CA went looking
    // for it mid-period.
    const hosted = new Set(Object.values(SECTIONS).flatMap((s) => s.map((x) => x.capability)));

    for (const capability of CAPABILITIES) {
      expect(hosted.has(capability), `${capability} has no home`).toBe(true);
    }
  });

  it("gives every capability exactly one home, so nothing is maintained twice", () => {
    const hosted = Object.values(SECTIONS).flatMap((s) => s.map((x) => x.capability));

    expect(new Set(hosted).size).toBe(hosted.length);
  });

  it("hosts sections only on routes that exist", () => {
    for (const route of Object.keys(SECTIONS)) {
      expect(ROUTES).toContain(route);
    }
  });

  it("reaches every hosting route from the nav", () => {
    // A section on an unreachable route is a capability that still exists in
    // the code and cannot be opened — the worst of both outcomes.
    const reachable = new Set(NAV.flatMap((g) => g.items.map((i) => i.route)));

    for (const route of Object.keys(SECTIONS) as RouteName[]) {
      expect(reachable.has(route), `${route} hosts sections but is not in the nav`).toBe(true);
    }
  });

  it("names every section, because a nameless tab cannot be navigated to", () => {
    for (const [route, sections] of Object.entries(SECTIONS)) {
      for (const section of sections) {
        expect(section.label, `${route} has an unlabelled section`).toBeTruthy();
      }
    }
  });

  it("puts the primary capability first on each host", () => {
    // The first section is what opens by default. A host whose default is a
    // secondary view makes the common case one click slower, every time.
    expect(sectionsFor("pipeline")[0]?.capability).toBe("upload");
    expect(sectionsFor("reconcile")[0]?.capability).toBe("reconcile");
    expect(sectionsFor("itc")[0]?.capability).toBe("itc-position");
    expect(sectionsFor("register")[0]?.capability).toBe("register");
    // Case detail is a *section* of the findings list now, not its own route:
    // you open a case from the list, and a route arrived at without a
    // selection shows an empty screen.
    expect(sectionsFor("register").map((s) => s.capability)).toContain("case-detail");
  });

  it("renders no tab strip for a single-capability route", () => {
    // A strip with one tab is chrome that costs vertical space and tells a
    // reader nothing. The capability is still listed — that is what keeps the
    // "every capability has a home" invariant honest rather than special-cased.
    expect(sectionsFor("agents")).toHaveLength(1);
    expect(hasTabs("agents")).toBe(false);
    expect(hasTabs("pipeline")).toBe(true);
  });

  it("cuts the route count roughly in half without losing anything", () => {
    // 30 routes was the mockup's own count and the plan's complaint. The
    // exact target is a judgement, but a consolidation that did not actually
    // consolidate would pass every test above.
    expect(ROUTES.length).toBeLessThanOrEqual(15);
    expect(CAPABILITIES.length).toBeGreaterThanOrEqual(25);
  });
});
