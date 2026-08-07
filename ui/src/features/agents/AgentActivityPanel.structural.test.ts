import { describe, expect, it } from "vitest";
import panelSource from "./AgentActivityPanel.tsx?raw";
import boltSource from "./BoltSessionsPanel.tsx?raw";

/** Epic 42 Slice F decision 5: "no control that mutates an agent's
 *  permissions." A backend `DELETE /agents/{id}` (revoke) route exists for
 *  other callers, but this feature must never expose it — or any other
 *  write — from the console. Reads the components' raw source (the same
 *  `?raw` structural pattern `VocabularyBrowser.structural.test.ts`
 *  established) rather than testing behaviour, because the property being
 *  proven is an *absence*: no unit test exercising only the read paths can
 *  tell "a mutating control was never built" from "one exists and no test
 *  happens to click it yet". */

describe("the agent activity and Bolt panels make no mutating request", () => {
  it.each([
    ["AgentActivityPanel.tsx", panelSource],
    ["BoltSessionsPanel.tsx", boltSource],
  ])("%s calls no method whose name suggests a write", (_name, source: string) => {
    expect(source).not.toMatch(/method:\s*["'](POST|PUT|PATCH|DELETE)["']/i);
  });

  it.each([
    ["AgentActivityPanel.tsx", panelSource],
    ["BoltSessionsPanel.tsx", boltSource],
  ])("%s never names a revoke/grant/mutate API call", (_name, source: string) => {
    expect(source).not.toMatch(/api\.(revoke|grant|setAgent|mutate)\w*/i);
  });
});
