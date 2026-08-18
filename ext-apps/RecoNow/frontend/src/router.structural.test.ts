import { describe, expect, it } from "vitest";
import source from "./router.tsx?raw";
import { ROUTES } from "./lib/routes";

describe("router.tsx stays derived from ROUTES, not a parallel list", () => {
  it("builds its route table from ROUTES.map, not a literal array", () => {
    expect(source).toMatch(/ROUTES\.map\(/);
  });

  it.each(ROUTES)('a route module exists for "%s"', async (route) => {
    const routeFiles = import.meta.glob("./routes/*.tsx");
    expect(Object.keys(routeFiles), `no route file for "${route}"`).toContain(`./routes/${route}.tsx`);
  });
});
