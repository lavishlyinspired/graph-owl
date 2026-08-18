import { useState } from "react";
import { SectionTabs } from "../components/SectionTabs";
import { sectionsFor } from "../lib/sections";
import type { Capability } from "../lib/sections";
import DatasourcesPanel from "../panels/datasources";
import ImportsPanel from "../panels/imports";
import MappingsPanel from "../panels/mappings";
import PipelineMainPanel from "../panels/pipelineMain";

/** Pipeline — Plan 123 Slice D.
 *
 *  Getting data in: the file, its mapping, what was wrong with it, and where it came from. Four routes for one act made it four journeys.
 *
 *  The panels below were separate routes until this slice. Each is unchanged;
 *  only where it is reached from moved. `sections.ts` records why each one
 *  lives here, and `sections.test.ts` fails if any capability the 30-route
 *  console had is left without a home. */
const PANELS: Partial<Record<Capability, React.ComponentType>> = {
  "upload": PipelineMainPanel,
  "mapping": MappingsPanel,
  "data-quality": PipelineMainPanel,
  "sources": DatasourcesPanel,
  "imports": ImportsPanel,
};

export default function PipelineRoute() {
  const sections = sectionsFor("pipeline");
  const [active, setActive] = useState<Capability>(sections[0]!.capability);
  const Panel = PANELS[active] ?? PANELS[sections[0]!.capability]!;

  return (
    <div>
      <SectionTabs route="pipeline" active={active} onSelect={setActive} />
      <Panel />
    </div>
  );
}
