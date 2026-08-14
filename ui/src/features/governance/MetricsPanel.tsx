/** Governed metrics and deployment-defined properties — Plan 110 Slice 4.
 *
 *  **Scheduled last on purpose, and built anyway.** Plan 110 put these behind
 *  reasoning, tests and the governance queues because they are genuine
 *  capability with no evidence anybody wants them yet. That ordering was right;
 *  it is not a reason to leave six routes permanently invisible, and each of
 *  these is a table over a list.
 *
 *  **The `gaps` column is the whole reason the metrics half is worth having.**
 *  A metric catalogue that lists names is a glossary; one that says *this
 *  metric claims to be governed and has no formula and no source assets* is a
 *  catalogue. The server computes that (`graph_owl_core::metric::gaps`) and
 *  this renders it rather than re-deriving it, so the console cannot disagree
 *  with the API about what is missing.
 *
 *  **Domain-agnostic**: a metric is a name, a formula and the assets it reads;
 *  a custom property is an extension field on an entity type. Nothing here
 *  interprets either, and the neutrality check fails the build if it starts to.
 *
 *  Read-only, for the same reason as `QualityPanel`: authoring a metric needs a
 *  formula editor and a source picker, which is a feature rather than a table. */

import { useEffect, useState } from "react";
import { Alert, Card, Empty, Space, Table, Tag, Typography } from "antd";
import { ApiError, api, type BusinessMetric, type CustomProperty } from "../../api";

const { Text, Paragraph } = Typography;

const COPY = {
  metricsTitle: "Business metrics",
  metricsHint:
    "Metric definitions and, for each, what it claims versus what it can show. A metric with no formula and no sources is a name with a number beside it — which is the thing a metric catalogue exists to stop.",
  metricsEmpty: "No metrics defined",
  metricsEmptyBody:
    "Nothing has defined a governed metric on this deployment. Definitions are created through the API; this console shows them rather than authoring them.",
  propsTitle: "Custom properties",
  propsHint:
    "Extension fields this deployment added to an entity type. They are part of the model, so a reviewer should be able to see them without reading a migration.",
  propsEmpty: "No custom properties",
  propsEmptyBody: "This deployment has not extended any entity type.",
  loadFailed: "Could not load this list",
  name: "Name",
  formula: "Formula",
  unit: "Unit",
  sources: "Sources",
  gaps: "Gaps",
  complete: "complete",
  entityType: "On",
  propertyType: "Type",
  description: "Description",
  none: "—",
};

function useList<T>(load: () => Promise<readonly T[]>) {
  const [rows, setRows] = useState<readonly T[] | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    load().then(
      (found) => {
        if (!live) return;
        // **A route that answers a shape this did not expect must degrade, not
        // blank the page.** `/business-metrics` paginates where its neighbours
        // return bare arrays, and rendering the envelope threw
        // `rows is not iterable` — which unmounted the whole Governance
        // section, not just this card. One unforeseen shape should cost one
        // empty table.
        setRows(Array.isArray(found) ? found : []);
      },
      (error: unknown) => {
        if (!live) return;
        setFailure(error instanceof ApiError ? error.problem.title : COPY.loadFailed);
        setRows([]);
      },
    );
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return { rows, failure };
}

export function MetricsPanel() {
  const metrics = useList<BusinessMetric>(() => api.businessMetrics());
  const properties = useList<CustomProperty>(() => api.customProperties());

  const metricsEmpty = metrics.rows !== null && metrics.rows.length === 0 && !metrics.failure;
  const propsEmpty = properties.rows !== null && properties.rows.length === 0 && !properties.failure;

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Card size="small" title={COPY.metricsTitle}>
        <Paragraph type="secondary" style={{ fontSize: 12 }}>
          {COPY.metricsHint}
        </Paragraph>
        {metrics.failure && <Alert type="error" showIcon message={metrics.failure} style={{ marginBottom: 8 }} />}
        {metricsEmpty ? (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={
              <Space direction="vertical">
                <Text strong>{COPY.metricsEmpty}</Text>
                <Text type="secondary">{COPY.metricsEmptyBody}</Text>
              </Space>
            }
          />
        ) : (
          <Table
            size="small"
            loading={metrics.rows === null}
            rowKey="id"
            dataSource={metrics.rows ? [...metrics.rows] : []}
            pagination={(metrics.rows?.length ?? 0) > 10 ? { pageSize: 10 } : false}
            scroll={{ x: "max-content" }}
            columns={[
              { title: COPY.name, dataIndex: "name", key: "name" },
              {
                title: COPY.formula,
                dataIndex: "formula",
                key: "formula",
                render: (v: string | null) => v ?? <Text type="secondary">{COPY.none}</Text>,
              },
              {
                title: COPY.unit,
                dataIndex: "unit",
                key: "unit",
                render: (v: string | null) => v ?? COPY.none,
              },
              {
                title: COPY.sources,
                dataIndex: "sourceAssets",
                key: "sourceAssets",
                align: "right" as const,
                render: (v: readonly string[] | undefined) => v?.length ?? 0,
              },
              {
                title: COPY.gaps,
                dataIndex: "gaps",
                key: "gaps",
                // **Computed by the server, rendered here.** Re-deriving it in
                // the console would let the two disagree about what is missing,
                // and the API's answer is the one an integrator also sees.
                render: (v: readonly string[] | undefined) =>
                  v && v.length > 0 ? (
                    <Space size={4} wrap>
                      {v.map((gap) => (
                        <Tag color="orange" key={gap}>
                          {gap}
                        </Tag>
                      ))}
                    </Space>
                  ) : (
                    <Tag color="green">{COPY.complete}</Tag>
                  ),
              },
            ]}
          />
        )}
      </Card>

      <Card size="small" title={COPY.propsTitle}>
        <Paragraph type="secondary" style={{ fontSize: 12 }}>
          {COPY.propsHint}
        </Paragraph>
        {properties.failure && (
          <Alert type="error" showIcon message={properties.failure} style={{ marginBottom: 8 }} />
        )}
        {propsEmpty ? (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={
              <Space direction="vertical">
                <Text strong>{COPY.propsEmpty}</Text>
                <Text type="secondary">{COPY.propsEmptyBody}</Text>
              </Space>
            }
          />
        ) : (
          <Table
            size="small"
            loading={properties.rows === null}
            rowKey="id"
            dataSource={properties.rows ? [...properties.rows] : []}
            pagination={(properties.rows?.length ?? 0) > 10 ? { pageSize: 10 } : false}
            scroll={{ x: "max-content" }}
            columns={[
              { title: COPY.name, dataIndex: "name", key: "name" },
              { title: COPY.entityType, dataIndex: "entityType", key: "entityType" },
              {
                title: COPY.propertyType,
                dataIndex: "propertyType",
                key: "propertyType",
                render: (v: string | undefined) => (v ? <Tag>{v}</Tag> : COPY.none),
              },
              {
                title: COPY.description,
                dataIndex: "description",
                key: "description",
                render: (v: string | null) => v ?? COPY.none,
              },
            ]}
          />
        )}
      </Card>
    </Space>
  );
}
