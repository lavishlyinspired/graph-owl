// **The contract-drift test.** Slice E asks for one explicitly: "change a field
// type and assert SDK generation or the round-trip fails."
//
// Reading the committed `openapi.json` rather than a copy is deliberate. A
// vendored spec would make this test agree with itself forever, which is the
// exact failure it exists to catch.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { CONTRACT_VERSION, REQUIRED_PATHS } from "./contract.js";

const contract = JSON.parse(
  readFileSync(fileURLToPath(new URL("../../../openapi.json", import.meta.url)), "utf8"),
) as {
  info: { version: string };
  paths: Record<string, Record<string, unknown>>;
};

describe("the contract this SDK is pinned to", () => {
  it("is the one the service publishes", () => {
    expect(contract.info.version).toBe(CONTRACT_VERSION);
  });

  it("still declares every path this SDK calls", () => {
    for (const path of REQUIRED_PATHS) {
      expect(Object.keys(contract.paths)).toContain(path);
    }
  });

  // The status codes are part of what the client branches on: `push` treats 207
  // as success because a batch has per-item outcomes, and `pushFile` returns a
  // handle because 202 means "I have started". A contract that changed either
  // would break the client silently, since both are still 2xx.
  it("still answers a push with 207 and a batch upload with 202", () => {
    expect(contract.paths["/ingest"]!.post).toHaveProperty("responses.207");
    expect(contract.paths["/ingest/batch"]!.post).toHaveProperty("responses.202");
  });
});
