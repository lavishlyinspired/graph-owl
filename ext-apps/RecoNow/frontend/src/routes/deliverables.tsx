import { useState } from "react";
import { SectionTabs } from "../components/SectionTabs";
import { sectionsFor } from "../lib/sections";
import type { Capability } from "../lib/sections";
import DeliverablesPanel from "../panels/deliverablesMain";
import ClientReportPanel from "../panels/clientReport";

const PANELS: Partial<Record<Capability, React.ComponentType>> = {
  deliverables: DeliverablesPanel,
  "client-report": ClientReportPanel,
};

export default function DeliverablesRoute() {
  const sections = sectionsFor("deliverables");
  const [active, setActive] = useState<Capability>(sections[0]!.capability);
  const Panel = PANELS[active] ?? DeliverablesPanel;

  return (
    <div>
      <SectionTabs route="deliverables" active={active} onSelect={setActive} />
      <Panel />
    </div>
  );
}
