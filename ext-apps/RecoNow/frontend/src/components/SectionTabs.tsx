import type { Capability } from "../lib/sections";
import { hasTabs, sectionsFor } from "../lib/sections";
import type { RouteName } from "../lib/routes";

/** The strip that makes a consolidated route navigable — Plan 123 Slice D.
 *
 *  Renders nothing at all for a single-capability route: a strip with one tab
 *  is chrome that costs vertical space and tells a reader nothing.
 *
 *  Each tab carries its own `because` as a title, so the question a later
 *  reader asks — *why is this a tab of that?* — is answerable from the UI
 *  rather than only from the source. A merge with no stated reason gets
 *  undone by the next person who finds it surprising. */
export function SectionTabs({
  route,
  active,
  onSelect,
}: {
  readonly route: RouteName;
  readonly active: Capability | string;
  readonly onSelect: (capability: Capability) => void;
}) {
  if (!hasTabs(route)) return null;

  return (
    <nav
      aria-label="Sections"
      className="flex gap-1 overflow-x-auto border-b border-reco-line px-6"
    >
      {sectionsFor(route)
        .filter((section) => section.label)
        .map((section) => {
        const selected = section.capability === active;
        return (
          <button
            key={section.capability}
            type="button"
            aria-current={selected ? "page" : undefined}
            title={section.because}
            onClick={() => onSelect(section.capability)}
            className={`whitespace-nowrap border-b-2 px-3 py-2 text-[12.5px] transition-colors ${
              selected
                ? "border-reco-accent text-reco-t1"
                : "border-transparent text-reco-t4 hover:text-reco-t2"
            }`}
          >
            {section.label}
          </button>
        );
      })}
    </nav>
  );
}
