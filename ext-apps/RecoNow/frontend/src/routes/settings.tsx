import { useState } from "react";
import { sectionsFor } from "../lib/sections";
import { SectionTabs } from "../components/SectionTabs";
import GstinsRoute from "../panels/gstins";
import RulesRoute from "../panels/rules";
import UsersRoute from "../panels/users";
import ResetRoute from "../panels/reset";

/** Settings — Plan 123 Slice D.
 *
 *  Four routes became four sections. None of them is a place anyone navigates
 *  to during a period's work, and each was a single table; separate routes
 *  made configuration feel like four destinations when it is one. */
const PANELS = {
  rules: RulesRoute,
  gstins: GstinsRoute,
  users: UsersRoute,
  "new-session": ResetRoute,
} as const;

export default function SettingsRoute() {
  const sections = sectionsFor("settings");
  const [active, setActive] = useState(sections[0]?.capability ?? "rules");
  const Panel = PANELS[active as keyof typeof PANELS] ?? RulesRoute;

  return (
    <div>
      <SectionTabs route="settings" active={active} onSelect={setActive} />
      <Panel />
    </div>
  );
}
