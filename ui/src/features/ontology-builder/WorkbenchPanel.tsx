/** SPARQL/Cypher over the same catalog graph, as a tab inside the Ontology
 *  Builder — Plan 120 Slice H.
 *
 *  **Extracted from `App.tsx`'s own `WorkbenchPage` (Plan 111 Slice B),
 *  unchanged in behaviour.** The standalone "Workbench" nav entry is gone;
 *  querying the vocabulary you are looking at and querying the graph it
 *  describes are the same activity, so this is a tab beside Visual/Code
 *  rather than a separate destination a reader has to navigate away to
 *  reach. Every helper this component calls (`ui/src/workbench/*.ts`) is
 *  untouched — this file only moves where the component using them lives. */

import { useMemo, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Space,
  Table,
  Tag,
  Text,
  Tooltip,
  Segmented,
  Paragraph,
  Input,
} from "../../components/ui/antd-compat";
import { Background, Controls, ReactFlow } from "@xyflow/react";
import { api, ApiError, type AlignmentReviewEntry, type SparqlResult } from "../../api";
import {
  type Solution,
  alignmentBadgeLabel,
  columns as resultColumns,
  display as displayTerm,
  graphShape,
  toGraph,
  verdict,
} from "../../workbench/results";
import {
  type Drafts,
  type Language,
  defaultQuery,
  isLanguage,
  keepDrafts,
  pastTenseNote,
} from "../../workbench/language";
import { readParam, writeParam } from "../deepLink";
import type { palette } from "../../theme";

type Colors = (typeof palette)["light"];

const COPY = {
  languageLabel: "Query language",
  sparqlOption: "SPARQL",
  cypherOption: "Cypher",
  run: "Run",
  federatedTooltip: "This answer includes data fetched live from these SERVICE endpoints.",
  queryFailedTitle: "The query did not run",
  planTitle: "Plan",
  noScanNeeded: "No scan was needed.",
  tableView: "Table",
  graphView: "Graph",
  noGraphHint: "These results are not triples, so there is no honest graph to draw.",
  unbound: "unbound",
};

/** `2 rows · 45 facts read` / `1 row · 45 facts read` — a single expression
 *  rather than the same sentence split across raw JSX text nodes, which
 *  `local/no-raw-jsx-text` refuses (this file has no App.tsx-era grandfather
 *  exemption from that rule, unlike the component this was extracted from). */
function rowsSummary(rowCount: number, factsScanned: number): string {
  return `${rowCount} row${rowCount === 1 ? "" : "s"} · ${factsScanned} facts read`;
}

function federatedTag(endpoint: string): string {
  return `federated: ${endpoint}`;
}

function alignmentTooltip(entry: AlignmentReviewEntry): string {
  const base = `${entry.left ?? "?"} → ${entry.right ?? "?"}`;
  const detail = entry.sourceDetail ? ` · ${entry.sourceDetail}` : "";
  const lossy = entry.lossyReverse ? " · lossy walking back from right to left" : "";
  return `${base}${detail}${lossy}`;
}

function noRowsMessage(factsScanned: number): string {
  return `The query ran and matched nothing. That is an answer — it read ${factsScanned} facts to establish it.`;
}

export function WorkbenchPanel({ colors, asOf }: { colors: Colors; asOf: string | null }) {
  // Plan 111 Slice B. `POST /cypher` had no console caller at all before
  // this — a property-graph query language implemented end to end and
  // reachable only with curl.
  const [language, setLanguageRaw] = useState<Language>(() => {
    const saved = readParam("lang");
    return isLanguage(saved) ? saved : "sparql";
  });
  const [query, setQuery] = useState(() => defaultQuery(language));
  // Per-language drafts. Toggling to look at the other language and back
  // must not silently discard real work.
  const [drafts, setDrafts] = useState<Drafts>({});
  const [result, setResult] = useState<SparqlResult | null>(null);
  const [running, setRunning] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);
  const [asGraph, setAsGraph] = useState(false);

  const setLanguage = (next: Language) => {
    const kept = keepDrafts(drafts, language, query);
    setDrafts(kept);
    setQuery(kept[next] ?? defaultQuery(next));
    setLanguageRaw(next);
    writeParam("lang", next === "sparql" ? null : next);
    // A result answered in the other language beside a query in this one
    // would be attributed to the wrong text. Clearing is the honest reset.
    setResult(null);
    setFailed(null);
  };

  const run = async () => {
    setRunning(true);
    setFailed(null);
    try {
      // Both routes render the identical outcome envelope server-side, which
      // is what lets one results table serve both without knowing which ran.
      setResult(
        language === "cypher" ? await api.cypher(query, asOf) : await api.sparql(query, asOf),
      );
    } catch (error) {
      // A parse error is the author's to fix and belongs on screen verbatim —
      // "query failed" sends them guessing at which line.
      setFailed(error instanceof ApiError ? error.problem.detail ?? error.problem.title : "the query did not run");
      setResult(null);
    } finally {
      setRunning(false);
    }
  };

  const asOfNote = pastTenseNote(asOf);

  const rows: Solution[] = useMemo(() => [...(result?.rows ?? [])], [result]);
  const shape = useMemo(() => graphShape(rows), [rows]);
  const notes = useMemo(
    () =>
      result
        ? verdict(rows, {
            truncated: result.truncated,
            factsScanned: result.factsScanned,
            plan: result.plan,
            silencedFailures: result.silencedFailures,
          })
        : null,
    [result, rows],
  );

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      {/* **Two languages, one graph, one results table.** The server renders
          both through the identical outcome envelope, which is what lets the
          plan, the budget and the error shape below be written once. */}
      <Segmented
        value={language}
        aria-label="Query language"
        onChange={(value) => setLanguage(value as Language)}
        options={[
          { label: COPY.sparqlOption, value: "sparql" },
          { label: COPY.cypherOption, value: "cypher" },
        ]}
      />
      {/* A result from the past that looks like a result from now is the one
          failure this surface cannot afford: historical and stale data are
          indistinguishable on screen unless something says which this is. */}
      {asOfNote && <Alert type="warning" showIcon message={asOfNote} />}
      <Input.TextArea
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        autoSize={{ minRows: 5, maxRows: 16 }}
        spellCheck={false}
        style={{ fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace", fontSize: 13 }}
      />

      <Space>
        <Button type="primary" loading={running} onClick={() => void run()}>
          {COPY.run}
        </Button>
        {result && (
          <Text type="secondary" style={{ fontSize: 12 }}>
            {rowsSummary(rows.length, result.factsScanned)}
          </Text>
        )}
        {/* Epic 101 Slice E: result-level, not per-row — spareval gives no
            hook to attribute one bound row to the SERVICE call that produced
            it, only the query as a whole (see the plan's own scope note). */}
        {result && result.federatedEndpoints.length > 0 && (
          <Tooltip title={COPY.federatedTooltip}>
            <span>
              {result.federatedEndpoints.map((endpoint) => (
                <Tag key={endpoint} color="blue">
                  {federatedTag(endpoint)}
                </Tag>
              ))}
            </span>
          </Tooltip>
        )}
        {/* Epic 104's console criterion: "on any cross-vocabulary result the
            alignment that made it reachable is inspectable — a result that
            crossed an approximate match must be distinguishable from one
            that did not, and not by colour alone." The tag's own text
            (`alignmentBadgeLabel`) carries curated-vs-computed and, for a
            computed match, its confidence — colour here is a supplementary
            cue, never the only signal. The tooltip is what makes it
            *inspectable*: the alignment's own left/right terms, source
            detail, and directionality, without cluttering the results
            table itself. */}
        {result &&
          result.alignmentsUsed.map((entry) => (
            <Tooltip key={entry.subject} title={alignmentTooltip(entry)}>
              <Tag color={entry.sourceKind === "computed" ? "gold" : "purple"}>
                {alignmentBadgeLabel(entry)}
              </Tag>
            </Tooltip>
          ))}
      </Space>

      {failed && <Alert type="error" showIcon message={COPY.queryFailedTitle} description={failed} />}

      {notes?.warnings.map((warning) => (
        <Alert key={warning} type="warning" showIcon message={warning} />
      ))}

      {result && (
        <Card size="small" title="Plan">
          {/* One line per scan. A single `? ? ?` is the whole graph, and that
              is exactly the entry worth seeing. */}
          {result.plan.map((scan, i) => (
            <div key={`${scan}-${i}`}>
              <Text code style={{ fontSize: 12 }}>
                {scan}
              </Text>
            </div>
          ))}
          {result.plan.length === 0 && (
            <Text type="secondary" style={{ fontSize: 12 }}>
              {COPY.noScanNeeded}
            </Text>
          )}
        </Card>
      )}

      {result && rows.length > 0 && (
        <>
          <Space>
            <Button size="small" type={asGraph ? "default" : "primary"} onClick={() => setAsGraph(false)}>
              {COPY.tableView}
            </Button>
            {/* Offered only when the results *are* triples. Drawing arbitrary
                columns as nodes asserts a relationship the query never
                returned, and a picture is believed more readily than a table. */}
            <Tooltip title={shape ? undefined : COPY.noGraphHint}>
              <Button
                size="small"
                disabled={!shape}
                type={asGraph ? "primary" : "default"}
                onClick={() => setAsGraph(true)}
              >
                {COPY.graphView}
              </Button>
            </Tooltip>
          </Space>

          {asGraph && shape ? (
            <Card size="small" styles={{ body: { height: 420, padding: 0 } }}>
              <ReactFlow
                nodes={toGraph(rows, shape).nodes.map((node, i) => ({
                  id: node.id,
                  position: { x: (i % 5) * 190, y: Math.floor(i / 5) * 110 },
                  data: { label: node.label },
                  style: {
                    background: colors.surface,
                    border: `1px solid ${colors.border}`,
                    borderRadius: 8,
                    fontSize: 12,
                    padding: 6,
                  },
                }))}
                edges={toGraph(rows, shape).edges.map((edge, i) => ({
                  id: `${edge.from}-${edge.to}-${i}`,
                  source: edge.from,
                  target: edge.to,
                  label: edge.label,
                }))}
                fitView
                proOptions={{ hideAttribution: true }}
              >
                <Background />
                <Controls />
              </ReactFlow>
            </Card>
          ) : (
            <Table
              size="small"
              rowKey={(_, i) => String(i)}
              dataSource={rows.map((row, i) => ({ ...row, __i: i }))}
              pagination={{ pageSize: 25, size: "small" }}
              columns={resultColumns(rows, result.variables).map((name) => ({
                title: name,
                dataIndex: name,
                key: name,
                render: (value: string | undefined) =>
                  value === undefined ? (
                    // An unbound variable is not an empty string. `OPTIONAL`
                    // produces exactly this, and rendering it blank makes
                    // "no value" indistinguishable from "the empty value".
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      {COPY.unbound}
                    </Text>
                  ) : (
                    <Tooltip title={value}>
                      <Text style={{ fontSize: 12 }}>{displayTerm(value)}</Text>
                    </Tooltip>
                  ),
              }))}
            />
          )}
        </>
      )}

      {result && rows.length === 0 && !failed && (
        <Card>
          <Paragraph type="secondary" style={{ margin: 0 }}>
            {noRowsMessage(result.factsScanned)}
          </Paragraph>
        </Card>
      )}
    </Space>
  );
}
