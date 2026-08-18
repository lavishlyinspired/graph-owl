import { useState } from "react";
import { SectionTabs } from "../components/SectionTabs";
import { sectionsFor } from "../lib/sections";
import type { Capability } from "../lib/sections";
import ExceptionsPanel from "../panels/exceptions";
import QueuePanel from "../panels/queue";
import RegisterMainPanel from "../panels/registerMain";

/** Register — Plan 123 Slice D.
 *
 *  Every invoice, and the two filters over it people asked for by name: what needs attention, and what needs a second pair of eyes.
 *
 *  The panels below were separate routes until this slice. Each is unchanged;
 *  only where it is reached from moved. `sections.ts` records why each one
 *  lives here, and `sections.test.ts` fails if any capability the 30-route
 *  console had is left without a home. */
const PANELS: Partial<Record<Capability, React.ComponentType>> = {
  "register": RegisterMainPanel,
  "exceptions": ExceptionsPanel,
  "review-queue": QueuePanel,
};

export default function RegisterRoute() {
  const sections = sectionsFor("register");
  const [active, setActive] = useState<Capability>(sections[0]!.capability);
  const Panel = PANELS[active] ?? PANELS[sections[0]!.capability]!;

  return (
    <div>
      <SectionTabs route="register" active={active} onSelect={setActive} />
      <Panel />
    </div>
  );
}
