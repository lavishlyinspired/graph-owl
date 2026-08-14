/** Data-quality tests and data contracts — Plan 110 Slice 2.
 *
 *  **Eleven routes, one coherent surface, and no console caller for any of
 *  them.** `test-definitions`, `test-cases`, `test-suites`, `test-results`,
 *  `contracts`, its `slas` and its `breaches` all existed server-side; the
 *  product could define an expectation, run it on a cadence, keep a result
 *  history and treat a breach as a contract event, and none of that was
 *  visible to anyone using a browser.
 *
 *  **Domain-agnostic by nature, not by effort.** A test is a named assertion
 *  with an expected cadence and a target; a contract is a promise about an
 *  asset and to whom. What the assertion asserts and what the promise promises
 *  is the deployment's business — GST's would be "no register row lacks a
 *  supplier", a healthcare pack's "no claim lacks a member id". This panel
 *  shows the assertion, its target and its cadence, and never interprets any
 *  of them.
 *
 *  **Read-only, deliberately.** Every route here has a `POST` twin, and a
 *  console that could define tests would need an editor for the assertion
 *  language, a cadence picker and a preview — a feature in its own right.
 *  Making the existing ones *visible* is the change that stops a deployment
 *  accumulating tests nobody knows are failing; authoring them is the next
 *  slice, and pretending otherwise would ship a half-editor.
 *
 *  **An empty list and an unreachable one look identical**, and only one means
 *  nothing is wrong — the same distinction every queue in this console draws. */

import { useEffect, useState } from "react";
import { Alert, Card, Empty, Space, Table, Tag, Typography } from "antd";
import { ApiError, api, type DataContract, type TestCase, type TestDefinition } from "../../api";

const { Text, Paragraph } = Typography;

const COPY = {
  definitionsTitle: "Test definitions",
  definitionsHint:
    "Reusable assertions, independent of what they are bound to. A definition with no cases is an expectation nobody has applied yet.",
  definitionsEmpty: "No test definitions",
  definitionsEmptyBody:
    "Nothing has defined an expectation for this deployment's data. Definitions are created through the API; this console shows them rather than authoring them.",
  casesTitle: "Tests bound to assets",
  casesHint:
    "One assertion against one asset, with the cadence it is expected to run at. A cadence nothing meets is the signal — a test that has not run is not a test that passed.",
  casesEmpty: "No tests bound",
  casesEmptyBody: "No definition has been bound to an asset yet.",
  contractsTitle: "Data contracts",
  contractsHint:
    "What an asset's producer promises, and to whom. Consumers are plural because a contract with one consumer is a special case of the real thing, not the other way round.",
  contractsEmpty: "No contracts",
  contractsEmptyBody: "No producer has made a promise about an asset on this deployment.",
  loadFailed: "Could not load this list",
  name: "Name",
  type: "Type",
  target: "Asset",
  cadence: "Expected cadence",
  description: "Description",
  producer: "Producer",
  consumers: "Consumers",
  slas: "SLAs",
  none: "—",
  noCadence: "not scheduled",
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
    // `load` is a stable module-level api method; depending on its identity
    // would refetch on every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return { rows, failure };
}

/** A card that draws the empty/failed/loaded distinction once, so three
 *  tables do not each reinvent it slightly differently. */
function ListCard<T extends object>({
  title,
  hint,
  emptyTitle,
  emptyBody,
  state,
  rowKey,
  columns,
}: {
  title: string;
  hint: string;
  emptyTitle: string;
  emptyBody: string;
  state: { rows: readonly T[] | null; failure: string | null };
  rowKey: string;
  columns: readonly object[];
}) {
  const empty = state.rows !== null && state.rows.length === 0 && !state.failure;
  return (
    <Card size="small" title={title}>
      <Paragraph type="secondary" style={{ fontSize: 12 }}>
        {hint}
      </Paragraph>
      {state.failure && <Alert type="error" showIcon message={state.failure} style={{ marginBottom: 8 }} />}
      {empty ? (
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description={
            <Space direction="vertical">
              <Text strong>{emptyTitle}</Text>
              <Text type="secondary">{emptyBody}</Text>
            </Space>
          }
        />
      ) : (
        <Table
          size="small"
          loading={state.rows === null}
          rowKey={rowKey}
          dataSource={state.rows ? [...state.rows] : []}
          pagination={(state.rows?.length ?? 0) > 10 ? { pageSize: 10 } : false}
          scroll={{ x: "max-content" }}
          columns={columns as never}
        />
      )}
    </Card>
  );
}

export function QualityPanel() {
  const definitions = useList<TestDefinition>(() => api.testDefinitions());
  const cases = useList<TestCase>(() => api.testCases());
  const contracts = useList<DataContract>(() => api.contracts());

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <ListCard
        title={COPY.definitionsTitle}
        hint={COPY.definitionsHint}
        emptyTitle={COPY.definitionsEmpty}
        emptyBody={COPY.definitionsEmptyBody}
        state={definitions}
        rowKey="id"
        columns={[
          { title: COPY.name, dataIndex: "name", key: "name" },
          { title: COPY.type, dataIndex: "testType", key: "testType", render: (v: string) => <Tag>{v}</Tag> },
          {
            title: COPY.cadence,
            dataIndex: "expectedCadence",
            key: "expectedCadence",
            // **"Not scheduled" and "no value" are different facts.** A test
            // with no cadence is one nothing will ever notice failing.
            render: (v: string | null) => v ?? <Text type="secondary">{COPY.noCadence}</Text>,
          },
          {
            title: COPY.description,
            dataIndex: "description",
            key: "description",
            render: (v: string | null) => v ?? COPY.none,
          },
        ]}
      />

      <ListCard
        title={COPY.casesTitle}
        hint={COPY.casesHint}
        emptyTitle={COPY.casesEmpty}
        emptyBody={COPY.casesEmptyBody}
        state={cases}
        rowKey="id"
        columns={[
          { title: COPY.name, dataIndex: "name", key: "name" },
          { title: COPY.target, dataIndex: "targetFqn", key: "targetFqn" },
          { title: COPY.type, dataIndex: "testType", key: "testType", render: (v: string) => <Tag>{v}</Tag> },
          {
            title: COPY.cadence,
            dataIndex: "expectedCadence",
            key: "expectedCadence",
            render: (v: string | null) => v ?? <Text type="secondary">{COPY.noCadence}</Text>,
          },
        ]}
      />

      <ListCard
        title={COPY.contractsTitle}
        hint={COPY.contractsHint}
        emptyTitle={COPY.contractsEmpty}
        emptyBody={COPY.contractsEmptyBody}
        state={contracts}
        rowKey="id"
        columns={[
          { title: COPY.name, dataIndex: "name", key: "name" },
          { title: COPY.target, dataIndex: "assetFqn", key: "assetFqn" },
          { title: COPY.producer, dataIndex: "producer", key: "producer" },
          {
            title: COPY.consumers,
            dataIndex: "consumers",
            key: "consumers",
            render: (v: readonly string[] | undefined) =>
              v && v.length > 0 ? (
                <Space size={4} wrap>
                  {v.map((consumer) => (
                    <Tag key={consumer}>{consumer}</Tag>
                  ))}
                </Space>
              ) : (
                COPY.none
              ),
          },
          {
            title: COPY.slas,
            dataIndex: "slas",
            key: "slas",
            align: "right" as const,
            render: (v: readonly unknown[] | undefined) => v?.length ?? 0,
          },
        ]}
      />
    </Space>
  );
}
