import { describe, expect, it } from "vitest";
import { cssVariables, palette, radius } from "./theme";

describe("cssVariables", () => {
  it("exposes every semantic palette colour as a --gowl-* custom property", () => {
    const vars = cssVariables(palette.light);
    expect(vars["--gowl-primary"]).toBe(palette.light.primary);
    expect(vars["--gowl-text"]).toBe(palette.light.text);
    expect(vars["--gowl-border"]).toBe(palette.light.border);
    expect(vars["--gowl-error"]).toBe(palette.light.error);
  });

  it("carries the radius scale so shadcn primitives share the same corner roles", () => {
    const vars = cssVariables(palette.light);
    expect(vars["--gowl-radius-control"]).toBe(`${radius.control}px`);
    expect(vars["--gowl-radius-card"]).toBe(`${radius.card}px`);
  });

  it("reflects dark mode's own colours, not light's, when given the dark palette", () => {
    const vars = cssVariables(palette.dark);
    expect(vars["--gowl-primary"]).toBe(palette.dark.primary);
    expect(vars["--gowl-page"]).toBe(palette.dark.page);
    expect(vars["--gowl-page"]).not.toBe(palette.light.page);
  });
});
