import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "../../api";
import {
  classificationVocabulary,
  domainVocabulary,
  glossaryVocabulary,
  ontologyPackVocabulary,
  type VocabularyConfig,
} from "./vocabularies";
import { buildVocabularyTree } from "./vocabularyTree";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("glossaryVocabulary", () => {
  it("names itself distinctly from every other vocabulary", () => {
    const config = glossaryVocabulary("g1");
    expect(config.key).toBe("glossary");
    expect(config.label).toBe("Glossary");
    expect(config.treeLabel).toBe("Glossary terms");
    expect(config.emptyTitle).toBe("No terms yet");
    expect(config.emptyDescription).toBe(
      "This glossary has no terms. Add one to start building its hierarchy.",
    );
  });

  it("shapes terms, their relations (labelled for display) and details for the detail pane", async () => {
    vi.spyOn(api, "glossaryTerms").mockResolvedValue([
      {
        id: "t1",
        glossaryId: "g1",
        name: "Customer",
        fullyQualifiedName: "g1.Customer",
        definition: "A party that purchases goods or services.",
        status: "approved",
        synonyms: [],
        abbreviations: [],
        version: "1.0",
        createdAt: "",
        updatedAt: "",
      },
      {
        id: "t2",
        glossaryId: "g1",
        name: "Individual customer",
        fullyQualifiedName: "g1.IndividualCustomer",
        definition: "A customer who is a natural person.",
        status: "draft",
        synonyms: [],
        abbreviations: [],
        version: "1.0",
        createdAt: "",
        updatedAt: "",
      },
    ]);
    vi.spyOn(api, "termRelations").mockImplementation(async (id: string) =>
      id === "t2" ? [{ kind: "broader", target: "t1" }] : [],
    );
    vi.spyOn(api, "termUsage").mockResolvedValue({ data: ["svc.db.public.customers"], paging: { after: null } });

    const config = glossaryVocabulary("g1");
    const data = await config.fetchData();
    expect(data.items).toEqual([
      { id: "t1", name: "Customer", raw: expect.objectContaining({ id: "t1" }) },
      { id: "t2", name: "Individual customer", raw: expect.objectContaining({ id: "t2" }) },
    ]);

    const detail = await config.detailFor("t1", data);
    expect(detail).toEqual({
      title: "Customer",
      fields: [
        { label: "Definition", value: "A party that purchases goods or services." },
        { label: "Status", value: "approved" },
      ],
      relationsLabel: "Relations",
      relations: [],
      usageLabel: "Used on",
      usage: ["svc.db.public.customers"],
    });

    // The *other* term's own name/relation — not `t1`'s — proves `.find`
    // is genuinely keying by the id argument rather than always returning
    // whichever item happens to be first.
    const childDetail = await config.detailFor("t2", data);
    expect(childDetail?.title).toBe("Individual customer");
    expect(childDetail?.relations).toEqual([{ label: "broader", target: "t1" }]);
  });

  it("returns null for an id nothing was fetched for, rather than throwing", async () => {
    vi.spyOn(api, "glossaryTerms").mockResolvedValue([]);
    const config = glossaryVocabulary("g1");
    const data = await config.fetchData();
    expect(await config.detailFor("does-not-exist", data)).toBeNull();
  });

  it("builds a real poly-hierarchy tree from what it fetches — proving the shape is compatible, not just typed the same", async () => {
    vi.spyOn(api, "glossaryTerms").mockResolvedValue([
      { id: "revenue", glossaryId: "g1", name: "Revenue", fullyQualifiedName: "g1.Revenue", definition: "", status: "approved", synonyms: [], abbreviations: [], version: "1.0", createdAt: "", updatedAt: "" },
      { id: "finance", glossaryId: "g1", name: "Finance", fullyQualifiedName: "g1.Finance", definition: "", status: "approved", synonyms: [], abbreviations: [], version: "1.0", createdAt: "", updatedAt: "" },
    ]);
    vi.spyOn(api, "termRelations").mockImplementation(async (id: string) =>
      id === "revenue" ? [{ kind: "broader", target: "finance" }] : [],
    );

    const config = glossaryVocabulary("g1");
    const data = await config.fetchData();
    const tree = buildVocabularyTree(data.items, data.relationsByItem);

    expect(tree.roots).toHaveLength(1);
    expect(tree.roots[0]?.termId).toBe("finance");
    expect(tree.roots[0]?.children[0]?.termId).toBe("revenue");
  });

  it("falls back to an empty relations list for a term whose id was never fetched", async () => {
    // Distinguishes the `?? []` fallback from "the map really does hold an
    // empty array for this id" — a mutant that replaces the fallback with
    // a placeholder value would still pass a test that never looks up a
    // genuinely-absent key.
    vi.spyOn(api, "glossaryTerms").mockResolvedValue([
      { id: "t1", glossaryId: "g1", name: "Customer", fullyQualifiedName: "g1.Customer", definition: "d", status: "approved", synonyms: [], abbreviations: [], version: "1.0", createdAt: "", updatedAt: "" },
    ]);
    vi.spyOn(api, "termRelations").mockResolvedValue([]);
    vi.spyOn(api, "termUsage").mockResolvedValue({ data: [], paging: { after: null } });

    const config = glossaryVocabulary("g1");
    const data = await config.fetchData();
    // Force a lookup against an id `relationsByItem` never received.
    const bareData = { items: data.items, relationsByItem: new Map() };
    const detail = await config.detailFor("t1", bareData);
    expect(detail?.relations).toEqual([]);
  });

  it("labels every SKOS relation kind, not only broader", async () => {
    vi.spyOn(api, "glossaryTerms").mockResolvedValue([
      { id: "t1", glossaryId: "g1", name: "Customer", fullyQualifiedName: "g1.Customer", definition: "", status: "approved", synonyms: [], abbreviations: [], version: "1.0", createdAt: "", updatedAt: "" },
    ]);
    vi.spyOn(api, "termRelations").mockResolvedValue([
      { kind: "narrower", target: "a" },
      { kind: "related", target: "b" },
      { kind: "exactMatch", target: "c" },
      { kind: "closeMatch", target: "d" },
    ]);
    vi.spyOn(api, "termUsage").mockResolvedValue({ data: [], paging: { after: null } });

    const config = glossaryVocabulary("g1");
    const data = await config.fetchData();
    const detail = await config.detailFor("t1", data);
    expect(detail?.relations).toEqual([
      { label: "narrower", target: "a" },
      { label: "related", target: "b" },
      { label: "exact match", target: "c" },
      { label: "close match", target: "d" },
    ]);
  });
});

describe("ontologyPackVocabulary", () => {
  const pack = {
    id: "p1",
    packId: "fibo",
    version: "1.0",
    licence: { kind: "permissive" as const, name: "CC0" },
    sourceUrl: "https://example.org/fibo.ttl",
    glossaryId: "pack-glossary-1",
    termCount: 0,
  };

  it("reads the pack's own glossary through the identical glossary calls, not a second API", async () => {
    const glossaryTerms = vi.spyOn(api, "glossaryTerms").mockResolvedValue([]);
    const config = ontologyPackVocabulary(pack);
    await config.fetchData();
    expect(glossaryTerms).toHaveBeenCalledWith("pack-glossary-1");
  });

  it("carries a read-only notice naming the pack, unlike a real glossary", () => {
    const glossary = glossaryVocabulary("g1");
    const config = ontologyPackVocabulary(pack);

    expect(glossary.readOnlyNotice).toBeUndefined();
    expect(config.readOnlyNotice).toContain("fibo");
    expect(config.readOnlyNotice).toContain(pack.sourceUrl);
  });

  it("names itself after the pack, distinctly from the glossary it reuses", () => {
    const config = ontologyPackVocabulary(pack);
    expect(config.key).toBe("ontology-pack");
    expect(config.label).toBe("Pack: fibo");
    expect(config.treeLabel).toBe("fibo 1.0 terms");
    expect(config.emptyTitle).toBe("No terms imported");
    expect(config.emptyDescription).toBe("This pack version imported no terms.");
  });
});

describe("classificationVocabulary", () => {
  function fixture() {
    vi.spyOn(api, "classifications").mockResolvedValue([
      { id: "tier", name: "Tier", description: "Data quality tier.", mutuallyExclusive: true },
      { id: "pii", name: "PII", description: "Personally identifying.", mutuallyExclusive: false },
    ]);
    vi.spyOn(api, "tags").mockResolvedValue([
      { id: "gold", name: "Gold", classificationId: "tier", fullyQualifiedName: "Tier.Gold", description: null },
      { id: "bronze", name: "Bronze", classificationId: "tier", fullyQualifiedName: "Tier.Bronze", description: null },
      { id: "sensitive", name: "Sensitive", classificationId: "pii", fullyQualifiedName: "PII.Sensitive", description: null },
      { id: "restricted", name: "Restricted", classificationId: "pii", fullyQualifiedName: "PII.Restricted", description: null },
    ]);
  }

  it("names itself distinctly from the other vocabularies", () => {
    const config = classificationVocabulary();
    expect(config.key).toBe("classification");
    expect(config.label).toBe("Classifications");
    expect(config.treeLabel).toBe("Classifications and tags");
    expect(config.emptyTitle).toBe("No classifications yet");
    expect(config.emptyDescription).toBe("No classification vocabulary has been declared yet.");
  });

  it("places every classification as a root and every tag as its child, via a synthetic broader relation", async () => {
    fixture();
    const config = classificationVocabulary();
    const data = await config.fetchData();
    const tree = buildVocabularyTree(data.items, data.relationsByItem);

    const tierRoot = tree.roots.find((r) => r.termId === "tier");
    expect(tierRoot?.children.map((c) => c.termId).sort()).toEqual(["bronze", "gold"]);
  });

  it("shows an empty description, not a placeholder, for a classification that never had one", async () => {
    vi.spyOn(api, "classifications").mockResolvedValue([
      { id: "undocumented", name: "Undocumented", description: null, mutuallyExclusive: false },
    ]);
    vi.spyOn(api, "tags").mockResolvedValue([]);
    const config = classificationVocabulary();
    const data = await config.fetchData();
    const detail = await config.detailFor("undocumented", data);
    expect(detail?.fields).toContainEqual({ label: "Description", value: "" });
  });

  it("shows a classification's own description and whether it is exclusive, selected as itself rather than as a tag", async () => {
    fixture();
    const config = classificationVocabulary();
    const data = await config.fetchData();

    const exclusive = await config.detailFor("tier", data);
    expect(exclusive).toEqual({
      title: "Tier",
      fields: [
        { label: "Description", value: "Data quality tier." },
        {
          label: "Mutually exclusive",
          value: "Yes — only one of its tags may be applied to an entity at a time",
        },
      ],
      relationsLabel: "",
      relations: [],
      usageLabel: "",
      usage: [],
    });

    const nonExclusive = await config.detailFor("pii", data);
    expect(nonExclusive?.fields).toContainEqual({
      label: "Mutually exclusive",
      value: "No — several of its tags may apply at once",
    });
  });

  it("returns null for an id nothing was fetched for, rather than throwing", async () => {
    fixture();
    const config = classificationVocabulary();
    const data = await config.fetchData();
    expect(await config.detailFor("does-not-exist", data)).toBeNull();
  });

  it("names the tags an exclusive classification's tag conflicts with, joined by comma — never a tag from a different classification", async () => {
    // `fixture()`'s own `pii` tags (`sensitive`, `restricted`) stay in the
    // mix here specifically so a mutant that stops checking *which*
    // classification a candidate tag belongs to — matching every tag
    // anywhere, not just this one's own siblings — has something to get
    // wrong.
    fixture();
    vi.spyOn(api, "tags").mockResolvedValue([
      { id: "gold", name: "Gold", classificationId: "tier", fullyQualifiedName: "Tier.Gold", description: null },
      { id: "bronze", name: "Bronze", classificationId: "tier", fullyQualifiedName: "Tier.Bronze", description: null },
      { id: "silver", name: "Silver", classificationId: "tier", fullyQualifiedName: "Tier.Silver", description: null },
      { id: "sensitive", name: "Sensitive", classificationId: "pii", fullyQualifiedName: "PII.Sensitive", description: null },
    ]);
    vi.spyOn(api, "tagUsage").mockResolvedValue({ total: 0, byKind: [] });
    const config = classificationVocabulary();
    const data = await config.fetchData();

    // Two conflicts, not one — the only way to observe the join separator
    // between them at all.
    const detail = await config.detailFor("gold", data);
    expect(detail).toEqual({
      title: "Gold",
      fields: [
        { label: "Description", value: "" },
        { label: "Conflicts with", value: "Bronze, Silver" },
      ],
      relationsLabel: "",
      relations: [],
      usageLabel: "Used on",
      usage: [],
    });
  });

  it("does not treat a tag whose classification is missing from what was fetched as a crash — nor as a conflict", async () => {
    fixture();
    vi.spyOn(api, "tags").mockResolvedValue([
      { id: "orphan", name: "Orphan", classificationId: "does-not-exist", fullyQualifiedName: "Ghost.Orphan", description: null },
    ]);
    vi.spyOn(api, "tagUsage").mockResolvedValue({ total: 0, byKind: [] });
    const config = classificationVocabulary();
    const data = await config.fetchData();

    const detail = await config.detailFor("orphan", data);
    expect(detail?.fields.some((f) => f.label === "Conflicts with")).toBe(false);
  });

  it("returns null for a raw record that is neither a classification nor a tag", async () => {
    fixture();
    const config = classificationVocabulary();
    const data = await config.fetchData();
    const corrupted = {
      items: [...data.items, { id: "mystery", name: "Mystery", raw: { unrelated: true } }],
      relationsByItem: data.relationsByItem,
    };
    expect(await config.detailFor("mystery", corrupted)).toBeNull();
  });

  it("does not throw when a raw record is not even an object — the `typeof` guard is load-bearing, not decorative", async () => {
    // `"x" in 42` throws a real `TypeError`; `isTag`/`isClassification`
    // short-circuit on `typeof raw === "object"` specifically so a
    // primitive `raw` is answered "no" rather than crashing the detail
    // pane. A mutant that drops that check to `true` never reaches this
    // through any object-shaped fixture — only a genuinely non-object
    // `raw` exercises it.
    fixture();
    const config = classificationVocabulary();
    const data = await config.fetchData();
    const corrupted = {
      items: [...data.items, { id: "primitive", name: "Primitive", raw: 42 }],
      relationsByItem: data.relationsByItem,
    };
    await expect(config.detailFor("primitive", corrupted)).resolves.toBeNull();
  });

  it("does not throw when a raw record is null specifically", async () => {
    // `typeof null === "object"` is JavaScript's own famous exception, so
    // the `typeof` check alone does not rule `null` out — the separate
    // `raw !== null` check is what does, and needs its own fixture rather
    // than riding along on the `raw: 42` case above.
    fixture();
    const config = classificationVocabulary();
    const data = await config.fetchData();
    const corrupted = {
      items: [...data.items, { id: "null-raw", name: "Null raw", raw: null }],
      relationsByItem: data.relationsByItem,
    };
    await expect(config.detailFor("null-raw", corrupted)).resolves.toBeNull();
  });

  it("does not claim a conflict for a tag under a non-exclusive classification that also has siblings", async () => {
    // Two `PII` tags, not one — a classification with only one tag would
    // report an empty conflict list whether or not exclusivity was even
    // checked, which proves nothing about the `mutuallyExclusive` branch
    // specifically.
    fixture();
    vi.spyOn(api, "tagUsage").mockResolvedValue({ total: 0, byKind: [] });
    const config = classificationVocabulary();
    const data = await config.fetchData();

    const detail = await config.detailFor("sensitive", data);
    expect(detail?.fields).toEqual([{ label: "Description", value: "" }]);
  });

  it("reports a tag's usage by kind, not as a list of individual FQNs", async () => {
    fixture();
    vi.spyOn(api, "tagUsage").mockResolvedValue({ total: 5, byKind: [{ kind: "column", count: 5 }] });
    const config = classificationVocabulary();
    const data = await config.fetchData();

    const detail = await config.detailFor("gold", data);
    expect(detail?.usage).toEqual(["5 column"]);
  });
});

describe("domainVocabulary", () => {
  it("names itself distinctly from the other vocabularies", () => {
    const config = domainVocabulary();
    expect(config.key).toBe("domain");
    expect(config.label).toBe("Domains");
    expect(config.treeLabel).toBe("Domains");
    expect(config.emptyTitle).toBe("No domains yet");
    expect(config.emptyDescription).toBe("No domain has been declared yet.");
  });

  it("nests a domain under its parent via parentId, and shows only its own data products", async () => {
    vi.spyOn(api, "domains").mockResolvedValue({
      data: [
        { id: "eng", name: "Engineering", fullyQualifiedName: "Engineering", parentId: null, description: "The engineering org.", domainType: "source-aligned", experts: ["asha", "priya"] },
        { id: "platform", name: "Platform", fullyQualifiedName: "Engineering.Platform", parentId: "eng", description: null, domainType: null, experts: [] },
      ],
      paging: { after: null },
    });
    vi.spyOn(api, "dataProducts").mockResolvedValue({
      data: [
        { id: "dp1", name: "Billing events", fullyQualifiedName: "billing-events", description: null, purpose: null, domainId: "platform" },
        { id: "dp2", name: "Marketing leads", fullyQualifiedName: "marketing-leads", description: null, purpose: null, domainId: "eng" },
      ],
      paging: { after: null },
    });

    const config = domainVocabulary();
    const data = await config.fetchData();
    const tree = buildVocabularyTree(data.items, data.relationsByItem);
    expect(tree.roots).toHaveLength(1);
    expect(tree.roots[0]?.children[0]?.termId).toBe("platform");
    // A root domain (`parentId: null`) must carry no synthetic relation at
    // all — not an empty-but-present one — or a mutant that always
    // records one regardless of `parentId` would still leave the tree
    // shape above looking identical (`vocabularyTree.ts`'s own target-
    // filtering absorbs a bogus `null` target silently).
    expect(data.relationsByItem.has("eng")).toBe(false);

    const engDetail = await config.detailFor("eng", data);
    expect(engDetail).toEqual({
      title: "Engineering",
      fields: [
        { label: "Description", value: "The engineering org." },
        { label: "Type", value: "source-aligned" },
        { label: "Experts", value: "asha, priya" },
      ],
      relationsLabel: "",
      relations: [],
      usageLabel: "Data products",
      usage: ["Marketing leads"],
    });

    const platformDetail = await config.detailFor("platform", data);
    expect(platformDetail?.fields).toEqual([
      { label: "Description", value: "" },
      { label: "Type", value: "" },
      { label: "Experts", value: "" },
    ]);
    expect(platformDetail?.usage).toEqual(["Billing events"]);
  });

  it("returns null for an id nothing was fetched for, rather than throwing", async () => {
    vi.spyOn(api, "domains").mockResolvedValue({ data: [], paging: { after: null } });
    const config = domainVocabulary();
    const data = await config.fetchData();
    expect(await config.detailFor("does-not-exist", data)).toBeNull();
  });
});

/** **The RED test the plan itself names**: adding a fifth, fictional
 *  vocabulary must render through the identical `VocabularyBrowser`/
 *  `buildVocabularyTree` machinery, using nothing this file does not
 *  already export as a public shape. Testing only the four real
 *  vocabularies cannot distinguish a parameterized component from one with
 *  four hardcoded paths — this fixture is deliberately not a real feature. */
describe("a fifth, fictional vocabulary", () => {
  it("renders through config alone, proving the pattern generalizes rather than assuming it", async () => {
    const fictional: VocabularyConfig = {
      key: "fictional-test-vocabulary",
      label: "Fictional",
      treeLabel: "Fictional items",
      emptyTitle: "empty",
      emptyDescription: "empty",
      fetchData: async () => ({
        items: [
          { id: "root", name: "Root", raw: null },
          { id: "child", name: "Child", raw: null },
        ],
        relationsByItem: new Map([["child", [{ kind: "broader", target: "root" }]]]),
      }),
      detailFor: async (itemId, data) => {
        const item = data.items.find((i) => i.id === itemId);
        return item
          ? { title: item.name, fields: [], relationsLabel: "", relations: [], usageLabel: "", usage: [] }
          : null;
      },
    };

    const data = await fictional.fetchData();
    const tree = buildVocabularyTree(data.items, data.relationsByItem);
    expect(tree.roots[0]?.termId).toBe("root");
    expect(tree.roots[0]?.children[0]?.termId).toBe("child");

    const detail = await fictional.detailFor("child", data);
    expect(detail?.title).toBe("Child");
  });
});
