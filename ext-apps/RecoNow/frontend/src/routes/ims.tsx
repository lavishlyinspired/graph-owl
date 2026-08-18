import { useState } from "react";
import { SectionTabs } from "../components/SectionTabs";
import { sectionsFor } from "../lib/sections";
import type { Capability } from "../lib/sections";
import ApprovalsPanel from "../panels/approvals";
import ImsMainPanel from "../panels/imsMain";

/** Ims — Plan 123 Slice D.
 *
 *  An IMS decision *is* an approval. Two screens made one act look like two.
 *
 *  The panels below were separate routes until this slice. Each is unchanged;
 *  only where it is reached from moved. `sections.ts` records why each one
 *  lives here, and `sections.test.ts` fails if any capability the 30-route
 *  console had is left without a home. */
const PANELS: Partial<Record<Capability, React.ComponentType>> = {
  "ims": ImsMainPanel,
  "approvals": ApprovalsPanel,
};

export default function ImsRoute() {
  const sections = sectionsFor("ims");
  const [active, setActive] = useState<Capability>(sections[0]!.capability);
  const Panel = PANELS[active] ?? PANELS[sections[0]!.capability]!;

  return (
    <div>
      <SectionTabs route="ims" active={active} onSelect={setActive} />
      <Panel />
    </div>
  );
}
