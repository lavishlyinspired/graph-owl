import { useState } from "react";
import { SectionTabs } from "../components/SectionTabs";
import { sectionsFor } from "../lib/sections";
import type { Capability } from "../lib/sections";
import AtriskPanel from "../panels/atrisk";
import EligibilityPanel from "../panels/eligibility";
import ItcMainPanel from "../panels/itcMain";

/** Itc — Plan 123 Slice D.
 *
 *  The credit position and the three readings of it — what is at risk, why each number landed where it did.
 *
 *  The panels below were separate routes until this slice. Each is unchanged;
 *  only where it is reached from moved. `sections.ts` records why each one
 *  lives here, and `sections.test.ts` fails if any capability the 30-route
 *  console had is left without a home. */
const PANELS: Partial<Record<Capability, React.ComponentType>> = {
  "itc-position": ItcMainPanel,
  "at-risk": AtriskPanel,
  "eligibility": EligibilityPanel,
};

export default function ItcRoute() {
  const sections = sectionsFor("itc");
  const [active, setActive] = useState<Capability>(sections[0]!.capability);
  const Panel = PANELS[active] ?? PANELS[sections[0]!.capability]!;

  return (
    <div>
      <SectionTabs route="itc" active={active} onSelect={setActive} />
      <Panel />
    </div>
  );
}
