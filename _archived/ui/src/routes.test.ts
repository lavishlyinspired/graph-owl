import { describe, expect, it } from "vitest";
import { MAX_ROUTES, ROUTES, checkRouteBudget } from "./routes";

describe("checkRouteBudget", () => {
  it("passes a route list at or under the budget", () => {
    const thirty = Array.from({ length: 30 }, (_, i) => `route-${i}`);
    const result = checkRouteBudget(thirty, 30);
    expect(result).toEqual({ count: 30, max: 30, ok: true });
  });

  it("the RED test: fails a fixture one route over the budget, not silently", () => {
    const thirtyOne = Array.from({ length: 31 }, (_, i) => `route-${i}`);
    const result = checkRouteBudget(thirtyOne, 30);
    expect(result.ok).toBe(false);
    expect(result.count).toBe(31);
  });

  it("defaults to the console's own 30-route ceiling", () => {
    const overDefault = Array.from({ length: MAX_ROUTES + 1 }, (_, i) => `route-${i}`);
    expect(checkRouteBudget(overDefault).ok).toBe(false);
  });
});

describe("the console's real routes", () => {
  it("CI-asserted: stays within the route budget — this is a build failure, not a warning", () => {
    const result = checkRouteBudget(ROUTES);
    expect(result.ok, `${result.count} routes exceeds the budget of ${result.max}: ${ROUTES.join(", ")}`).toBe(true);
  });

  it("every route is a distinct, non-empty identifier", () => {
    expect(new Set(ROUTES).size).toBe(ROUTES.length);
    expect(ROUTES.every((route) => route.length > 0)).toBe(true);
  });
});
