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
import { api, type PackConsoleConfig, type PackFinding } from "../../api";
import { lexical } from "../../workbench/results";
import { importThroughSurface } from "../packs/importFile";
import {
  surfacesFor,
  surfacesFromConsole,
  unreadableFormats,
  type PackImportSurface,
} from "../packs/packSurfaces";
import {
  buildStatement,
  distinctInvoices,
  evidenceOf,
  first,
  guidanceFrom,
  scenarioFor,
  sourceSummary,
  statementCsv,
  supplierView,
  type BoundInvoice,
  type GuidanceIndex,
  type Heads,
  type ReconcilingItem,
  type SourceInvoice,
  type SupplierRow,
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
  unreadable:
    "This pack declares a file format this console cannot read, so that upload is not offered. Either the manifest names the wrong format, or this console is older than the pack it is serving.",
  whereToGetIt: "Where do I get this?",
  booksTotal: "As per your books",
  authorityTotal: "As per GSTR-2B",
  differenceTotal: "Difference",
  differenceHint: "books \u2212 GSTR-2B",
  explainedTotal: "Explained by findings",
  explainedHint: "sum of the items below",
  invoicesSuffix: "invoices",
  headsTitle: "Head-wise",
  headsHint:
    "ITC is claimed head by head in GSTR-3B, so a total that agrees is not the same as a reconciliation that agrees. An intra-state supply booked as inter-state nets to zero on the total and is still a reversal exposure.",
  headBooks: "Books",
  headAuthority: "GSTR-2B",
  headDifference: "Difference",
  step5: "6 · By supplier",
  supplierHint:
    "One supplier is one subject in the graph, pointed at by the register, the GSTR-2A and the GSTR-2B alike — so which documents describe it is a question the graph answers directly. Chasing is done supplier by supplier, and this is the order to make the calls in.",
  inAll: "in all three",
  notFiled: "nothing filed",
  notBooked: "not in your books",
  partial: "partly filed",
  sourceBooks: "books",
  sourceGstr1: "GSTR-2A",
  sourceAuthority: "GSTR-2B",
  graphTitle: "5 · What the graph knows",
  graphHint:
    "These are not three spreadsheets joined by a batch job. Each import lands in its own named graph, one party subject is shared across all of them, and every finding is the live result of a query with the statute it rests on attached — nothing is written onto an invoice as a flag.",
  graphSubjects: "Invoice records",
  graphSubjectsHint: "one per source, joined by a normalized key",
  graphSuppliers: "Supplier subjects",
  graphSuppliersHint: "shared across every document that names them",
  graphSources: "Named graphs",
  graphSourcesHint: "one per imported document, never the default graph",
  graphRules: "Rules evaluated",
  graphRulesHint: "each a SPARQL file plus the provision it cites",
  graphVocab: "Vocabulary",
  graphVocabHint: "classes and predicates the pack registered at runtime",
  openWorkbench: "Query the graph directly",
  openVocabulary: "Browse the vocabulary",
};

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
/** The query for one source, built from **the pack's own vocabulary**.
 *
 *  **This hardcoded `PREFIX gst:` and nine `gst:` predicate names**, which is
 *  the same defect the source list and the guidance had: a healthcare or
 *  banking pack would have had the console ask for GST's predicates against
 *  its data and get nothing. The console now knows the *roles* — an identity,
 *  a matching key, a date, a period, the party a record is attributed to, and
 *  the measures that have to agree — and the pack names each one.
 *
 *  Every pattern sits inside its own `GRAPH ?g { }`: an import lands in
 *  `graph:import:{source}`, never the default graph, so a pattern outside one
 *  matches nothing at all — silently, which reads exactly like "you have not
 *  uploaded anything yet".
 *
 *  Everything but the identity is `OPTIONAL`, and deliberately: a required
 *  pattern silently drops every record missing that field, so a register
 *  exported without a supplier name would vanish from its own totals and the
 *  page would look like it worked. */
function sourceQuery(spec: {
  namespace: string;
  className: string;
  identity: string;
  matchKey: string;
  fields: NonNullable<NonNullable<PackConsoleConfig["reconciliation"]>["fields"]>;
  measures: readonly string[];
}): string {
  const { namespace, className, identity, matchKey, fields, measures } = spec;
  const optional = (variable: string, predicate: string | undefined, subject = "?invoice") =>
    predicate ? `  OPTIONAL { GRAPH ?g_${variable} { ${subject} p:${predicate} ?${variable} } }` : "";

  const measureLines = measures.map((name) => optional(name, name)).join("\n");
  const measureVars = measures.map((name) => `?${name}`).join(" ");

  return `PREFIX p: <${namespace}>
SELECT ?invoice ?invoiceNumber ?matchKey ?gstin ?supplierName ?invoiceDate ?period ${measureVars}
WHERE {
  GRAPH ?g {
    ?invoice a p:${className} ;
             p:${identity} ?invoiceNumber .
  }
${optional("matchKey", matchKey)}
${optional("invoiceDate", fields.date)}
${optional("period", fields.period)}
${
  fields.party
    ? `  OPTIONAL {
    GRAPH ?g_party { ?invoice p:${fields.party} ?party }
${fields.partyId ? `    OPTIONAL { GRAPH ?g_pid { ?party p:${fields.partyId} ?gstin } }` : ""}
${fields.partyName ? `    OPTIONAL { GRAPH ?g_pname { ?party p:${fields.partyName} ?supplierName } }` : ""}
  }`
    : ""
}
${measureLines}
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

/** The sources this pack declares, in the order it declares them.
 *
 *  **This was a TypeScript constant naming GST's three documents, and a second
 *  domain made that a defect rather than a shortcut.** Install a healthcare,
 *  banking or automotive pack and the page would have rendered its data under
 *  GST's headings, or nothing. It now comes from `[console.reconciliation]` in
 *  the pack's own manifest — see `packs/gst/pack.toml`. */
function sourcesFromConfig(config: PackConsoleConfig | null): readonly SourceSpec[] {
  return (config?.reconciliation?.sources ?? []).map((source) => ({
    key: source.key,
    className: source.class,
    label: source.label,
    role: source.description ?? "",
    surfaceKey: source.surface ?? source.key,
  }));
}

/** One SPARQL solution as a source row.
 *
 *  **Every field goes through `lexical`, and skipping it broke this page in
 *  two ways at once.** `POST /sparql` returns N-Triples, so a literal arrives
 *  as `"18000.00"` *with* the quotes: `Number` of that is `NaN`, so every
 *  total rendered as `₹NaN`, and `"INV-1001"` never equalled the finding's
 *  own `INV-1001`, so every invoice joined to nothing and each reconciling
 *  line valued itself at zero. Found in a browser against real data — the
 *  unit tests could not see it, because their fixtures were already plain
 *  strings.
 *
 *  Missing bindings become empty strings and `"0.00"`, so a total is never
 *  `NaN` for the other reason either — an unreadable figure in a tax
 *  statement is worse than a visibly absent one. */
function toSourceInvoice(row: Readonly<Record<string, string>>): BoundInvoice {
  const value = (name: string, fallback = "") =>
    row[name] === undefined ? fallback : lexical(row[name]);
  return {
    // The invoice's own subject, kept so `distinctInvoices` can collapse the
    // repeated bindings an `OPTIONAL` across named graphs produces.
    subject: value("invoice"),
    invoiceNumber: value("invoiceNumber"),
    gstin: value("gstin"),
    supplierName: value("supplierName"),
    invoiceDate: value("invoiceDate"),
    taxableValue: value("taxableValue", "0.00"),
    taxAmount: value("taxAmount", "0.00"),
    matchKey: value("matchKey"),
    igst: value("igst", "0.00"),
    cgst: value("cgst", "0.00"),
    sgst: value("sgst", "0.00"),
    cess: value("cess", "0.00"),
    period: value("period"),
  };
}

function SourceCard({
  spec,
  rows,
  surface,
  onImported,
  money,
  packId,
}: {
  spec: SourceSpec;
  rows: readonly SourceInvoice[];
  surface: PackImportSurface | undefined;
  onImported: () => void;
  money: (value: number) => string;
  /** Whose pack this upload belongs to — discovered by the page, never the
   *  literal `"gst"` this used to pass. */
  packId: string;
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
        const outcome = await importThroughSurface(packId, surface, await blob.text());
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
    [surface, onImported, packId],
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
          <Text style={{ fontSize: 22, fontWeight: 600 }}>{money(summary.taxAmount)}</Text>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {`${summary.count} invoice${summary.count === 1 ? "" : "s"} · taxable ${money(summary.taxableValue)}`}
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

function invoiceColumns(money: (v: number) => string) {
  return [
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
    render: (v: string) => money(Number(v || 0)),
  },
  {
    title: "Tax",
    dataIndex: "taxAmount",
    key: "taxAmount",
    align: "right" as const,
    render: (v: string) => money(Number(v || 0)),
  },
  ];
}

/** The statement's own arithmetic, laid out the way a practitioner's format
 *  lays it out: an opening balance, the reconciling items, a closing balance,
 *  and — the line a spreadsheet cannot produce — whatever is left over. */
function StatementPanel({
  statement,
  measures,
  money,
}: {
  statement: Statement;
  /** Declared by the pack, not assumed — see `sourcesFromConfig`. */
  measures: readonly { key: keyof Heads; label: string }[];
  money: (value: number) => string;
}) {
  return (
    <Card size="small">
      <Row gutter={[24, 16]}>
        <Col xs={24} md={6}>
          <Statistic
            title={COPY.booksTotal}
            value={money(statement.books.taxAmount)}
            valueStyle={{ fontSize: 20 }}
          />
          <Text type="secondary" style={{ fontSize: 12 }}>{`${statement.books.count} ${COPY.invoicesSuffix}`}</Text>
        </Col>
        <Col xs={24} md={6}>
          <Statistic
            title={COPY.authorityTotal}
            value={money(statement.authority.taxAmount)}
            valueStyle={{ fontSize: 20 }}
          />
          <Text type="secondary" style={{ fontSize: 12 }}>{`${statement.authority.count} ${COPY.invoicesSuffix}`}</Text>
        </Col>
        <Col xs={24} md={6}>
          <Statistic
            title={COPY.differenceTotal}
            value={money(statement.difference.taxAmount)}
            valueStyle={{ fontSize: 20 }}
          />
          <Text type="secondary" style={{ fontSize: 12 }}>{COPY.differenceHint}</Text>
        </Col>
        <Col xs={24} md={6}>
          <Statistic
            title={COPY.explainedTotal}
            value={money(statement.explained.taxAmount)}
            valueStyle={{ fontSize: 20 }}
          />
          <Text type="secondary" style={{ fontSize: 12 }}>{COPY.explainedHint}</Text>
        </Col>
      </Row>

      {/* **Head-wise, because that is how ITC is claimed.** Every practitioner
        *  reconciliation format checked carries these as separate columns, and
        *  a total-only statement cannot show the error they exist to catch:
        *  moving tax between heads nets to zero on the total. */}
      <div style={{ marginTop: 20 }}>
        <Text strong>{COPY.headsTitle}</Text>
        <Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 8 }}>
          {COPY.headsHint}
        </Paragraph>
        <Table
          size="small"
          rowKey="head"
          pagination={false}
          dataSource={measures.map((head) => ({
            head: head.label,
            books: statement.books[head.key],
            authority: statement.authority[head.key],
            difference: statement.difference[head.key],
          }))}
          columns={[
            { title: "", dataIndex: "head", key: "head", width: 130 },
            {
              title: COPY.headBooks,
              dataIndex: "books",
              key: "books",
              align: "right" as const,
              render: (v: number) => money(v),
            },
            {
              title: COPY.headAuthority,
              dataIndex: "authority",
              key: "authority",
              align: "right" as const,
              render: (v: number) => money(v),
            },
            {
              title: COPY.headDifference,
              dataIndex: "difference",
              key: "difference",
              align: "right" as const,
              // A head that agrees is the ordinary case and should read as
              // quiet; one that does not is the whole reason this table exists.
              render: (v: number) =>
                Math.abs(v) < 0.005 ? (
                  <Text type="secondary">{money(v)}</Text>
                ) : (
                  <Text strong type="warning">
                    {money(v)}
                  </Text>
                ),
            },
          ]}
        />
      </div>

      <Alert
        style={{ marginTop: 16 }}
        type={statement.reconciled ? "success" : "warning"}
        showIcon
        icon={statement.reconciled ? <CheckCircleOutlined /> : <WarningOutlined />}
        message={
          statement.reconciled
            ? COPY.reconciledTitle
            : `${COPY.unexplainedTitle}: ${money(statement.unexplained.taxAmount)}`
        }
        description={
          statement.reconciled ? (
            COPY.reconciledBody
          ) : (
            <Space direction="vertical" size={8} style={{ width: "100%" }}>
              <span>{COPY.unexplainedBody}</span>
              {/* **Naming them is what makes the number worth printing.** A
                *  bare "₹9,900 unaccounted for" sends a reviewer through the
                *  whole period; the invoices are a minute's work. */}
              {statement.unexplained.invoices.length > 0 && (
                <Table
                  size="small"
                  rowKey="invoiceNumber"
                  dataSource={[...statement.unexplained.invoices]}
                  columns={invoiceColumns(money)}
                  pagination={statement.unexplained.invoices.length > 10 ? { pageSize: 10 } : false}
                  scroll={{ x: "max-content" }}
                />
              )}
            </Space>
          )
        }
      />
    </Card>
  );
}

/** One kind of finding: what it means, what to do, and the invoices behind it.
 *
 *  **The extra evidence is rendered for the rules whose whole point is a pair
 *  of numbers.** A mismatch shown as one figure is unreviewable, and a
 *  late-filing finding without the filing date is just "unmatched" again. */
function FindingGroup({
  item,
  findings,
  money,
  guidance,
}: {
  item: ReconcilingItem;
  findings: readonly PackFinding[];
  money: (value: number) => string;
  /** The pack's own words for its own rules. */
  guidance: GuidanceIndex;
}) {
  const scenario = scenarioFor(item.label, guidance);
  const mine = findings.filter((f) => f.label === item.label && f.status !== "rejected");
  // **The rule renders its own evidence.** A predicate does not identify a
  // fact here: `PaymentOverdue` projects `gst:atTime` twice and
  // `GoodsReceiptTiming` once, so reading by predicate name alone labelled a
  // purchase date as a receipt date — the page asserting something false
  // about somebody's tax position, not a formatting slip.
  const detail = scenario.detail
    ? mine
        .map((finding) => {
          const evidence = evidenceOf(finding);
          const line = scenario.detail?.(evidence) ?? "";
          const number = first(evidence, "invoiceNumber");
          return line === "" ? "" : number === "" ? line : `${number}: ${line}`;
        })
        .filter((line) => line !== "")
    : [];

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
          <Text type="secondary" style={{ fontSize: 12 }}>{money(item.atStake)}</Text>
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
        columns={invoiceColumns(money)}
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

export function ReconciliationWorkspace({
  onReview,
  onWorkbench,
  onVocabulary,
}: {
  onReview: () => void;
  /** The console already has a SPARQL workbench and a vocabulary browser; this
   *  page's job is to point at them, not to grow its own copies. */
  onWorkbench: () => void;
  onVocabulary: () => void;
}) {
  const [ruleCount, setRuleCount] = useState<number | null>(null);
  const [config, setConfig] = useState<PackConsoleConfig | null>(null);
  const [guidance, setGuidance] = useState<GuidanceIndex>({});
  /** Which pack this page is showing. Discovered, never assumed — the page
   *  addressed `"gst"` by name in five places even after it had discovered a
   *  pack, so any other domain would have had its uploads, its rules and its
   *  findings all pointed at GST. */
  const [packId, setPackId] = useState<string | null>(null);
  const [rows, setRows] = useState<Record<string, SourceInvoice[]> | null>(null);
  const [findings, setFindings] = useState<readonly PackFinding[]>([]);
  const [truncated, setTruncated] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [surfaces, setSurfaces] = useState<readonly PackImportSurface[]>([]);
  const [packInstalled, setPackInstalled] = useState<boolean | null>(null);
  /** Formats this pack declared that this console cannot read. Named rather
   *  than dropped: a surface silently missing looks like a pack that forgot
   *  to declare it. */
  const [unreadable, setUnreadable] = useState<readonly string[]>([]);

  const refresh = useCallback(async () => {
    setFailure(null);
    try {
      const namespaces = await api.namespaces();
      // The first installed pack that declares a reconciliation. A deployment
      // with several picks the first; a deployment with none renders empty.
      const candidates = surfacesFor(namespaces.map((n) => n.declaredBy));
      let pack = candidates[0] ?? undefined;
      for (const candidate of candidates) {
        const declared = await api.packConsole(candidate.packId);
        if (declared?.reconciliation) {
          pack = candidate;
          break;
        }
      }
      setPackInstalled(pack !== undefined);
      setPackId(pack?.packId ?? null);
      if (!pack) {
        setSurfaces([]);
        setRows({});
        return;
      }

      const declared = await api.packConsole(pack.packId);
      setConfig(declared);
      // **The pack's own `[[console.imports]]` wins over the built-in
      // registry** — Plan 111 Slice E. The registry is the fallback for a
      // pack installed before it declared its files, not the source of truth:
      // a second domain's upload surfaces must not need a React change, which
      // is the test this whole plan applies to itself. `unreadableFormats`
      // reports a declared format this console has no reader for rather than
      // dropping it silently.
      const fromPack = surfacesFromConsole(pack.packId, pack.label, declared);
      setSurfaces(fromPack.length > 0 ? fromPack : pack.imports);
      setUnreadable(unreadableFormats(declared));
      const specs = sourcesFromConfig(declared);
      if (specs.length === 0) {
        // A pack with no declared reconciliation is not an error — it simply
        // does not appear on this page.
        setRows({});
        return;
      }

      const results = await Promise.all(
        specs.map((source) =>
          api.sparql(
            sourceQuery({
              // The pack's own namespace, from the namespace row its loader
              // declared — never a constant in the console.
              namespace:
                namespaces.find((n) => n.declaredBy === `pack:${pack.packId}`)?.iri ?? "",
              className: source.className,
              identity: declared?.reconciliation?.identity ?? "invoiceNumber",
              matchKey: declared?.reconciliation?.matchKey ?? "",
              fields: declared?.reconciliation?.fields ?? {},
              measures: (declared?.reconciliation?.measures ?? []).map((m) => m.name),
            }),
          ),
        ),
      );
      const next: Record<string, SourceInvoice[]> = {};
      specs.forEach((source, index) => {
        next[source.key] = distinctInvoices((results[index]?.rows ?? []).map(toSourceInvoice));
      });
      setRows(next);
      // **Surfaced, never inferred from the row count.** A truncated answer
      // that looks complete is the failure this project refuses everywhere,
      // and here it would be a tax figure quietly missing invoices.
      setTruncated(results.some((result) => result.truncated));
      setFindings(await api.findings({ pack: pack.packId }));
      // The pack's own registered rules — real data, already admin-gated, and
      // the honest answer to "what is actually evaluating my data".
      setRuleCount(await api.findingRules(pack.packId).then((r) => r.length).catch(() => null));
      // Per-rule wording as the pack declared it. It arrives with the console
      // config rather than with the rules, because the finding-rule registry
      // has no field for it — see `read_console_config`.
      setGuidance(
        guidanceFrom(
          Object.entries(declared?.guidance ?? {}).map(([label, g]) => ({ label, guidance: g })),
        ),
      );
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
      if (packId === null) return;
      const outcome = await api.reconcilePack(packId);
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
        ? buildStatement({ books: rows.books ?? [], authority: rows.authority ?? [], findings, guidance })
        : null,
    [rows, findings],
  );

  /** Whatever the installed pack declares — empty for a pack that declares no
   *  reconciliation, which renders an honest empty state rather than GST's
   *  headings over somebody else's data. */
  const sources = useMemo(() => sourcesFromConfig(config), [config]);

  /** The measures this pack reconciles in. A pack reconciling something other
   *  than tax declares its own — quantities, claim amounts, whatever has to
   *  agree — and this table follows it. */
  const measures = useMemo(
    () =>
      (config?.reconciliation?.measures ?? []).map((m) => ({
        key: m.name as keyof Heads,
        label: m.label,
      })),
    [config],
  );

  /** Currency and grouping are a domain fact, not a global preference: ₹ in
   *  Indian grouping for GST, and whatever a different pack declares for
   *  itself. Falls back to plain grouping when a pack declares neither. */
  const money = useMemo(() => {
    const currency = config?.reconciliation?.currency;
    const locale = config?.reconciliation?.locale ?? "en-US";
    const format = new Intl.NumberFormat(locale, {
      ...(currency ? { style: "currency" as const, currency } : {}),
      maximumFractionDigits: 2,
      minimumFractionDigits: 2,
    });
    return (value: number) => format.format(value);
  }, [config]);

  const suppliers = useMemo(
    () =>
      rows
        ? supplierView({
            books: rows.books ?? [],
            gstr1: rows.gstr1 ?? [],
            authority: rows.authority ?? [],
          })
        : [],
    [rows],
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

      {/* **A declared format this console cannot read is named, not dropped.**
          A surface that silently disappears looks like a pack that forgot to
          declare it; naming the format tells an operator exactly which line of
          `pack.toml` to fix, or that this console build is older than the pack
          it is serving. */}
      {unreadable.length > 0 && (
        <Alert
          type="warning"
          showIcon
          message={COPY.unreadable}
          description={unreadable.join(", ")}
        />
      )}

      <div>
        <Title level={5}>{COPY.step1}</Title>
        <Row gutter={[16, 16]}>
          {sources.map((source) => (
            <Col xs={24} md={8} key={source.key}>
              <SourceCard
                spec={source}
                rows={rows[source.key] ?? []}
                surface={surfaces.find((s) => s.key === source.surfaceKey)}
                onImported={() => void refresh()}
                money={money}
                packId={packId ?? ""}
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
        <StatementPanel statement={statement} measures={measures} money={money} />
      </div>

      {/* **The graph made visible, from data already fetched.** The complaint
        *  this answers is a fair one: a page can reconcile perfectly and give
        *  no sign that a graph is involved at all. Every figure here is a real
        *  count off the same three queries the statement uses, plus the pack's
        *  own registered rules and vocabulary. */}
      <div>
        <Title level={5}>{COPY.graphTitle}</Title>
        <Paragraph type="secondary" style={{ fontSize: 12 }}>
          {COPY.graphHint}
        </Paragraph>
        <Row gutter={[16, 16]}>
          {[
            {
              label: COPY.graphSubjects,
              hint: COPY.graphSubjectsHint,
              value: (rows.books?.length ?? 0) + (rows.gstr1?.length ?? 0) + (rows.authority?.length ?? 0),
            },
            { label: COPY.graphSuppliers, hint: COPY.graphSuppliersHint, value: suppliers.length },
            {
              label: COPY.graphSources,
              hint: COPY.graphSourcesHint,
              value: sources.filter((s) => (rows[s.key]?.length ?? 0) > 0).length,
            },
            { label: COPY.graphRules, hint: COPY.graphRulesHint, value: ruleCount ?? 0 },
          ].map((tile) => (
            <Col xs={12} md={6} key={tile.label}>
              <Card size="small">
                <Statistic title={tile.label} value={tile.value} valueStyle={{ fontSize: 22 }} />
                <Text type="secondary" style={{ fontSize: 11 }}>
                  {tile.hint}
                </Text>
              </Card>
            </Col>
          ))}
        </Row>
        <Space style={{ marginTop: 12 }} wrap>
          <Button size="small" onClick={onWorkbench}>
            {COPY.openWorkbench}
          </Button>
          <Button size="small" onClick={onVocabulary}>
            {COPY.openVocabulary}
          </Button>
        </Space>
      </div>

      <div>
        <Title level={5}>{COPY.step5}</Title>
        <Paragraph type="secondary" style={{ fontSize: 12 }}>
          {COPY.supplierHint}
        </Paragraph>
        <Table
          size="small"
          rowKey="gstin"
          dataSource={[...suppliers]}
          pagination={suppliers.length > 10 ? { pageSize: 10 } : false}
          scroll={{ x: "max-content" }}
          columns={[
            {
              title: "Supplier",
              key: "supplier",
              render: (_: unknown, row: SupplierRow) => (
                <Space direction="vertical" size={0}>
                  <Text>{row.supplierName || "—"}</Text>
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    {row.gstin}
                  </Text>
                </Space>
              ),
            },
            {
              title: "Described by",
              key: "sources",
              render: (_: unknown, row: SupplierRow) => (
                <Space size={4} wrap>
                  {row.inBooks && <Tag>{COPY.sourceBooks}</Tag>}
                  {row.inGstr1 && <Tag>{COPY.sourceGstr1}</Tag>}
                  {row.inAuthority && <Tag color="green">{COPY.sourceAuthority}</Tag>}
                </Space>
              ),
            },
            {
              title: "Standing",
              key: "standing",
              render: (_: unknown, row: SupplierRow) =>
                row.inBooks && row.inGstr1 && row.inAuthority ? (
                  <Tag color="green">{COPY.inAll}</Tag>
                ) : !row.inGstr1 && !row.inAuthority ? (
                  <Tag color="red">{COPY.notFiled}</Tag>
                ) : !row.inBooks ? (
                  <Tag color="blue">{COPY.notBooked}</Tag>
                ) : (
                  <Tag color="orange">{COPY.partial}</Tag>
                ),
            },
            {
              title: "Your invoices",
              dataIndex: "invoices",
              key: "invoices",
              align: "right" as const,
            },
            {
              title: "Tax in books",
              dataIndex: "booksTax",
              key: "booksTax",
              align: "right" as const,
              render: (v: number) => money(v),
            },
            {
              title: "Tax in GSTR-2B",
              dataIndex: "authorityTax",
              key: "authorityTax",
              align: "right" as const,
              render: (v: number) => money(v),
            },
          ]}
        />
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
            <FindingGroup key={item.label} item={item} findings={findings} money={money} guidance={guidance} />
          ))
        )}
      </div>
    </Space>
  );
}
