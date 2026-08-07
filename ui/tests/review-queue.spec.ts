import { expect, test } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

/** Epic 42 Slice C: merge adjudication, against a real server. An
 *  ambiguous pair — same-schema tables with matching columns but
 *  different names — is created and resolved through the real API, the
 *  same "seed through the real endpoint" pattern `vocabulary.spec.ts`
 *  already established. */

async function createAsset(baseURL: string, kind: string, name: string, parentId?: string) {
  const response = await fetch(`${baseURL}/assets`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(parentId ? { kind, name, parentId } : { kind, name }),
  });
  return (await response.json()) as { id: string };
}

async function ambiguousPair(baseURL: string, suffix: string) {
  const service = await createAsset(baseURL, "service", `svc-${suffix}`);
  const database = await createAsset(baseURL, "database", "db", service.id);
  const schema = await createAsset(baseURL, "schema", "sch", database.id);
  const a = await createAsset(baseURL, "table", "orders", schema.id);
  const b = await createAsset(baseURL, "table", "orders_v2", schema.id);
  for (const parent of [a, b]) {
    await createAsset(baseURL, "column", "id", parent.id);
    await createAsset(baseURL, "column", "amount", parent.id);
  }
  await fetch(`${baseURL}/assets/${b.id}/resolve`, { method: "POST" });
  return { a, b };
}

async function queueEntryFor(baseURL: string, targetId: string) {
  const response = await fetch(`${baseURL}/resolution/queue?status=pending`);
  const body = (await response.json()) as {
    data: { id: string; target: string }[];
  };
  const entry = body.data.find((e) => e.target === targetId);
  if (!entry) throw new Error(`no pending queue entry for target ${targetId}`);
  return entry;
}

test("the review queue: side-by-side comparison, reject requires a reason, zero axe violations", async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? "";
  await ambiguousPair(base, "review-1");

  await page.goto("/?section=review");

  const list = page.getByRole("list");
  await expect(list).toBeVisible();
  await expect(page.getByText(/match$/)).toBeVisible();

  await page.getByRole("listitem").first().click();
  await expect(page.getByRole("columnheader", { name: "Target" })).toBeVisible();
  // `exact: true` on both — "orders" is a substring of "orders_v2", and the
  // fully-qualified-name row also contains "orders_v2"/"orders" as a
  // substring of a longer cell value, which a non-exact match also catches.
  await expect(page.getByRole("cell", { name: "orders_v2", exact: true })).toBeVisible();
  await expect(page.getByRole("cell", { name: "orders", exact: true })).toBeVisible();

  // Reject requires a reason — the button stays disabled until one is typed.
  await page.getByRole("button", { name: "Reject" }).click();
  const rejectOk = page.getByRole("dialog").getByRole("button", { name: "Reject" });
  await expect(rejectOk).toBeDisabled();
  await page.getByPlaceholder("Why is this not a match?").fill("different tables, coincidental column overlap");
  await expect(rejectOk).toBeEnabled();
  await rejectOk.click();

  await expect(page.getByText("Rejected.")).toBeVisible();

  const axeResults = await new AxeBuilder({ page }).analyze();
  expect(axeResults.violations, JSON.stringify(axeResults.violations, null, 2)).toEqual([]);
});

test("a rejected candidate does not reappear in the pending queue", async ({ page, baseURL }) => {
  const base = baseURL ?? "";
  const { b } = await ambiguousPair(base, "review-2");
  const entry = await queueEntryFor(base, b.id);

  await fetch(`${base}/resolution/queue/${entry.id}/reject`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ reason: "not a match" }),
  });

  // The claim itself, checked directly against the API a fresh resolution
  // run would query: rejected means gone from pending, not merely absent
  // from whatever the page happened to render.
  const pendingAfter = (await fetch(`${base}/resolution/queue?status=pending`).then((r) => r.json())) as {
    data: { id: string }[];
  };
  expect(pendingAfter.data.some((e) => e.id === entry.id)).toBe(false);

  // Viewable later, not just gone — "does not reappear" means decided and
  // hidden from pending, not decided and lost.
  await page.goto("/?section=review");
  await page.locator(".ant-segmented-item", { hasText: "Rejected" }).click();
  await expect(page.getByText("Reason: not a match")).toBeVisible();
});

test("two reviewers on the same candidate: the second sees the resolution, not a conflict error", async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? "";
  const { b } = await ambiguousPair(base, "review-3");
  const entry = await queueEntryFor(base, b.id);

  await page.goto("/?section=review");
  await page.getByRole("listitem").first().click();
  await expect(page.getByRole("columnheader", { name: "Target" })).toBeVisible();

  // A second reviewer decides the identical entry directly, out from under
  // the page already open in this browser.
  await fetch(`${base}/resolution/queue/${entry.id}/confirm`, { method: "POST" });

  await page.getByRole("button", { name: "Reject" }).click();
  await page.getByPlaceholder("Why is this not a match?").fill("too late, but trying anyway");
  await page.getByRole("dialog").getByRole("button", { name: "Reject" }).click();

  await expect(page.getByText("Someone else already decided this candidate.")).toBeVisible();
});
