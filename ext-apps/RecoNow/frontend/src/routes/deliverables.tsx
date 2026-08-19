import { useState } from "react";
import { SectionTabs } from "../components/SectionTabs";
import { sectionsFor } from "../lib/sections";
import type { Capability } from "../lib/sections";
import DeliverablesPanel from "../panels/deliverablesMain";
import ClientReportPanel from "../panels/clientReport";

export default function DeliverablesRoute() {
  const sections = sectionsFor("deliverables");
  const [active, setActive] = useState<Capability>(sections[0]!.capability);

  return (
    <div>
      <SectionTabs route="deliverables" active={active} onSelect={setActive} />
      {active === "client-report" ? (
        <ClientReportPanel />
      ) : (
        // The header's two buttons were inert. They now go where their labels
        // say: the primary to the client report this screen already hosts, the
        // secondary to the working paper screen that builds Table 4.
        <DeliverablesPanel onGenerateReport={() => setActive("client-report")} />
      )}
    </div>
  );
}
