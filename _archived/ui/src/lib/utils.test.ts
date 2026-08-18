import { describe, expect, it } from "vitest";
import { cn } from "./utils";

describe("cn", () => {
  it("joins plain class name strings", () => {
    expect(cn("a", "b")).toBe("a b");
  });

  it("drops falsy values", () => {
    expect(cn("a", false, null, undefined, "b")).toBe("a b");
  });

  it("resolves conflicting Tailwind utilities to the last one given", () => {
    // tailwind-merge's whole job: two padding utilities of the same axis
    // conflict, and a plain string-join would emit both, letting CSS's own
    // source-order tiebreak (not intent) decide which one wins.
    expect(cn("p-2", "p-4")).toBe("p-4");
  });
});
