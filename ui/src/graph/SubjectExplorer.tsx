/** Explore any subject's neighbourhood — Plan 113 Slice C.
 *
 *  **Answers the question `GraphExplorer` cannot, for anything that is not a
 *  catalog asset.** `/assets/{id}/graph` needs a UUID with a real relational
 *  row behind it; a GST invoice has neither. `Catalog::graph_context`
 *  already walked from any `Sid` and had no caller — this is that caller.
 *
 *  **Same three things `GraphExplorer` gives a table**: the neighbourhood
 *  (as a list — `00f`'s own non-negotiable that a picture needs an
 *  accessible equivalent, built here first rather than after a canvas), the
 *  relationship filter, and connectivity. `ConnectivityPanel` is reused
 *  unchanged, fed `api.graphContextAnalytics` instead of `api.assetAnalytics`.
 *
 *  A full interactive canvas for this shape is a deliberate non-goal here —
 *  see `subjectContext.ts`'s own doc comment for why. */

import { useEffect, useState } from "react";
import { Alert, Card, Select, Space, Tag, Typography } from "./../components/ui/antd-compat";
import { ApiError, api, type GraphContext } from "../api";
import { palette } from "../theme";
import { filterParam, kindsFromAnalytics, whyEmpty } from "./edgeFilter";
import { edgeRows, nodeRows, summarize } from "./subjectContext";
import { ConnectivityPanel } from "./ConnectivityPanel";
import { ReasoningView } from "./ReasoningView";

const { Text, Paragraph } = Typography;

const COPY = {
  hint: "Everything asserted about this subject that this walk reached — not only what a finding's rule happened to name.",
  failed: "Could not load this neighbourhood",
  relationships: "Relationships",
  everyRelationship: "every relationship",
  nodesLabel: "Nodes",
  edgesLabel: "Relationships",
  sourcesLabel: "sources",
  walkStopped: "walk stopped at its limit",
  derivedTag: "derived",
  reasoningTitle: "What follows from this",
  reasoningHint:
    "Conclusions the reasoner reached about this subject that no document states. Everything above is something asserted; these are facts the axioms make unavoidable.",
};

export function SubjectExplorer({
  seed,
  label,
  hops = 2,
  colors = palette.light,
}: {
  /** A bare UUID (a catalog asset) or a `namespace:local` identifier (a pack
   *  subject) — the same shape `findPaths` already accepts. */
  seed: string;
  label: string;
  hops?: number;
  /** Only the derivation chain's indent rules read this. Defaulted rather
   *  than required so a caller that has no theme handy — the findings queue
   *  is a generic pack surface — is not forced to thread one through. */
  colors?: (typeof palette)["light"];
}) {
  const [selected, setSelected] = useState<readonly string[]>([]);
  const [kinds, setKinds] = useState<readonly string[]>([]);
  const [context, setContext] = useState<GraphContext | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setContext(null);
    setFailure(null);
    const filter = filterParam(selected);
    api.graphContext({ seed, hops, relationshipTypes: filter }).then(
      (found) => live && setContext(found),
      (error: unknown) => {
        if (!live) return;
        setFailure(error instanceof ApiError ? error.problem.title : COPY.failed);
      },
    );
    return () => {
      live = false;
    };
  }, [seed, hops, selected]);

  useEffect(() => {
    let live = true;
    api
      .graphContextAnalytics({ seed, hops })
      .then((found) => live && setKinds(kindsFromAnalytics(found.edgeTypes)))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [seed, hops]);

  const names = new Map<string, string>([[seed, label]]);
  const emptyReason = context
    ? whyEmpty({ selected, hasAnyEdge: kinds.length > 0, edgesShown: context.edges.length })
    : null;

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Paragraph type="secondary" style={{ fontSize: 12 }}>
        {COPY.hint}
      </Paragraph>

      {kinds.length > 0 && (
        <Select
          mode="multiple"
          allowClear
          size="small"
          aria-label={COPY.relationships}
          placeholder={COPY.everyRelationship}
          style={{ minWidth: 260 }}
          value={[...selected]}
          onChange={(next) => setSelected(Array.isArray(next) ? next.map(String) : [])}
          options={kinds.map((kind) => ({ value: kind, label: kind }))}
        />
      )}

      {failure && <Alert type="error" showIcon message={failure} />}
      {emptyReason && <Alert type="info" showIcon message={emptyReason} />}

      {context && (
        <Card size="small" title={summarize(context)}>
          {context.truncated && <Tag color="warning">{COPY.walkStopped}</Tag>}
          <Text strong style={{ fontSize: 12 }}>
            {COPY.nodesLabel}
          </Text>
          {nodeRows(context, names).map((row) => (
            <div key={row.id}>
              <Text>{row.label}</Text>
              {row.sources.length > 0 && (
                <Space size={4} style={{ marginLeft: 6 }}>
                  {row.sources.map((source) => (
                    <Tag key={source}>{source}</Tag>
                  ))}
                </Space>
              )}
            </div>
          ))}
          {context.edges.length > 0 && (
            <>
              <Text strong style={{ fontSize: 12, display: "block", marginTop: 8 }}>
                {COPY.edgesLabel}
              </Text>
              {edgeRows(context, names).map((row) => (
                // Keyed on the triple itself, not position: two edges can
                // share a subject/relationship/object shape (a derived and an
                // asserted copy of the same fact) only if they are the same
                // fact twice, which collapsing to one row is the right
                // behaviour for.
                <div key={`${row.from}→${row.relationship}→${row.to}`}>
                  <Text type="secondary">
                    {row.from} {row.relationship} {row.to}
                    {row.derived && (
                      <Tag color="processing" style={{ marginLeft: 6 }}>
                        {COPY.derivedTag}
                      </Tag>
                    )}
                  </Text>
                </div>
              ))}
            </>
          )}
        </Card>
      )}

      <ConnectivityPanel
        cacheKey={`${seed}:${hops}:${selected.join(",")}`}
        load={() =>
          api.graphContextAnalytics({ seed, hops, relationshipTypes: filterParam(selected) })
        }
        names={names}
      />

      {/* **What follows that nobody wrote down — Plan 113 Slice D.** The walk
          above and the degree counts beside it both report what *was*
          asserted. An entailment is the one answer in this console that is
          neither asserted nor absent, and until this slice it was reachable
          only from an asset's own tab. */}
      <Card size="small" title={COPY.reasoningTitle}>
        <Paragraph type="secondary" style={{ fontSize: 12 }}>
          {COPY.reasoningHint}
        </Paragraph>
        <ReasoningView subject={seed} colors={colors} />
      </Card>
    </Space>
  );
}
