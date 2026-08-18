import { describe, expect, it } from "vitest";
import source from "./ReviewQueue.tsx?raw";

/** Epic 42 Slice D's own named requirement: "a structural test asserts
 *  `ReviewQueue.tsx` has no queue-specific branch." Reads the component's
 *  raw source via Vite's `?raw` import — the identical technique
 *  `VocabularyBrowser.structural.test.ts` already established, for the
 *  identical reason: a unit test exercising only the real queues cannot
 *  distinguish a parameterized component from one with hardcoded paths
 *  that happen to agree with its tests. */

const QUEUE_KEYS = ["resolution", "extraction", "drift"];

describe("ReviewQueue.tsx has no queue-specific branch", () => {
  it.each(QUEUE_KEYS)("never names the %s queue as a string literal", (key) => {
    expect(source).not.toMatch(new RegExp(`["'\`]${key}["'\`]`, "i"));
  });

  it("never branches on a queue kind via switch", () => {
    expect(source).not.toMatch(/\bswitch\s*\(/);
  });

  it("never imports the API client directly — only through config", () => {
    // Every queue-specific type a hardcoded branch would need (Evidence,
    // ReviewQueueEntry, PendingClaim, DriftItem, ...) lives in ../../api
    // and can only reach this file through that path.
    expect(source).not.toMatch(/from\s+["']\.\.\/\.\.\/api["']/);
  });

  it("never imports a specific queue's own config module", () => {
    expect(source).not.toMatch(/from\s+["']\.\/(resolutionQueue|extractionQueue|driftQueue)["']/);
  });
});
