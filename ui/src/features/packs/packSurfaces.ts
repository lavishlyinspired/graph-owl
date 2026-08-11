/** Which console surfaces a domain pack brings with it — Epic 105.
 *
 *  **The console has no GST tab, and must not grow one.** It has an Import
 *  section that is empty until a pack is installed, and renders whatever
 *  surfaces the installed packs declare. Install the hospitality pack instead
 *  and the same section renders that pack's surfaces; install neither and the
 *  section says so and offers nothing.
 *
 *  **Installed-ness is discovered, not configured.** `POST /namespaces` records
 *  `declaredBy: "pack:<id>"` when a pack loads, so `GET /namespaces` already
 *  answers "which packs does this deployment have" without a new route, a new
 *  table, or a build-time list. A deployment that never installs a pack simply
 *  reports no pack namespaces and the section stays empty — which is the
 *  correct behaviour rather than a special case.
 *
 *  This is the same split `queues.ts` uses and for the same reason: the config
 *  carries the difference, the component stays generic. What is new here is
 *  that the config is *gated at runtime* — a queue is always available, a pack
 *  surface exists only while its pack does. */

import { normalize, toTurtle } from "./gstr2b";

/** A file a pack knows how to turn into graph facts. */
export interface PackImportSurface {
  /** Stable id, used as the import graph's source name. */
  readonly key: string;
  readonly label: string;
  /** What the user is being asked for, in their words — not the API's. */
  readonly description: string;
  /** `accept` for the file input. */
  readonly accept: string;
  /** Where the file comes from, so somebody who has never done this knows. */
  readonly howToObtain: string;
  /** Parse and convert. Throws with a message a non-engineer can act on. */
  convert(text: string): { readonly turtle: string; readonly count: number };
}

export interface PackSurfaces {
  readonly packId: string;
  readonly label: string;
  readonly imports: readonly PackImportSurface[];
}

/** The GST pack's surfaces.
 *
 *  Nothing outside this object knows what GST is. Adding a second domain is
 *  adding a second entry, and the components below change not at all. */
const GST: PackSurfaces = {
  packId: "gst",
  label: "GST",
  imports: [
    {
      key: "gstr2b",
      label: "GSTR-2B",
      description:
        "The return your suppliers filed, as the authority reports it. Reconciled against your purchase register.",
      accept: "application/json,.json",
      howToObtain:
        "Download it from the GST portal: Returns Dashboard → select the period → GSTR-2B → Download JSON. " +
        "A response saved from a GSP API works too — the two are the same document.",
      convert(text) {
        const invoices = normalize(JSON.parse(text));
        return { turtle: toTurtle(invoices), count: invoices.length };
      },
    },
  ],
};

const REGISTRY: readonly PackSurfaces[] = [GST];

/** A pack id out of a namespace's `declaredBy`, or null if it was not a pack.
 *
 *  The loader writes `pack:<id>`; a namespace declared by an operator or a
 *  connector (`connector:erpnext`) is deliberately not a pack and contributes
 *  no console surface. */
export function packIdOf(declaredBy: string): string | null {
  const prefix = "pack:";
  return declaredBy.startsWith(prefix) ? declaredBy.slice(prefix.length) : null;
}

/** The surfaces to render, given what the deployment reports as installed.
 *
 *  A pack with no registered surfaces contributes nothing rather than an empty
 *  card — a heading with nothing under it reads as a broken feature. */
export function surfacesFor(declaredBys: readonly string[]): readonly PackSurfaces[] {
  const installed = new Set(declaredBys.map(packIdOf).filter((id): id is string => id !== null));
  return REGISTRY.filter((surface) => installed.has(surface.packId) && surface.imports.length > 0);
}

/** One installed pack, for browsing rather than for uploading into. */
export interface InstalledPack {
  readonly packId: string;
  readonly label: string;
  /** One representative namespace's own code/IRI — a pack may declare
   *  several (`105c`'s law namespace beside its main one); the first row
   *  `GET /namespaces` reports for this pack id is what is shown, since
   *  this is an identity check, not a full namespace listing. */
  readonly namespaceCode: number;
  readonly iri: string;
}

/** A friendly label for a pack id, from the registry when this pack
 *  happens to have an import surface, title-cased otherwise. Reading a
 *  label off the registry is a convenience, not a requirement — a pack
 *  with no upload surface must still be listed, just without a curated
 *  name. */
function labelFor(packId: string): string {
  const known = REGISTRY.find((surface) => surface.packId === packId);
  return known ? known.label : packId.charAt(0).toUpperCase() + packId.slice(1);
}

/** Every distinct pack this deployment reports as installed, **whether or
 *  not it has a registered import surface** — the property `surfacesFor`
 *  cannot provide, since it filters to `imports.length > 0` and would
 *  silently omit a pack with none. An admin confirming "did the pack I
 *  just loaded actually register" needs the unfiltered list. */
export function installedPacks(
  rows: readonly { readonly code: number; readonly iri: string; readonly declaredBy: string }[],
): readonly InstalledPack[] {
  const seen = new Set<string>();
  const out: InstalledPack[] = [];
  for (const row of rows) {
    const packId = packIdOf(row.declaredBy);
    if (packId === null || seen.has(packId)) continue;
    seen.add(packId);
    out.push({ packId, label: labelFor(packId), namespaceCode: row.code, iri: row.iri });
  }
  return out;
}
