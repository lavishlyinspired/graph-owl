/** Packs on disk this deployment has not installed yet — Epic 105 F1, the
 *  one step left in `Admin > Packs` that still needed a terminal after
 *  browsing and importing moved into this same panel.
 *
 *  **Does not know which packs are already installed.** `GET /packs/
 *  available` already excludes them server-side (`pack_install::
 *  scan_available_packs`, cross-referenced against the same `declaredBy:
 *  "pack:<id>"` marker `PackAdminPanel` uses) — this component only has to
 *  render whatever it is handed. */

import { useCallback, useEffect, useState } from "react";
import { Alert, Button, Card, List, Space, Typography, message } from "./../../components/ui/antd-compat";
import { DownloadOutlined } from "@ant-design/icons";
import { api } from "../../api";

const { Text, Paragraph } = Typography;

const COPY = {
  title: "Available to install",
  subtitle: "Found on disk, not yet installed.",
  loadFailed: "Could not read which packs are available to install.",
  install: "Install",
  installing: "Installing…",
  installFailed: "The pack loader could not be started",
  loaderFailedTitle: "The loader ran and reported a failure",
};

function InstallAction({
  packId,
  onInstalled,
}: {
  packId: string;
  onInstalled: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [output, setOutput] = useState<string | null>(null);
  const [loaderOk, setLoaderOk] = useState<boolean | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  const run = useCallback(async () => {
    setBusy(true);
    setOutput(null);
    setLoaderOk(null);
    setFailure(null);
    try {
      const result = await api.installPack(packId);
      setLoaderOk(result.ok);
      setOutput(result.output);
      if (result.ok) {
        message.success(`${packId} installed.`);
        onInstalled();
      }
    } catch (error) {
      setFailure(error instanceof Error ? error.message : COPY.installFailed);
    } finally {
      setBusy(false);
    }
  }, [packId, onInstalled]);

  return (
    <Space direction="vertical" size={4} align="end" style={{ width: "100%" }}>
      <Button size="small" icon={<DownloadOutlined />} loading={busy} onClick={() => void run()}>
        {busy ? COPY.installing : COPY.install}
      </Button>
      {failure && <Alert style={{ width: "100%" }} type="error" showIcon message={failure} />}
      {loaderOk === false && output && (
        <Alert
          style={{ width: "100%" }}
          type="error"
          showIcon
          message={COPY.loaderFailedTitle}
          description={
            <pre style={{ whiteSpace: "pre-wrap", fontSize: 11, margin: 0, maxHeight: 200, overflow: "auto" }}>
              {output}
            </pre>
          }
        />
      )}
    </Space>
  );
}

export function AvailablePacks({ onInstalled }: { onInstalled: () => void }) {
  const [packs, setPacks] = useState<{ id: string; description: string }[] | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  const refresh = useCallback(() => api.availablePacks().then(setPacks), []);

  useEffect(() => {
    let live = true;
    refresh().catch(() => {
      if (live) setFailure(COPY.loadFailed);
    });
    return () => {
      live = false;
    };
  }, [refresh]);

  if (failure) return <Alert type="error" showIcon message={failure} />;
  // Nothing to install is not an error, and not worth a card of its own —
  // `PackAdminPanel`'s own empty state already covers "no packs anywhere".
  if (packs === null || packs.length === 0) return null;

  return (
    <Card size="small" title={COPY.title}>
      <Paragraph type="secondary" style={{ marginBottom: 12 }}>
        {COPY.subtitle}
      </Paragraph>
      <List
        size="small"
        dataSource={packs}
        renderItem={(pack) => (
          <List.Item style={{ alignItems: "flex-start", gap: 16 }}>
            <Space direction="vertical" size={0} style={{ flex: 1 }}>
              <Text strong>{pack.id}</Text>
              <Text type="secondary" style={{ fontSize: 12 }}>
                {pack.description}
              </Text>
            </Space>
            <div style={{ width: 200 }}>
              <InstallAction
                packId={pack.id}
                onInstalled={() => {
                  onInstalled();
                  void refresh();
                }}
              />
            </div>
          </List.Item>
        )}
      />
    </Card>
  );
}
