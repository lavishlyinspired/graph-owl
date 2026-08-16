/** The reconciliation, as one page a CA works down — Plan 108 Slice 7.
 *
 *  **Why this exists when the Review queue already lists findings.** The queue
 *  is a reviewer's tool: one finding at a time, with its rule and its evidence
 *  chain, which is exactly right for deciding whether *one* accusation stands.
 *  It is not how a reconciliation is done. A CA closing a period needs the
 *  three totals side by side, the difference between them, and what accounts
 *  for that difference, with the arithmetic already done.
 *
 *  **Read-only, deliberately — Plan 120 Slice E.** This page used to also
 *  offer upload cards and a manual "Run reconciliation" button; both are
 *  gone. reco-now is the only place a CA uploads a period or triggers a run
 *  now — its own upload flow already calls graph-owl's native rule engine
 *  automatically (`_run_graphowl_reconcile` in `ext-apps/Reco/backend/app/
 *  main.py`) the moment column mapping is confirmed, so a second, manual
 *  trigger here duplicated a step nobody needed to take twice. This page
 *  still reads the graph fresh on every mount (`refresh()`, below) — the
 *  statement is never stale just because nobody clicked a button on this
 *  particular screen; it reflects whatever the last real upload, from
 *  wherever it came from, actually landed.
 *
 *  So this page is the statement, read the moment it opens: the totals side
 *  by side, the exceptions worked, the working paper taken away. Nothing
 *  here duplicates the queue's job — every finding still opens into the
 *  queue's evidence chain, which is where the argument for a finding lives.
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
  Modal,
  Row,
  Select,
  Space,
  Statistic,
  Table,
  Tag,
  Typography,
} from "./../../components/ui/antd-compat";
import {
  CheckCircleOutlined,
  DownloadOutlined,
  QuestionCircleOutlined,
  WarningOutlined,
} from "@ant-design/icons";
import { api, type FindingRuleDef, type PackConsoleConfig, type PackFinding } from "../../api";
import { lexical } from "../../workbench/results";
import { surfacesFor } from "../packs/packSurfaces";
import {
  ALL_PERIODS,
  buildStatement,
  distinctInvoices,
  evidenceOf,
  findingsForPeriod,
  first,
  forPeriod,
  guidanceFrom,
  localName,
  periodsOf,
  scenarioFor,
  sourceSummary,
  statementCsv,
  supplierView,
  type BoundInvoice,
  type GuidanceIndex,
  type Heads,
  type ReconcilingItem,
  type SourceInvoice,
  type SourceSummary,
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
  step3: "1 · The statement",
  step4: "4 · What to do about it",
  loading: "Reading the graph…",
  loadFailed: "Could not read the reconciliation",
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
  filterPeriod: "GST period",
  filterAll: "All",
  filterHint:
    "Narrows the statement, the findings and the by-supplier view to one filing period. A period you are not working on is not deleted.",
  truncated:
    "The graph returned more rows than one read allows, so these totals are of what was read, not of everything held. Narrow the period before relying on them.",
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
  step5: "3 · By supplier",
  supplierHint:
    "One supplier is one subject in the graph, pointed at by the register, the GSTR-2A and the GSTR-2B alike — so which documents describe it is a question the graph answers directly. Chasing is done supplier by supplier, and this is the order to make the calls in.",
  inAll: "in all three",
  notFiled: "nothing filed",
  notBooked: "not in your books",
  partial: "partly filed",
  sourceBooks: "books",
  sourceGstr1: "GSTR-2A",
  sourceAuthority: "GSTR-2B",
  graphTitle: "2 · What the graph knows",
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
  totalRow: "Total",
  rulesTrigger: "What these rules are",
  noRules: "No rule is registered for this pack yet.",
  statementHelp: "How the statement is built",
  graphTileTitles: {
    invoices: "Invoice records in the graph",
    suppliers: "Supplier subjects",
    graphs: "Named graphs",
  },
  graphSource: "Source",
  graphRecordCount: "Records",
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
      title: "IGST",
      dataIndex: "igst",
      key: "igst",
      align: "right" as const,
      render: (v: string) => money(Number(v || 0)),
    },
    {
      title: "CGST",
      dataIndex: "cgst",
      key: "cgst",
      align: "right" as const,
      render: (v: string) => money(Number(v || 0)),
    },
    {
      title: "SGST",
      dataIndex: "sgst",
      key: "sgst",
      align: "right" as const,
      render: (v: string) => money(Number(v || 0)),
    },
    {
      title: "Cess",
      dataIndex: "cess",
      key: "cess",
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

/** The totals row every invoice table ends on — the source's own sums, never
 *  a per-page total, because a table of ₹ values that does not total the rows
 *  it actually shows is not a table of ₹ values. `offset`/`labelSpan` let a
 *  table with a leading column (e.g. per-source records) keep the amounts
 *  under the same columns they describe. */
function invoiceTotals(summary: SourceSummary, money: (v: number) => string, offset = 0, labelSpan = 5) {
  return () => (
    <Table.Summary.Row>
      <Table.Summary.Cell index={0} colSpan={labelSpan}>
        <Text strong>{COPY.totalRow}</Text>
      </Table.Summary.Cell>
      <Table.Summary.Cell index={offset + 5} align="right">
        <Text strong>{money(summary.taxableValue)}</Text>
      </Table.Summary.Cell>
      <Table.Summary.Cell index={offset + 6} align="right">
        <Text strong>{money(summary.igst)}</Text>
      </Table.Summary.Cell>
      <Table.Summary.Cell index={offset + 7} align="right">
        <Text strong>{money(summary.cgst)}</Text>
      </Table.Summary.Cell>
      <Table.Summary.Cell index={offset + 8} align="right">
        <Text strong>{money(summary.sgst)}</Text>
      </Table.Summary.Cell>
      <Table.Summary.Cell index={offset + 9} align="right">
        <Text strong>{money(summary.cess)}</Text>
      </Table.Summary.Cell>
      <Table.Summary.Cell index={offset + 10} align="right">
        <Text strong>{money(summary.taxAmount)}</Text>
      </Table.Summary.Cell>
    </Table.Summary.Row>
  );
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

/** One supplier per row, and which documents describe it — used by the
 *  "By supplier" section and opened again from the graph's "Supplier
 *  subjects" tile, so the two can never disagree about a standing. */
function SupplierTable({
  suppliers,
  money,
}: {
  suppliers: readonly SupplierRow[];
  money: (value: number) => string;
}) {
  return (
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
  );
}

/** The registered rules, each with the pack's own words for it — opened from
 *  the "Run the rules" section and from the graph's "Rules evaluated" tile,
 *  so neither can drift from the other. `scenarioFor` supplies the meaning
 *  and next action the pack declared; the rule's one-line `summary` comes
 *  from the registry itself. */
function RulesPanel({
  rules,
  guidance,
}: {
  rules: readonly FindingRuleDef[] | null;
  guidance: GuidanceIndex;
}) {
  if (!rules) return <Text type="secondary">{COPY.loading}</Text>;
  if (rules.length === 0) return <Text type="secondary">{COPY.noRules}</Text>;
  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      {rules.map((rule) => {
        const scenario = scenarioFor(rule.label, guidance);
        return (
          <Card key={rule.label} size="small">
            <Space wrap>
              <Tag>{localName(rule.label)}</Tag>
              <Tag color="blue">{localName(rule.governedBy)}</Tag>
            </Space>
            <Paragraph style={{ margin: "8px 0 0" }}>{rule.summary}</Paragraph>
            {scenario.meaning !== "" && (
              <Paragraph type="secondary" style={{ margin: "8px 0 0", fontSize: 12 }}>
                {scenario.meaning}
              </Paragraph>
            )}
            {scenario.nextAction !== "" && (
              <Paragraph style={{ margin: "8px 0 0", fontSize: 12 }}>
                <Text strong>{`${COPY.nextAction}: `}</Text>
                {scenario.nextAction}
              </Paragraph>
            )}
          </Card>
        );
      })}
    </Space>
  );
}

/** What the statement's four figures mean and where each one comes from —
 *  the answer to "where does the books taxable figure come from" that a
 *  total with no provenance cannot give. Amounts are described, never
 *  printed, so another pack's statement explains itself in the same words. */
function StatementHelp({
  statement,
}: {
  statement: Statement;
}) {
  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Paragraph style={{ margin: 0 }}>
        <Text strong>{`${COPY.booksTotal}: `}</Text>
        {`what your own register records — ${statement.books.count} ${COPY.invoicesSuffix}. The card's taxable figure is the sum of the taxable values of the invoices; its tax figure is the sum of their tax.`}
      </Paragraph>
      <Paragraph style={{ margin: 0 }}>
        <Text strong>{`${COPY.authorityTotal}: `}</Text>
        {`what the authority made available to claim — ${statement.authority.count} ${COPY.invoicesSuffix}.`}
      </Paragraph>
      <Paragraph style={{ margin: 0 }}>
        <Text strong>{`${COPY.differenceTotal}: `}</Text>
        {`books minus GSTR-2B (${COPY.differenceHint}). This is the number the period is driven to zero.`}
      </Paragraph>
      <Paragraph style={{ margin: 0 }}>
        <Text strong>{`${COPY.explainedTotal}: `}</Text>
        {`the sum of the finding rows below (${COPY.explainedHint}). Whatever they do not cover stays as ${COPY.unexplainedTitle} — the amount the statement is honest about not having explained yet.`}
      </Paragraph>
      <Paragraph style={{ margin: 0 }}>
        <Text strong>{`${COPY.headsTitle}: `}</Text>
        {`ITC is claimed head by head in GSTR-3B, so IGST, CGST, SGST and Cess are compared separately. A total can agree while a head disagrees — that is the reversal exposure a total-only reconciliation cannot see.`}
      </Paragraph>
    </Space>
  );
}

/** Every invoice record the graph holds, one row per record with the source
 *  it arrived in — the graph's own subject count, which is deliberately not
 *  the statement's invoice count, because the same invoice is three subjects
 *  (register, GSTR-1, GSTR-2B). */
function InvoiceRecordsModal({
  rows,
  money,
}: {
  rows: Record<string, SourceInvoice[]>;
  money: (value: number) => string;
}) {
  const sources = Object.entries(rows).filter(([, list]) => list.length > 0);
  const flat = sources.flatMap(([sourceKey, list]) => list.map((row) => ({ ...row, sourceKey })));
  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Space wrap>
        {sources.map(([sourceKey, list]) => (
          <Tag key={sourceKey}>{`${sourceKey}: ${list.length}`}</Tag>
        ))}
        <Tag color="blue">{`${COPY.totalRow}: ${flat.length}`}</Tag>
      </Space>
      <Table
        size="small"
        rowKey={(row) => `${row.sourceKey}:${row.invoiceNumber}`}
        dataSource={flat}
        pagination={false}
        scroll={{ x: "max-content", y: 320 }}
        columns={[
          { title: COPY.graphSource, dataIndex: "sourceKey", key: "sourceKey", width: 130 },
          ...invoiceColumns(money),
        ]}
        summary={invoiceTotals(sourceSummary(flat), money, 1, 6)}
      />
    </Space>
  );
}

/** The loaded named graphs, one row each, with what it holds — "one per
 *  imported document, never the default graph" made concrete. */
function NamedGraphsModal({
  sources,
  rows,
}: {
  sources: readonly SourceSpec[];
  rows: Record<string, SourceInvoice[]>;
}) {
  return (
    <Table
      size="small"
      rowKey="key"
      pagination={false}
      dataSource={sources.map((source) => ({ ...source, count: rows[source.key]?.length ?? 0 }))}
      columns={[
        { title: COPY.graphSource, dataIndex: "label", key: "label" },
        { title: COPY.graphRecordCount, dataIndex: "count", key: "count", align: "right" as const },
        { title: "Role", dataIndex: "role", key: "role" },
      ]}
    />
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
  const [rules, setRules] = useState<readonly FindingRuleDef[] | null>(null);
  const [rulesOpen, setRulesOpen] = useState(false);
  const [statementHelpOpen, setStatementHelpOpen] = useState(false);
  const [graphTileOpen, setGraphTileOpen] = useState<"invoices" | "suppliers" | "graphs" | null>(null);
  const [config, setConfig] = useState<PackConsoleConfig | null>(null);
  const [guidance, setGuidance] = useState<GuidanceIndex>({});
  const [rows, setRows] = useState<Record<string, SourceInvoice[]> | null>(null);
  const [findings, setFindings] = useState<readonly PackFinding[]>([]);
  const [truncated, setTruncated] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [packInstalled, setPackInstalled] = useState<boolean | null>(null);
  /** Which filing period the statement, findings and suppliers are narrowed to.
   *  `"all"` is the default, and must behave exactly as the workspace did
   *  before the filter existed. */
  const [period, setPeriod] = useState<string>(ALL_PERIODS);

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
      if (!pack) {
        setRows({});
        return;
      }

      const declared = await api.packConsole(pack.packId);
      setConfig(declared);
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
      setRules(await api.findingRules(pack.packId).catch(() => null));
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

  /** Whatever the installed pack declares — empty for a pack that declares no
   *  reconciliation, which renders an honest empty state rather than GST's
   *  headings over somebody else's data. */
  const sources = useMemo(() => sourcesFromConfig(config), [config]);

  /** The periods any loaded source covers — the union the period filter offers.
   *  Computed here, beside the rows it reads, so the Select and the filter can
   *  never disagree about what a period is. */
  const allPeriods = useMemo(
    () => (rows === null ? [] : periodsOf(...sources.map((source) => rows[source.key] ?? []))),
    [rows, sources],
  );

  /** The rows narrowed to the selected period, per source. The source cards
   *  keep the full load — a card answers "what have I uploaded" — while the
   *  statement, findings and supplier view work from the narrowed set, so one
   *  CA filing July does not read a statement that mixes July and August.
   *
   *  `"all"` is identity: no filter behaves exactly like the workspace did
   *  before the filter existed, including a record that shipped without a
   *  period. */
  const filteredRows = useMemo(() => {
    const out: Record<string, SourceInvoice[]> = {};
    for (const source of sources) out[source.key] = [...forPeriod(rows?.[source.key] ?? [], period)];
    return out;
  }, [rows, sources, period]);

  /** Findings narrowed by the same join the statement uses — a finding about an
   *  invoice outside the period must not explain rupees that are not on the
   *  screen. */
  const filteredFindings = useMemo(
    () =>
      period === ALL_PERIODS
        ? findings
        : findingsForPeriod(findings, filteredRows.books ?? [], filteredRows.authority ?? []),
    [period, findings, filteredRows],
  );

  const statement = useMemo(
    () =>
      rows
        ? buildStatement({
            books: filteredRows.books ?? [],
            authority: filteredRows.authority ?? [],
            findings: filteredFindings,
            guidance,
          })
        : null,
    [rows, filteredRows, filteredFindings, guidance],
  );

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

  /** The number of registered rules — derived from the same fetch that powers
   *  the rules popup, so the tile can never disagree with what it opens. */
  const ruleCount = rules?.length ?? 0;

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
            books: filteredRows.books ?? [],
            gstr1: filteredRows.gstr1 ?? [],
            authority: filteredRows.authority ?? [],
          })
        : [],
    [rows, filteredRows],
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

      {/* **The period filter, placed where the truncated alert tells the CA to
          narrow the period.** One filing period at a time is how a CA works,
          and a statement mixing July and August is the failure the filter
          exists to prevent. Hidden until something is loaded, because a filter
          with nothing to filter is noise. */}
      {allPeriods.length > 0 && (
        <Space direction="vertical" size={2}>
          <Space wrap align="center">
            <Text strong style={{ fontSize: 13 }}>
              {COPY.filterPeriod}
            </Text>
            <Select
              value={period}
              onChange={(v) => setPeriod(typeof v === "string" ? v : ALL_PERIODS)}
              style={{ minWidth: 120 }}
              options={[
                { value: ALL_PERIODS, label: COPY.filterAll },
                ...allPeriods.map((value) => ({ value, label: value })),
              ]}
            />
          </Space>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {COPY.filterHint}
          </Text>
        </Space>
      )}

      <div>
        <Title level={5}>
          <Space size={4}>
            {COPY.step3}
            <Button type="link" size="small" icon={<QuestionCircleOutlined />} onClick={() => setStatementHelpOpen(true)}>
              {COPY.statementHelp}
            </Button>
          </Space>
        </Title>
        {/* **Read-only actions, not workflow steps — Plan 120 Slice E.**
            Uploading and running are reco-now's job now; what is left here is
            exporting and reading what the last real upload, from wherever it
            came from, already produced. */}
        <Space wrap style={{ marginBottom: 12 }}>
          <Button icon={<DownloadOutlined />} disabled={statement.items.length === 0} onClick={download}>
            {statement.items.length === 0 ? COPY.nothingToExport : COPY.exportCsv}
          </Button>
          <Button onClick={onReview}>{COPY.openInReview}</Button>
          <Button size="small" icon={<QuestionCircleOutlined />} onClick={() => setRulesOpen(true)}>
            {COPY.rulesTrigger}
          </Button>
        </Space>
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
              key: "invoices" as const,
              label: COPY.graphSubjects,
              hint: COPY.graphSubjectsHint,
              value: (rows.books?.length ?? 0) + (rows.gstr1?.length ?? 0) + (rows.authority?.length ?? 0),
            },
            {
              key: "suppliers" as const,
              label: COPY.graphSuppliers,
              hint: COPY.graphSuppliersHint,
              value: suppliers.length,
            },
            {
              key: "graphs" as const,
              label: COPY.graphSources,
              hint: COPY.graphSourcesHint,
              value: sources.filter((s) => (rows[s.key]?.length ?? 0) > 0).length,
            },
            { key: "rules" as const, label: COPY.graphRules, hint: COPY.graphRulesHint, value: ruleCount },
          ].map((tile) => (
            <Col xs={12} md={6} key={tile.key}>
              {/* **Every figure here is a door into the graph it came from.** A
                  number with nothing behind it is the very thing this section
                  exists to disprove — the rules tile opens the same rules the
                  "Run the rules" section opens, so neither can drift. */}
              <Card
                size="small"
                hoverable
                onClick={() => (tile.key === "rules" ? setRulesOpen(true) : setGraphTileOpen(tile.key))}
              >
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
        <SupplierTable suppliers={suppliers} money={money} />
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

      {/* The three explanations the page can give about itself, opened from
          where the question arises: the rules behind the run button, the
          arithmetic behind the statement, and each graph tile's own content. */}
      <Modal
        open={rulesOpen}
        title={`${COPY.rulesTrigger} · ${ruleCount}`}
        onCancel={() => setRulesOpen(false)}
        footer={null}
        width={720}
      >
        <RulesPanel rules={rules} guidance={guidance} />
      </Modal>

      <Modal
        open={statementHelpOpen}
        title={COPY.statementHelp}
        onCancel={() => setStatementHelpOpen(false)}
        footer={null}
        width={640}
      >
        <StatementHelp statement={statement} />
      </Modal>

      <Modal
        open={graphTileOpen !== null}
        title={graphTileOpen ? COPY.graphTileTitles[graphTileOpen] : ""}
        onCancel={() => setGraphTileOpen(null)}
        footer={null}
        width={960}
      >
        {graphTileOpen === "invoices" && <InvoiceRecordsModal rows={rows} money={money} />}
        {graphTileOpen === "suppliers" && <SupplierTable suppliers={suppliers} money={money} />}
        {graphTileOpen === "graphs" && <NamedGraphsModal sources={sources} rows={rows} />}
      </Modal>
    </Space>
  );
}
