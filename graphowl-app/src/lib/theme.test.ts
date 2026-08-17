import { describe, expect, it, vi } from "vitest";
import { applyTheme, persistTheme, resolveInitialTheme, toggleTheme } from "./theme";

describe("resolveInitialTheme", () => {
  it("defaults to dark when nothing is stored", () => {
    const storage = { getItem: vi.fn().mockReturnValue(null) };
    expect(resolveInitialTheme(storage)).toBe("dark");
  });

  it("respects a stored light preference", () => {
    const storage = { getItem: vi.fn().mockReturnValue("light") };
    expect(resolveInitialTheme(storage)).toBe("light");
  });

  it("falls back to dark on an unrecognized stored value, not light", () => {
    const storage = { getItem: vi.fn().mockReturnValue("garbage") };
    expect(resolveInitialTheme(storage)).toBe("dark");
  });
});

describe("toggleTheme", () => {
  it("flips dark to light", () => {
    expect(toggleTheme("dark")).toBe("light");
  });

  it("flips light to dark — the direction a naive one-branch toggle gets wrong", () => {
    expect(toggleTheme("light")).toBe("dark");
  });
});

describe("applyTheme / persistTheme", () => {
  it("sets data-theme on the given root", () => {
    const root = { setAttribute: vi.fn() };
    applyTheme("light", root);
    expect(root.setAttribute).toHaveBeenCalledWith("data-theme", "light");
  });

  it("writes the mode under the app's own storage key, not a shared one", () => {
    const storage = { setItem: vi.fn() };
    persistTheme("dark", storage);
    expect(storage.setItem).toHaveBeenCalledWith("graphowl-theme", "dark");
  });
});
