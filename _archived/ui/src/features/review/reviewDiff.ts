/** Epic 42 Slice C: "side-by-side comparison with matching fields
 *  highlighted and conflicting ones flagged" reduced to a pure function —
 *  the same "decision a screen renders but does not make" split
 *  `vocabularyTree.ts` already established for the vocabulary browser. */

import type { Asset } from "../../api";

export interface FieldComparison {
  readonly field: string;
  readonly label: string;
  readonly targetValue: string;
  readonly candidateValue: string;
  readonly matches: boolean;
}

type ComparableField = "name" | "fullyQualifiedName" | "kind" | "parentId" | "description";

const FIELDS: { readonly field: ComparableField; readonly label: string }[] = [
  { field: "name", label: "Name" },
  { field: "fullyQualifiedName", label: "Fully qualified name" },
  { field: "kind", label: "Kind" },
  { field: "parentId", label: "Parent" },
  { field: "description", label: "Description" },
];

// Every `ComparableField` types as `string | AssetKind | null` on `Asset`
// — never `undefined` — so only `null` is a reachable input here.
function displayValue(value: string | null): string {
  return value === null ? "" : value;
}

function normalizedProperties(properties: Asset["properties"]): string {
  const entries = Object.entries(properties ?? {}).sort(([a], [b]) => a.localeCompare(b));
  return JSON.stringify(entries);
}

export function compareAssets(target: Asset, candidate: Asset): readonly FieldComparison[] {
  const scalarComparisons = FIELDS.map(({ field, label }) => {
    const targetValue = displayValue(target[field]);
    const candidateValue = displayValue(candidate[field]);
    return { field, label, targetValue, candidateValue, matches: targetValue === candidateValue };
  });

  const targetProperties = normalizedProperties(target.properties);
  const candidateProperties = normalizedProperties(candidate.properties);

  return [
    ...scalarComparisons,
    {
      field: "properties",
      label: "Properties",
      targetValue: targetProperties,
      candidateValue: candidateProperties,
      matches: targetProperties === candidateProperties,
    },
  ];
}
