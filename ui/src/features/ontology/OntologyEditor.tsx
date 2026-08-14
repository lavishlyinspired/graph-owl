/** Epic 42 Slice G: a text editor beside a live graph of what it declares.
 *  The editor's text is the source; the graph is a rendering of it, never
 *  an alternative input.
 *
 *  **The RED test lives in `ontologyDocument.ts`**: `applyParseOutcome` never
 *  clears `lastGood` on a syntax error. This component's own job is to
 *  drive that state machine from real keystrokes (debounced, not
 *  per-character — "as the author types" without a network round trip on
 *  every code point) and render whatever `lastGood` currently holds, which
 *  is why the graph pane reads `state.lastGood`, never the live parse
 *  attempt directly. */

import { useEffect, useMemo, useRef, useState } from "react";
import cytoscape from "cytoscape";
import { Alert, Button, Input, Select, Space, Tag, Typography } from "antd";
import { api, type OntologyEditFormat } from "../../api";
import {
  applyParseOutcome,
  initialEditorState,
  localName,
  namespacesIn,
  predicatesIn,
  toOntologyElements,
  type EditorState,
  type GraphFilter,
  type ParseOutcome,
} from "./ontologyDocument";
import {
  loadedSourcesFromSparql,
  NAMED_GRAPHS_QUERY,
  ntriplesFromRows,
  ontologySourceFor,
  triplesQuery,
  type LoadedSource,
} from "../packs/packData";
import { installedPacks, type InstalledPack } from "../packs/packSurfaces";
import type {
  OntologyDryRunResult,
  OntologyPreviewResult,
  OntologySaveResult,
} from "../../api";
import { brand, palette } from "../../theme";

const { Title, Text, Paragraph } = Typography;
const { TextArea } = Input;

const COPY = {
  title: "Ontology editor",
  intro: "The graph on the right is a rendering of the text on the left, never a second input.",
  formatLabel: "Format",
  documentPlaceholder: "Write Turtle, N-Triples, or JSON-LD here…",
  syntaxErrorPrefix: "Syntax error",
  noGraphYet: "Nothing parses yet — start writing to see the graph.",
  namespaceFilter: "Filter by namespace",
  predicateFilter: "Filter by predicate",
  allNamespaces: "All namespaces",
  allPredicates: "All predicates",
  check: "Check",
  save: "Save",
  loadPackLabel: "Load installed pack",
  loadPackPlaceholder: "Choose a pack…",
  load: "Load",
  noOntologyLoaded: "This pack has no ontology loaded yet.",
  checkedTitle: "Would be accepted",
  newInferences: "New inferences",
  rejectedTitle: "Would be refused",
  savedTitle: "Saved",
  saveErrorTitle: "Could not save",
};

function outcomeFrom(result: OntologyPreviewResult): ParseOutcome {
  if (result.kind === "syntaxError") {
    return { kind: "syntaxError", message: result.message, line: result.line, column: result.column };
  }
  return { kind: "preview", preview: { triples: result.triples, declared: result.declared } };
}

/** The graph pane. A separate component so its own `useEffect` pair (create
 *  once, update elements in place) does not compete with the editor's own
 *  debounce timer — the same "elements are replaced in place, not
 *  remounted" reasoning the Explorer's own canvas already establishes,
 *  here for the identical reason: a remount would lose whatever the reader
 *  had panned or zoomed to. */
function OntologyGraph({
  elements,
  colors,
}: {
  elements: ReturnType<typeof toOntologyElements>;
  colors: (typeof palette)["light"];
}) {
  const host = useRef<HTMLDivElement | null>(null);
  const cy = useRef<cytoscape.Core | null>(null);

  useEffect(() => {
    if (!host.current) return undefined;
    const instance = cytoscape({
      container: host.current,
      elements: elements as cytoscape.ElementDefinition[],
      style: [
        {
          selector: "node",
          style: {
            label: "data(label)",
            "font-size": 11,
            color: colors.text,
            "text-valign": "bottom",
            "text-margin-y": 4,
            "background-color": colors.primary,
            width: 18,
            height: 18,
          },
        },
        // A term this document merely references — never asserted about —
        // is drawn hollow. An author who cannot tell this from a declared
        // term will "fix" somebody else's vocabulary.
        {
          selector: "node.referenced",
          style: {
            "background-opacity": 0.15,
            "border-width": 2,
            "border-color": colors.primary,
          },
        },
        { selector: "edge", style: { width: 1, "line-color": colors.border, "curve-style": "straight", label: "data(label)", "font-size": 9, color: colors.textMuted } },
        // Subsumption reads differently from an ordinary property — a
        // class hierarchy is not just another relationship.
        {
          selector: "edge.subsumption",
          style: { width: 2, "line-color": brand.cyan400, "target-arrow-color": brand.cyan400, "target-arrow-shape": "triangle" },
        },
      ],
      layout: { name: "breadthfirst", directed: true, animate: false, spacingFactor: 1.2, padding: 24 },
      autoungrabify: true,
    });
    cy.current = instance;
    return () => {
      instance.destroy();
      cy.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [colors]);

  useEffect(() => {
    const instance = cy.current;
    if (!instance) return;
    instance.elements().remove();
    instance.add(elements as cytoscape.ElementDefinition[]);
    instance.layout({ name: "breadthfirst", directed: true, animate: false, spacingFactor: 1.2, padding: 24 }).run();
  }, [elements]);

  return (
    <div
      ref={host}
      role="img"
      aria-label="The ontology this document declares"
      style={{
        height: 420,
        border: `1px solid ${colors.border}`,
        borderRadius: 16,
        background: colors.raised,
      }}
    />
  );
}

export function OntologyEditor({ colors }: { colors: (typeof palette)["light"] }) {
  const [state, setState] = useState<EditorState>(() => initialEditorState());
  const [filter, setFilter] = useState<GraphFilter>({ namespace: null, predicate: null });
  const [dryRun, setDryRun] = useState<OntologyDryRunResult | null>(null);
  const [dryRunBusy, setDryRunBusy] = useState(false);
  const [saveResult, setSaveResult] = useState<OntologySaveResult | null>(null);
  const [saveBusy, setSaveBusy] = useState(false);

  // The installed-pack picker's own state — Plan 116 Slice A. Fetched once
  // on mount, the same two calls `PackDataExplorer` already makes, so this
  // picker can never offer a pack or source the Explore sider does not also
  // know about.
  const [packs, setPacks] = useState<readonly InstalledPack[]>([]);
  const [sources, setSources] = useState<readonly LoadedSource[]>([]);
  const [selectedPack, setSelectedPack] = useState<string | null>(null);
  const [loadBusy, setLoadBusy] = useState(false);

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const [namespaces, graphs] = await Promise.all([api.namespaces(), api.sparql(NAMED_GRAPHS_QUERY)]);
        if (!live) return;
        setPacks(installedPacks(namespaces));
        setSources(loadedSourcesFromSparql(graphs.rows));
      } catch {
        // No packs installed, or the graph could not be read — the picker
        // simply offers nothing, the same "absent is the default" rule
        // `PackDataExplorer` already applies; manual paste is unaffected.
      }
    })();
    return () => {
      live = false;
    };
  }, []);

  const ontologySource = selectedPack ? ontologySourceFor(selectedPack, sources) : null;

  const runLoad = () => {
    if (!ontologySource) return;
    setLoadBusy(true);
    api
      .sparql(triplesQuery(ontologySource.iri))
      .then((result) => {
        const document = ntriplesFromRows(result.rows);
        setState((prev) => ({ ...prev, format: "ntriples", document }));
      })
      .finally(() => setLoadBusy(false));
  };

  // Debounced, not per-keystroke — "as the author types" without a network
  // round trip on every code point. `prev.document` (not the closed-over
  // `state.document`) is what `applyParseOutcome` writes back, so a
  // response that arrives after the author kept typing does not overwrite
  // what they have since typed with a stale value.
  useEffect(() => {
    const timer = setTimeout(() => {
      api.ontologyEditorPreview(state.format, state.document).then(
        (result) => {
          const outcome = outcomeFrom(result);
          setState((prev) => applyParseOutcome(prev, prev.document, outcome));
        },
        () => {
          // A network failure is not a syntax error — leave the last good
          // graph and the last error exactly as they were rather than
          // inventing a state for a condition this slice does not model.
        },
      );
    }, 400);
    return () => clearTimeout(timer);
  }, [state.document, state.format]);

  const elements = useMemo(
    () => (state.lastGood ? toOntologyElements(state.lastGood, filter) : []),
    [state.lastGood, filter],
  );
  const namespaces = useMemo(
    () => (state.lastGood ? namespacesIn(state.lastGood) : []),
    [state.lastGood],
  );
  const predicates = useMemo(
    () => (state.lastGood ? predicatesIn(state.lastGood) : []),
    [state.lastGood],
  );

  const runCheck = () => {
    setDryRunBusy(true);
    setDryRun(null);
    api
      .ontologyEditorDryRun(state.format, state.document)
      .then(setDryRun)
      .finally(() => setDryRunBusy(false));
  };

  const runSave = () => {
    setSaveBusy(true);
    setSaveResult(null);
    api
      .ontologyEditorSave(state.format, state.document)
      .then(setSaveResult)
      .finally(() => setSaveBusy(false));
  };

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <div>
        <Title level={3} style={{ margin: 0, fontWeight: 600, fontSize: 16 }}>
          {COPY.title}
        </Title>
        <Text type="secondary">{COPY.intro}</Text>
      </div>

      <Space wrap>
        <Select<OntologyEditFormat>
          value={state.format}
          aria-label={COPY.formatLabel}
          onChange={(format) => setState((prev) => ({ ...prev, format }))}
          style={{ minWidth: 140 }}
          options={[
            { value: "turtle", label: "Turtle" },
            { value: "ntriples", label: "N-Triples" },
            { value: "jsonld", label: "JSON-LD" },
          ]}
        />
        <Button onClick={runCheck} loading={dryRunBusy} disabled={state.error !== null}>
          {COPY.check}
        </Button>
        <Button type="primary" onClick={runSave} loading={saveBusy} disabled={state.error !== null}>
          {COPY.save}
        </Button>
      </Space>

      {packs.length > 0 && (
        <Space wrap>
          <Select
            allowClear
            virtual={false}
            aria-label={COPY.loadPackLabel}
            placeholder={COPY.loadPackPlaceholder}
            style={{ minWidth: 200 }}
            value={selectedPack}
            onChange={(packId) => setSelectedPack(packId ?? null)}
            options={packs.map((pack) => ({ value: pack.packId, label: pack.label }))}
          />
          <Button onClick={runLoad} loading={loadBusy} disabled={!ontologySource}>
            {COPY.load}
          </Button>
          {selectedPack !== null && !ontologySource && (
            <Text type="secondary">{COPY.noOntologyLoaded}</Text>
          )}
        </Space>
      )}

      <div style={{ display: "flex", gap: 16, width: "100%" }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <TextArea
            value={state.document}
            onChange={(e) =>
              setState((prev) => ({ ...prev, document: e.target.value }))
            }
            placeholder={COPY.documentPlaceholder}
            autoSize={{ minRows: 16, maxRows: 24 }}
            spellCheck={false}
            style={{ fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace", fontSize: 13 }}
          />
          {state.error && (
            <Alert
              style={{ marginTop: 8 }}
              type="error"
              showIcon
              message={
                state.error.line !== null
                  ? `${COPY.syntaxErrorPrefix} — line ${state.error.line}${state.error.column !== null ? `, column ${state.error.column}` : ""}`
                  : COPY.syntaxErrorPrefix
              }
              description={state.error.message}
            />
          )}
        </div>

        <div style={{ flex: 1, minWidth: 0 }}>
          <Space wrap style={{ marginBottom: 8 }}>
            <Select
              allowClear
              // A namespace list is small and bounded (one document's own
              // vocabulary set) — virtualizing it buys nothing and costs a
              // screen reader accurate option counts, since a virtualized
              // listbox only ever has the scrolled-into-view options in the
              // DOM at once.
              virtual={false}
              placeholder={COPY.allNamespaces}
              aria-label={COPY.namespaceFilter}
              style={{ minWidth: 200 }}
              value={filter.namespace}
              onChange={(namespace) => setFilter((prev) => ({ ...prev, namespace: namespace ?? null }))}
              options={namespaces.map((ns) => ({ value: ns, label: ns }))}
            />
            <Select
              allowClear
              virtual={false}
              placeholder={COPY.allPredicates}
              aria-label={COPY.predicateFilter}
              style={{ minWidth: 200 }}
              value={filter.predicate}
              onChange={(predicate) => setFilter((prev) => ({ ...prev, predicate: predicate ?? null }))}
              options={predicates.map((p) => ({ value: p, label: localName(p) }))}
            />
          </Space>
          {state.lastGood === null ? (
            <Paragraph type="secondary">{COPY.noGraphYet}</Paragraph>
          ) : (
            <OntologyGraph elements={elements} colors={colors} />
          )}
        </div>
      </div>

      {dryRun && (
        <div>
          {dryRun.kind === "syntaxError" ? (
            <Alert type="error" showIcon message={COPY.syntaxErrorPrefix} description={dryRun.message} />
          ) : (
            <Space direction="vertical" size="small" style={{ width: "100%" }}>
              <Space wrap>
                <Text strong>{`${COPY.checkedTitle}:`}</Text>
                {dryRun.accepted.map((subject) => (
                  <Tag key={subject} color="green">
                    {subject}
                  </Tag>
                ))}
                <Text type="secondary">{`${COPY.newInferences}: ${dryRun.newInferences}`}</Text>
              </Space>
              {dryRun.rejected.length > 0 && (
                <Space direction="vertical" size={4}>
                  <Text strong>{`${COPY.rejectedTitle}:`}</Text>
                  {dryRun.rejected.map(([subject, reason]) => (
                    <Alert key={subject} type="warning" showIcon message={subject} description={reason} />
                  ))}
                </Space>
              )}
            </Space>
          )}
        </div>
      )}

      {saveResult && (
        <div>
          {saveResult.kind === "syntaxError" ? (
            <Alert type="error" showIcon message={COPY.saveErrorTitle} description={saveResult.message} />
          ) : (
            <Alert
              type="success"
              showIcon
              message={COPY.savedTitle}
              description={`${saveResult.landed.length} subject${saveResult.landed.length === 1 ? "" : "s"} landed.`}
            />
          )}
        </div>
      )}
    </Space>
  );
}
