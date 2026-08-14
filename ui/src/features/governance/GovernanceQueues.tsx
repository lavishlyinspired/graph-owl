/** Two governance queues that were one route away from being visible —
 *  Plan 110 Slice 3.
 *
 *  **Both existed server-side with no console caller.** `GET /label-suggestions`
 *  returns labels a classifier proposed that nobody has vouched for;
 *  `GET /recertification-queue` returns certifications expired or expiring.
 *  Neither is a large capability, and that is exactly why they were worth doing
 *  together: each is a table over a list, and leaving them unreachable meant a
 *  deployment could be accumulating unreviewed classifications and lapsed
 *  certifications with nothing in the product that would ever say so.
 *
 *  **Domain-agnostic.** A label is `{classification}.{tag}` over an asset FQN
 *  and a certification is a dated attestation about one — what the tags and
 *  types *mean* is the deployment's business. Nothing here names a pack, and
 *  `scripts/check-namespace-neutrality.py` fails the build if it ever does.
 *
 *  **An empty queue and an unreachable one look identical, and only one of them
 *  means nothing is wrong** — the same distinction every other queue in this
 *  console already draws, so a load failure says so rather than rendering as
 *  "nothing to review". */

import { useEffect, useState } from "react";
import { Alert, Button, Card, Empty, Popconfirm, Space, Table, Tag, Typography, message } from "./../../components/ui/antd-compat";
import { ApiError, api, type Certification, type TagLabel } from "../../api";

const { Text, Paragraph } = Typography;

const COPY = {
  labelsTitle: "Suggested classifications",
  labelsHint:
    "Labels a classifier proposed and nobody has confirmed. Left unreviewed they are neither applied nor rejected — the state that looks like a decision and is not.",
  labelsEmpty: "Nothing suggested",
  labelsEmptyBody: "No classifier has proposed a label that is still waiting on a human.",
  recertTitle: "Due for recertification",
  recertHint:
    "Certifications that have expired or are about to. A certification nobody renews keeps asserting something nobody has checked since it was issued.",
  recertEmpty: "Nothing due",
  recertEmptyBody: "No certification has expired or is inside its renewal window.",
  loadFailed: "Could not load this queue",
  target: "Asset",
  tag: "Tag",
  source: "Source",
  state: "State",
  appliedBy: "Applied by",
  confirm: "Confirm",
  reject: "Reject",
  confirmed: "Confirmed.",
  rejected: "Rejected.",
  actionFailed: "That decision could not be recorded",
  rejectTitle: "Reject this suggestion?",
  rejectBody: "The label will not be applied. The classifier may propose it again on the next run.",
  type: "Certification",
  issuer: "Issuer",
  expires: "Expires",
  status: "Status",
};

/** `expired` is not the same as `expiring`, and a queue that coloured them
 *  alike would bury the ones already asserting something untrue. */
function statusColour(status: string): string {
  const text = status.toLowerCase();
  if (text.includes("expired")) return "red";
  if (text.includes("expiring") || text.includes("due")) return "orange";
  return "green";
}

function useQueue<T>(load: () => Promise<readonly T[]>) {
  const [rows, setRows] = useState<readonly T[] | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    load().then(
      (found) => live && setRows(found),
      (error: unknown) => {
        if (!live) return;
        setFailure(error instanceof ApiError ? error.problem.title : COPY.loadFailed);
        setRows([]);
      },
    );
    return () => {
      live = false;
    };
    // `load` is a stable module-level api method; re-running on identity would
    // refetch on every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return { rows, failure };
}

export function GovernanceQueues() {
  const labels = useQueue<TagLabel>(() => api.labelSuggestions());
  const recert = useQueue<Certification>(() => api.recertificationQueue());
  /** Decided rows, removed locally rather than by refetching the whole queue —
   *  a reviewer working down a list should not have it reorder under them
   *  after every decision. */
  const [decided, setDecided] = useState<ReadonlySet<string>>(new Set());

  const decide = async (row: TagLabel, confirm: boolean) => {
    const key = `${row.targetFqn}-${row.tagFqn}`;
    try {
      await (confirm ? api.confirmLabel(row.targetFqn, row.tagFqn) : api.rejectLabel(row.targetFqn, row.tagFqn));
      setDecided((seen) => new Set(seen).add(key));
      message.success(confirm ? COPY.confirmed : COPY.rejected);
    } catch (error) {
      message.error(error instanceof ApiError ? error.problem.title : COPY.actionFailed);
    }
  };

  const pending = (labels.rows ?? []).filter((row) => !decided.has(`${row.targetFqn}-${row.tagFqn}`));

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Card size="small" title={COPY.labelsTitle}>
        <Paragraph type="secondary" style={{ fontSize: 12 }}>
          {COPY.labelsHint}
        </Paragraph>
        {labels.failure && <Alert type="error" showIcon message={labels.failure} style={{ marginBottom: 8 }} />}
        {labels.rows !== null && pending.length === 0 && !labels.failure ? (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={
              <Space direction="vertical">
                <Text strong>{COPY.labelsEmpty}</Text>
                <Text type="secondary">{COPY.labelsEmptyBody}</Text>
              </Space>
            }
          />
        ) : (
          <Table
            size="small"
            loading={labels.rows === null}
            rowKey={(row) => `${row.targetFqn}-${row.tagFqn}`}
            dataSource={[...pending]}
            pagination={pending.length > 10 ? { pageSize: 10 } : false}
            scroll={{ x: "max-content" }}
            columns={[
              { title: COPY.target, dataIndex: "targetFqn", key: "targetFqn" },
              { title: COPY.tag, dataIndex: "tagFqn", key: "tagFqn", render: (v: string) => <Tag>{v}</Tag> },
              { title: COPY.source, dataIndex: "labelType", key: "labelType" },
              { title: COPY.state, dataIndex: "state", key: "state" },
              { title: COPY.appliedBy, dataIndex: "appliedBy", key: "appliedBy" },
              {
                title: "",
                key: "decide",
                width: 180,
                render: (_: unknown, row: TagLabel) => (
                  <Space size={4}>
                    <Button size="small" onClick={() => void decide(row, true)}>
                      {COPY.confirm}
                    </Button>
                    {/* **Confirming is instant, rejecting asks.** Confirming
                      *  applies what a classifier already proposed; rejecting
                      *  discards work and is the one a misclick should not do
                      *  silently — the same asymmetry the findings queue draws. */}
                    <Popconfirm
                      title={COPY.rejectTitle}
                      description={COPY.rejectBody}
                      onConfirm={() => void decide(row, false)}
                    >
                      <Button size="small" danger>
                        {COPY.reject}
                      </Button>
                    </Popconfirm>
                  </Space>
                ),
              },
            ]}
          />
        )}
      </Card>

      <Card size="small" title={COPY.recertTitle}>
        <Paragraph type="secondary" style={{ fontSize: 12 }}>
          {COPY.recertHint}
        </Paragraph>
        {recert.failure && <Alert type="error" showIcon message={recert.failure} style={{ marginBottom: 8 }} />}
        {recert.rows !== null && recert.rows.length === 0 && !recert.failure ? (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={
              <Space direction="vertical">
                <Text strong>{COPY.recertEmpty}</Text>
                <Text type="secondary">{COPY.recertEmptyBody}</Text>
              </Space>
            }
          />
        ) : (
          <Table
            size="small"
            loading={recert.rows === null}
            rowKey="id"
            dataSource={recert.rows ? [...recert.rows] : []}
            pagination={(recert.rows?.length ?? 0) > 10 ? { pageSize: 10 } : false}
            scroll={{ x: "max-content" }}
            columns={[
              { title: COPY.target, dataIndex: "targetFqn", key: "targetFqn" },
              { title: COPY.type, dataIndex: "typeName", key: "typeName" },
              { title: COPY.issuer, dataIndex: "issuer", key: "issuer" },
              {
                title: COPY.expires,
                dataIndex: "expiresAt",
                key: "expiresAt",
                render: (v: string) => (v ? v.slice(0, 10) : "—"),
              },
              {
                title: COPY.status,
                dataIndex: "status",
                key: "status",
                render: (v: string) => <Tag color={statusColour(v ?? "")}>{v ?? "—"}</Tag>,
              },
            ]}
          />
        )}
      </Card>
    </Space>
  );
}
