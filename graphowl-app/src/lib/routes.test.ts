import { describe, expect, it } from "vitest";
import { checkRouteBudget, MAX_ROUTES, ROUTES } from "./routes";

describe("checkRouteBudget", () => {
  it("passes when route count is at or under the max", () => {
    expect(checkRouteBudget(["a", "b"], 2)).toEqual({ count: 2, max: 2, ok: true });
  });

  it("fails when route count exceeds the max — the named RED case", () => {
    const thirtyOne = Array.from({ length: 31 }, (_, i) => `route-${i}`);
    const result = checkRouteBudget(thirtyOne, 30);
    expect(result.ok).toBe(false);
    expect(result.count).toBe(31);
  });

  it("defaults to the project's 30-route ceiling", () => {
    expect(checkRouteBudget(["only-one"]).max).toBe(MAX_ROUTES);
  });
});

describe("the real console route list", () => {
  it("stays within the CI-enforced budget", () => {
    const result = checkRouteBudget(ROUTES);
    expect(result.ok).toBe(true);
    expect(result.count).toBeLessThanOrEqual(MAX_ROUTES);
  });

  it("has no duplicate route names", () => {
    expect(new Set(ROUTES).size).toBe(ROUTES.length);
  });
});
