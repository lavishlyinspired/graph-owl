/** The list + detail pane shell — Epic 42 decision 2. Every difference
 *  between merge candidates, extraction claims, drift and proposals lives
 *  in a `QueueConfig` (`queues.ts`); this file names none of them. If a
 *  queue-specific `if`/`switch` ever appears here, the pattern has failed
 *  — `ReviewQueue.structural.test.ts` asserts exactly that, the same way
 *  `VocabularyBrowser.structural.test.ts` does for the vocabulary
 *  browser. */

import { useCallback, useEffect, useMemo, useState } from "react";
import { Alert, Button, Empty, Flex, Input, List, Modal, Segmented, Space, Spin, Tag, Typography } from "antd";
import type { QueueAction, QueueConfig, QueueEntry } from "./queues";
import { readParam, writeParam } from "../deepLink";

const { Text, Title, Paragraph } = Typography;

const COPY = {
  loading: "Loading the review queue…",
  loadError: "The review queue could not be loaded.",
  detailLoadError: "This entry could not be loaded.",
  detailPlaceholder: "Select an entry to see it in full.",
  alreadyDecided: "Someone else already decided this.",
  deferHint: "Deferred entries stay pending on the server. They are hidden here only until this page reloads.",
  reasonPrefix: "Reason: ",
};

// A config's `fetchEntries`/`fetchDetail` may still throw (only
// `performAction`'s conflict case is a return value, not a throw) — this
// reads whatever message the thrown value carries without needing to
// know it came from `ApiError` specifically, which is what keeps this
// file out of the structural test's `../../api` ban.
function errorMessage(err: unknown, fallback: string): string {
  return err instanceof Error && err.message ? err.message : fallback;
}

export interface ReviewQueueProps {
  readonly config: QueueConfig;
}

export function ReviewQueue({ config }: ReviewQueueProps) {
  const [status, setStatusRaw] = useState<string>(() => {
    const named = readParam("status");
    return named && config.statuses.some((s) => s.key === named) ? named : config.openStatus;
  });
  const [entries, setEntries] = useState<readonly QueueEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [deferredIds, setDeferredIds] = useState<ReadonlySet<string>>(new Set());
  const [selectedId, setSelectedIdRaw] = useState<string | null>(() => readParam("entry"));
  const [detail, setDetail] = useState<React.ReactNode>(null);
  const [detailError, setDetailError] = useState<string | null>(null);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [confirmAction, setConfirmAction] = useState<QueueAction | null>(null);
  const [textAction, setTextAction] = useState<QueueAction | null>(null);
  const [textValue, setTextValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(() => {
    setError(null);
    config.fetchEntries(status).then(
      (fetched) => setEntries(fetched),
      (err) => {
        setError(errorMessage(err, COPY.loadError));
        setEntries([]);
      },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `config` is stable for the component's lifetime (the consumer remounts on queue switch); only `status` should retrigger this
  }, [status]);

  useEffect(() => {
    load();
  }, [load]);

  const setStatusTab = (next: string) => {
    setStatusRaw(next);
    writeParam("status", next === config.openStatus ? null : next);
    setSelectedIdRaw(null);
    writeParam("entry", null);
  };

  const setSelectedId = (id: string | null) => {
    setSelectedIdRaw(id);
    writeParam("entry", id);
  };

  // Deferring is client-only: no queue built so far has a "deferred"
  // status, and an entry nobody decided is, by definition, correctly
  // represented by leaving it untouched server-side.
  const visibleEntries = useMemo(
    () => (entries ?? []).filter((entry) => status !== config.openStatus || !deferredIds.has(entry.id)),
    [entries, status, deferredIds, config.openStatus],
  );

  const selectedEntry = useMemo(
    () => entries?.find((entry) => entry.id === selectedId) ?? null,
    [entries, selectedId],
  );

  useEffect(() => {
    if (!selectedEntry) {
      setDetail(null);
      return;
    }
    let cancelled = false;
    setDetailError(null);
    setLoadingDetail(true);
    config.fetchDetail(selectedEntry).then(
      (fetched) => {
        if (cancelled) return;
        setDetail(fetched);
        setLoadingDetail(false);
      },
      (err) => {
        if (cancelled) return;
        setDetailError(errorMessage(err, COPY.detailLoadError));
        setLoadingDetail(false);
      },
    );
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `config` is stable for the component's lifetime
  }, [selectedEntry]);

  const afterDecision = useCallback(
    (message: string) => {
      setConfirmAction(null);
      setTextAction(null);
      setTextValue("");
      setSelectedId(null);
      setNotice(message);
      load();
    },
    [load],
  );

  const runAction = (action: QueueAction, text: string | undefined) => {
    if (!selectedEntry) return;
    setBusy(true);
    config
      .performAction(selectedEntry, action.key, text)
      .then((result) => {
        // Epic 42 decision — "two reviewers acting on the same entry: the
        // second sees the resolution, not a conflict error." The config's
        // own `performAction` already turned the server's 409 into this
        // return value rather than a throw; what a reviewer sees here is
        // a plain refresh, not an error.
        afterDecision(result.conflict ? COPY.alreadyDecided : (action.doneMessage ?? `${action.label} done.`));
      })
      .catch((err) => {
        setDetailError(errorMessage(err, `${action.label} was refused`));
        setConfirmAction(null);
        setTextAction(null);
      })
      .finally(() => setBusy(false));
  };

  const clickAction = (action: QueueAction) => {
    if (!selectedEntry) return;
    if (action.kind === "clientOnly") {
      setDeferredIds((prev) => new Set(prev).add(selectedEntry.id));
      setSelectedId(null);
      return;
    }
    if (action.kind === "withText") {
      setTextAction(action);
      return;
    }
    if (action.confirmTitle) {
      setConfirmAction(action);
      return;
    }
    runAction(action, undefined);
  };

  if (error) return <Alert type="error" showIcon message={error} />;

  return (
    <Space direction="vertical" size="large" style={{ width: "100%" }}>
      <div>
        {/* `level={2}`, not `4` — the chrome's own `<h1>graph-owl</h1>` is
            the only heading above this one; a jump straight to `4` is an
            axe `heading-order` violation, found and fixed in Epic 42
            Slice C. `fontSize` pins the intended visual size. */}
        <Title level={2} style={{ margin: 0, fontSize: 20, fontWeight: 600 }}>
          {config.label}
        </Title>
        <Paragraph type="secondary" style={{ margin: "4px 0 0", fontSize: 13 }}>
          {config.subtitle}
        </Paragraph>
      </div>

      {notice && <Alert type="success" showIcon closable message={notice} onClose={() => setNotice(null)} />}

      <Segmented
        value={status}
        onChange={(value) => setStatusTab(value as string)}
        options={config.statuses.map((s) => ({ label: s.label, value: s.key }))}
      />

      {entries === null ? (
        <Space direction="vertical" align="center" style={{ width: "100%", padding: 48 }}>
          <Spin />
          <Text>{COPY.loading}</Text>
        </Space>
      ) : visibleEntries.length === 0 ? (
        <Empty description={<Text>{config.emptyDescription(status)}</Text>}>{config.emptyTitle(status)}</Empty>
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
                    <Tag color="blue">{entry.summary}</Tag>
                    {entry.decidedSummary && <Tag>{entry.decidedSummary}</Tag>}
                  </Space>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {entry.detail}
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
            ) : loadingDetail ? (
              <Spin />
            ) : (
              <Space direction="vertical" size="middle" style={{ width: "100%" }}>
                {detail}

                {selectedEntry.status === config.openStatus && (
                  <Space>
                    {config.actions.map((action) => (
                      <Button
                        key={action.key}
                        type={action.key === config.actions[0]?.key ? "primary" : "default"}
                        danger={action.danger}
                        onClick={() => clickAction(action)}
                      >
                        {action.label}
                      </Button>
                    ))}
                  </Space>
                )}
                {selectedEntry.status === config.openStatus && config.actions.some((a) => a.kind === "clientOnly") && (
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
        open={confirmAction !== null}
        title={confirmAction?.confirmTitle}
        okText={confirmAction?.label}
        confirmLoading={busy}
        onCancel={() => setConfirmAction(null)}
        onOk={() => confirmAction && runAction(confirmAction, undefined)}
      >
        <Paragraph type="secondary" style={{ fontSize: 13 }}>
          {confirmAction?.confirmBody}
        </Paragraph>
      </Modal>

      <Modal
        open={textAction !== null}
        title={textAction?.textModalTitle}
        okText={textAction?.label}
        okButtonProps={{ disabled: textValue.trim().length === 0, danger: textAction?.danger }}
        confirmLoading={busy}
        onCancel={() => {
          setTextAction(null);
          setTextValue("");
        }}
        onOk={() => textAction && runAction(textAction, textValue.trim())}
      >
        <Paragraph type="secondary" style={{ fontSize: 13 }}>
          {textAction?.textModalHint}
        </Paragraph>
        <Input.TextArea
          autoFocus
          rows={3}
          value={textValue}
          placeholder={textAction?.textPlaceholder}
          onChange={(event) => setTextValue(event.target.value)}
        />
      </Modal>
    </Space>
  );
}
