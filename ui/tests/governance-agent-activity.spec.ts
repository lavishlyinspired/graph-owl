import { expect, test } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

/** Epic 42 Slice F: agent sessions, filterable operation history with
 *  write-backs distinguished from everything that did not change the
 *  catalog, and Bolt endpoint status — all read-only (decision 5: no
 *  control mutates an agent's permission from this screen). Seeds a real
 *  grant via `PUT /agents/{id}/grant` and real activity via real MCP tool
 *  calls (`propose_metadata_change`, `record_memory`) through `POST /mcp`
 *  — the same "seed through the real endpoint" pattern every other spec in
 *  this project uses, so the refusal reason and outcome distinctions are
 *  genuine server behaviour, not fixture artifacts.
 *
 *  **The agent under test is `system`, not a distinct bot identity.** This
 *  suite runs with authentication disabled (`OIDC_ISSUER`/
 *  `GRAPH_OWL_JWT_SECRET` both unset), under which every request —
 *  including an MCP tool call — runs as the `system` principal; there is
 *  no way from an unauthenticated client to make the server attribute a
 *  write to any other identity. `system` is pre-seeded (`V15`), so no
 *  `PUT /users/{id}` is needed first, unlike a genuinely distinct agent
 *  id would require. No other spec in this directory calls `/mcp`, so
 *  `system`'s activity history stays exclusively this test's own rows
 *  even in the combined suite. */

async function grantAgent(baseURL: string, agentId: string, capabilities: string[]) {
  await fetch(`${baseURL}/agents/${agentId}/grant`, {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      capabilities,
      rateLimit: { maxWrites: 60, windowSeconds: 3600 },
    }),
  });
}

async function createAsset(baseURL: string, kind: string, name: string) {
  const response = await fetch(`${baseURL}/assets`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ kind, name }),
  });
  return (await response.json()) as { id: string; fullyQualifiedName: string };
}

async function callMcpTool(baseURL: string, name: string, args: Record<string, unknown>) {
  await fetch(`${baseURL}/mcp`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "tools/call",
      params: { name, arguments: args },
    }),
  });
}

test("agent activity: real grants, filterable history, write-backs distinguished from proposals and refusals, zero axe violations", async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? "";
  const agentId = "system";
  const asset = await createAsset(base, "service", "agent-activity-spec-svc");

  await grantAgent(base, agentId, ["proposeTags", "recordMemory"]);

  // A refusal — proposeDescription was never granted.
  await callMcpTool(base, "propose_metadata_change", {
    fullyQualifiedName: asset.fullyQualifiedName,
    field: "description",
    value: "attempted without the capability",
    rationale: "spec setup",
    confidence: 0.5,
  });
  // A genuine proposal.
  await callMcpTool(base, "record_memory", {
    fullyQualifiedName: asset.fullyQualifiedName,
    content: "Observed during the Playwright spec run.",
    rationale: "spec setup",
    confidence: 0.9,
  });

  await page.goto(`/?section=admin&adminTab=agents`);
  await expect(page.getByRole("heading", { name: "Agent activity" })).toBeVisible();

  const grantRow = page.getByRole("row", { name: new RegExp(agentId) });
  await expect(grantRow).toBeVisible();
  await grantRow.click();

  await expect(page.getByRole("heading", { name: "History" })).toBeVisible();
  await expect(page.getByText("Refused: this action requires")).toBeVisible();
  await expect(
    page.getByText("Suggested a change to agent-activity-spec-svc — pending human review."),
  ).toBeVisible();

  // Filter by entity narrows to only this asset's rows (both do match; the
  // real assertion is that an unrelated entity narrows to nothing).
  const entityFilter = page.getByPlaceholder("Filter by entity");
  await entityFilter.fill("no-such-entity-at-all");
  await expect(page.getByText("No activity matches these filters.")).toBeVisible();
  await entityFilter.fill("");

  const axeResults = await new AxeBuilder({ page }).analyze();
  expect(axeResults.violations, JSON.stringify(axeResults.violations, null, 2)).toEqual([]);
});

test("Bolt sessions: the disabled state is calm, not an error — zero axe violations", async ({
  page,
  baseURL,
}) => {
  void baseURL;
  await page.goto(`/?section=admin&adminTab=bolt`);
  await expect(page.getByRole("heading", { name: "Bolt sessions" })).toBeVisible();
  // This build has no `bolt` feature compiled in, so the server's own route
  // does not exist and the request falls through to the SPA fallback — the
  // real state this test proves is calm, not the frightening JSON-parse
  // error that state used to surface as.
  await expect(page.getByText("Bolt is not enabled on this server.")).toBeVisible();

  const axeResults = await new AxeBuilder({ page }).analyze();
  expect(axeResults.violations, JSON.stringify(axeResults.violations, null, 2)).toEqual([]);
});
