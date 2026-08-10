/** Importing a domain pack's own documents — Epic 105.
 *
 *  **Nothing in this file knows what GST is.** It asks the deployment which
 *  packs are installed, looks up whatever surfaces those packs declare, and
 *  renders a file picker per surface. Install a different pack and the same
 *  component renders that pack's imports; install none and it says so.
 *
 *  **Conversion happens in the browser, and that is a deliberate trade.** The
 *  server's import route takes RDF, not a provider's JSON, and giving it a
 *  domain-specific normalizer would put GST inside the server — the one thing
 *  `plans/105-domain-neutrality.md` refuses. So the pack's surface owns the
 *  conversion and the server keeps taking RDF from everyone. */

import { useCallback, useEffect, useState } from "react";
import { Alert, Card, Empty, Space, Tag, Typography, Upload, message } from "antd";
import { InboxOutlined } from "@ant-design/icons";
import type { UploadFile } from "antd/es/upload/interface";
import { api } from "../../api";
import { surfacesFor, type PackImportSurface, type PackSurfaces } from "./packSurfaces";

const { Text, Paragraph } = Typography;

const COPY = {
  title: "Pack imports",
  subtitle:
    "Documents the installed domain packs know how to read. Each one lands in its own named graph and can be re-imported without duplicating.",
  emptyTitle: "No domain pack installed",
  emptyBody:
    "Install a pack with graph-owl-load-pack and its own imports appear here. A deployment with no pack has nothing to import, which is why this is empty rather than hidden.",
  loadFailed: "Could not read which packs are installed",
  choose: "Choose a file",
  importing: "Importing…",
  unreadable: "That file could not be read",
};

function ImportSurface({ packId, surface }: { packId: string; surface: PackImportSurface }) {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ landed: number; rejected: number } | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  const handle = useCallback(
    async (file: UploadFile & { originFileObj?: File }) => {
      const blob = (file.originFileObj ?? file) as unknown as File;
      setBusy(true);
      setResult(null);
      setFailure(null);
      try {
        const text = await blob.text();
        // Convert first. A file the pack cannot read must never reach the
        // graph — a partial import is harder to undo than a refused one.
        const { turtle, count } = surface.convert(text);
        if (count === 0) {
          // Not an error: a period nobody filed against is a legitimate and
          // informative answer. Saying so beats a silent success.
          setResult({ landed: 0, rejected: 0 });
          return;
        }
        const landed = await api.importRdf(`${packId}-${surface.key}`, turtle);
        setResult({ landed: landed.landed, rejected: landed.rejected.length });
        message.success(`${landed.landed} facts imported.`);
      } catch (error) {
        // The message is the pack's own, written for whoever is uploading —
        // "not a GSTR-2B download", not "unexpected token < in JSON".
        setFailure(error instanceof Error ? error.message : COPY.unreadable);
      } finally {
        setBusy(false);
      }
    },
    [packId, surface],
  );

  return (
    <Card size="small" title={surface.label} style={{ marginBottom: 12 }}>
      <Paragraph type="secondary" style={{ marginBottom: 8 }}>
        {surface.description}
      </Paragraph>
      <Paragraph style={{ marginBottom: 12, fontSize: 13 }}>
        <Text strong>Where to get it: </Text>
        {surface.howToObtain}
      </Paragraph>

      <Upload.Dragger
        accept={surface.accept}
        maxCount={1}
        showUploadList={false}
        disabled={busy}
        beforeUpload={(file) => {
          void handle(file as unknown as UploadFile);
          // Returning false keeps antd from attempting its own upload — this
          // component posts the converted RDF itself.
          return false;
        }}
      >
        <p className="ant-upload-drag-icon">
          <InboxOutlined />
        </p>
        <p className="ant-upload-text">{busy ? COPY.importing : COPY.choose}</p>
      </Upload.Dragger>

      {result && (
        <Alert
          style={{ marginTop: 12 }}
          type={result.landed === 0 ? "info" : "success"}
          showIcon
          message={
            result.landed === 0
              ? "That file contained no invoices — a period nobody filed against is a valid answer."
              : `${result.landed} facts landed${result.rejected ? `, ${result.rejected} rejected` : ""}.`
          }
        />
      )}
      {failure && <Alert style={{ marginTop: 12 }} type="error" showIcon message={failure} />}
    </Card>
  );
}

export function PackImportPanel() {
  const [packs, setPacks] = useState<readonly PackSurfaces[] | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api
      .namespaces()
      .then((rows) => {
        if (live) setPacks(surfacesFor(rows.map((r) => r.declaredBy)));
      })
      .catch(() => {
        if (live) setFailure(COPY.loadFailed);
      });
    return () => {
      live = false;
    };
  }, []);

  if (failure) return <Alert type="error" showIcon message={failure} />;
  if (packs === null) return <Text type="secondary">Loading…</Text>;

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <div>
        <Text strong>{COPY.title}</Text>
        <div>
          <Text type="secondary">{COPY.subtitle}</Text>
        </div>
      </div>

      {packs.length === 0 ? (
        <Empty description={<Space direction="vertical"><Text strong>{COPY.emptyTitle}</Text><Text type="secondary">{COPY.emptyBody}</Text></Space>} />
      ) : (
        packs.map((pack) => (
          <div key={pack.packId}>
            <Space style={{ marginBottom: 8 }}>
              <Text strong>{pack.label}</Text>
              <Tag color="blue">{pack.packId}</Tag>
            </Space>
            {pack.imports.map((surface) => (
              <ImportSurface key={surface.key} packId={pack.packId} surface={surface} />
            ))}
          </div>
        ))
      )}
    </Space>
  );
}
