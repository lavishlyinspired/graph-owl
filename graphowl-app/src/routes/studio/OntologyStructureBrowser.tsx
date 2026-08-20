import { useMemo, useState } from "react";
import { matchesOntologyFilter } from "../../lib/ontology/ontologyFilter";
import type { OntologyModel } from "../../lib/ontology/ontologyModel";
import { strings } from "../../lib/strings";

const PANEL = "rounded-lg border border-gowl-line bg-gowl-panel";
const PANEL_HEADER =
  "border-b border-gowl-line bg-gowl-panel-2 px-3 py-2 font-mono text-[11px] tracking-widest text-gowl-t6";
const ROW = "border-b border-gowl-row px-3 py-2 text-[14px] text-gowl-t2 last:border-b-0";

/** Reltio's own model browser is the reference point here: a filterable
 *  list of entity types (classes) and their attributes/relationships,
 *  separate from — and alongside — the graph diagram, not only inside it.
 *  A node in a force-directed layout can only ever show a handful of
 *  characters before it stops being a diagram, so this is the place every
 *  class, relationship and property gets its full name rather than a
 *  three-letter glyph. */
export function OntologyStructureBrowser({ model }: { readonly model: OntologyModel }) {
  const [query, setQuery] = useState("");

  const nameOf = useMemo(() => {
    const map = new Map<string, string>();
    for (const cls of model.classes) map.set(cls.id, cls.name);
    return map;
  }, [model.classes]);

  const classes = model.classes.filter((c) => matchesOntologyFilter(c.name, query));
  const relationships = model.relationships.filter((r) => matchesOntologyFilter(r.label, query));
  const properties = model.properties.filter((p) => matchesOntologyFilter(p.name, query));

  return (
    <div className="flex flex-col gap-4">
      <input
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        placeholder={strings.studioOntologyFilterPlaceholder}
        aria-label={strings.studioOntologyFilterPlaceholder}
        className="w-full max-w-sm rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 text-[14px] text-gowl-t1"
      />

      <div className="grid grid-cols-2 gap-4">
        <div className={PANEL}>
          <div className={PANEL_HEADER}>
            {`${strings.studioOntologyClassesHeading} (${classes.length})`}
          </div>
          <div className="max-h-[420px] overflow-y-auto">
            {classes.length === 0 && <p className={`${ROW} text-gowl-t5`}>{strings.studioOntologyNoMatches}</p>}
            {classes.map((cls) => (
              <div key={cls.id} className={ROW}>
                <div className="text-gowl-t1">{cls.name}</div>
                <div className="mt-0.5 truncate font-mono text-[12px] text-gowl-t6">{cls.namespace}</div>
              </div>
            ))}
          </div>
        </div>

        <div className={PANEL}>
          <div className={PANEL_HEADER}>
            {`${strings.studioOntologyRelationshipsHeading} (${relationships.length})`}
          </div>
          <div className="max-h-[420px] overflow-y-auto">
            {relationships.length === 0 && (
              <p className={`${ROW} text-gowl-t5`}>{strings.studioOntologyNoMatches}</p>
            )}
            {relationships.map((rel) => (
              <div key={rel.id} className={ROW}>
                <div className="text-gowl-t1">{rel.label}</div>
                <div className="mt-0.5 font-mono text-[12px] text-gowl-t6">
                  {`${nameOf.get(rel.from) ?? rel.from} → ${nameOf.get(rel.to) ?? rel.to}`}
                </div>
              </div>
            ))}
          </div>
        </div>

        <div className={`${PANEL} col-span-2`}>
          <div className={PANEL_HEADER}>
            {`${strings.studioOntologyPropertiesHeading} (${properties.length})`}
          </div>
          <div className="grid max-h-[320px] grid-cols-3 overflow-y-auto">
            {properties.length === 0 && <p className={`${ROW} text-gowl-t5`}>{strings.studioOntologyNoMatches}</p>}
            {properties.map((prop) => (
              <div key={prop.id} className={ROW}>
                <div className="truncate text-gowl-t1" title={prop.name}>
                  {prop.name}
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
