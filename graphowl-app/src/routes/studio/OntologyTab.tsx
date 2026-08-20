import { useEffect, useMemo, useState } from "react";
import { OntologyCanvas } from "../../graph/OntologyCanvas";
import { OntologyStructureBrowser } from "./OntologyStructureBrowser";
import { OntologyEditorPanel } from "./OntologyEditorPanel";
import { AlignmentReviewPanel } from "./AlignmentReviewPanel";
import { GRAPH_COLORS } from "../../lib/graph/graphColors";
import { toOntologyG6Data } from "../../lib/ontology/ontologyGraphModel";
import { ontologyGraphQuery, ontologyModelFromSparqlRows } from "../../lib/ontology/ontologyModel";
import { fetchInstalledPacks, runSparql, type InstalledPack } from "../../lib/api";
import { resolveInitialTheme } from "../../lib/theme";
import { strings } from "../../lib/strings";

type OntologyView = "graph" | "table" | "editor" | "alignments";

const VIEW_LABEL: Record<OntologyView, string> = {
  graph: strings.studioOntologyViewGraph,
  table: strings.studioOntologyViewTable,
  editor: strings.studioOntologyViewEditor,
  alignments: strings.studioOntologyViewAlignments,
};

/** The installed pack's own OWL-flavoured model, as a graph — Plan
 *  `ontology-graph.md` slice 1.
 *
 *  Read entirely through the same generic `/sparql` endpoint every other
 *  real screen in this console uses, over the pack's own ontology import
 *  graph (`lib/ontology/ontologyModel.ts`'s own doc comment has the
 *  verified graph-IRI convention). No dedicated backend surface exists or
 *  is needed for this. */
export function OntologyTab() {
  const mode = useMemo(() => resolveInitialTheme(), []);
  const colors = GRAPH_COLORS[mode];

  const [packs, setPacks] = useState<readonly InstalledPack[] | null>(null);
  const [selectedPackId, setSelectedPackId] = useState<string>("");
  const [rows, setRows] = useState<readonly Record<string, string>[] | null>(null);
  const [error, setError] = useState(false);
  const [view, setView] = useState<OntologyView>("graph");

  useEffect(() => {
    fetchInstalledPacks()
      .then((installed) => {
        setPacks(installed);
        setSelectedPackId((current) => current || (installed[0]?.packId ?? ""));
      })
      .catch(() => setError(true));
  }, []);

  useEffect(() => {
    setRows(null);
    setError(false);
    if (!selectedPackId) return;
    runSparql(ontologyGraphQuery(selectedPackId))
      .then((result) => setRows(result.rows))
      .catch(() => setError(true));
  }, [selectedPackId]);

  const model = useMemo(
    () => (rows ? ontologyModelFromSparqlRows(rows) : null),
    [rows],
  );
  const data = useMemo(() => (model ? toOntologyG6Data(model, mode) : null), [model, mode]);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="mb-3 flex items-center gap-2.5">
        <span className="text-[12.5px] text-gowl-t5">{strings.studioOntologyPackLabel}</span>
        <select
          value={selectedPackId}
          onChange={(event) => setSelectedPackId(event.target.value)}
          aria-label={strings.studioOntologyPackLabel}
          className="rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 text-[14px] text-gowl-t1"
        >
          {(packs ?? []).map((pack) => (
            <option key={pack.packId} value={pack.packId}>
              {pack.packId}
            </option>
          ))}
        </select>
        {model && (
          <span className="font-mono text-[12.5px] text-gowl-t6">
            {`${model.classes.length} classes · ${model.relationships.length} relationships · ${model.properties.length} properties`}
          </span>
        )}

        <div className="ml-auto flex gap-1 rounded-md border border-gowl-line-2 p-0.5">
          {(["graph", "table", "editor", "alignments"] as const).map((v) => (
            <button
              key={v}
              type="button"
              onClick={() => setView(v)}
              className={`rounded px-2.5 py-1 text-[13px] ${
                view === v ? "bg-gowl-accent text-gowl-bg" : "text-gowl-t5 hover:text-gowl-t2"
              }`}
            >
              {VIEW_LABEL[v]}
            </button>
          ))}
        </div>
      </div>

      {/* **The named, deferred gap, stated rather than hidden.** A plain
       *  literal-valued property has no declared domain, so which class it
       *  belongs to is not derivable from the ontology's own declarations —
       *  only from which class's *instances* actually carry it. */}
      <p className="mb-3 text-[12.5px] text-gowl-t6">{strings.studioOntologyAttributesGap}</p>

      {error && <div className="text-[14.5px] text-gowl-bad">{strings.studioError}</div>}
      {!error && !packs && <div className="text-[14.5px] text-gowl-t5">{strings.studioLoading}</div>}
      {!error && packs && !selectedPackId && (
        <div className="text-[14.5px] text-gowl-t5">{strings.studioOntologyNoPacks}</div>
      )}
      {!error && selectedPackId && !data && (
        <div className="text-[14.5px] text-gowl-t5">{strings.studioLoading}</div>
      )}
      {!error && data && model && view === "graph" && (
        <OntologyCanvas
          data={data}
          colors={colors}
          label={`${selectedPackId} ontology`}
        />
      )}
      {!error && model && view === "table" && <OntologyStructureBrowser model={model} />}
      {view === "editor" && <OntologyEditorPanel />}
      {view === "alignments" && <AlignmentReviewPanel />}
    </div>
  );
}
