/** Browsing, importing into, and installing domain packs — Epic 105 F1,
 *  consolidated per direct product decision: packs used to be split across
 *  this tab (browse/switch/reconcile) and the Connectors page's own
 *  `PackImportPanel` (upload data into a pack) — two places to look for one
 *  concept. Both now live here.
 *
 *  **The old split existed for a real reason, kept, just solved
 *  differently.** An admin confirming a pack loaded correctly should not
 *  have to scroll past every upload surface each installed pack exposes —
 *  so upload + "what's inside" content sits behind `Table`'s own
 *  `expandable` row, collapsed by default. Browsing several installed
 *  packs still costs one glance at the table.
 *
 *  **"What's inside this pack" is assembled from data already real** — the
 *  finding rules a pack registered (`GET /packs/{pack}/finding-rules`,
 *  already admin-gated, already existed for the reconcile engine itself)
 *  and the glossary it imported (`GET /ontology-packs`, Epic 33) — not a
 *  second copy of `pack.toml` re-typed into this file. Each section says
 *  where in the console the real thing actually shows up, since "what did
 *  installing this pack even do" was the concrete question this answers. */

import { useCallback, useEffect, useState } from "react";
import { Alert, Button, Card, Empty, List, Space, Table, Tag, Typography, Upload, message } from "antd";
import { InboxOutlined, SyncOutlined } from "@ant-design/icons";
import type { UploadFile } from "antd/es/upload/interface";
import { api, type FindingRuleDef, type OntologyPack } from "../../api";
import {
  installedPacks,
  surfacesFor,
  type InstalledPack,
  type PackImportSurface,
} from "./packSurfaces";
import { AvailablePacks } from "./AvailablePacks";

const { Text, Paragraph } = Typography;

const COPY = {
  title: "Packs",
  subtitle:
    "Every domain pack this deployment reports as installed, discovered from the namespaces each one registered — not a build-time list.",
  emptyTitle: "No domain pack installed",
  emptyBody:
    "Install one below. A deployment with no pack has nothing to browse or switch between, which is why this is empty rather than hidden.",
  loadFailed: "Could not read which packs are installed",
  loading: "Loading…",
  viewObligations: "View obligations",
  reconcile: "Reconcile now",
  reconciling: "Reconciling…",
  reconcileFailed: "Reconciliation could not be triggered",
  contentsTitle: "What's inside this pack",
  contentsLoadFailed: "Could not read this pack's contents",
  findingsHeading: "Findings it can detect",
  findingsEmpty: "This pack registers no finding rules.",
  findingsWhere: "Run reconciliation below, then review matches in the Review tab.",
  glossaryHeading: "Glossary",
  glossaryWhere: "Browse in Vocabulary → Glossary / Ontology packs.",
  glossaryEmpty: "This pack registered no glossary.",
  obligationsHeading: "Obligations",
  obligationsWhere: "See the calendar in the Obligations tab.",
  importHeading: "Import data",
  importEmpty: "This pack declares no upload surface — its data lands only via its own bundled fixtures at install time.",
  whereToGetIt: "Where to get it: ",
  choose: "Choose a file",
  importing: "Importing…",
  unreadable: "That file could not be read",
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
        <Text strong>{COPY.whereToGetIt}</Text>
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

/** "What's inside this pack" — findings it can detect, the glossary it
 *  imported, and a pointer to obligations, each read from the real,
 *  already-registered data rather than re-describing the pack's manifest. */
function PackContents({ packId }: { packId: string }) {
  const [findings, setFindings] = useState<FindingRuleDef[] | null>(null);
  const [ontologyPack, setOntologyPack] = useState<OntologyPack | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    Promise.all([api.findingRules(packId), api.ontologyPacks()])
      .then(([rules, packs]) => {
        if (!live) return;
        setFindings(rules);
        setOntologyPack(packs.find((p) => p.packId === packId) ?? null);
      })
      .catch(() => {
        if (live) setFailure(COPY.contentsLoadFailed);
      });
    return () => {
      live = false;
    };
  }, [packId]);

  if (failure) return <Alert type="error" showIcon message={failure} />;
  if (findings === null) return <Text type="secondary">{COPY.loading}</Text>;

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Text strong>{COPY.contentsTitle}</Text>

      <div>
        <Text strong style={{ fontSize: 13 }}>
          {COPY.findingsHeading}
        </Text>
        {findings.length === 0 ? (
          <Paragraph type="secondary" style={{ marginBottom: 4 }}>
            {COPY.findingsEmpty}
          </Paragraph>
        ) : (
          <List
            size="small"
            dataSource={findings}
            renderItem={(rule) => (
              <List.Item style={{ border: "none", padding: "4px 0" }}>
                <Space direction="vertical" size={0}>
                  <Text strong style={{ fontSize: 13 }}>
                    {rule.label}
                  </Text>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {rule.summary}
                  </Text>
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    {rule.governedBy}
                  </Text>
                </Space>
              </List.Item>
            )}
          />
        )}
        <Text type="secondary" style={{ fontSize: 12 }}>{`→ ${COPY.findingsWhere}`}</Text>
      </div>

      <div>
        <Text strong style={{ fontSize: 13 }}>
          {COPY.glossaryHeading}
        </Text>
        {ontologyPack ? (
          <Paragraph style={{ marginBottom: 4 }}>
            {`${ontologyPack.termCount} term(s), version ${ontologyPack.version}.`}
          </Paragraph>
        ) : (
          <Paragraph type="secondary" style={{ marginBottom: 4 }}>
            {COPY.glossaryEmpty}
          </Paragraph>
        )}
        <Text type="secondary" style={{ fontSize: 12 }}>{`→ ${COPY.glossaryWhere}`}</Text>
      </div>

      <div>
        <Text strong style={{ fontSize: 13 }}>
          {COPY.obligationsHeading}
        </Text>
        <div>
          <Text type="secondary" style={{ fontSize: 12 }}>{`→ ${COPY.obligationsWhere}`}</Text>
        </div>
      </div>
    </Space>
  );
}

function PackDetailRow({ packId }: { packId: string }) {
  const pack = surfacesFor([`pack:${packId}`])[0];

  return (
    <Space direction="vertical" size="large" style={{ width: "100%", padding: "8px 0" }}>
      <PackContents packId={packId} />
      <div>
        <Text strong>{COPY.importHeading}</Text>
        {!pack || pack.imports.length === 0 ? (
          <Paragraph type="secondary">{COPY.importEmpty}</Paragraph>
        ) : (
          pack.imports.map((surface) => (
            <ImportSurface key={surface.key} packId={packId} surface={surface} />
          ))
        )}
      </div>
    </Space>
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

  const refresh = useCallback(() => {
    return api.namespaces().then((rows) => setPacks(installedPacks(rows)));
  }, []);

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
            expandable={{
              expandedRowRender: (pack: InstalledPack) => <PackDetailRow packId={pack.packId} />,
            }}
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

      <AvailablePacks onInstalled={() => void refresh()} />
    </Space>
  );
}
