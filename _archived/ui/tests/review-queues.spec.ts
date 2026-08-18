import { expect, test } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

/** Epic 42 Slice D: generalizing the review queue to extraction claims and
 *  drift, proved against a real server — the same "seed through the real
 *  endpoint" pattern every other spec in this project already uses.
 *  Proposals (Epic 35) is not wired in: there is no catalog-wide pending
 *  listing endpoint, only per-entity and per-user ones, and the frontend
 *  has no "who am I" endpoint to resolve the latter — see
 *  `ReviewSection.tsx`'s own header comment. */

async function createAsset(baseURL: string, kind: string, name: string) {
  const response = await fetch(`${baseURL}/assets`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ kind, name }),
  });
  return (await response.json()) as { id: string; fullyQualifiedName: string };
}

async function submitExtraction(baseURL: string, subjectFqn: string, extractorSuffix: string) {
  const text = `The ${subjectFqn} team confirmed that revenue grew this quarter.`;
  const start = text.indexOf("revenue grew");
  const end = start + "revenue grew".length;
  const response = await fetch(`${baseURL}/extraction/runs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      document: { sourceId: `doc-${extractorSuffix}`, mediaType: "text/plain", text },
      result: {
        claims: [
          {
            subject: subjectFqn,
            predicate: "description",
            object: "revenue grew",
            confidence: 0.65,
            provenance: {
              sourceId: `doc-${extractorSuffix}`,
              extractor: `playwright-extractor-${extractorSuffix}`,
              extractorVersion: "1.0.0",
              extractedAt: new Date(0).toISOString(),
              evidence: { start, end },
            },
          },
        ],
      },
      extractor: `playwright-extractor-${extractorSuffix}`,
      extractorVersion: "1.0.0",
    }),
  });
  return response.json();
}

async function pushDrift(baseURL: string, fqn: string, declaredValue: string) {
  const response = await fetch(`${baseURL}/drift/reports`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      items: [{ fullyQualifiedName: fqn, field: "description", kind: "unapplied", liveValue: null, declaredValue }],
    }),
  });
  return (await response.json()) as { id: string }[];
}

test("extraction claims: the source passage renders with the extracted span highlighted, zero axe violations", async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? "";
  const asset = await createAsset(base, "service", "playwright-extraction-svc");
  await submitExtraction(base, asset.fullyQualifiedName, "provenance-check");

  await page.goto("/?section=review&kind=extraction");

  await page.getByRole("listitem").first().click();
  // The extraction-provenance RED test itself: a claim without its source
  // sentence visible is unreviewable. The passage section must be on
  // screen, and specifically the extracted phrase within it — not just a
  // confidence score, and not merely present anywhere on the page (the
  // list row's own summary line also contains this phrase as raw text).
  await expect(page.getByText("Source passage")).toBeVisible();
  await expect(page.locator("mark", { hasText: "revenue grew" })).toBeVisible();

  const axeResults = await new AxeBuilder({ page }).analyze();
  expect(axeResults.violations, JSON.stringify(axeResults.violations, null, 2)).toEqual([]);
});

test("extraction claims: accepting one removes it from the pending list", async ({ page, baseURL }) => {
  const base = baseURL ?? "";
  const asset = await createAsset(base, "service", "playwright-extraction-accept-svc");
  await submitExtraction(base, asset.fullyQualifiedName, "accept-check");

  await page.goto("/?section=review&kind=extraction");
  // Scoped to this test's own claim by its unique asset name, rather than
  // `.first()` — another claim already pending from an earlier test in
  // this file would otherwise be the one accepted.
  const row = page.getByRole("listitem").filter({ hasText: asset.fullyQualifiedName });
  await expect(row).toHaveCount(1);

  await row.click();
  await page.getByRole("button", { name: "Accept" }).click();
  await expect(page.getByText("Accepted.")).toBeVisible();

  await expect(page.getByRole("listitem").filter({ hasText: asset.fullyQualifiedName })).toHaveCount(0);
});

test("drift: shows declared vs live as a diff, and apply writes the declared value through", async ({
  page,
  baseURL,
}) => {
  const base = baseURL ?? "";
  const asset = await createAsset(base, "service", "playwright-drift-svc");
  await pushDrift(base, asset.fullyQualifiedName, "Declared via metadata-as-code.");

  await page.goto("/?section=review&kind=drift");

  await page.getByRole("listitem").first().click();
  await expect(page.getByText("Declared via metadata-as-code.")).toBeVisible();
  await expect(page.getByText("(none)")).toBeVisible();

  await page.getByRole("button", { name: "Apply declared value" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Apply declared value" }).click();
  await expect(page.getByText("Applied.")).toBeVisible();
  // The confirm Modal's own close animation — its `role="dialog"` element
  // is still in the DOM mid-fade (`ant-zoom-leave-active`), and antd
  // detaches its `aria-labelledby` wiring before that animation finishes,
  // which axe's `aria-dialog-name` rule correctly flags if scanned then.
  // A settled dialog (confirmed by hand: open, the same element carries
  // `aria-labelledby` correctly) has no such gap.
  await expect(page.getByRole("dialog")).toHaveCount(0);

  const live = await fetch(`${base}/assets/${asset.id}`).then((r) => r.json());
  expect(live.description).toBe("Declared via metadata-as-code.");

  const axeResults = await new AxeBuilder({ page }).analyze();
  expect(axeResults.violations, JSON.stringify(axeResults.violations, null, 2)).toEqual([]);
});

test("drift: ignoring requires a reason, and the entry moves to the Ignored tab", async ({ page, baseURL }) => {
  const base = baseURL ?? "";
  const asset = await createAsset(base, "service", "playwright-drift-ignore-svc");
  await pushDrift(base, asset.fullyQualifiedName, "A declared value nobody wants applied.");

  await page.goto("/?section=review&kind=drift");
  await page.getByRole("listitem").first().click();

  await page.getByRole("button", { name: "Ignore" }).click();
  const ignoreOk = page.getByRole("dialog").getByRole("button", { name: "Ignore" });
  await expect(ignoreOk).toBeDisabled();
  await page.getByPlaceholder("Why should this drift stand?").fill("the live value is intentional");
  await expect(ignoreOk).toBeEnabled();
  await ignoreOk.click();
  await expect(page.getByText("Ignored.")).toBeVisible();

  await page.locator(".ant-segmented-item", { hasText: "Ignored" }).click();
  await expect(page.getByText("Reason: the live value is intentional")).toBeVisible();
});
