import { useState } from "react";
import { SectionTabs } from "../components/SectionTabs";
import { sectionsFor } from "../lib/sections";
import type { Capability } from "../lib/sections";
import AuthorityPanel from "../panels/authority";
import ObligationsMainPanel from "../panels/obligationsMain";

/** Obligations — Plan 123 Slice D.
 *
 *  What the authority requires of you and what it says are one topic.
 *
 *  The panels below were separate routes until this slice. Each is unchanged;
 *  only where it is reached from moved. `sections.ts` records why each one
 *  lives here, and `sections.test.ts` fails if any capability the 30-route
 *  console had is left without a home. */
const PANELS: Partial<Record<Capability, React.ComponentType>> = {
  "obligations": ObligationsMainPanel,
  "authority": AuthorityPanel,
};

export default function ObligationsRoute() {
  const sections = sectionsFor("obligations");
  const [active, setActive] = useState<Capability>(sections[0]!.capability);
  const Panel = PANELS[active] ?? PANELS[sections[0]!.capability]!;

  return (
    <div>
      <SectionTabs route="obligations" active={active} onSelect={setActive} />
      <Panel />
    </div>
  );
}
