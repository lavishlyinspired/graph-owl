/** The reconciliation, as one page a CA works down — Plan 108 Slice 7.
 *
 *  **Why this exists when the Review queue already lists findings.** The queue
 *  is a reviewer's tool: one finding at a time, with its rule and its evidence
 *  chain, which is exactly right for deciding whether *one* accusation stands.
 *  It is not how a reconciliation is done. A CA closing a period needs the
 *  three totals side by side, the difference between them, and what accounts
 *  for that difference — and until now that meant uploading through an admin
 *  screen, clicking a button in a table row, and reading a flat list in
 *  another tab, with the arithmetic done by hand afterwards.
 *
 *  So this page is the workflow, in the order it is actually performed: load
 *  the three sources, run the rules, read the statement, work the exceptions,
 *  take the working paper away. Nothing here duplicates the queue's job —
 *  every finding still opens into the queue's evidence chain, which is where
 *  the argument for a finding lives.
 *
 *  **The console still has no GST tab.** This page renders for whichever pack
 *  declares import surfaces and finding rules; `statement.ts` holds the one
 *  per-pack table, following the pattern `packSurfaces.ts` already
 *  established and states. Install the hospitality pack instead and this page
 *  renders that pack's sources and that pack's findings.
 *
 *  **The residual is the point.** A statement that always balances means
 *  nothing; this one says how much of the difference between books and
 *  GSTR-2B the rules could not account for, which is the number that tells a
 *  CA there is something still to find. */

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Col,
  Empty,
  Row,
  Space,
  Spin,
  Statistic,
  Table,
  Tag,
  Typography,
  Upload,
  message,
} from "antd";
import {
  CheckCircleOutlined,
  DownloadOutlined,
  InboxOutlined,
  SyncOutlined,
  WarningOutlined,
} from "@ant-design/icons";
import type { UploadFile } from "antd/es/upload/interface";
import { api, type PackFinding } from "../../api";
import { importThroughSurface } from "../packs/importFile";
import { surfacesFor, type PackImportSurface } from "../packs/packSurfaces";
import {
  buildStatement,
  evidenceOf,
  first,
  scenarioFor,
  sourceSummary,
  statementCsv,
  values,
  type ReconcilingItem,
  type SourceInvoice,
  type Statement,
} from "./statement";

const { Text, Title, Paragraph } = Typography;

const COPY = {
  title: "Reconciliation",
  subtitle:
    "Your books against what your suppliers declared and what the authority made available — with the reason for every difference.",
  noPack: "No domain pack is installed",
  noPackBody:
    "This page reconciles whatever sources an installed pack declares. Install one from Admin → Packs and it will appear here.",
  step1: "1 · Load your data",
  step2: "2 · Run the rules",
  step3: "3 · The statement",
  step4: "4 · What to do about it",
  loading: "Reading the graph…",
  loadFailed: "Could not read the reconciliation",
  run: "Run reconciliation",
  running: "Running…",
  runAgain: "Run again",
  runHint: "Re-runs are safe: findings are computed from the graph each time, never stored as flags on an invoice.",
  runFailed: "Reconciliation could not be run",
  exportCsv: "Download working paper (CSV)",
  nothingToExport: "Nothing to export yet",
  emptyFindings: "Nothing to look at",
  emptyFindingsBody:
    "No rule produced a finding. With every source loaded that means the period reconciles — silence is the signal here, and nothing is written to say so.",
  unexplainedTitle: "Not accounted for",
  reconciledTitle: "Fully accounted for",
  reconciledBody: "Every rupee of the difference between your books and GSTR-2B is explained by a finding below.",
  unexplainedBody:
    "The rules account for part of the difference and not all of it. What is left is a real gap — most often a source that has not been loaded for this period, or an invoice neither side records the same way.",
  nextAction: "What to do",
  governedBy: "Rule",
  openInReview: "Review the evidence for these",
  sourceLoaded: "loaded",
  sourceEmpty: "not loaded",
  periodsLabel: "Periods",
  truncated:
    "The graph returned more rows than one read allows, so these totals are of what was read, not of everything held. Narrow the period before relying on them.",
  noSurface: "This pack declares no upload surface for this source.",
  whereToGetIt: "Where do I get this?",
  booksTotal: "As per your books",
  authorityTotal: "As per GSTR-2B",
  differenceTotal: "Difference",
  differenceHint: "books \u2212 GSTR-2B",
  explainedTotal: "Explained by findings",
  explainedHint: "sum of the items below",
  invoicesSuffix: "invoices",
};

/** ₹ the way an Indian statement writes it — lakhs and crores, not thousands.
 *  A figure grouped `1,000,000` in a GST working paper reads as wrong before
 *  it reads as foreign. */
const RUPEES = new Intl.NumberFormat("en-IN", {
  style: "currency",
  currency: "INR",
  maximumFractionDigits: 2,
  minimumFractionDigits: 2,
});

function rupees(value: number): string {
  return RUPEES.format(value);
}

/** Every invoice in one class, with the fields the statement totals.
 *
 *  **`supplierName` is the only OPTIONAL, and deliberately so.** A required
 *  pattern silently drops every invoice missing that field — a register whose
 *  export had no supplier-name column would simply not appear in its own
 *  totals, and the page would look like it worked. The six that are required
 *  are the six both importers always write.
 *
 *  Every pattern sits inside its own `GRAPH ?g { }`: an import lands in
 *  `graph:import:{source}`, never the default graph, so a pattern outside one
 *  matches nothing at all — silently, which reads exactly like "you have not
 *  uploaded anything yet". */
function sourceQuery(className: string): string {
  return `PREFIX gst: <https://graph-owl.dev/packs/gst#>
SELECT ?invoiceNumber ?gstin ?supplierName ?invoiceDate ?taxableValue ?taxAmount ?period
WHERE {
  GRAPH ?g {
    ?invoice a gst:${className} ;
             gst:issuedBy      ?supplier ;
             gst:invoiceNumber ?invoiceNumber ;
             gst:invoiceDate   ?invoiceDate ;
             gst:taxableValue  ?taxableValue ;
             gst:taxAmount     ?taxAmount ;
             gst:period        ?period .
    ?supplier gst:supplierGstin ?gstin .
  }
  OPTIONAL {
    GRAPH ?names { ?supplier gst:supplierName ?supplierName }
  }
}`;
}

interface SourceSpec {
  readonly key: string;
  readonly className: string;
  readonly label: string;
  readonly role: string;
  /** Which import surface fills it, when the pack declares one. */
  readonly surfaceKey: string;
}

/** The three sources of a GST reconciliation, in the order it is done.
 *
 *  Lives here rather than in `pack.toml` for the same reason `SCENARIOS` does
 *  — see `statement.ts`. When a second domain needs a workspace, this is the
 *  table that moves to the manifest. */
const SOURCES: readonly SourceSpec[] = [
  {
    key: "books",
    className: "PurchaseInvoice",
    label: "Your books",
    role: "What you have recorded",
    surfaceKey: "books",
  },
  {
    key: "gstr1",
    className: "Gstr1Invoice",
    label: "GSTR-2A / GSTR-1",
    role: "What your suppliers declared",
    surfaceKey: "gstr1",
  },
  {
    key: "authority",
    className: "Gstr2bInvoice",
    label: "GSTR-2B",
    role: "What credit the authority made available",
    surfaceKey: "gstr2b",
  },
];

/** One SPARQL solution as a source row. Missing bindings become empty strings
 *  and `"0.00"`, so a total is never `NaN` — an unreadable figure in a tax
 *  statement is worse than a visibly absent one. */
function toSourceInvoice(row: Readonly<Record<string, string>>): SourceInvoice {
  return {
    invoiceNumber: row.invoiceNumber ?? "",
    gstin: row.gstin ?? "",
    supplierName: row.supplierName ?? "",
    invoiceDate: row.invoiceDate ?? "",
    taxableValue: row.taxableValue ?? "0.00",
    taxAmount: row.taxAmount ?? "0.00",
    period: row.period ?? "",
  };
}

function SourceCard({
  spec,
  rows,
  surface,
  onImported,
}: {
  spec: SourceSpec;
  rows: readonly SourceInvoice[];
  surface: PackImportSurface | undefined;
  onImported: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const summary = useMemo(() => sourceSummary(rows), [rows]);

  const handle = useCallback(
    async (file: UploadFile & { originFileObj?: File }) => {
      if (!surface) return;
      const blob = (file.originFileObj ?? file) as unknown as File;
      setBusy(true);
      setFailure(null);
      try {
        const outcome = await importThroughSurface("gst", surface, await blob.text());
        message.success(
          outcome.count === 0
            ? "That file held no invoices — a period nobody filed against is a valid answer."
            : `${outcome.count} invoice(s) read, ${outcome.landed} facts added.`,
        );
        onImported();
      } catch (error) {
        // The pack's own message, written for whoever is uploading — "no GSTIN
        // column found in that file", not "unexpected token < in JSON".
        setFailure(error instanceof Error ? error.message : "That file could not be read");
      } finally {
        setBusy(false);
      }
    },
    [surface, onImported],
  );

  const loaded = summary.count > 0;

  return (
    <Card
      size="small"
      style={{ height: "100%" }}
      title={
        <Space>
          <Text strong>{spec.label}</Text>
          <Tag color={loaded ? "green" : "default"}>{loaded ? COPY.sourceLoaded : COPY.sourceEmpty}</Tag>
        </Space>
      }
    >
      <Paragraph type="secondary" style={{ marginBottom: 12, fontSize: 12 }}>
        {spec.role}
      </Paragraph>

      {loaded && (
        <Space direction="vertical" size={2} style={{ marginBottom: 12, width: "100%" }}>
          <Text style={{ fontSize: 22, fontWeight: 600 }}>{rupees(summary.taxAmount)}</Text>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {`${summary.count} invoice${summary.count === 1 ? "" : "s"} · taxable ${rupees(summary.taxableValue)}`}
          </Text>
          {summary.periods.length > 0 && (
            <Text type="secondary" style={{ fontSize: 12 }}>
              {`${COPY.periodsLabel}: ${summary.periods.join(", ")}`}
            </Text>
          )}
        </Space>
      )}

      {surface ? (
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
          style={{ padding: "4px 0" }}
        >
          <p className="ant-upload-drag-icon" style={{ marginBottom: 4 }}>
            {busy ? <Spin /> : <InboxOutlined />}
          </p>
          <p className="ant-upload-text" style={{ fontSize: 13 }}>
            {busy ? "Reading…" : loaded ? "Replace or add a period" : "Upload"}
          </p>
        </Upload.Dragger>
      ) : (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {COPY.noSurface}
        </Text>
      )}

      <details style={{ marginTop: 10 }}>
        <summary style={{ cursor: "pointer", fontSize: 12, color: "var(--ant-color-text-secondary)" }}>
          {COPY.whereToGetIt}
        </summary>
        <Paragraph type="secondary" style={{ fontSize: 12, marginTop: 6, marginBottom: 0 }}>
          {surface?.howToObtain ?? "—"}
        </Paragraph>
      </details>

      {failure && <Alert style={{ marginTop: 10 }} type="error" showIcon message={failure} />}
    </Card>
  );
}

/** The statement's own arithmetic, laid out the way a practitioner's format
 *  lays it out: an opening balance, the reconciling items, a closing balance,
 *  and — the line a spreadsheet cannot produce — whatever is left over. */
function StatementPanel({ statement }: { statement: Statement }) {
  return (
    <Card size="small">
      <Row gutter={[24, 16]}>
        <Col xs={24} md={6}>
          <Statistic
            title={COPY.booksTotal}
            value={rupees(statement.books.taxAmount)}
            valueStyle={{ fontSize: 20 }}
          />
          <Text type="secondary" style={{ fontSize: 12 }}>{`${statement.books.count} ${COPY.invoicesSuffix}`}</Text>
        </Col>
        <Col xs={24} md={6}>
          <Statistic
            title={COPY.authorityTotal}
            value={rupees(statement.authority.taxAmount)}
            valueStyle={{ fontSize: 20 }}
          />
          <Text type="secondary" style={{ fontSize: 12 }}>{`${statement.authority.count} ${COPY.invoicesSuffix}`}</Text>
        </Col>
        <Col xs={24} md={6}>
          <Statistic
            title={COPY.differenceTotal}
            value={rupees(statement.difference.taxAmount)}
            valueStyle={{ fontSize: 20 }}
          />
          <Text type="secondary" style={{ fontSize: 12 }}>{COPY.differenceHint}</Text>
        </Col>
        <Col xs={24} md={6}>
          <Statistic
            title={COPY.explainedTotal}
            value={rupees(statement.explained.taxAmount)}
            valueStyle={{ fontSize: 20 }}
          />
          <Text type="secondary" style={{ fontSize: 12 }}>{COPY.explainedHint}</Text>
        </Col>
      </Row>

      <Alert
        style={{ marginTop: 16 }}
        type={statement.reconciled ? "success" : "warning"}
        showIcon
        icon={statement.reconciled ? <CheckCircleOutlined /> : <WarningOutlined />}
        message={
          statement.reconciled
            ? COPY.reconciledTitle
            : `${COPY.unexplainedTitle}: ${rupees(statement.unexplained.taxAmount)}`
        }
        description={statement.reconciled ? COPY.reconciledBody : COPY.unexplainedBody}
      />
    </Card>
  );
}

const INVOICE_COLUMNS = [
  { title: "Invoice", dataIndex: "invoiceNumber", key: "invoiceNumber", width: 130 },
  { title: "Supplier", dataIndex: "supplierName", key: "supplierName", render: (v: string) => v || "—" },
  { title: "GSTIN", dataIndex: "gstin", key: "gstin", width: 170 },
  { title: "Date", dataIndex: "invoiceDate", key: "invoiceDate", width: 110 },
  { title: "Period", dataIndex: "period", key: "period", width: 90 },
  {
    title: "Taxable",
    dataIndex: "taxableValue",
    key: "taxableValue",
    align: "right" as const,
    render: (v: string) => rupees(Number(v || 0)),
  },
  {
    title: "Tax",
    dataIndex: "taxAmount",
    key: "taxAmount",
    align: "right" as const,
    render: (v: string) => rupees(Number(v || 0)),
  },
];

/** One kind of finding: what it means, what to do, and the invoices behind it.
 *
 *  **The extra evidence is rendered for the rules whose whole point is a pair
 *  of numbers.** A mismatch shown as one figure is unreviewable, and a
 *  late-filing finding without the filing date is just "unmatched" again. */
function FindingGroup({ item, findings }: { item: ReconcilingItem; findings: readonly PackFinding[] }) {
  const scenario = scenarioFor(item.label);
  const mine = findings.filter((f) => f.label === item.label && f.status !== "rejected");
  const detail = mine
    .map((finding) => {
      const evidence = evidenceOf(finding);
      const number = first(evidence, "invoiceNumber");
      const parts: string[] = [];
      const filed = first(evidence, "filedDate");
      const received = first(evidence, "atTime");
      const citation = first(evidence, "citation");
      const declared = values(evidence, "taxableValue");
      if (filed) parts.push(`filed ${filed}`);
      if (received) parts.push(`received ${received}`);
      // Exactly two is the shape a mismatch rule projects — the register's
      // figure and the other side's. Rendering one of them alone would show a
      // "mismatch" against nothing.
      if (declared.length === 2) {
        parts.push(`books ${rupees(Number(declared[0]))} vs ${rupees(Number(declared[1]))}`);
      }
      if (citation) parts.push(citation);
      return parts.length > 0 ? `${number}: ${parts.join(" · ")}` : "";
    })
    .filter((line) => line !== "");

  return (
    <Card
      size="small"
      style={{ marginBottom: 12 }}
      title={
        <Space wrap>
          <Tag color={scenario.tone === "danger" ? "red" : scenario.tone === "warning" ? "orange" : "blue"}>
            {item.count}
          </Tag>
          <Text strong>{scenario.title}</Text>
          <Text type="secondary" style={{ fontSize: 12 }}>{rupees(item.taxAmount)}</Text>
        </Space>
      }
    >
      <Paragraph style={{ marginBottom: 8 }}>{scenario.meaning}</Paragraph>
      <Paragraph style={{ marginBottom: 12 }}>
        <Text strong>{`${COPY.nextAction}: `}</Text>
        {scenario.nextAction}
      </Paragraph>

      {detail.length > 0 && (
        <Space direction="vertical" size={0} style={{ marginBottom: 12 }}>
          {detail.map((line) => (
            <Text key={line} type="secondary" style={{ fontSize: 12 }}>
              {line}
            </Text>
          ))}
        </Space>
      )}

      <Table
        size="small"
        rowKey="invoiceNumber"
        dataSource={[...item.rows]}
        columns={INVOICE_COLUMNS}
        pagination={item.rows.length > 10 ? { pageSize: 10 } : false}
        scroll={{ x: "max-content" }}
      />

      <div style={{ marginTop: 8 }}>
        <Text type="secondary" style={{ fontSize: 12 }}>
          {`${COPY.governedBy}: `}
        </Text>
        <Tag>{mine[0]?.governedBy ?? "—"}</Tag>
      </div>
    </Card>
  );
}

export function ReconciliationWorkspace({ onReview }: { onReview: () => void }) {
  const [rows, setRows] = useState<Record<string, SourceInvoice[]> | null>(null);
  const [findings, setFindings] = useState<readonly PackFinding[]>([]);
  const [truncated, setTruncated] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [surfaces, setSurfaces] = useState<readonly PackImportSurface[]>([]);
  const [packInstalled, setPackInstalled] = useState<boolean | null>(null);

  const refresh = useCallback(async () => {
    setFailure(null);
    try {
      const namespaces = await api.namespaces();
      const pack = surfacesFor(namespaces.map((n) => n.declaredBy)).find((p) => p.packId === "gst");
      setPackInstalled(pack !== undefined);
      setSurfaces(pack?.imports ?? []);
      if (!pack) {
        setRows({});
        return;
      }

      const results = await Promise.all(SOURCES.map((source) => api.sparql(sourceQuery(source.className))));
      const next: Record<string, SourceInvoice[]> = {};
      SOURCES.forEach((source, index) => {
        next[source.key] = (results[index]?.rows ?? []).map(toSourceInvoice);
      });
      setRows(next);
      // **Surfaced, never inferred from the row count.** A truncated answer
      // that looks complete is the failure this project refuses everywhere,
      // and here it would be a tax figure quietly missing invoices.
      setTruncated(results.some((result) => result.truncated));
      setFindings(await api.findings({ pack: "gst" }));
    } catch (error) {
      setFailure(error instanceof Error ? error.message : COPY.loadFailed);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const run = useCallback(async () => {
    setRunning(true);
    try {
      const outcome = await api.reconcilePack("gst");
      message.success(
        outcome.found === 0 ? "Reconciliation ran — no rule matched." : `${outcome.found} finding(s).`,
      );
      await refresh();
    } catch (error) {
      message.error(error instanceof Error ? error.message : COPY.runFailed);
    } finally {
      setRunning(false);
    }
  }, [refresh]);

  const statement = useMemo(
    () =>
      rows
        ? buildStatement({ books: rows.books ?? [], authority: rows.authority ?? [], findings })
        : null,
    [rows, findings],
  );

  const download = useCallback(() => {
    if (!statement) return;
    const blob = new Blob([statementCsv(statement.items)], { type: "text/csv;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = `gst-reconciliation-${new Date().toISOString().slice(0, 10)}.csv`;
    link.click();
    URL.revokeObjectURL(url);
  }, [statement]);

  if (failure) return <Alert type="error" showIcon message={COPY.loadFailed} description={failure} />;
  if (rows === null || statement === null) return <Text type="secondary">{COPY.loading}</Text>;
  if (packInstalled === false) {
    return (
      <Empty
        description={
          <Space direction="vertical">
            <Text strong>{COPY.noPack}</Text>
            <Text type="secondary">{COPY.noPackBody}</Text>
          </Space>
        }
      />
    );
  }

  return (
    <Space direction="vertical" size="large" style={{ width: "100%" }}>
      <div>
        <Title level={4} style={{ marginBottom: 4 }}>
          {COPY.title}
        </Title>
        <Text type="secondary">{COPY.subtitle}</Text>
      </div>

      {truncated && <Alert type="warning" showIcon message={COPY.truncated} />}

      <div>
        <Title level={5}>{COPY.step1}</Title>
        <Row gutter={[16, 16]}>
          {SOURCES.map((source) => (
            <Col xs={24} md={8} key={source.key}>
              <SourceCard
                spec={source}
                rows={rows[source.key] ?? []}
                surface={surfaces.find((s) => s.key === source.surfaceKey)}
                onImported={() => void refresh()}
              />
            </Col>
          ))}
        </Row>
      </div>

      <div>
        <Title level={5}>{COPY.step2}</Title>
        <Space wrap>
          <Button type="primary" icon={<SyncOutlined spin={running} />} loading={running} onClick={() => void run()}>
            {running ? COPY.running : findings.length > 0 ? COPY.runAgain : COPY.run}
          </Button>
          <Button icon={<DownloadOutlined />} disabled={statement.items.length === 0} onClick={download}>
            {statement.items.length === 0 ? COPY.nothingToExport : COPY.exportCsv}
          </Button>
          <Button onClick={onReview}>{COPY.openInReview}</Button>
        </Space>
        <div style={{ marginTop: 6 }}>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {COPY.runHint}
          </Text>
        </div>
      </div>

      <div>
        <Title level={5}>{COPY.step3}</Title>
        <StatementPanel statement={statement} />
      </div>

      <div>
        <Title level={5}>{COPY.step4}</Title>
        {statement.items.length === 0 ? (
          <Empty
            description={
              <Space direction="vertical">
                <Text strong>{COPY.emptyFindings}</Text>
                <Text type="secondary">{COPY.emptyFindingsBody}</Text>
              </Space>
            }
          />
        ) : (
          statement.items.map((item) => (
            <FindingGroup key={item.label} item={item} findings={findings} />
          ))
        )}
      </div>
    </Space>
  );
}
