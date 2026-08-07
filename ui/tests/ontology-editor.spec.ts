import { expect, test } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

/** Epic 42 Slice G's own named RED test: a half-typed edit must not blank
 *  the picture the author was just looking at. `applyParseOutcome`
 *  (`ontologyDocument.ts`) is unit- and mutation-tested for this already;
 *  this spec is the same claim made against real keystrokes, a real
 *  debounce timer, and a real server — the syntax-error banner must appear
 *  *beside* the last good graph, never instead of it.
 *
 *  Also covers two real bugs a manual pass through the running app found
 *  (neither caught by the unit suite, since both are wire-shape bugs a
 *  round trip cannot see): `dry_run_rdf_edit`'s `accepted` list was
 *  emitting `Sid`'s internal `1:Widget` form instead of the IRI a Turtle
 *  author actually wrote, and `RdfEditDryRun::Checked`'s `new_inferences`
 *  was reaching the wire as snake_case beside every camelCase sibling
 *  field, so the console printed "New inferences: undefined". */

const VALID_DOCUMENT = `@prefix ex: <https://graph-owl.dev/ns/catalog#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

ex:OeWidget a ex:OeProduct ;
    rdfs:subClassOf ex:OeItem ;
    ex:name "An ontology-editor widget" .
`;

const UNTERMINATED_DOCUMENT = `@prefix ex: <https://graph-owl.dev/ns/catalog#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

ex:OeWidget a ex:OeProduct ;
    rdfs:subClassOf ex:OeItem ;
    ex:name "unterminated
`;

async function openOntologyEditor(page: import("@playwright/test").Page) {
  await page.goto("/?section=workbench");
  await page.locator(".ant-segmented-item", { hasText: "Ontology editor" }).click();
  await expect(page.getByRole("heading", { name: "Ontology editor" })).toBeVisible();
}

test("a syntax error keeps the last good graph on screen and names its own line, zero axe violations", async ({
  page,
}) => {
  await openOntologyEditor(page);
  const editor = page.getByPlaceholder(/Write Turtle/);

  await expect(page.getByText("Nothing parses yet")).toBeVisible();

  await editor.fill(VALID_DOCUMENT);
  const graph = page.getByRole("img", { name: "The ontology this document declares" });
  await expect(graph).toBeVisible();
  await expect(page.getByText("Nothing parses yet")).not.toBeVisible();

  // The RED test: a half-typed edit must not blank the graph the author
  // was just looking at.
  await editor.fill(UNTERMINATED_DOCUMENT);
  await expect(page.getByText(/^Syntax error — line \d+/)).toBeVisible();
  await expect(graph).toBeVisible();
  await expect(page.getByText("Nothing parses yet")).not.toBeVisible();

  const axeResults = await new AxeBuilder({ page }).analyze();
  expect(axeResults.violations, JSON.stringify(axeResults.violations, null, 2)).toEqual([]);
});

test("fixing the syntax error clears the banner and Check reports a real IRI, not Sid's internal form", async ({
  page,
}) => {
  await openOntologyEditor(page);
  const editor = page.getByPlaceholder(/Write Turtle/);

  await editor.fill(UNTERMINATED_DOCUMENT);
  await expect(page.getByText(/^Syntax error/)).toBeVisible();

  await editor.fill(VALID_DOCUMENT);
  await expect(page.getByText(/^Syntax error/)).not.toBeVisible();

  await page.getByRole("button", { name: "Check" }).click();
  await expect(page.getByText("Would be accepted:")).toBeVisible();
  // The regression: this used to read "1:OeWidget" — Sid's own internal
  // `{namespace_code}:{id}` form — because `dry_run_rdf_edit` displayed the
  // subject with `Sid`'s `Display` impl instead of `to_iri()`.
  await expect(page.getByText("https://graph-owl.dev/ns/catalog#OeWidget")).toBeVisible();
  await expect(page.getByText(/^1:OeWidget$/)).toHaveCount(0);
  // The regression: this used to read "New inferences: undefined" because
  // the enum's `rename_all` only renames variant tags, not the fields
  // inside them — `new_inferences` needs `rename_all_fields` too.
  await expect(page.getByText(/^New inferences: \d+$/)).toBeVisible();
  await expect(page.getByText("New inferences: undefined")).toHaveCount(0);
});

test("Save writes the document, and a later Check still reports it as accepted, not silently dropped", async ({
  page,
}) => {
  await openOntologyEditor(page);
  const editor = page.getByPlaceholder(/Write Turtle/);

  await editor.fill(VALID_DOCUMENT);
  await expect(page.getByText("Nothing parses yet")).not.toBeVisible();

  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Saved")).toBeVisible();
  await expect(page.getByText(/subject.*landed/)).toBeVisible();

  // `dry_run_rdf_edit` must not reuse `import_rdf`'s own dedup, which skips
  // a subject already present under this source — that is correct for "an
  // external batch was not reimported", and wrong for "the author saved,
  // then immediately re-checks", where it would silently misreport
  // "accepted" as absent.
  await page.getByRole("button", { name: "Check" }).click();
  await expect(page.getByText("Would be accepted:")).toBeVisible();
  await expect(page.getByText("https://graph-owl.dev/ns/catalog#OeWidget")).toBeVisible();
});

test("the namespace filter lists every namespace the document uses", async ({ page }) => {
  await openOntologyEditor(page);
  const editor = page.getByPlaceholder(/Write Turtle/);

  await editor.fill(VALID_DOCUMENT);
  await expect(page.getByRole("img", { name: "The ontology this document declares" })).toBeVisible();

  await page.getByRole("combobox", { name: "Filter by namespace" }).click();
  await expect(page.getByRole("option", { name: "https://graph-owl.dev/ns/catalog#" })).toBeVisible();
  await expect(
    page.getByRole("option", { name: "http://www.w3.org/2000/01/rdf-schema#" }),
  ).toBeVisible();
});

test("switching back to Query keeps the SPARQL panel mounted, not reset", async ({ page }) => {
  await page.goto("/?section=workbench");
  // Both views stay mounted (`display: none` on the inactive one, never
  // conditional JSX — the same pattern `KnowledgeGraphToggle` established),
  // so `textarea` alone matches both the SPARQL box and the ontology
  // editor's own textarea regardless of which is on screen. `:visible`
  // is the one predicate that actually encodes "whichever view is active".
  const sparqlBox = page.locator("textarea:visible").first();
  await sparqlBox.fill("SELECT ?s WHERE { ?s a <https://graph-owl.dev/ns/catalog#OeWidget> }");

  await page.locator(".ant-segmented-item", { hasText: "Ontology editor" }).click();
  await expect(page.getByRole("heading", { name: "Ontology editor" })).toBeVisible();

  await page.locator(".ant-segmented-item", { hasText: "Query" }).click();
  await expect(sparqlBox).toHaveValue(
    "SELECT ?s WHERE { ?s a <https://graph-owl.dev/ns/catalog#OeWidget> }",
  );
});
