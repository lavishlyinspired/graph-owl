/** Category glyphs for business-entity nodes, derived from the entity's own
 *  name — free-text categories (person/organization/location/money/event/
 *  document/generic), not a closed technical taxonomy.
 *
 *  Lives in `graph/` (not a feature folder) because two callers share it:
 *  the Ontology Builder's entity types (`EntityType` has no `kind` field —
 *  it never asked the user to classify entities into a fixed taxonomy, and
 *  adding one is a model change, not a styling one) and the Explorer's
 *  pack-subject nodes, whose `semanticType` is exactly the same kind of
 *  open-ended, free-text name. The Explorer's *catalog-asset* nodes
 *  (`AssetKind`: service/database/schema/table/column) are a separate,
 *  closed, technical taxonomy with no natural mapping onto these
 *  business-entity glyphs — those get their own icon set where they are
 *  used, not shoehorned in here.
 *
 *  Every glyph below is an original, hand-authored geometric composition —
 *  not sourced from any icon library or external reference. */

export type EntityIconCategory =
  | "person"
  | "organization"
  | "location"
  | "event"
  | "money"
  | "document"
  | "generic";

const CATEGORY_KEYWORDS: readonly (readonly [
  Exclude<EntityIconCategory, "generic">,
  readonly string[],
])[] = [
  [
    "person",
    ["person", "user", "customer", "contact", "employee", "individual", "member", "citizen"],
  ],
  [
    "organization",
    ["organization", "organisation", "company", "supplier", "vendor", "business", "account", "party", "agency"],
  ],
  ["location", ["location", "address", "site", "place", "region", "branch", "warehouse"]],
  ["money", ["payment", "invoice", "amount", "tax", "price", "receipt", "return", "ledger"]],
  ["event", ["event", "transaction", "activity", "interaction", "meeting", "appointment", "filing"]],
  ["document", ["document", "order", "contract", "record", "form", "report", "file", "note", "statement"]],
];

export function iconCategoryFor(name: string): EntityIconCategory {
  const lower = name.toLowerCase();
  for (const [category, keywords] of CATEGORY_KEYWORDS) {
    if (keywords.some((keyword) => lower.includes(keyword))) return category;
  }
  return "generic";
}

/** Stroke-only, viewBox 0 0 24 24 — sized to sit inside a light-tinted
 *  circular node with room to spare. */
const GLYPH_BODY: Record<EntityIconCategory, string> = {
  person:
    '<circle cx="12" cy="8.2" r="3.2"/><path d="M5.3 19c1.2-3.7 4-5.6 6.7-5.6s5.5 1.9 6.7 5.6"/>',
  organization:
    '<rect x="5" y="9" width="14" height="10" rx="1"/><path d="M9 9V6.2a3 3 0 0 1 6 0V9"/>',
  location:
    '<path d="M12 20.5s6-5.9 6-10.7A6 6 0 0 0 6 9.8c0 4.8 6 10.7 6 10.7z"/><circle cx="12" cy="9.8" r="2.1"/>',
  money:
    '<circle cx="12" cy="12" r="8"/><path d="M12 7.2v9.6M9.6 9.6c0-1.4 1.2-2.4 2.6-2.4 1.6 0 2.7.9 2.7 2.1 0 2.9-5.3 1.5-5.3 4.4 0 1.2 1.1 2.1 2.7 2.1 1.5 0 2.6-.9 2.6-2.3"/>',
  event:
    '<rect x="4" y="6" width="16" height="14" rx="1.5"/><path d="M4 10h16M8 4v4M16 4v4"/>',
  document:
    '<path d="M7 3h7l4 4v14H7z"/><path d="M14 3v4h4"/><path d="M9.5 12.5h5M9.5 16h5"/>',
  generic: '<rect x="5" y="5" width="14" height="14" rx="4"/>',
};

/** An inline SVG data URI for the given category, coloured with `stroke`.
 *
 *  `width`/`height` on the root element are required, not decorative — an
 *  SVG with only a `viewBox` gets an ambiguous intrinsic size in some
 *  browsers, which Cytoscape's `background-fit`/`background-width` then
 *  scale from, producing a cropped, off-centre icon (verified live: adding
 *  these two attributes was the entire fix). */
export function iconDataUri(category: EntityIconCategory, stroke: string): string {
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" ` +
    `stroke="${stroke}" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">` +
    `${GLYPH_BODY[category]}</svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}
