/** How connected is this neighbourhood — Plan 112 Slice C.
 *
 *  **`GET /assets/{id}/analytics` and `api.assetAnalytics` both existed and
 *  nothing in the console called either.** `graph-owl-analytics` computes
 *  degree centrality, connected components and orphan detection; a grep for
 *  `assetAnalytics` across `ui/src` matched only the client definition. Plan
 *  111 named this defect twice, and this is a third instance of it.
 *
 *  **Bounded, and the panel says so.** The server computes over the same
 *  already-authorized, already-capped walk the explorer draws — never the
 *  whole graph, which Epic 38's purity boundary forbids on a synchronous
 *  request. So a truncated walk is reported as truncated: "3 nodes" from a
 *  walk that stopped early is a claim the server never made, and this project
 *  refuses that everywhere.
 *
 *  **`PageRank` is deliberately absent**, matching the facade: its meaning
 *  depends on whole-graph scope, and computing it over an arbitrary bounded
 *  neighbourhood produces a number shaped like PageRank without meaning what
 *  PageRank means.
 *
 *  Every judgement lives in `graph/analytics.ts`; this mounts, fetches, draws. */

import { useEffect, useState } from "react";
import { Alert, Card, Empty, Space, Table, Tag, Typography } from "antd";
import { ApiError, api, type AssetAnalytics } from "../api";
import { connectivityRows, describeAnalytics } from "./analytics";

const { Text, Paragraph } = Typography;

const COPY = {
  title: "How connected is this?",
  hint: "Degree counts over the same bounded neighbourhood the picture above draws — not the whole graph. The hub of a neighbourhood is usually the thing worth reading next.",
  failed: "Could not compute connectivity",
  empty: "Nothing to measure",
  emptyBody: "This neighbourhood has no nodes beyond the one you are looking at.",
  node: "Node",
  incoming: "Incoming",
  outgoing: "Outgoing",
  orphanTag: "orphan",
  truncatedTag: "walk stopped at its limit",
  /** Named for what it means to a reader, not for the field: `orphans` is a
   *  count of nodes connected to nothing else *within this walk*, which is a
   *  much weaker statement than "connected to nothing". */
  orphanHint: "connected to nothing else within this walk",
};

export function ConnectivityPanel({
  assetId,
  hops,
  names,
}: {
  assetId: string;
  hops: number;
  /** Names the canvas above already resolved for these nodes. **Passed in
   *  rather than fetched**: the explorer walked this neighbourhood and has
   *  them, and a table of raw UUIDs makes the reader match hex strings by eye.
   *  A node this map does not cover reads as its identity, never as a blank. */
  names: ReadonlyMap<string, string>;
}) {
  const [analytics, setAnalytics] = useState<AssetAnalytics | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setAnalytics(null);
    setFailure(null);
    api.assetAnalytics(assetId, { hops }).then(
      (found) => live && setAnalytics(found),
      (error: unknown) => {
        if (!live) return;
        setFailure(error instanceof ApiError ? error.problem.title : COPY.failed);
      },
    );
    return () => {
      live = false;
    };
  }, [assetId, hops]);

  // A contract violation in the payload (vectors of different lengths) throws
  // rather than rendering a shorter prefix that would look complete. Caught
  // here so it costs one card rather than the whole tab.
  let rows: ReturnType<typeof connectivityRows> = [];
  let summary = "";
  let malformed: string | null = null;
  if (analytics) {
    try {
      rows = connectivityRows(analytics, names);
      summary = describeAnalytics(analytics);
    } catch (error) {
      malformed = error instanceof Error ? error.message : COPY.failed;
    }
  }

  return (
    <Card size="small" title={COPY.title} style={{ marginTop: 16 }}>
      <Paragraph type="secondary" style={{ fontSize: 12 }}>
        {COPY.hint}
      </Paragraph>
      {failure && <Alert type="error" showIcon message={failure} />}
      {malformed && <Alert type="error" showIcon message={malformed} />}
      {analytics && !malformed && (
        <Space direction="vertical" size={8} style={{ width: "100%" }}>
          <Space size={8} wrap>
            <Text strong>{summary}</Text>
            {/* Stated, never inferred from a count. */}
            {analytics.truncated && <Tag color="warning">{COPY.truncatedTag}</Tag>}
          </Space>
          {rows.length === 0 ? (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={
                <Space direction="vertical">
                  <Text strong>{COPY.empty}</Text>
                  <Text type="secondary">{COPY.emptyBody}</Text>
                </Space>
              }
            />
          ) : (
            <Table
              size="small"
              rowKey="id"
              dataSource={[...rows]}
              pagination={rows.length > 10 ? { pageSize: 10 } : false}
              scroll={{ x: "max-content" }}
              columns={[
                {
                  title: COPY.node,
                  dataIndex: "label",
                  key: "label",
                  render: (label: string, row: (typeof rows)[number]) => (
                    <Space size={4}>
                      <Text>{label}</Text>
                      {row.orphan && (
                        <Tag color="default" title={COPY.orphanHint}>
                          {COPY.orphanTag}
                        </Tag>
                      )}
                    </Space>
                  ),
                },
                {
                  title: COPY.incoming,
                  dataIndex: "inDegree",
                  key: "inDegree",
                  align: "right" as const,
                },
                {
                  title: COPY.outgoing,
                  dataIndex: "outDegree",
                  key: "outDegree",
                  align: "right" as const,
                },
              ]}
            />
          )}
        </Space>
      )}
    </Card>
  );
}
