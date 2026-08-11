/** Browsing and switching between installed domain packs — Epic 105 F1.
 *
 *  **`PackImportPanel` answers "how do I load data into a pack"; this
 *  answers "which packs are installed, and what do I do with each one."**
 *  Different question, same discovery mechanism (`GET /namespaces`'
 *  `declaredBy: "pack:<id>"`), and deliberately not merged into the same
 *  component — an admin confirming a pack loaded correctly should not have
 *  to scroll past every upload surface each one exposes, and a pack that
 *  has landed data but declares no upload surface (a namespace registered
 *  only through `graph-owl-load-pack`, no import needed) would otherwise
 *  be invisible: `surfacesFor` filters to `imports.length > 0`;
 *  `installedPacks` does not, which is the whole reason this panel calls
 *  the latter rather than reusing the former. */

import { useCallback, useEffect, useState } from "react";
import { Alert, Button, Card, Empty, Space, Table, Tag, Typography, message } from "antd";
import { SyncOutlined } from "@ant-design/icons";
import { api } from "../../api";
import { installedPacks, type InstalledPack } from "./packSurfaces";

const { Text } = Typography;

const COPY = {
  title: "Packs",
  subtitle:
    "Every domain pack this deployment reports as installed, discovered from the namespaces each one registered — not a build-time list.",
  emptyTitle: "No domain pack installed",
  emptyBody:
    "Install one with graph-owl-load-pack. A deployment with no pack has nothing to browse or switch between, which is why this is empty rather than hidden.",
  loadFailed: "Could not read which packs are installed",
  loading: "Loading…",
  viewObligations: "View obligations",
  reconcile: "Reconcile now",
  reconciling: "Reconciling…",
  reconcileFailed: "Reconciliation could not be triggered",
};

function ReconcileAction({ packId }: { packId: string }) {
  const [busy, setBusy] = useState(false);

  const run = useCallback(async () => {
    setBusy(true);
    try {
      const outcome = await api.reconcilePack(packId);
      message.success(
        outcome.found === 0
          ? `${packId}: reconciliation ran — no rule matched.`
          : `${packId}: ${outcome.found} finding(s) — see Review.`,
      );
    } catch (error) {
      message.error(error instanceof Error ? error.message : COPY.reconcileFailed);
    } finally {
      setBusy(false);
    }
  }, [packId]);

  return (
    <Button size="small" icon={<SyncOutlined spin={busy} />} loading={busy} onClick={() => void run()}>
      {busy ? COPY.reconciling : COPY.reconcile}
    </Button>
  );
}

export function PackAdminPanel({
  onViewObligations,
}: {
  /** Switches the console to the Obligations section, scoped to one pack —
   *  threaded down from `App.tsx`'s own `setSection`/`?pack=` state rather
   *  than owned here, since this panel has no reason to know how the
   *  console navigates between sections, only that it can ask it to. */
  onViewObligations: (packId: string) => void;
}) {
  const [packs, setPacks] = useState<readonly InstalledPack[] | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api
      .namespaces()
      .then((rows) => {
        if (live) setPacks(installedPacks(rows));
      })
      .catch(() => {
        if (live) setFailure(COPY.loadFailed);
      });
    return () => {
      live = false;
    };
  }, []);

  if (failure) return <Alert type="error" showIcon message={failure} />;
  if (packs === null) return <Text type="secondary">{COPY.loading}</Text>;

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <div>
        <Text strong>{COPY.title}</Text>
        <div>
          <Text type="secondary">{COPY.subtitle}</Text>
        </div>
      </div>

      {packs.length === 0 ? (
        <Empty
          description={
            <Space direction="vertical">
              <Text strong>{COPY.emptyTitle}</Text>
              <Text type="secondary">{COPY.emptyBody}</Text>
            </Space>
          }
        />
      ) : (
        <Card size="small">
          <Table
            size="small"
            rowKey="packId"
            dataSource={packs}
            pagination={false}
            columns={[
              {
                title: "Pack",
                key: "pack",
                render: (_: unknown, pack: InstalledPack) => (
                  <Space>
                    <Text strong>{pack.label}</Text>
                    <Tag color="blue">{pack.packId}</Tag>
                  </Space>
                ),
              },
              {
                title: "Namespace",
                key: "namespace",
                render: (_: unknown, pack: InstalledPack) => (
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {`${pack.namespaceCode} · ${pack.iri}`}
                  </Text>
                ),
              },
              {
                title: "",
                key: "actions",
                width: 260,
                render: (_: unknown, pack: InstalledPack) => (
                  <Space>
                    <Button size="small" onClick={() => onViewObligations(pack.packId)}>
                      {COPY.viewObligations}
                    </Button>
                    <ReconcileAction packId={pack.packId} />
                  </Space>
                ),
              },
            ]}
          />
        </Card>
      )}
    </Space>
  );
}
