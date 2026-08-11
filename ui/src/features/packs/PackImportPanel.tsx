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
import { Alert, Button, Card, Empty, Space, Tag, Typography, Upload, message } from "antd";
import { InboxOutlined, SyncOutlined } from "@ant-design/icons";
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
  reconcile: "Run reconciliation",
  reconciling: "Reconciling…",
  reconcileFailed: "Reconciliation could not be triggered",
};

/** The first `gst:period "..."`-shaped literal in the converted Turtle, used
 *  to scope the import source. Reading it back out of the Turtle rather than
 *  changing every surface's `convert` signature keeps `PackImportSurface`
 *  generic — a surface with no period concept simply produces none, and the
 *  import falls back to the pack-wide source name. */
export function invoicePeriod(turtle: string): string | null {
  const match = turtle.match(/:period\s+"(\d{4}-\d{2})"/);
  return match?.[1] ?? null;
}

function ImportSurface({ packId, surface }: { packId: string; surface: PackImportSurface }) {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ landed: number; skipped: number; rejected: number } | null>(null);
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
          setResult({ landed: 0, skipped: 0, rejected: 0 });
          return;
        }
        // Scoped by period rather than the pack's own `${packId}-${surface.key}`
        // source name — that name is also what the pack's *bundled demo
        // fixture* imports into, so a real upload whose invoice numbers
        // happened to coincide with the fixture's would silently skip as
        // "already imported". Scoping by period both avoids that collision
        // and gives a natural idempotence key: re-uploading the same period
        // is a no-op, uploading a different one lands separately.
        const period = invoicePeriod(turtle);
        const outcome = await api.importRdf(
          period ? `${packId}-${surface.key}-${period}` : `${packId}-${surface.key}`,
          turtle,
        );
        setResult({ landed: outcome.landed.length, skipped: outcome.skipped.length, rejected: outcome.rejected.length });
        message.success(`${outcome.landed.length} facts imported.`);
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
            result.landed === 0 && result.skipped === 0
              ? "That file contained no invoices — a period nobody filed against is a valid answer."
              : `${result.landed} facts landed` +
                (result.skipped ? `, ${result.skipped} already present for this period` : "") +
                (result.rejected ? `, ${result.rejected} rejected` : "") +
                "."
          }
        />
      )}
      {failure && <Alert style={{ marginTop: 12 }} type="error" showIcon message={failure} />}
    </Card>
  );
}

/** The last step of the pipeline: uploading and registering land facts and
 *  rules, but nothing evaluates them until this is clicked. `Catalog::
 *  reconcile_pack` (Epic 105 P5b) does the work; this button is the whole
 *  of what reaches it — no polling, no separate "did it finish" check,
 *  because the request itself only returns once the run is done. */
function ReconcileButton({ packId }: { packId: string }) {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{
    evaluated: number;
    found: number;
    opened: number;
    alreadyOpen: number;
  } | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  const run = useCallback(async () => {
    setBusy(true);
    setResult(null);
    setFailure(null);
    try {
      const outcome = await api.reconcilePack(packId);
      setResult(outcome);
      message.success(
        outcome.found === 0
          ? "Reconciliation ran — no rule matched."
          : `${outcome.found} finding(s) — see Review.`,
      );
    } catch (error) {
      setFailure(error instanceof Error ? error.message : COPY.reconcileFailed);
    } finally {
      setBusy(false);
    }
  }, [packId]);

  return (
    <Space direction="vertical" size="small" style={{ width: "100%" }}>
      <Button icon={<SyncOutlined spin={busy} />} loading={busy} onClick={() => void run()}>
        {busy ? COPY.reconciling : COPY.reconcile}
      </Button>
      {result && (
        <Alert
          type={result.found === 0 ? "info" : "success"}
          showIcon
          message={
            `${result.evaluated} rule(s) evaluated, ${result.found} finding(s)` +
            (result.opened ? `, ${result.opened} newly opened` : "") +
            (result.alreadyOpen ? `, ${result.alreadyOpen} already open` : "") +
            "."
          }
        />
      )}
      {failure && <Alert type="error" showIcon message={failure} />}
    </Space>
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
            <Card size="small" title="Reconciliation" style={{ marginBottom: 12 }}>
              <Paragraph type="secondary" style={{ marginBottom: 12 }}>
                Evaluates every rule this pack has registered against what has
                landed so far. Uploading a file does not do this by itself —
                run it after an upload to see findings in Review.
              </Paragraph>
              <ReconcileButton packId={pack.packId} />
            </Card>
          </div>
        ))
      )}
    </Space>
  );
}
