import { useState } from "react";
import { SectionTabs } from "../components/SectionTabs";
import { sectionsFor } from "../lib/sections";
import type { Capability } from "../lib/sections";
import RegisterMainPanel from "../panels/registerMain";
import CasePanel from "../panels/case";
import ExceptionsPanel from "../panels/exceptions";
import QueuePanel from "../panels/queue";

/** Findings — Plan 123 Slice D, with case detail merged in.
 *
 *  **"Findings", not "All invoices".** The Reconcile screen's "All invoices"
 *  lists every *invoice* on either side, bucketed. This lists every *finding*
 *  — one per problem, so an invoice with two problems appears twice. Both were
 *  labelled "all invoices" and showed different counts, which made a correct
 *  product look broken.
 *
 *  Case detail was its own route and is now a section: you open a case *from*
 *  this list, and a route arrived at without a selection shows an empty
 *  screen. */
const PANELS: Partial<Record<Capability, React.ComponentType>> = {
  register: RegisterMainPanel,
  "case-detail": CasePanel,
  exceptions: ExceptionsPanel,
  "review-queue": QueuePanel,
};

export default function RegisterRoute() {
  const sections = sectionsFor("register");
  const [active, setActive] = useState<Capability>(sections[0]!.capability);
  const Panel = PANELS[active] ?? RegisterMainPanel;

  return (
    <div>
      <SectionTabs route="register" active={active} onSelect={setActive} />
      <Panel />
    </div>
  );
}
