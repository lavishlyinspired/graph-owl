import { describe, expect, it } from "vitest";
import { userTextDir } from "./direction";

describe("userTextDir", () => {
  it("is always auto, never a hard-coded ltr", () => {
    // The store does not carry a base-direction flag on labels (Epic 94's
    // rdf:dirLangString is not yet threaded through the API). `auto` asks the
    // browser's own bidi algorithm to read the first strong character, which
    // is correct today and stays correct if a real direction field arrives
    // later without this call site needing to change.
    expect(userTextDir("Hello")).toBe("auto");
    expect(userTextDir("مرحبا")).toBe("auto");
    expect(userTextDir(null)).toBe("auto");
    expect(userTextDir(undefined)).toBe("auto");
    expect(userTextDir("")).toBe("auto");
  });
});
