import { useState } from "react";
import { SectionTabs } from "../components/SectionTabs";
import { sectionsFor } from "../lib/sections";
import type { Capability } from "../lib/sections";
import AnalyticsMainPanel from "../panels/analyticsMain";
import RiskPanel from "../panels/risk";

/** Analytics — Plan 123 Slice D.
 *
 *  Structure rather than arithmetic: rings, centrality, orphans, and the per-supplier reading of the same signals.
 *
 *  The panels below were separate routes until this slice. Each is unchanged;
 *  only where it is reached from moved. `sections.ts` records why each one
 *  lives here, and `sections.test.ts` fails if any capability the 30-route
 *  console had is left without a home. */
const PANELS: Partial<Record<Capability, React.ComponentType>> = {
  "patterns": AnalyticsMainPanel,
  "supplier-risk": RiskPanel,
  "analytics": AnalyticsMainPanel,
};

export default function AnalyticsRoute() {
  const sections = sectionsFor("analytics");
  const [active, setActive] = useState<Capability>(sections[0]!.capability);
  const Panel = PANELS[active] ?? PANELS[sections[0]!.capability]!;

  return (
    <div>
      <SectionTabs route="analytics" active={active} onSelect={setActive} />
      <Panel />
    </div>
  );
}
