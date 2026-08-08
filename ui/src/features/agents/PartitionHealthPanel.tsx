/** Epic 102's write-side partition backlog, read-only, plus a manual
 *  compact trigger — Phase 3 item, closing the epic's last open item
 *  (`GET /admin/partition-health` and `POST /admin/compact` both shipped
 *  earlier this session with no console consumer).
 *
 *  `deltaRows`/`oldestDeltaT` both `null` is a real, legitimate state — a
 *  storage backend with no partition split — mirroring `BoltSessionsPanel`'s
 *  own `{enabled: false}` precedent rather than treating it as an error.
 *  `oldestDeltaT` is a transaction time, not wall-clock: this system's `t`
 *  is a monotonic counter, and `partition_health`'s own DTO carries no
 *  wall-clock companion to convert it with, so it is shown as what it is
 *  rather than a fabricated "age". */

import { useCallback, useEffect, useState } from "react";
import { Alert, Button, Space, Spin, Statistic, Typography } from "antd";
import { ApiError, api } from "../../api";

const { Text, Title, Paragraph } = Typography;

const COPY = {
  title: "Partition health",
  intro: "The write-side delta backlog behind Epic 102's split — read-only, plus a manual fold.",
  loading: "Loading…",
  loadError: "Could not load partition health.",
  noSplit: "This storage backend has no read/write partition split.",
  deltaRowsLabel: "Delta rows",
  oldestDeltaLabel: "Oldest delta transaction",
  none: "none",
  compactAction: "Compact now",
  compacting: "Compacting…",
  compactError: "Compaction failed.",
  movedRow: "row",
  movedRows: "rows",
  movedSuffix: "into the main partition.",
};

type PanelState =
  | { kind: "loading" }
  | { kind: "noSplit" }
  | { kind: "error"; message: string }
  | { kind: "ready"; health: { deltaRows: number; oldestDeltaT: number | null } };

export function PartitionHealthPanel() {
  const [state, setState] = useState<PanelState>({ kind: "loading" });
  const [compacting, setCompacting] = useState(false);
  const [compactError, setCompactError] = useState<string | null>(null);
  const [lastMoved, setLastMoved] = useState<number | null>(null);

  const load = useCallback(() => {
    api.partitionHealth().then(
      (health) =>
        setState(
          health.deltaRows === null
            ? { kind: "noSplit" }
            : { kind: "ready", health: { deltaRows: health.deltaRows, oldestDeltaT: health.oldestDeltaT } },
        ),
      (e: unknown) => setState({ kind: "error", message: e instanceof ApiError ? e.message : "unknown error" }),
    );
  }, []);

  useEffect(load, [load]);

  const compact = () => {
    setCompacting(true);
    setCompactError(null);
    api
      .compactPartition()
      .then((result) => {
        setLastMoved(result.moved);
        load();
      })
      .catch((e: unknown) => setCompactError(e instanceof ApiError ? e.message : "unknown error"))
      .finally(() => setCompacting(false));
  };

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <div>
        <Title level={3} style={{ margin: 0, fontWeight: 600, fontSize: 16 }}>
          {COPY.title}
        </Title>
        <Text type="secondary">{COPY.intro}</Text>
      </div>

      {state.kind === "loading" ? (
        <Spin />
      ) : state.kind === "noSplit" ? (
        <Alert type="info" showIcon message={COPY.noSplit} />
      ) : state.kind === "error" ? (
        <Alert type="error" showIcon message={COPY.loadError} description={state.message} />
      ) : (
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <Space size={32}>
            <Statistic title={COPY.deltaRowsLabel} value={state.health.deltaRows} />
            <Statistic title={COPY.oldestDeltaLabel} value={state.health.oldestDeltaT ?? COPY.none} />
          </Space>
          <Button onClick={compact} loading={compacting} disabled={state.health.deltaRows === 0}>
            {compacting ? COPY.compacting : COPY.compactAction}
          </Button>
          {compactError && <Alert type="error" showIcon message={COPY.compactError} description={compactError} />}
          {lastMoved !== null && !compacting && (
            <Paragraph type="secondary" style={{ margin: 0 }}>
              {`Moved ${lastMoved} ${lastMoved === 1 ? COPY.movedRow : COPY.movedRows} ${COPY.movedSuffix}`}
            </Paragraph>
          )}
        </Space>
      )}
    </Space>
  );
}
