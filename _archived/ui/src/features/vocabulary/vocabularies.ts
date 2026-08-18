/** Per-vocabulary configuration — Epic 42 Slice B, the proof of decision 1.
 *  `VocabularyBrowser.tsx` never names a vocabulary; every difference
 *  between glossary, classification, domain and ontology pack lives here,
 *  in `VocabularyConfig` implementations, or it does not exist at all.
 *
 *  **Every vocabulary reduces to the same two shapes `vocabularyTree.ts`
 *  already handles**: a flat `{id, name}` list, and each item's own
 *  `broader` relation(s). A classification's tags declare a synthetic
 *  `broader` pointing at their classification; a domain's `parentId`
 *  becomes the same thing. Neither needed a single line of new tree logic —
 *  which is the whole point of Slice A's tree being keyed by identity
 *  rather than by vocabulary shape. */

import { api, type Classification, type Domain, type GlossaryTerm, type OntologyPack, type Tag } from "../../api";
import type { Relation } from "./vocabularyTree";

export interface VocabularyItem {
  readonly id: string;
  readonly name: string;
  /** The full per-vocabulary record — opaque to `VocabularyBrowser.tsx`
   *  and to `vocabularyTree.ts`, which only ever read `id`/`name`. Each
   *  `VocabularyConfig` is the only code that ever reads its own `raw`. */
  readonly raw: unknown;
}

export interface VocabularyData {
  readonly items: readonly VocabularyItem[];
  readonly relationsByItem: ReadonlyMap<string, readonly Relation[]>;
}

export interface DetailField {
  readonly label: string;
  readonly value: string;
}

export interface VocabularyDetail {
  readonly title: string;
  readonly fields: readonly DetailField[];
  readonly relationsLabel: string;
  readonly relations: readonly { readonly label: string; readonly target: string }[];
  readonly usageLabel: string;
  readonly usage: readonly string[];
}

export interface VocabularyConfig {
  readonly key: string;
  readonly label: string;
  readonly treeLabel: string;
  readonly emptyTitle: string;
  readonly emptyDescription: string;
  /** Present only for a vocabulary nothing in this browser can write to —
   *  Epic 33 decision 3's own requirement for ontology packs: say so,
   *  rather than rendering controls that do not exist and are not merely
   *  disabled. */
  readonly readOnlyNotice?: string;
  fetchData(): Promise<VocabularyData>;
  detailFor(itemId: string, data: VocabularyData): Promise<VocabularyDetail | null>;
}

const RELATION_LABEL: Record<Relation["kind"], string> = {
  broader: "broader",
  narrower: "narrower",
  related: "related",
  exactMatch: "exact match",
  closeMatch: "close match",
};

function skosRelationsAsDetail(relations: readonly Relation[]) {
  return relations.map((r) => ({ label: RELATION_LABEL[r.kind], target: r.target }));
}

// ---- Glossary ----

export function glossaryVocabulary(glossaryId: string): VocabularyConfig {
  return {
    key: "glossary",
    label: "Glossary",
    treeLabel: "Glossary terms",
    emptyTitle: "No terms yet",
    emptyDescription: "This glossary has no terms. Add one to start building its hierarchy.",
    async fetchData() {
      const terms = await api.glossaryTerms(glossaryId);
      const relationsList = await Promise.all(terms.map((t) => api.termRelations(t.id)));
      const relationsByItem = new Map<string, readonly Relation[]>(
        terms.map((t, i) => [t.id, relationsList[i] ?? []]),
      );
      return {
        items: terms.map((t) => ({ id: t.id, name: t.name, raw: t })),
        relationsByItem,
      };
    },
    async detailFor(itemId, data) {
      const item = data.items.find((i) => i.id === itemId);
      if (!item) return null;
      const term = item.raw as GlossaryTerm;
      const relations = data.relationsByItem.get(itemId) ?? [];
      const usagePage = await api.termUsage(itemId);
      return {
        title: term.name,
        fields: [
          { label: "Definition", value: term.definition },
          { label: "Status", value: term.status },
        ],
        relationsLabel: "Relations",
        relations: skosRelationsAsDetail(relations),
        usageLabel: "Used on",
        usage: usagePage.data,
      };
    },
  };
}

// ---- Ontology packs ----

/** A pack's terms are ordinary glossary terms in the pack's own
 *  pack-owned glossary (`graph_owl_ontology::pack::OntologyPack.glossary_id`,
 *  Epic 33 decision 4) — read through the identical glossary calls, not a
 *  second term API. Only the labelling and the read-only notice differ. */
export function ontologyPackVocabulary(pack: OntologyPack): VocabularyConfig {
  const glossary = glossaryVocabulary(pack.glossaryId);
  return {
    ...glossary,
    key: "ontology-pack",
    label: `Pack: ${pack.packId}`,
    treeLabel: `${pack.packId} ${pack.version} terms`,
    emptyTitle: "No terms imported",
    emptyDescription: "This pack version imported no terms.",
    readOnlyNotice: `Imported from ${pack.sourceUrl} (${pack.packId} ${pack.version}) — read-only here. Extend it with an override rather than editing a term directly.`,
  };
}

// ---- Classifications and tags ----

function isTag(raw: unknown): raw is Tag {
  return typeof raw === "object" && raw !== null && "classificationId" in raw;
}

function isClassification(raw: unknown): raw is Classification {
  return typeof raw === "object" && raw !== null && "mutuallyExclusive" in raw;
}

export function classificationVocabulary(): VocabularyConfig {
  return {
    key: "classification",
    label: "Classifications",
    treeLabel: "Classifications and tags",
    emptyTitle: "No classifications yet",
    emptyDescription: "No classification vocabulary has been declared yet.",
    async fetchData() {
      const [classifications, tags] = await Promise.all([api.classifications(), api.tags()]);
      const items: VocabularyItem[] = [
        ...classifications.map((c) => ({ id: c.id, name: c.name, raw: c })),
        ...tags.map((t) => ({ id: t.id, name: t.name, raw: t })),
      ];
      const relationsByItem = new Map<string, readonly Relation[]>();
      for (const tag of tags) {
        relationsByItem.set(tag.id, [{ kind: "broader", target: tag.classificationId }]);
      }
      return { items, relationsByItem };
    },
    async detailFor(itemId, data) {
      const item = data.items.find((i) => i.id === itemId);
      if (!item) return null;

      if (isClassification(item.raw)) {
        const classification = item.raw;
        return {
          title: classification.name,
          fields: [
            { label: "Description", value: classification.description ?? "" },
            {
              label: "Mutually exclusive",
              value: classification.mutuallyExclusive
                ? "Yes — only one of its tags may be applied to an entity at a time"
                : "No — several of its tags may apply at once",
            },
          ],
          relationsLabel: "",
          relations: [],
          usageLabel: "",
          usage: [],
        };
      }

      if (!isTag(item.raw)) return null;
      const tag = item.raw;
      const usage = await api.tagUsage(tag.fullyQualifiedName);
      const classificationItem = data.items.find((i) => i.id === tag.classificationId);
      const classification = classificationItem && isClassification(classificationItem.raw)
        ? classificationItem.raw
        : null;
      // Epic 25 decision 4, surfaced where a reviewer actually looks at a
      // tag: the sibling tags a mutually exclusive classification refuses
      // to let coexist, named — not just "this classification is
      // exclusive" as an abstract property elsewhere.
      const conflicts = classification?.mutuallyExclusive
        ? data.items
            .filter(
              (i) => isTag(i.raw) && (i.raw as Tag).classificationId === tag.classificationId && i.id !== tag.id,
            )
            .map((i) => i.name)
        : [];

      return {
        title: tag.name,
        fields: [
          { label: "Description", value: tag.description ?? "" },
          ...(conflicts.length > 0
            ? [{ label: "Conflicts with", value: conflicts.join(", ") }]
            : []),
        ],
        relationsLabel: "",
        relations: [],
        usageLabel: "Used on",
        usage: usage.byKind.map((b) => `${b.count} ${b.kind}`),
      };
    },
  };
}

// ---- Domains and data products ----

export function domainVocabulary(): VocabularyConfig {
  return {
    key: "domain",
    label: "Domains",
    treeLabel: "Domains",
    emptyTitle: "No domains yet",
    emptyDescription: "No domain has been declared yet.",
    async fetchData() {
      const page = await api.domains();
      const relationsByItem = new Map<string, readonly Relation[]>();
      for (const domain of page.data) {
        if (domain.parentId) {
          relationsByItem.set(domain.id, [{ kind: "broader", target: domain.parentId }]);
        }
      }
      return {
        items: page.data.map((d) => ({ id: d.id, name: d.name, raw: d })),
        relationsByItem,
      };
    },
    async detailFor(itemId, data) {
      const item = data.items.find((i) => i.id === itemId);
      if (!item) return null;
      const domain = item.raw as Domain;
      const productsPage = await api.dataProducts();
      const products = productsPage.data.filter((p) => p.domainId === domain.id);
      return {
        title: domain.name,
        fields: [
          { label: "Description", value: domain.description ?? "" },
          { label: "Type", value: domain.domainType ?? "" },
          { label: "Experts", value: domain.experts.join(", ") },
        ],
        relationsLabel: "",
        relations: [],
        usageLabel: "Data products",
        usage: products.map((p) => p.name),
      };
    },
  };
}
