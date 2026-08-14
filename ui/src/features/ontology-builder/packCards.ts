/** What the pack picker shows, derived from what is actually installed and
 *  actually loaded — never invented. A pack only offers "load" when its own
 *  `{packId}-ontology` source has genuinely been imported. */

import { ontologySourceFor, type LoadedSource } from "../packs/packData";
import type { InstalledPack } from "../packs/packSurfaces";

export interface PackCardView {
  readonly packId: string;
  readonly label: string;
  readonly hasOntology: boolean;
}

export function packCards(
  packs: readonly InstalledPack[],
  sources: readonly LoadedSource[],
): readonly PackCardView[] {
  return packs.map((pack) => ({
    packId: pack.packId,
    label: pack.label,
    hasOntology: ontologySourceFor(pack.packId, sources) !== null,
  }));
}
