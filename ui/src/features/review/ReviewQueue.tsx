/** Epic 42 Slice C: merge adjudication — the first review queue instance.
 *  Deliberately built directly against Epic 17's resolution queue for now,
 *  the same way `VocabularyBrowser.tsx` started glossary-only in Slice A;
 *  Slice D's job is to generalize this into a config-driven
 *  `ReviewQueue.tsx` the way Slice B generalized the vocabulary browser,
 *  proven by adding the other three queues without touching this file's
 *  structure. */

import { useCallback, useEffect, useMemo, useState } from "react";
import { Alert, Button, Empty, Flex, Input, List, Modal, Segmented, Space, Spin, Tag, Typography } from "antd";
import CheckCircleOutlined from "@ant-design/icons/es/icons/CheckCircleOutlined";
import WarningOutlined from "@ant-design/icons/es/icons/WarningOutlined";
import { api, ApiError, type Asset, type Evidence, type ReviewQueueEntry, type ReviewStatus } from "../../api";
import { compareAssets } from "./reviewDiff";
import { readParam, writeParam } from "../deepLink";

const { Text, Title, Paragraph } = Typography;

const COPY = {
  title: "Merge review",
  subtitle: "Ambiguous entity matches the resolver could not decide on its own.",
  loading: "Loading the review queue…",
  loadError: "The review queue could not be loaded.",
  emptyPendingTitle: "Nothing to review",
  emptyPendingDescription: "No ambiguous matches are waiting for a decision.",
  emptyConfirmedTitle: "No merges yet",
  emptyConfirmedDescription: "No candidate has been confirmed as a merge yet.",
  emptyRejectedTitle: "No rejections yet",
  emptyRejectedDescription: "No candidate has been rejected yet.",
  detailPlaceholder: "Select a candidate to compare it against its match.",
  confirmTitle: "Merge these two entities?",
  confirmBody:
    "This can be undone later by splitting the merge — it is not a silent, permanent change.",
  confirmOk: "Merge",
  rejectTitle: "Reject this match",
  rejectHint:
    "A rejection has to say why — without a reason, nobody reviewing this later can tell why the resolver's suggestion was wrong.",
  rejectPlaceholder: "Why is this not a match?",
  rejectOk: "Reject",
  deferHint:
    "Deferred candidates stay pending on the server. They are hidden here only until this page reloads.",
  alreadyDecided: "Someone else already decided this candidate.",
  targetColumn: "Target",
  candidateColumn: "Candidate",
  matchSuffix: "% match",
  reasonPrefix: "Reason: ",
  mergeAction: "Merge",
  rejectAction: "Reject",
  deferAction: "Defer",
};

const STATUS_OPTIONS: { label: string; value: ReviewStatus }[] = [
  { label: "Pending", value: "pending" },
  { label: "Confirmed", value: "confirmed" },
  { label: "Rejected", value: "rejected" },
];

function evidenceLabel(evidence: Evidence): string {
  switch (evidence.kind) {
    case "exactFqn":
      return "exact fully-qualified name";
    case "normalizedFqn":
      return "normalized fully-qualified name";
    case "exactName":
      return `exact name within ${evidence.scope}`;
    case "nameSimilarity":
      return `name similarity (${evidence.metric} ${evidence.value.toFixed(2)})`;
    case "structuralOverlap":
      return `${evidence.sharedColumns}/${evidence.total} shared columns`;
    case "sameParent":
      return "same parent";
    case "sameSourceSystem":
      return "same source system";
  }
}

function deciderLabel(entry: ReviewQueueEntry): string {
  if (!entry.decidedBy) return "";
  if (entry.decidedBy.kind === "human") return `by ${entry.decidedBy.userId}`;
  if (entry.decidedBy.kind === "agent") return `by agent ${entry.decidedBy.agentId}`;
  return "automatically";
}

function isConflict(status: ApiError): boolean {
  return status.problem.status === 409;
}

export function ReviewQueue() {
  const [status, setStatusRaw] = useState<ReviewStatus>(() => {
    const named = readParam("status");
    return named === "confirmed" || named === "rejected" ? named : "pending";
  });
  const [entries, setEntries] = useState<ReviewQueueEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [deferredIds, setDeferredIds] = useState<ReadonlySet<string>>(new Set());
  const [selectedId, setSelectedIdRaw] = useState<string | null>(() => readParam("entry"));
  const [targetAsset, setTargetAsset] = useState<Asset | null>(null);
  const [candidateAsset, setCandidateAsset] = useState<Asset | null>(null);
  const [detailError, setDetailError] = useState<string | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [rejectOpen, setRejectOpen] = useState(false);
  const [rejectReason, setRejectReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(() => {
    setError(null);
    api.reviewQueue({ status }).then(
      (page) => setEntries(page.data),
      (err) => {
        setError(err instanceof ApiError ? err.problem.title : COPY.loadError);
        setEntries([]);
      },
    );
  }, [status]);

  useEffect(() => {
    load();
  }, [load]);

  const setStatusTab = (next: ReviewStatus) => {
    setStatusRaw(next);
    writeParam("status", next === "pending" ? null : next);
    setSelectedIdRaw(null);
    writeParam("entry", null);
  };

  const setSelectedId = (id: string | null) => {
    setSelectedIdRaw(id);
    writeParam("entry", id);
  };

  // Deferring is client-only: no backend action names "defer" (Epic 17
  // only knows pending/confirmed/rejected), and a deferred candidate is by
  // definition one nobody decided, so leaving it untouched server-side
  // *is* the correct behavior — it stays pending for the next reviewer or
  // the next visit here.
  const visibleEntries = useMemo(
    () => (entries ?? []).filter((entry) => status !== "pending" || !deferredIds.has(entry.id)),
    [entries, status, deferredIds],
  );

  const selectedEntry = useMemo(
    () => entries?.find((entry) => entry.id === selectedId) ?? null,
    [entries, selectedId],
  );

  useEffect(() => {
    if (!selectedEntry) {
      setTargetAsset(null);
      setCandidateAsset(null);
      return;
    }
    let cancelled = false;
    setDetailError(null);
    Promise.all([api.asset(selectedEntry.target), api.asset(selectedEntry.candidate)]).then(
      ([target, candidate]) => {
        if (cancelled) return;
        setTargetAsset(target);
        setCandidateAsset(candidate);
      },
      (err) => {
        if (cancelled) return;
        setDetailError(
          err instanceof ApiError ? err.problem.title : "the two entities could not be loaded",
        );
      },
    );
    return () => {
      cancelled = true;
    };
  }, [selectedEntry]);

  const comparison = useMemo(
    () => (targetAsset && candidateAsset ? compareAssets(targetAsset, candidateAsset) : []),
    [targetAsset, candidateAsset],
  );

  const afterDecision = useCallback(
    (message: string) => {
      setConfirmOpen(false);
      setRejectOpen(false);
      setRejectReason("");
      setSelectedId(null);
      setNotice(message);
      load();
    },
    [load],
  );

  // Epic 42 decision — "two reviewers acting on the same candidate: the
  // second sees the resolution, not a conflict error." The server's own
  // 409 is correct (a second decision on a decided entry is genuinely a
  // conflict); the requirement is what the *reviewer* sees, so this turns
  // that 409 into a plain refresh rather than an error toast.
  const handleConflict = useCallback(
    (err: unknown) => {
      if (err instanceof ApiError && isConflict(err)) {
        afterDecision(COPY.alreadyDecided);
        return true;
      }
      return false;
    },
    [afterDecision],
  );

  const confirmMerge = () => {
    if (!selectedEntry) return;
    setBusy(true);
    api
      .confirmReview(selectedEntry.id)
      .then(() => afterDecision("Merged."))
      .catch((err) => {
        if (!handleConflict(err)) {
          setDetailError(
            err instanceof ApiError ? (err.problem.detail ?? err.problem.title) : "the merge was refused",
          );
          setConfirmOpen(false);
        }
      })
      .finally(() => setBusy(false));
  };

  const rejectMatch = () => {
    if (!selectedEntry) return;
    setBusy(true);
    api
      .rejectReview(selectedEntry.id, rejectReason.trim())
      .then(() => afterDecision("Rejected."))
      .catch((err) => {
        if (!handleConflict(err)) {
          setDetailError(
            err instanceof ApiError
              ? (err.problem.detail ?? err.problem.title)
              : "the rejection was refused",
          );
          setRejectOpen(false);
        }
      })
      .finally(() => setBusy(false));
  };

  const deferCandidate = () => {
    if (!selectedEntry) return;
    setDeferredIds((prev) => new Set(prev).add(selectedEntry.id));
    setSelectedId(null);
  };

  if (error) return <Alert type="error" showIcon message={error} />;

  const emptyTitle =
    status === "pending"
      ? COPY.emptyPendingTitle
      : status === "confirmed"
        ? COPY.emptyConfirmedTitle
        : COPY.emptyRejectedTitle;
  const emptyDescription =
    status === "pending"
      ? COPY.emptyPendingDescription
      : status === "confirmed"
        ? COPY.emptyConfirmedDescription
        : COPY.emptyRejectedDescription;

  return (
    <Space direction="vertical" size="large" style={{ width: "100%" }}>
      <div>
        {/* `level={2}`, not `4` — the chrome's own `<h1>graph-owl</h1>` is
            the only heading above this one, and `level={4}` here jumped
            straight from 1 to 4, an axe `heading-order` violation found
            testing this page (and, it turned out, already present and
            uncaught in the vocabulary browser's own per-term title —
            fixed there too). `fontSize` pins the previous visual size. */}
        <Title level={2} style={{ margin: 0, fontSize: 20, fontWeight: 600 }}>
          {COPY.title}
        </Title>
        <Paragraph type="secondary" style={{ margin: "4px 0 0", fontSize: 13 }}>
          {COPY.subtitle}
        </Paragraph>
      </div>

      {notice && <Alert type="success" showIcon closable message={notice} onClose={() => setNotice(null)} />}

      <Segmented value={status} onChange={(value) => setStatusTab(value as ReviewStatus)} options={STATUS_OPTIONS} />

      {entries === null ? (
        <Space direction="vertical" align="center" style={{ width: "100%", padding: 48 }}>
          <Spin />
          <Text>{COPY.loading}</Text>
        </Space>
      ) : visibleEntries.length === 0 ? (
        <Empty description={<Text>{emptyDescription}</Text>}>{emptyTitle}</Empty>
      ) : (
        <Flex gap={24} align="flex-start">
          <List
            style={{ width: 360, flex: "none" }}
            dataSource={visibleEntries}
            rowKey="id"
            renderItem={(entry) => (
              <List.Item
                onClick={() => setSelectedId(entry.id)}
                style={{
                  cursor: "pointer",
                  padding: 12,
                  background: entry.id === selectedId ? "rgba(22,119,255,0.08)" : undefined,
                }}
              >
                <Space direction="vertical" size={4} style={{ width: "100%" }}>
                  <Space>
                    <Tag color="blue">
                      {Math.round(entry.score * 100)}
                      {COPY.matchSuffix}
                    </Tag>
                    {entry.status !== "pending" && (
                      <Tag color={entry.status === "confirmed" ? "success" : "default"}>
                        {entry.status} {deciderLabel(entry)}
                      </Tag>
                    )}
                  </Space>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {entry.evidence.map(evidenceLabel).join(", ")}
                  </Text>
                  {entry.reason && (
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      {COPY.reasonPrefix}
                      {entry.reason}
                    </Text>
                  )}
                </Space>
              </List.Item>
            )}
          />

          <div style={{ flex: "auto" }}>
            {!selectedEntry ? (
              <Empty description={<Text>{COPY.detailPlaceholder}</Text>} />
            ) : detailError ? (
              <Alert type="error" showIcon message={detailError} />
            ) : !targetAsset || !candidateAsset ? (
              <Spin />
            ) : (
              <Space direction="vertical" size="middle" style={{ width: "100%" }}>
                <table style={{ width: "100%", borderCollapse: "collapse" }}>
                  <thead>
                    <tr>
                      <th style={{ textAlign: "left", padding: 8 }} />
                      <th style={{ textAlign: "left", padding: 8 }}>{COPY.targetColumn}</th>
                      <th style={{ textAlign: "left", padding: 8 }}>{COPY.candidateColumn}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {comparison.map((field) => (
                      <tr key={field.field} style={{ borderTop: "1px solid rgba(0,0,0,0.06)" }}>
                        <td style={{ padding: 8 }}>
                          <Space size={4}>
                            {field.matches ? (
                              <CheckCircleOutlined style={{ color: "#52c41a" }} />
                            ) : (
                              <WarningOutlined style={{ color: "#faad14" }} />
                            )}
                            <Text strong>{field.label}</Text>
                          </Space>
                        </td>
                        <td style={{ padding: 8 }}>{field.targetValue}</td>
                        <td style={{ padding: 8 }}>{field.candidateValue}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>

                {selectedEntry.status === "pending" && (
                  <Space>
                    <Button type="primary" onClick={() => setConfirmOpen(true)}>
                      {COPY.mergeAction}
                    </Button>
                    <Button danger onClick={() => setRejectOpen(true)}>
                      {COPY.rejectAction}
                    </Button>
                    <Button onClick={deferCandidate}>{COPY.deferAction}</Button>
                  </Space>
                )}
                {selectedEntry.status === "pending" && (
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    {COPY.deferHint}
                  </Text>
                )}
              </Space>
            )}
          </div>
        </Flex>
      )}

      <Modal
        open={confirmOpen}
        title={COPY.confirmTitle}
        okText={COPY.confirmOk}
        confirmLoading={busy}
        onCancel={() => setConfirmOpen(false)}
        onOk={confirmMerge}
      >
        <Paragraph type="secondary" style={{ fontSize: 13 }}>
          {COPY.confirmBody}
        </Paragraph>
      </Modal>

      <Modal
        open={rejectOpen}
        title={COPY.rejectTitle}
        okText={COPY.rejectOk}
        okButtonProps={{ disabled: rejectReason.trim().length === 0, danger: true }}
        confirmLoading={busy}
        onCancel={() => {
          setRejectOpen(false);
          setRejectReason("");
        }}
        onOk={rejectMatch}
      >
        <Paragraph type="secondary" style={{ fontSize: 13 }}>
          {COPY.rejectHint}
        </Paragraph>
        <Input.TextArea
          autoFocus
          rows={3}
          value={rejectReason}
          placeholder={COPY.rejectPlaceholder}
          onChange={(event) => setRejectReason(event.target.value)}
        />
      </Modal>
    </Space>
  );
}
