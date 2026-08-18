import { useEffect, useState } from "react";
import { BuildTab } from "./studio/BuildTab";
import { GlossaryTab } from "./studio/GlossaryTab";
import { BusinessTab } from "./studio/BusinessTab";
import { GraphTab } from "./studio/GraphTab";
import { SparqlTab } from "./studio/SparqlTab";
import { NotYetBuilt } from "./studio/NotYetBuilt";
import { createGlossary, fetchGlossaries, type Glossary } from "../lib/api";
import { strings } from "../lib/strings";

const TABS = [
  "build",
  "glossary",
  "business",
  "proposals",
  "graph",
  "validate",
  "sparql",
  "export",
] as const;
type Tab = (typeof TABS)[number];

const TAB_LABEL: Record<Tab, string> = {
  build: strings.studioTabBuild,
  glossary: strings.studioTabGlossary,
  business: strings.studioTabBusiness,
  proposals: strings.studioTabProposals,
  graph: strings.studioTabGraph,
  validate: strings.studioTabValidate,
  sparql: strings.studioTabSparql,
  export: strings.studioTabExport,
};

export default function StudioRoute() {
  const [glossaries, setGlossaries] = useState<readonly Glossary[] | null>(null);
  const [error, setError] = useState(false);
  const [selectedGlossaryId, setSelectedGlossaryId] = useState<string | null>(null);
  const [newGlossaryName, setNewGlossaryName] = useState("");
  const [tab, setTab] = useState<Tab>("build");
  const [busy, setBusy] = useState(false);

  const load = () => {
    fetchGlossaries()
      .then((fetched) => {
        setGlossaries(fetched);
        setSelectedGlossaryId((prev) => prev ?? fetched[0]?.id ?? null);
      })
      .catch(() => setError(true));
  };

  useEffect(load, []);

  if (error) {
    return <div className="p-8 text-[13px] text-gowl-bad">{strings.studioError}</div>;
  }
  if (!glossaries) {
    return <div className="p-8 text-[13px] text-gowl-t5">{strings.studioLoading}</div>;
  }

  const runCreateGlossary = async () => {
    if (newGlossaryName.trim().length === 0) return;
    setBusy(true);
    try {
      const created = await createGlossary(newGlossaryName.trim());
      setNewGlossaryName("");
      setSelectedGlossaryId(created.id);
      load();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="p-8">
      <h1 className="mb-1 text-[21px] font-semibold text-gowl-t1">{strings.studioTitle}</h1>
      <p className="mb-5 text-[12.5px] text-gowl-t5">{strings.studioDescription}</p>

      <div className="mb-4 flex items-end gap-2">
        <div>
          <div className="mb-1 text-[11px] text-gowl-t5">{strings.studioGlossaryPicker}</div>
          <select
            value={selectedGlossaryId ?? ""}
            onChange={(e) => setSelectedGlossaryId(e.target.value || null)}
            aria-label={strings.studioGlossaryPicker}
            className="rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 text-[12.5px] text-gowl-t1"
          >
            {glossaries.map((g) => (
              <option key={g.id} value={g.id}>
                {g.name}
              </option>
            ))}
          </select>
        </div>
        <input
          value={newGlossaryName}
          onChange={(e) => setNewGlossaryName(e.target.value)}
          placeholder={strings.studioGlossaryNamePlaceholder}
          className="rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 text-[12px] text-gowl-t1"
        />
        <button
          type="button"
          disabled={busy || newGlossaryName.trim().length === 0}
          onClick={runCreateGlossary}
          className="rounded-md border border-gowl-line-2 px-3 py-1.5 text-[12px] text-gowl-t2 disabled:opacity-40"
        >
          {strings.studioNewGlossary}
        </button>
      </div>

      <div className="mb-4 flex gap-1 border-b border-gowl-line">
        {TABS.map((t) => (
          <button
            key={t}
            type="button"
            onClick={() => setTab(t)}
            className={`px-3 py-2 text-[12.5px] ${
              tab === t ? "border-b-2 border-gowl-accent text-gowl-accent" : "text-gowl-t5 hover:text-gowl-t2"
            }`}
          >
            {TAB_LABEL[t]}
          </button>
        ))}
      </div>

      {!selectedGlossaryId ? (
        <p className="text-[13px] text-gowl-t5">{strings.studioNoGlossaries}</p>
      ) : (
        <>
          {tab === "build" && <BuildTab glossaryId={selectedGlossaryId} />}
          {tab === "glossary" && <GlossaryTab glossaryId={selectedGlossaryId} />}
          {tab === "business" && <BusinessTab glossaryId={selectedGlossaryId} />}
          {tab === "proposals" && <NotYetBuilt body={strings.studioNotYetBuiltProposals} />}
          {tab === "graph" && <GraphTab glossaryId={selectedGlossaryId} />}
          {tab === "validate" && <NotYetBuilt body={strings.studioNotYetBuiltValidate} />}
          {tab === "sparql" && <SparqlTab />}
          {tab === "export" && <NotYetBuilt body={strings.studioNotYetBuiltExport} />}
        </>
      )}
    </div>
  );
}
