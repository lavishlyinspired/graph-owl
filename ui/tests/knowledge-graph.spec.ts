import { expect, test } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

/** Epic 42 Slice E's own named RED test: a view toggle that silently drops
 *  what does not map teaches a reader the two models are equivalent when
 *  Epic 7c's whole `MappingReport` exists to say they are not. Seeds a
 *  real two-level asset hierarchy (a database under a service) through the
 *  real API — the same "seed through the real endpoint" pattern every
 *  other spec in this project uses — so the child's own `parentService`
 *  reference is a genuine lossy mapping, not a fixture artifact. */

async function createAsset(baseURL: string, kind: string, name: string, parentId?: string) {
  const response = await fetch(`${baseURL}/assets`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(parentId ? { kind, name, parentId } : { kind, name }),
  });
  return (await response.json()) as { id: string; name: string };
}

async function openAssetDetail(page: import("@playwright/test").Page, name: string) {
  const searchBox = page.getByPlaceholder(/search/i).first();
  await searchBox.click();
  await searchBox.fill(name);
  await expect(page.getByRole("heading", { name: new RegExp(`1 result for`, "i") })).toBeVisible();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name })).toBeVisible();
}

test("the Knowledge tab's property-graph view names what did not carry across, zero axe violations", async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? "";
  const service = await createAsset(base, "service", "kgtoggle-alpha-svc");
  const database = await createAsset(base, "database", "kgtoggle-alpha-db", service.id);

  await page.goto("/");
  await openAssetDetail(page, database.name);

  await page.getByRole("tab", { name: /knowledge/i }).click();
  const panel = page.getByRole("tabpanel", { name: /knowledge/i });
  await expect(panel.getByText("Facts about this asset")).toBeVisible();

  // Triples view (the default): real outbound facts about this asset,
  // rendered as subject-scoped rows — not a placeholder, not empty.
  await expect(panel.getByText("This asset says")).toBeVisible();
  await expect(panel.getByText(/catalog#fqn/)).toBeVisible();

  // The toggle itself.
  await panel.locator(".ant-segmented-item", { hasText: "Property graph" }).click();
  await expect(panel.getByText("Labels")).toBeVisible();
  // Scoped to the Knowledge panel: "database" also appears as an unrelated
  // kind filter chip elsewhere on the page, so an unscoped exact-text
  // locator is a strict-mode violation, not a real ambiguity in the panel.
  await expect(panel.getByText("database", { exact: true })).toBeVisible();

  // The RED test: a real parent reference must be named as a loss, not
  // silently dropped from the property-graph view.
  await expect(
    panel.getByText('The reference in "parentService" was flattened to plain text — it no longer traverses as an edge.'),
  ).toBeVisible();

  const axeResults = await new AxeBuilder({ page }).analyze();
  expect(axeResults.violations, JSON.stringify(axeResults.violations, null, 2)).toEqual([]);
});

test("toggling back to triples keeps the panel mounted, not reset", async ({ page, baseURL }) => {
  const base = baseURL ?? "";
  const service = await createAsset(base, "service", "kgtoggle-beta-svc");
  const database = await createAsset(base, "database", "kgtoggle-beta-db", service.id);

  await page.goto("/");
  await openAssetDetail(page, database.name);
  await page.getByRole("tab", { name: /knowledge/i }).click();
  const panel = page.getByRole("tabpanel", { name: /knowledge/i });

  await panel.locator(".ant-segmented-item", { hasText: "Property graph" }).click();
  await expect(panel.getByText("Labels")).toBeVisible();

  await panel.locator(".ant-segmented-item", { hasText: "Triples" }).click();
  // Still populated — the fetch was not re-triggered and lost, nor did the
  // panel unmount into a fresh loading state a reader would have to wait
  // through again.
  await expect(panel.getByText("This asset says")).toBeVisible();
  await expect(panel.getByText(/catalog#fqn/)).toBeVisible();
});
