import { useState } from "react";
import {
  TrendingUp,
  AlertTriangle,
  ArrowRight,
  ArrowLeft,
  Scale,
  FileText,
  Wand2,
  Upload,
} from "lucide-react";
import { Button, Card, Tabs } from "../components/ui.jsx";
import { inrFormat } from "../format.js";

function MatchBar({ label, value, total, color, barColor }) {
  const pct = total ? Math.round((value / total) * 100) : 0;
  return (
    <div className="flex items-center gap-3">
      <span className="w-28 text-sm text-matcha-text-secondary shrink-0">{label}</span>
      <div className="flex-1 h-6 bg-matcha-bg rounded-md overflow-hidden">
        <div
          className={`h-full ${barColor} rounded-md flex items-center px-2 text-xs text-matcha-bg font-semibold`}
          style={{ width: `${Math.max(pct, 2)}%` }}
        >
          {pct > 0 ? pct + "%" : ""}
        </div>
      </div>
      <span className="w-16 text-right text-sm font-mono text-matcha-text-primary shrink-0">{value}</span>
    </div>
  );
}

function WorkingPaperWizard({ stats }) {
  const [step, setStep] = useState(0);
  const [answers, setAnswers] = useState({});

  const auto = {
    a5: inrFormat(stats.confirmed_itc),
    excess: stats.only_gstr2b ?? 0,
    net: inrFormat((stats.gross_itc ?? 0)),
  };

  if (step === 0) {
    return (
      <Card className="p-6">
        <div className="flex items-center gap-2 mb-4">
          <Wand2 size={18} className="text-matcha-accent" />
          <h3 className="font-semibold">GSTR-3B Working Paper Wizard</h3>
        </div>
        <p className="text-sm text-matcha-text-secondary mb-5">
          AI will walk you through each section of Table 4, asking what it needs. Upload supporting documents inline. Live table updates as you answer.
        </p>
        <div className="bg-matcha-bg border border-matcha-border rounded-lg p-4 text-sm text-matcha-text-primary leading-relaxed">
          <p className="font-medium text-matcha-text-secondary mb-2">Intelligence</p>
          I've analyzed your reconciliation data. Here's what I can auto-compute:
          <ul className="mt-2 space-y-1 text-matcha-text-secondary">
            <li>• 4A5 Part A (GSTR-2B matched): <span className="font-mono text-matcha-accent">{auto.a5}</span> from {stats.matched} matched invoices</li>
            <li>• 4B2 Excess (in 2B, not in books): <span className="font-mono">{auto.excess}</span> invoices to reverse</li>
            <li>• 4C Net ITC will be calculated automatically</li>
          </ul>
        </div>
        <div className="mt-4 bg-matcha-bg border border-matcha-border rounded-lg p-4">
          <div className="flex items-center justify-between mb-2">
            <span className="text-sm font-medium">4A5 · Section 16(4)</span>
          </div>
          <p className="text-sm text-matcha-text-secondary">
            Do you have ITC from invoices that appeared in a previous period's GSTR-2B but weren't claimed then?
          </p>
          <p className="text-xs text-matcha-text-tertiary mt-2">
            Check previous months' GSTR-2B downloads.
          </p>
          <div className="flex items-center gap-3 mt-4">
            <Button variant="outline" onClick={() => setStep(1)}>No</Button>
            <Button onClick={() => setStep(1)}>Yes — upload previous 2B</Button>
          </div>
        </div>
      </Card>
    );
  }

  return (
    <Card className="p-6">
      <div className="flex items-center gap-2 mb-4">
        <Wand2 size={18} className="text-matcha-accent" />
        <h3 className="font-semibold">GSTR-3B Working Paper</h3>
      </div>
      <div className="bg-matcha-bg border border-matcha-border rounded-lg p-4 text-sm text-matcha-text-primary leading-relaxed">
        <p className="font-medium text-matcha-text-secondary mb-2">Intelligence</p>
        {answers["a5"] === "yes" ? (
          <p>Uploaded prior-period GSTR-2B. Excess ITC will be added to 4A5 when you confirm the file.</p>
        ) : (
          <p>No prior-period ITC selected. Proceeding with current-period figures.</p>
        )}
      </div>
      <div className="mt-4 space-y-3">
        <div className="flex items-center justify-between text-sm">
          <span className="text-matcha-text-secondary">4A5 Part A — eligible ITC</span>
          <span className="font-mono text-matcha-accent">{auto.a5}</span>
        </div>
        <div className="flex items-center justify-between text-sm">
          <span className="text-matcha-text-secondary">4B2 Excess — reverse</span>
          <span className="font-mono text-matcha-amber">{auto.excess}</span>
        </div>
        <div className="flex items-center justify-between text-sm font-medium border-t border-matcha-border pt-2">
          <span>4C Net ITC</span>
          <span className="font-mono text-matcha-accent">{auto.net}</span>
        </div>
      </div>
      <div className="flex items-center gap-3 mt-5">
        <Button variant="outline" onClick={() => setStep(0)}>Back</Button>
        <Button
          onClick={() =>
            setAnswers((prev) => ({ ...prev, done: true }))
          }
        >
          <FileText size={16} /> Generate Working Paper
        </Button>
      </div>
    </Card>
  );
}

export default function IntelligencePage({ overview, onBack, onAct }) {
  const [tab, setTab] = useState("mismatches");
  const stats = overview.stats || {};
  const classifications = overview.classifications || [];
  const supplierHealth = overview.supplier_health || [];
  const total = stats.total || 0;

  const nonFiling = classifications.find((c) => c.key === "supplier_non_filing");
  const discrepancy = classifications.find((c) => c.key === "amount_discrepancy");
  const itcAtRisk = (nonFiling?.itc ?? 0) + (discrepancy?.itc ?? 0);

  const distribution = [
    { label: "Matched", value: stats.matched ?? 0, barColor: "bg-matcha-accent" },
    { label: "Mismatched", value: stats.review ?? 0, barColor: "bg-matcha-amber" },
    { label: "Only Books", value: stats.only_books ?? 0, barColor: "bg-matcha-red" },
    { label: "Only Portal", value: stats.only_gstr2b ?? 0, barColor: "bg-matcha-blue" },
  ];

  return (
    <div className="space-y-6">
      <h1 className="text-3xl font-bold tracking-tight">Intelligence</h1>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <Card className="p-5">
          <h3 className="font-semibold text-matcha-text-secondary">Overall Match Rate</h3>
          <p className="text-3xl font-bold mt-3 font-mono text-matcha-accent">{stats.match_rate}%</p>
        </Card>
        <Card className="p-5">
          <h3 className="font-semibold text-matcha-text-secondary">Total Invoices</h3>
          <p className="text-3xl font-bold mt-3 font-mono">{total}</p>
        </Card>
        <Card className="p-5">
          <h3 className="font-semibold text-matcha-text-secondary">ITC at Risk</h3>
          <p className="text-3xl font-bold mt-3 font-mono text-matcha-amber">{inrFormat(itcAtRisk)}</p>
        </Card>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        {[
          { label: "Matched", value: stats.matched },
          { label: "Mismatch", value: stats.review },
          { label: "Not in Portal", value: stats.only_books },
          { label: "Not in Books", value: stats.only_gstr2b },
        ].map((chip) => (
          <span
            key={chip.label}
            className="px-3 py-1.5 rounded-full border border-matcha-border text-sm text-matcha-text-secondary"
          >
            {chip.label} <span className="font-semibold text-matcha-text-primary">{chip.value}</span>
          </span>
        ))}
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <Card className="p-5">
          <div className="flex items-center gap-2 mb-4">
            <TrendingUp size={16} className="text-matcha-accent" />
            <h3 className="font-semibold">Match Distribution</h3>
          </div>
          <div className="flex items-center justify-between mb-3">
            <span className="text-sm text-matcha-text-tertiary">Total</span>
            <span className="font-mono text-sm">{total}</span>
          </div>
          <div className="space-y-3">
            {distribution.map((d) => (
              <MatchBar key={d.label} {...d} total={total} />
            ))}
          </div>
        </Card>

        <Card className="p-5">
          <div className="flex items-center gap-2 mb-4">
            <AlertTriangle size={16} className="text-matcha-amber" />
            <h3 className="font-semibold">ITC at Risk ({inrFormat(itcAtRisk)})</h3>
          </div>
          <div className="space-y-3">
            <div className="flex items-center justify-between text-sm">
              <span className="text-matcha-text-secondary">Supplier Non-Filing</span>
              <span className="font-mono text-matcha-red">{inrFormat(nonFiling?.itc ?? 0)}</span>
            </div>
            <div className="flex items-center justify-between text-sm">
              <span className="text-matcha-text-secondary">Amount Discrepancy</span>
              <span className="font-mono text-matcha-amber">{inrFormat(discrepancy?.itc ?? 0)}</span>
            </div>
          </div>
        </Card>
      </div>

      <Tabs
        tabs={[
          { key: "mismatches", label: "Mismatches" },
          { key: "supplier_health", label: "Supplier Health" },
          { key: "working_paper", label: "ITC Working Paper" },
        ]}
        active={tab}
        onChange={setTab}
      />

      {tab === "mismatches" && (
        <div>
          <h2 className="text-xl font-semibold mb-4">Mismatch Classification</h2>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            {classifications.map((item) => (
              <Card key={item.key} className="p-5">
                <div className="flex items-center gap-2 mb-2">
                  <Scale size={16} className="text-matcha-accent" />
                  <h3 className="font-semibold">{item.title}</h3>
                </div>
                <div className="flex items-baseline gap-2">
                  <span className="text-2xl font-bold font-mono">{item.count}</span>
                  <span className="font-mono text-matcha-amber">{inrFormat(item.itc)}</span>
                </div>
                <button className="mt-3 text-xs text-matcha-text-tertiary bg-matcha-bg border border-matcha-border rounded-md px-2.5 py-1">
                  {item.reference}
                </button>
                <p className="text-sm text-matcha-text-secondary mt-3">{item.action}</p>
              </Card>
            ))}
          </div>
        </div>
      )}

      {tab === "supplier_health" && (
        <Card className="overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="bg-matcha-bg text-left text-xs text-matcha-text-tertiary uppercase tracking-wide">
                <th className="px-4 py-2.5 font-medium">Supplier</th>
                <th className="px-4 py-2.5 font-medium">GSTIN</th>
                <th className="px-4 py-2.5 font-medium text-right">Filing (6mo)</th>
                <th className="px-4 py-2.5 font-medium text-right">ITC Blocked</th>
                <th className="px-4 py-2.5 font-medium">Risk</th>
              </tr>
            </thead>
            <tbody>
              {supplierHealth.map((s) => (
                <tr key={s.gstin} className="border-t border-matcha-border/50">
                  <td className="px-4 py-2.5">{s.supplier}</td>
                  <td className="px-4 py-2.5 font-mono text-xs">{s.gstin}</td>
                  <td className="px-4 py-2.5 text-right font-mono text-xs text-matcha-text-tertiary">{s.filing_6mo || "—"}</td>
                  <td className="px-4 py-2.5 text-right font-mono">{inrFormat(s.itc)}</td>
                  <td className="px-4 py-2.5">
                    <span className="inline-flex items-center gap-1 text-xs text-matcha-red bg-matcha-red/10 border border-matcha-red/30 rounded-full px-2.5 py-0.5">
                      <AlertTriangle size={12} /> {s.risk}
                    </span>
                  </td>
                </tr>
              ))}
              {supplierHealth.length === 0 && (
                <tr>
                  <td colSpan={5} className="px-4 py-8 text-center text-matcha-text-tertiary">
                    No suppliers flagged.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </Card>
      )}

      {tab === "working_paper" && <WorkingPaperWizard stats={stats} />}

      <div className="flex items-center gap-3 pt-2">
        <Button variant="outline" onClick={onBack}>
          <ArrowLeft size={16} /> Back
        </Button>
        <Button onClick={onAct}>
          Proceed to Actions <ArrowRight size={16} />
        </Button>
      </div>
    </div>
  );
}
