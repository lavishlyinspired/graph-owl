import { useState } from "react";
import { SectionTabs } from "../components/SectionTabs";
import { sectionsFor } from "../lib/sections";
import type { Capability } from "../lib/sections";
import CrossperiodPanel from "../panels/crossperiod";
import PeriodsPanel from "../panels/periods";
import ReconcileMainPanel from "../panels/reconcileMain";

/** Reconcile — Plan 123 Slice D.
 *
 *  Matching the two sides — this period, every period, and across them. The same question at three widths.
 *
 *  The panels below were separate routes until this slice. Each is unchanged;
 *  only where it is reached from moved. `sections.ts` records why each one
 *  lives here, and `sections.test.ts` fails if any capability the 30-route
 *  console had is left without a home. */
const PANELS: Partial<Record<Capability, React.ComponentType>> = {
  "reconcile": ReconcileMainPanel,
  "periods": PeriodsPanel,
  "cross-period": CrossperiodPanel,
};

export default function ReconcileRoute() {
  const sections = sectionsFor("reconcile");
  const [active, setActive] = useState<Capability>(sections[0]!.capability);
  const Panel = PANELS[active] ?? PANELS[sections[0]!.capability]!;

  return (
    <div>
      <SectionTabs route="reconcile" active={active} onSelect={setActive} />
      <Panel />
    </div>
  );
}
