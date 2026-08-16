/** Visual ontology builder.
 *
 *  A standalone client surface for designing entity types, their attributes,
 *  and the relationships between them. The model persists to `localStorage`
 *  and can be exported/imported as JSON, Turtle, OWL/XML, JSON-LD, or
 *  N-Triples; there is no server-side `/entity-types` collection yet, so
 *  entity types and relationships are edited here and round-tripped through
 *  those serialisations rather than saved through a catalog API.
 *
 *  **The Code tab is `features/ontology/OntologyEditor.tsx`, merged in** —
 *  `00f-ui-architecture.md`'s 14 Aug 2026 revision (Plan 117 Slice G). That
 *  screen's real value was raw-text editing plus Check/Save against the
 *  actual `/ontology-editor/*` API; both are now here as a second tab on
 *  the same model, rather than a second, competing screen with its own
 *  data model. Editing the Code tab's text re-parses it back into the
 *  visual model (`importModel`) on every change — a syntax error leaves the
 *  visual model exactly as it was, the same "never clear the last good
 *  state on a parse error" rule `OntologyEditor` used to enforce itself.
 *
 *  A pack in this console is an installed domain vocabulary (GST,
 *  hospitality, ...). Selecting one here loads *that pack's own* ontology —
 *  the `{packId}-ontology` named graph it shipped, if it has been imported —
 *  as a starting diagram, the same "load installed pack" pattern the old
 *  ontology editor used. A pack with no ontology imported yet is shown, not
 *  hidden, but cannot be loaded.
 *
 *  The layout follows the reference screenshots: a header with pack picker,
 *  counts, and file actions; a diagram canvas whose own controls float over
 *  it rather than occupying a separate toolbar row, so the canvas keeps the
 *  space; a Code tab; and a right-hand detail panel. */

import { useEffect, useMemo, useState } from "react";
import { api, type OntologyEditFormat } from "../../api";
import { palette } from "../../theme";
import { Button } from "../../components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "../../components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../../components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../../components/ui/tabs";
import { Textarea } from "../../components/ui/textarea";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "../../components/ui/tooltip";
import { AddEntityTypeDialog } from "./AddEntityTypeDialog";
import { AddRelationshipDialog } from "./AddRelationshipDialog";
import { EntityTypePanel } from "./EntityTypePanel";
import { PackPicker } from "./PackPicker";
import { RelationshipPanel } from "./RelationshipPanel";
import { SupportingVocabulary } from "./SupportingVocabulary";
import { OntologyCanvas, type EdgeStyle, type LayoutName } from "./OntologyCanvas";
import { loadModel, saveModel } from "./state";
import {
  entityById,
  filterModelByNamespace,
  namespacesIn,
  relationshipById,
  toFlowElements,
} from "./flowModel";
import { WorkbenchPanel } from "./WorkbenchPanel";
import {
  exportModel,
  importModel,
  importNTriples,
  FORMAT_EXTENSIONS,
  FORMAT_LABELS,
  type ExportFormat,
} from "./formats";
import {
  loadedSourcesFromSparql,
  NAMED_GRAPHS_QUERY,
  ntriplesFromRows,
  ontologySourceFor,
  triplesQuery,
  type LoadedSource,
} from "../packs/packData";
import { installedPacks, type InstalledPack } from "../packs/packSurfaces";
import type { OntologyModel, OntologyPackOption } from "./types";

type Colors = (typeof palette)["light"];

/** This builder's own IRI space for exported classes/properties — no
 *  server-side catalog owns these entity types yet, so they mint under the
 *  builder's own namespace rather than under any pack's. */
const BASE_IRI = "https://graph-owl.dev/ontology-builder#";

const FORMAT_MIME: Record<ExportFormat, string> = {
  json: "application/json",
  ttl: "text/turtle",
  owl: "application/rdf+xml",
  jsonld: "application/ld+json",
  ntriples: "application/n-triples",
};

/** The Code tab's Check/Save API only understands three of the builder's
 *  five export formats — the other two (plain JSON, OWL/XML) have no
 *  matching server-side parser at `/ontology-editor/*`. Chosen inside the
 *  Code tab, independent of the Export dialog's own format picker. */
const CODE_FORMATS: { value: OntologyEditFormat; label: string; exportFormat: ExportFormat }[] = [
  { value: "turtle", label: "Turtle", exportFormat: "ttl" },
  { value: "ntriples", label: "N-Triples", exportFormat: "ntriples" },
  { value: "jsonld", label: "JSON-LD", exportFormat: "jsonld" },
];

const COPY = {
  title: "Ontology Builder",
  subtitle: "Design entity types, attributes, and relationships.",
  entities: "Entities",
  relationships: "Relationships",
  interactions: "Interactions",
  referenceData: "Reference Data",
  sources: "Sources",
  layout: "Layout",
  edges: "Edges",
  namespace: "Namespace",
  allNamespaces: "All namespaces",
  format: "Format",
  filter: "Filter",
  resetView: "Reset view",
  export: "Export",
  import: "Import",
  importError: "Could not import ontology.",
  noSelection: "Select a node or edge to edit.",
  emptyState: "No entity types yet. Add one to start building the ontology.",
  packLoaded: (label: string) => `Loaded ${label}'s ontology`,
  packLoadFailed: "Could not load that pack's ontology.",
  visualTab: "Visual",
  codeTab: "Code",
  workbenchTab: "Workbench",
  check: "Check",
  save: "Save",
  checkedTitle: "Would be accepted",
  rejectedTitle: "Would be refused",
  savedTitle: "Saved",
  saveErrorTitle: "Could not save",
  syntaxError: "Syntax error",
  newInferencesLabel: "new inferences",
  vocabularyDialogTitle: "Supporting vocabulary",
};

const LAYOUT_OPTIONS: { value: LayoutName; label: string }[] = [
  { value: "radial", label: "Radial" },
  { value: "tree", label: "Tree" },
  { value: "force", label: "Force" },
];

const EDGE_OPTIONS: { value: EdgeStyle; label: string }[] = [
  { value: "polyline", label: "Polyline" },
  { value: "orthogonal", label: "Orthogonal" },
];

/** A real namespace is always an IRI, so this can never collide with one —
 *  the sentinel Radix's `Select` needs since it has no native "unselected"
 *  value, standing in for `namespaceFilter === null` ("all namespaces"). */
const ALL_NAMESPACES = "__all__";

const FORMAT_OPTIONS: { value: ExportFormat; label: string }[] = (
  Object.keys(FORMAT_LABELS) as ExportFormat[]
).map((value) => ({ value, label: FORMAT_LABELS[value] }));

interface OntologyBuilderProps {
  readonly colors: Colors;
  /** Threaded through to the Workbench tab's SPARQL/Cypher queries — Plan
   *  120 Slice H. `null` means "now", matching every other consumer of the
   *  app's global time-travel clock. */
  readonly asOf: string | null;
}

function FilterIcon() {
  return (
    <svg aria-hidden viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2">
      <path d="M4 5h16l-6 8v6l-4-2v-4z" />
    </svg>
  );
}

function ResetIcon() {
  return (
    <svg aria-hidden viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2">
      <path d="M4 4v6h6M20 20v-6h-6" />
      <path d="M4.5 15a8 8 0 0 0 14.5 3M19.5 9A8 8 0 0 0 5 6" />
    </svg>
  );
}

function DownloadIcon() {
  return (
    <svg aria-hidden viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2">
      <path d="M12 3v12m0 0-4-4m4 4 4-4M4 19h16" />
    </svg>
  );
}

function UploadIcon() {
  return (
    <svg aria-hidden viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2">
      <path d="M12 21V9m0 0-4 4m4-4 4 4M4 5h16" />
    </svg>
  );
}

export function OntologyBuilder({ colors, asOf }: OntologyBuilderProps) {
  const [model, setModel] = useState<OntologyModel>(() => loadModel());
  const [layout, setLayout] = useState<LayoutName>("radial");
  const [edgeStyle, setEdgeStyle] = useState<EdgeStyle>("polyline");
  /** `null` means "all namespaces" — the filter's own default, and the same
   *  value `filterModelByNamespace` treats as unfiltered. Plan 120 Slice B. */
  const [namespaceFilter, setNamespaceFilter] = useState<string | null>(null);
  const [format, setFormat] = useState<ExportFormat>("json");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [resetToken, setResetToken] = useState(0);
  const [packs, setPacks] = useState<readonly OntologyPackOption[]>([]);
  const [vocabularyOpen, setVocabularyOpen] = useState(false);
  const [notice, setNotice] = useState<{ kind: "success" | "error"; text: string } | null>(null);

  // The pack picker's own state: which packs this deployment reports as
  // installed, and which of them have actually had data imported —
  // discovered the same two-call way the old ontology editor's picker did,
  // so this picker can never offer a pack the rest of the console does not
  // also know about.
  const [installed, setInstalled] = useState<readonly InstalledPack[]>([]);
  const [loadedSources, setLoadedSources] = useState<readonly LoadedSource[]>([]);
  const [selectedPackId, setSelectedPackId] = useState<string | null>(null);
  const [packLoadBusy, setPackLoadBusy] = useState(false);

  // Code tab state — the merged OntologyEditor. `codeText` is edited
  // freely; a successful parse writes straight into `model` (single source
  // of truth), a failed one leaves `model` untouched and only sets
  // `codeError`, mirroring `applyParseOutcome`'s own "never clear the last
  // good state on a syntax error" rule.
  const [codeFormat, setCodeFormat] = useState<OntologyEditFormat>("turtle");
  const [codeText, setCodeText] = useState("");
  const [codeError, setCodeError] = useState<string | null>(null);
  const [checkResult, setCheckResult] = useState<
    { kind: "syntaxError"; message: string } | { kind: "checked"; accepted: string[]; rejected: [string, string][]; newInferences: number } | null
  >(null);
  const [checkBusy, setCheckBusy] = useState(false);
  const [saveResult, setSaveResult] = useState<
    { kind: "syntaxError"; message: string } | { kind: "saved"; landed: string[] } | null
  >(null);
  const [saveBusy, setSaveBusy] = useState(false);

  const notify = (kind: "success" | "error", text: string) => {
    setNotice({ kind, text });
    setTimeout(() => setNotice(null), 4000);
  };

  useEffect(() => {
    saveModel(model);
  }, [model]);

  useEffect(() => {
    let live = true;
    api
      .ontologyPacks()
      .then((list) => {
        if (!live) return;
        setPacks(
          list.map((pack) => ({
            id: pack.id,
            name: pack.packId,
            description: pack.sourceUrl,
          })),
        );
      })
      .catch(() => {
        // Packs are optional — the builder works without them.
      });
    void (async () => {
      try {
        const [namespaces, graphs] = await Promise.all([api.namespaces(), api.sparql(NAMED_GRAPHS_QUERY)]);
        if (!live) return;
        setInstalled(installedPacks(namespaces));
        setLoadedSources(loadedSourcesFromSparql(graphs.rows));
      } catch {
        // No packs installed, or the graph could not be read — the picker
        // simply offers nothing.
      }
    })();
    return () => {
      live = false;
    };
    // Runs once on mount — the effect body itself decides liveness.
  }, []);

  /** Every namespace an entity in this model actually has — hidden from
   *  the filter control entirely when empty, matching this codebase's own
   *  "a filter with nothing to filter is noise" rule. */
  const availableNamespaces = useMemo(() => namespacesIn(model), [model]);
  const elements = useMemo(
    () => toFlowElements(filterModelByNamespace(model, namespaceFilter)),
    [model, namespaceFilter],
  );

  const selectedEntity = useMemo(
    () => (selectedId ? entityById(model, selectedId) ?? null : null),
    [model, selectedId],
  );

  const selectedRelationship = useMemo(
    () => (selectedId ? relationshipById(model, selectedId) ?? null : null),
    [model, selectedId],
  );

  const handleImport = (file: File): boolean => {
    void file.text().then((text) => {
      try {
        const imported = importModel(text, format);
        setModel(imported);
        setSelectedId(null);
        notify("success", "Ontology imported");
      } catch {
        notify("error", COPY.importError);
      }
    });
    return false;
  };

  const handleExport = () => {
    const blob = new Blob([exportModel(model, format, BASE_IRI)], { type: FORMAT_MIME[format] });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `ontology.${FORMAT_EXTENSIONS[format]}`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const handleSelectPack = (packId: string) => {
    setSelectedPackId(packId);
    const source = ontologySourceFor(packId, loadedSources);
    if (!source) return;
    const pack = installed.find((p) => p.packId === packId);
    setPackLoadBusy(true);
    api
      .sparql(triplesQuery(source.iri))
      .then((result) => {
        const loadedModel = importNTriples(ntriplesFromRows(result.rows));
        setModel(loadedModel);
        setSelectedId(null);
        notify("success", COPY.packLoaded(pack?.label ?? packId));
      })
      .catch(() => notify("error", COPY.packLoadFailed))
      .finally(() => setPackLoadBusy(false));
  };

  const loadCodeTab = (nextFormat: OntologyEditFormat) => {
    const exportFormat = CODE_FORMATS.find((f) => f.value === nextFormat)!.exportFormat;
    setCodeFormat(nextFormat);
    setCodeText(exportModel(model, exportFormat, BASE_IRI));
    setCodeError(null);
  };

  const handleCodeChange = (text: string) => {
    setCodeText(text);
    const exportFormat = CODE_FORMATS.find((f) => f.value === codeFormat)!.exportFormat;
    try {
      const parsed = importModel(text, exportFormat);
      setModel(parsed);
      setCodeError(null);
    } catch (err) {
      setCodeError(err instanceof Error ? err.message : COPY.syntaxError);
    }
  };

  const runCheck = () => {
    setCheckBusy(true);
    setCheckResult(null);
    api.ontologyEditorDryRun(codeFormat, codeText).then(setCheckResult).finally(() => setCheckBusy(false));
  };

  const runSave = () => {
    setSaveBusy(true);
    setSaveResult(null);
    api.ontologyEditorSave(codeFormat, codeText).then(setSaveResult).finally(() => setSaveBusy(false));
  };

  const stats: { label: string; value: number }[] = [
    { label: COPY.entities, value: model.entityTypes.length },
    { label: COPY.relationships, value: model.relationships.length },
    { label: COPY.interactions, value: model.interactions.length },
    { label: COPY.referenceData, value: model.referenceData.length },
    { label: COPY.sources, value: model.sources.length },
  ];

  return (
    <TooltipProvider>
      <div className="flex h-full flex-col">
        {/* Header */}
        <div
          className="flex flex-wrap items-center justify-between gap-4 border-b px-4 py-3"
          style={{ borderColor: colors.border, background: colors.raised }}
        >
          <div className="flex items-center gap-4">
            <div>
              <h4 className="m-0 text-base font-semibold" style={{ color: colors.text }}>
                {COPY.title}
              </h4>
              <p className="m-0 text-xs" style={{ color: colors.textSubtle }}>
                {COPY.subtitle}
              </p>
            </div>
            <PackPicker
              packs={installed}
              sources={loadedSources}
              selectedPackId={selectedPackId}
              loading={packLoadBusy}
              onSelect={handleSelectPack}
            />
          </div>

          <div className="flex flex-wrap items-center gap-5">
            {stats.map((stat) => (
              <div key={stat.label}>
                <div className="text-lg font-semibold" style={{ color: colors.text }}>
                  {stat.value}
                </div>
                <div className="text-xs" style={{ color: colors.textSubtle }}>
                  {stat.label}
                </div>
              </div>
            ))}
          </div>

          <div className="flex items-center gap-2">
            <Select value={format} onValueChange={(v) => setFormat(v as ExportFormat)}>
              <SelectTrigger className="w-[170px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {FORMAT_OPTIONS.map((opt) => (
                  <SelectItem key={opt.value} value={opt.value}>
                    {opt.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button variant="outline" onClick={handleExport} className="gap-1.5">
              <DownloadIcon />
              {COPY.export}
            </Button>
            <label>
              <input
                type="file"
                accept={`.${FORMAT_EXTENSIONS[format]}`}
                className="hidden"
                onChange={(e) => {
                  const file = e.target.files?.[0];
                  if (file) handleImport(file);
                  e.target.value = "";
                }}
              />
              <Button variant="outline" className="gap-1.5" asChild>
                <span>
                  <UploadIcon />
                  {COPY.import}
                </span>
              </Button>
            </label>
          </div>
        </div>

        {notice && (
          <div
            className="px-4 py-1.5 text-sm"
            style={{
              background: notice.kind === "success" ? `${colors.success}1a` : `${colors.error}1a`,
              color: notice.kind === "success" ? colors.success : colors.error,
            }}
          >
            {notice.text}
          </div>
        )}

        <Tabs defaultValue="visual" className="flex min-h-0 flex-1 flex-col">
          <div className="border-b px-4 pt-2" style={{ borderColor: colors.border }}>
            <TabsList>
              <TabsTrigger value="visual">{COPY.visualTab}</TabsTrigger>
              <TabsTrigger value="code" onClick={() => loadCodeTab(codeFormat)}>
                {COPY.codeTab}
              </TabsTrigger>
              <TabsTrigger value="workbench">{COPY.workbenchTab}</TabsTrigger>
            </TabsList>
          </div>

          <TabsContent value="visual" className="mt-0 flex min-h-0 flex-1">
            {/* Main canvas + optional side panel */}
            <div className="flex min-h-0 flex-1">
              <div className="relative min-w-0 flex-1 p-4">
                {/* Canvas overlay — create + filter */}
                <div className="absolute left-6 top-6 z-10 flex items-center gap-2">
                  <AddEntityTypeDialog model={model} onChange={setModel} />
                  <AddRelationshipDialog model={model} onChange={setModel} />
                  <Dialog open={vocabularyOpen} onOpenChange={setVocabularyOpen}>
                    <DialogTrigger asChild>
                      <Button variant="outline" className="gap-1.5">
                        <FilterIcon />
                        {COPY.filter}
                      </Button>
                    </DialogTrigger>
                    <DialogContent className="max-w-2xl">
                      <DialogHeader>
                        <DialogTitle>{COPY.vocabularyDialogTitle}</DialogTitle>
                      </DialogHeader>
                      <SupportingVocabulary model={model} packs={packs} onChange={setModel} />
                    </DialogContent>
                  </Dialog>
                </div>

                {/* Canvas overlay — view controls */}
                <div
                  className="absolute right-6 top-6 z-10 flex items-center gap-2 rounded-xl border px-3 py-2"
                  style={{ background: colors.raised, borderColor: colors.border, boxShadow: colors.shadowSmall }}
                >
                  <span className="text-xs uppercase tracking-wide" style={{ color: colors.textSubtle }}>
                    {COPY.layout}
                  </span>
                  <Select value={layout} onValueChange={(v) => setLayout(v as LayoutName)}>
                    <SelectTrigger className="w-[110px]">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {LAYOUT_OPTIONS.map((opt) => (
                        <SelectItem key={opt.value} value={opt.value}>
                          {opt.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <span className="text-xs uppercase tracking-wide" style={{ color: colors.textSubtle }}>
                    {COPY.edges}
                  </span>
                  <Select value={edgeStyle} onValueChange={(v) => setEdgeStyle(v as EdgeStyle)}>
                    <SelectTrigger className="w-[130px]">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {EDGE_OPTIONS.map((opt) => (
                        <SelectItem key={opt.value} value={opt.value}>
                          {opt.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  {/* Plan 120 Slice B — restored after commit 5cd2fd4 dropped
                      it merging OntologyEditor.tsx's Check/Save logic into
                      this file without carrying the filter over. Hidden
                      entirely when the model has no namespaced entity yet —
                      a filter with one option ("all") that changes nothing
                      is noise, not a control. */}
                  {availableNamespaces.length > 0 && (
                    <>
                      <span className="text-xs uppercase tracking-wide" style={{ color: colors.textSubtle }}>
                        {COPY.namespace}
                      </span>
                      <Select
                        value={namespaceFilter ?? ALL_NAMESPACES}
                        onValueChange={(v) =>
                          setNamespaceFilter(typeof v === "string" && v !== ALL_NAMESPACES ? v : null)
                        }
                      >
                        <SelectTrigger className="w-[160px]">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value={ALL_NAMESPACES}>{COPY.allNamespaces}</SelectItem>
                          {availableNamespaces.map((ns) => (
                            <SelectItem key={ns} value={ns}>
                              {ns}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </>
                  )}
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label={COPY.resetView}
                        onClick={() => setResetToken((t) => t + 1)}
                      >
                        <ResetIcon />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>{COPY.resetView}</TooltipContent>
                  </Tooltip>
                </div>

                {model.entityTypes.length === 0 ? (
                  <div
                    className="flex h-full items-center justify-center rounded-2xl border"
                    style={{ borderColor: colors.border, background: colors.raised }}
                  >
                    <p style={{ color: colors.textSubtle }}>{COPY.emptyState}</p>
                  </div>
                ) : (
                  <OntologyCanvas
                    elements={elements}
                    colors={colors}
                    layout={layout}
                    edgeStyle={edgeStyle}
                    selectedId={selectedId}
                    resetToken={resetToken}
                    onSelectNode={setSelectedId}
                    onSelectEdge={setSelectedId}
                    onClearSelection={() => setSelectedId(null)}
                  />
                )}
              </div>

              {selectedEntity && (
                <div
                  className="w-[360px] overflow-auto border-l p-4"
                  style={{ borderColor: colors.border, background: colors.raised }}
                >
                  <EntityTypePanel model={model} entity={selectedEntity} onChange={setModel} />
                </div>
              )}
              {selectedRelationship && !selectedEntity && (
                <div
                  className="w-[360px] overflow-auto border-l p-4"
                  style={{ borderColor: colors.border, background: colors.raised }}
                >
                  <RelationshipPanel model={model} relationship={selectedRelationship} onChange={setModel} />
                </div>
              )}
            </div>
          </TabsContent>

          <TabsContent value="code" className="mt-0 flex min-h-0 flex-1 flex-col gap-3 overflow-auto p-4">
            <div className="flex flex-wrap items-center gap-2">
              <Select
                value={codeFormat}
                onValueChange={(v) => loadCodeTab(v as OntologyEditFormat)}
              >
                <SelectTrigger className="w-[140px]">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {CODE_FORMATS.map((f) => (
                    <SelectItem key={f.value} value={f.value}>
                      {f.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Button onClick={runCheck} disabled={checkBusy || codeError !== null}>
                {COPY.check}
              </Button>
              <Button onClick={runSave} disabled={saveBusy || codeError !== null}>
                {COPY.save}
              </Button>
            </div>

            <Textarea
              value={codeText}
              onChange={(e) => handleCodeChange(e.target.value)}
              spellCheck={false}
              className="min-h-[360px] font-mono text-xs"
            />
            {codeError && (
              <p className="text-sm" style={{ color: colors.error }}>
                {`${COPY.syntaxError}: ${codeError}`}
              </p>
            )}

            {checkResult && (
              <div className="text-sm">
                {checkResult.kind === "syntaxError" ? (
                  <p style={{ color: colors.error }}>{`${COPY.syntaxError}: ${checkResult.message}`}</p>
                ) : (
                  <div>
                    <p style={{ color: colors.text }}>
                      {`${COPY.checkedTitle}: ${checkResult.accepted.join(", ") || "—"} ${COPY.newInferencesLabel}: ${checkResult.newInferences}`}
                    </p>
                    {checkResult.rejected.length > 0 && (
                      <ul>
                        {checkResult.rejected.map(([subject, reason]) => (
                          <li key={subject} style={{ color: colors.warning }}>
                            {`${subject}: ${reason}`}
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                )}
              </div>
            )}

            {saveResult && (
              <p className="text-sm" style={{ color: saveResult.kind === "saved" ? colors.success : colors.error }}>
                {saveResult.kind === "saved"
                  ? `${COPY.savedTitle}: ${saveResult.landed.length} subject${saveResult.landed.length === 1 ? "" : "s"} landed.`
                  : `${COPY.saveErrorTitle}: ${saveResult.message}`}
              </p>
            )}
          </TabsContent>

          {/* Plan 120 Slice H — SPARQL/Cypher over the same graph, as a tab
              beside Visual/Code rather than a separate "Workbench"
              destination. Extracted unchanged from App.tsx's own
              WorkbenchPage (Plan 111 Slice B). */}
          <TabsContent value="workbench" className="mt-0 flex min-h-0 flex-1 flex-col gap-3 overflow-auto p-4">
            <WorkbenchPanel colors={colors} asOf={asOf} />
          </TabsContent>
        </Tabs>
      </div>
    </TooltipProvider>
  );
}
