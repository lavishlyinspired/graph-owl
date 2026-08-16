import { useMemo, useState } from "react";
import { Download, Search, ShieldCheck, AlertTriangle, ChevronLeft, ChevronRight, ArrowRight, ArrowLeft, FileSpreadsheet, RefreshCw, ExternalLink } from "lucide-react";
import { Button, Card, StatusPill } from "../components/ui.jsx";
import { api } from "../api.js";
import { amount, confidence, diff, inrFormat, statusLabel } from "../format.js";

const PAGE_SIZES = [10, 25, 50, 100];

/** Where "Open in GraphOWL" points a row at — graph-owl's own review queue,
 *  already deep-linkable to one specific finding
 *  (`?section=review&kind=findings&entry=<id>`, confirmed working code, no
 *  graph-owl-side change needed for this). `null` when the row carries no
 *  `finding_id` (a matched invoice with no finding, or graph-owl's base
 *  URL isn't known yet) — there is nothing honest to link to. */
function graphOwlFindingUrl(graphowlUrl, findingId) {
  if (!graphowlUrl || !findingId) return null;
  const params = new URLSearchParams({ section: "review", kind: "findings", entry: findingId });
  return `${graphowlUrl}/?${params.toString()}`;
}

function rowView(row) {
  const book = row.book;
  const portal = row.portal;
  return {
    ...row,
    gstin: (book || portal)?.gstin || "",
    supplier: (book || portal)?.supplier || "",
    inv2b: portal?.invoice_no || "—",
    invBooks: book?.invoice_no || "—",
    voucher: book?.voucher_no || "—",
    taxable2b: portal?.taxable,
    tax2b: portal?.tax,
    taxableBooks: book?.taxable,
    taxBooks: book?.tax,
    hsn: book?.hsn || portal?.hsn || "—",
    imsStatus: portal?.ims_status || book?.ims_status || "—",
  };
}

export default function ReconcilePage({ overview, onMapping, onIntelligence }) {
  const results = overview.results || [];
  const stats = overview.stats || {};
  const [filter, setFilter] = useState("all");
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState(10);

  const rows = useMemo(() => results.map(rowView), [results]);

  const filters = [
    { key: "all", label: "All" },
    { key: "matched", label: "Matched" },
    { key: "review", label: "Review" },
    { key: "only_books", label: "Only Books" },
    { key: "only_gstr2b", label: "Only GSTR-2B" },
  ];

  const counts = {
    all: rows.length,
    matched: stats.matched ?? 0,
    review: stats.review ?? 0,
    only_books: stats.only_books ?? 0,
    only_gstr2b: stats.only_gstr2b ?? 0,
  };

  const visible = rows.filter((row) => {
    const inFilter = filter === "all" || row.status === filter;
    if (!inFilter) return false;
    if (!search.trim()) return true;
    const q = search.toLowerCase();
    return (
      row.gstin.toLowerCase().includes(q) ||
      row.supplier.toLowerCase().includes(q) ||
      (row.inv2b !== "—" && row.inv2b.toLowerCase().includes(q)) ||
      (row.invBooks !== "—" && row.invBooks.toLowerCase().includes(q))
    );
  });

  const pageCount = Math.max(1, Math.ceil(visible.length / pageSize));
  const safePage = Math.min(page, pageCount - 1);
  const pageRows = visible.slice(safePage * pageSize, safePage * pageSize + pageSize);

  const atRiskCount = (stats.review ?? 0) + (stats.only_books ?? 0);
  const reversals = 0;
  const netItc = (stats.gross_itc ?? 0) - reversals;

  const download = (path) => {
    api.download(path);
  };

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Findings</h1>
          <p className="text-matcha-text-secondary mt-2">
            Review findings and evidence below
          </p>
        </div>
        <div className="flex items-center gap-3">
          <Button variant="outline" onClick={() => download("/api/export/csv")}>
            <Download size={16} /> Export CSV
          </Button>
          <Button variant="outline" onClick={() => download("/api/export/working-paper.xlsx")}>
            <FileSpreadsheet size={16} /> Export Working Paper (.xlsx)
          </Button>
          <Button variant="outline" onClick={() => download("/api/export/itc-register.xlsx")}>
            <RefreshCw size={16} /> ITC Register (.xlsx)
          </Button>
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <Card className="p-5">
          <div className="flex items-center gap-2 text-matcha-accent">
            <ShieldCheck size={18} />
            <h3 className="font-semibold text-matcha-text-primary">ITC Confirmed Safe</h3>
          </div>
          <p className="text-3xl font-bold mt-3 font-mono">{inrFormat(stats.confirmed_itc)}</p>
          <p className="text-sm text-matcha-text-tertiary mt-1">
            {stats.matched} matched invoices — claim with confidence
          </p>
        </Card>

        <Card className="p-5">
          <div className="flex items-center gap-2 text-matcha-amber">
            <AlertTriangle size={18} />
            <h3 className="font-semibold text-matcha-text-primary">ITC at Risk</h3>
          </div>
          <p className="text-3xl font-bold mt-3 font-mono">{inrFormat(stats.at_risk_itc)}</p>
          <p className="text-sm text-matcha-text-tertiary mt-1">
            {atRiskCount} invoices need action
          </p>
        </Card>

        <Card className="p-5 flex flex-col justify-between">
          <h3 className="font-semibold text-matcha-text-primary">Match Rate</h3>
          <div className="flex items-end justify-between mt-3">
            <p className="text-3xl font-bold font-mono text-matcha-accent">{stats.match_rate}%</p>
            <div className="text-right text-xs text-matcha-text-tertiary">
              <div>{stats.matched} matched</div>
              <div>{rows.length} total</div>
            </div>
          </div>
          <div className="mt-3 h-2 rounded-full bg-matcha-bg overflow-hidden">
            <div
              className="h-full bg-matcha-accent rounded-full transition-all"
              style={{ width: `${stats.match_rate}%` }}
            />
          </div>
        </Card>
      </div>

      <Card className="overflow-hidden">
        <div className="flex flex-wrap items-center gap-2 px-4 py-3 border-b border-matcha-border">
          {filters.map((f) => (
            <button
              key={f.key}
              onClick={() => {
                setFilter(f.key);
                setPage(0);
              }}
              className={[
                "px-3 py-1.5 rounded-full border text-sm font-medium transition-colors",
                filter === f.key
                  ? "bg-matcha-accent-surface border-matcha-accent text-matcha-accent"
                  : "border-matcha-border text-matcha-text-secondary hover:text-matcha-text-primary",
              ].join(" ")}
            >
              {f.label === "All" ? "All" : f.label} {counts[f.key]}
            </button>
          ))}
          <div className="flex-1" />
          <div className="relative">
            <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-matcha-text-tertiary" />
            <input
              value={search}
              onChange={(e) => {
                setSearch(e.target.value);
                setPage(0);
              }}
              placeholder="Search by GSTIN, supplier, invoice number..."
              className="bg-matcha-bg border border-matcha-border rounded-lg pl-8 pr-3 py-1.5 text-sm w-72 focus:outline-none focus:border-matcha-accent"
            />
          </div>
        </div>

        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="bg-matcha-bg text-left text-xs text-matcha-text-tertiary uppercase tracking-wide">
                <th className="px-4 py-2.5 font-medium">GSTIN</th>
                <th className="px-4 py-2.5 font-medium">Supplier</th>
                <th className="px-4 py-2.5 font-medium">Inv (GSTR-2B)</th>
                <th className="px-4 py-2.5 font-medium">Inv (Books)</th>
                <th className="px-4 py-2.5 font-medium">Voucher No</th>
                <th className="px-4 py-2.5 font-medium">HSN</th>
                <th className="px-4 py-2.5 font-medium">IMS Status</th>
                <th className="px-4 py-2.5 font-medium text-right">Taxable (2B)</th>
                <th className="px-4 py-2.5 font-medium text-right">Tax (2B)</th>
                <th className="px-4 py-2.5 font-medium text-right">Taxable (Books)</th>
                <th className="px-4 py-2.5 font-medium text-right">Tax (Books)</th>
                <th className="px-4 py-2.5 font-medium text-right">Diff</th>
                <th className="px-4 py-2.5 font-medium text-right">ITC Amt</th>
                <th className="px-4 py-2.5 font-medium">Reason</th>
                <th className="px-4 py-2.5 font-medium">Status</th>
                <th className="px-4 py-2.5 font-medium" />
              </tr>
            </thead>
            <tbody>
              {pageRows.map((row, i) => {
                const graphowlUrl = graphOwlFindingUrl(overview.graphowl_url, row.finding_id);
                return (
                <tr key={i} className="border-t border-matcha-border/50 hover:bg-matcha-bg-secondary/60">
                  <td className="px-4 py-2.5 font-mono text-xs">{row.gstin}</td>
                  <td className="px-4 py-2.5">{row.supplier}</td>
                  <td className="px-4 py-2.5 font-mono text-xs">{row.inv2b}</td>
                  <td className="px-4 py-2.5 font-mono text-xs">{row.invBooks}</td>
                  <td className="px-4 py-2.5 font-mono text-xs text-matcha-text-secondary">{row.voucher}</td>
                  <td className="px-4 py-2.5 font-mono text-xs text-matcha-text-secondary">{row.hsn}</td>
                  <td className="px-4 py-2.5">
                    {row.imsStatus !== "—" ? (
                      <span className={`text-xs px-2 py-0.5 rounded-full border font-medium ${
                        row.imsStatus === "Accepted"
                          ? "bg-matcha-accent/10 border-matcha-accent/30 text-matcha-accent"
                          : row.imsStatus === "Rejected"
                          ? "bg-matcha-red/10 border-matcha-red/30 text-matcha-red"
                          : "bg-matcha-bg border-matcha-border text-matcha-text-tertiary"
                      }`}>{row.imsStatus}</span>
                    ) : <span className="text-matcha-text-tertiary text-xs">—</span>}
                  </td>
                  <td className="px-4 py-2.5 text-right font-mono text-xs">{amount(row.taxable2b)}</td>
                  <td className="px-4 py-2.5 text-right font-mono text-xs">{amount(row.tax2b)}</td>
                  <td className="px-4 py-2.5 text-right font-mono text-xs">{amount(row.taxableBooks)}</td>
                  <td className="px-4 py-2.5 text-right font-mono text-xs">{amount(row.taxBooks)}</td>
                  <td className="px-4 py-2.5 text-right font-mono text-xs">{diff(row.diff)}</td>
                  <td className="px-4 py-2.5 text-right font-mono text-xs">{inrFormat(row.itc)}</td>
                  <td className="px-4 py-2.5 text-xs">{row.reason}</td>
                  <td className="px-4 py-2.5"><StatusPill status={row.status} /></td>
                  <td className="px-4 py-2.5">
                    {graphowlUrl && (
                      <a
                        href={graphowlUrl}
                        target="_blank"
                        rel="noopener noreferrer"
                        title="Open in GraphOWL"
                        className="inline-flex items-center gap-1 text-matcha-accent hover:text-matcha-accent/80"
                      >
                        <ExternalLink size={14} />
                      </a>
                    )}
                  </td>
                </tr>
                );
              })}
              {pageRows.length === 0 && (
                <tr>
                  <td colSpan={16} className="px-4 py-10 text-center text-matcha-text-tertiary">
                    No {filter === "all" ? "" : statusLabel(filter).toLowerCase()} invoices found.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

        <div className="flex flex-wrap items-center justify-between px-4 py-3 border-t border-matcha-border">
          <div className="flex items-center gap-2 text-sm text-matcha-text-tertiary">
            <span>Rows per page:</span>
            {PAGE_SIZES.map((size) => (
              <button
                key={size}
                onClick={() => {
                  setPageSize(size);
                  setPage(0);
                }}
                className={[
                  "px-2 py-0.5 rounded text-xs",
                  pageSize === size ? "text-matcha-accent bg-matcha-accent-surface" : "hover:text-matcha-text-primary",
                ].join(" ")}
              >
                {size}
              </button>
            ))}
            <span className="ml-4">
              {visible.length === 0 ? "0–0" : `${safePage * pageSize + 1}–${Math.min(visible.length, (safePage + 1) * pageSize)}`} of {visible.length}
            </span>
          </div>
          <div className="flex items-center gap-1">
            <Button variant="ghost" className="!px-2" disabled={safePage === 0} onClick={() => setPage(safePage - 1)}>
              <ChevronLeft size={16} /> Prev
            </Button>
            <Button variant="ghost" className="!px-2" disabled={safePage >= pageCount - 1} onClick={() => setPage(safePage + 1)}>
              Next <ChevronRight size={16} />
            </Button>
          </div>
        </div>
      </Card>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <Card className="p-5">
          <h3 className="font-semibold flex items-center gap-2">
            <AlertTriangle size={16} className="text-matcha-amber" />
            ITC Reversals — Section 17(5), Rule 42/43 reversals
          </h3>
          <p className="text-sm text-matcha-text-tertiary mt-3">
            {reversals === 0 ? "None recorded" : `${reversals} reversals recorded`}
          </p>
        </Card>

        <Card className="p-5">
          <h3 className="font-semibold">Net ITC for GSTR-3B Table 4</h3>
          <p className="text-3xl font-bold mt-3 font-mono text-matcha-accent">{inrFormat(netItc)}</p>
          <div className="mt-4 space-y-2 text-sm">
            <div className="flex justify-between text-matcha-text-secondary">
              <span>Gross ITC (matched invoices)</span>
              <span className="font-mono">{inrFormat(stats.gross_itc)}</span>
            </div>
            <div className="flex justify-between text-matcha-text-secondary">
              <span>Reversals</span>
              <span className="font-mono">{inrFormat(reversals)}</span>
            </div>
            <div className="flex justify-between font-medium border-t border-matcha-border pt-2">
              <span>Net Eligible ITC (GSTR-3B Table 4)</span>
              <span className="font-mono">{inrFormat(netItc)}</span>
            </div>
          </div>
        </Card>
      </div>

      <div className="text-sm text-matcha-text-tertiary">
        ITC pending (supplier not filed): <span className="font-mono text-matcha-amber">{inrFormat(stats.at_risk_itc)}</span> — claimable in future periods once supplier files.
      </div>

      <div className="flex items-center gap-3 pt-2">
        <Button variant="outline" onClick={onMapping}>
          <ArrowLeft size={16} /> Back to Mapping
        </Button>
        <Button onClick={onIntelligence}>
          Intelligence & Filing <ArrowRight size={16} />
        </Button>
      </div>
    </div>
  );
}
