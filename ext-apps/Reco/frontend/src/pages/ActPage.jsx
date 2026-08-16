import { useRef, useState } from "react";
import {
  Download,
  MessageSquare,
  RefreshCw,
  AlertTriangle,
  FileText,
  ArrowLeft,
  Copy,
  Check,
  Landmark,
  Loader2,
  Sparkles,
  Printer,
  RotateCcw,
  ClipboardCheck,
} from "lucide-react";
import { Button, Card, Tabs } from "../components/ui.jsx";
import { api, pollJob } from "../api.js";
import { inrFormat } from "../format.js";

// ─── Rich Markdown renderer ────────────────────────────────────────────────────
function Markdown({ text }) {
  const lines = text.split("\n");
  const elements = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];

    // Horizontal rule
    if (/^---+$/.test(line.trim())) {
      elements.push(<hr key={i} className="border-matcha-border my-4" />);
      i++;
      continue;
    }
    // H1
    if (line.startsWith("# ")) {
      elements.push(
        <h2 key={i} className="text-2xl font-bold text-matcha-text-primary mt-6 mb-2">
          {renderInline(line.slice(2))}
        </h2>
      );
      i++;
      continue;
    }
    // H2
    if (line.startsWith("## ")) {
      elements.push(
        <h3 key={i} className="text-lg font-semibold text-matcha-accent mt-5 mb-2">
          {renderInline(line.slice(3))}
        </h3>
      );
      i++;
      continue;
    }
    // H3
    if (line.startsWith("### ")) {
      elements.push(
        <h4 key={i} className="text-base font-semibold text-matcha-text-primary mt-4 mb-1">
          {renderInline(line.slice(4))}
        </h4>
      );
      i++;
      continue;
    }
    // Numbered list
    if (/^\d+\.\s/.test(line.trim())) {
      const items = [];
      while (i < lines.length && /^\d+\.\s/.test(lines[i].trim())) {
        items.push(lines[i].replace(/^\d+\.\s/, ""));
        i++;
      }
      elements.push(
        <ol key={`ol-${i}`} className="list-decimal list-inside space-y-1 my-2">
          {items.map((item, j) => (
            <li key={j} className="text-sm text-matcha-text-secondary leading-relaxed pl-2">
              {renderInline(item)}
            </li>
          ))}
        </ol>
      );
      continue;
    }
    // Bullet list
    if (line.trim().startsWith("- ") || line.trim().startsWith("* ")) {
      const items = [];
      while (i < lines.length && (lines[i].trim().startsWith("- ") || lines[i].trim().startsWith("* "))) {
        items.push(lines[i].replace(/^[\s]*[-*]\s/, ""));
        i++;
      }
      elements.push(
        <ul key={`ul-${i}`} className="space-y-1 my-2">
          {items.map((item, j) => (
            <li key={j} className="flex gap-2 text-sm text-matcha-text-secondary leading-relaxed">
              <span className="text-matcha-accent shrink-0 mt-0.5">•</span>
              <span>{renderInline(item)}</span>
            </li>
          ))}
        </ul>
      );
      continue;
    }
    // Blank line
    if (line.trim() === "") {
      elements.push(<div key={i} className="h-2" />);
      i++;
      continue;
    }
    // Normal paragraph
    elements.push(
      <p key={i} className="text-sm text-matcha-text-secondary leading-relaxed">
        {renderInline(line)}
      </p>
    );
    i++;
  }

  return <div className="space-y-1">{elements}</div>;
}

function renderInline(text) {
  // Handle **bold** and `code`
  const parts = text.split(/(\*\*[^*]+\*\*|`[^`]+`)/g);
  return parts.map((part, i) => {
    if (part.startsWith("**") && part.endsWith("**")) {
      return (
        <strong key={i} className="text-matcha-text-primary font-semibold">
          {part.slice(2, -2)}
        </strong>
      );
    }
    if (part.startsWith("`") && part.endsWith("`")) {
      return (
        <code key={i} className="font-mono text-matcha-accent bg-matcha-bg px-1 rounded text-xs">
          {part.slice(1, -1)}
        </code>
      );
    }
    return <span key={i}>{part}</span>;
  });
}

function InfoBox({ title, children }) {
  return (
    <div className="bg-matcha-bg border border-matcha-border rounded-lg p-4 text-sm leading-relaxed">
      <p className="font-medium text-matcha-text-primary mb-1.5">{title}</p>
      <div className="text-matcha-text-secondary">{children}</div>
    </div>
  );
}

// ─── Report Download helpers ──────────────────────────────────────────────────
function downloadMarkdown(text, period) {
  const blob = new Blob([text], { type: "text/markdown;charset=utf-8;" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `reconow-report-${period.month}-${period.year}.md`;
  a.click();
  URL.revokeObjectURL(url);
}

function downloadTextAsHtml(text, period) {
  // Convert markdown to basic HTML for printing
  const html = text
    .replace(/^# (.+)$/gm, "<h1>$1</h1>")
    .replace(/^## (.+)$/gm, "<h2>$1</h2>")
    .replace(/^### (.+)$/gm, "<h3>$1</h3>")
    .replace(/^\d+\. (.+)$/gm, "<li>$1</li>")
    .replace(/^- (.+)$/gm, "<li>$1</li>")
    .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
    .replace(/`(.+?)`/g, "<code>$1</code>")
    .replace(/^---+$/gm, "<hr>")
    .replace(/\n\n/g, "</p><p>")
    .replace(/\n/g, "<br>");

  const win = window.open("", "_blank");
  win.document.write(`
    <!DOCTYPE html><html><head>
    <title>RecoNow Report — ${period.month} ${period.year}</title>
    <style>
      body { font-family: 'Georgia', serif; max-width: 800px; margin: 40px auto; padding: 20px; color: #1a1a1a; line-height: 1.7; }
      h1 { font-size: 1.8em; border-bottom: 2px solid #4ade80; padding-bottom: 10px; }
      h2 { font-size: 1.3em; color: #166534; margin-top: 2em; }
      h3 { font-size: 1.1em; margin-top: 1.5em; }
      li { margin: 4px 0; }
      code { background: #f0fdf4; padding: 2px 6px; border-radius: 4px; font-family: monospace; font-size: 0.9em; }
      strong { font-weight: 700; }
      hr { border: none; border-top: 1px solid #ccc; margin: 2em 0; }
      @media print { body { margin: 20px; } }
    </style>
    </head><body>
    <p style="color:#888;font-size:0.85em">Generated by RecoNow · ${period.month} ${period.year}</p>
    <p>${html}</p>
    </body></html>
  `);
  win.document.close();
  win.focus();
  setTimeout(() => win.print(), 500);
}

// ─── Main Component ───────────────────────────────────────────────────────────
export default function ActPage({ overview, onBack, onRestart }) {
  const [tab, setTab] = useState("working_paper");
  const stats = overview.stats || {};
  const [messages, setMessages] = useState(null);
  const [generating, setGenerating] = useState(false);
  const [progress, setProgress] = useState(null);
  const [report, setReport] = useState(null);
  const [reporting, setReporting] = useState(false);
  const [summary, setSummary] = useState(null);
  const [summarizing, setSummarizing] = useState(false);
  const [copied, setCopied] = useState(null);
  const [reportCopied, setReportCopied] = useState(false);
  const period = overview.period || { month: "March", year: 2026 };
  const imsActions = overview.ims_actions || [];
  const deadlineYear = period.year + 1;

  const generateMessages = async () => {
    setGenerating(true);
    setProgress(null);
    try {
      const started = await api.followUps();
      const result = await pollJob(started.job_id, {
        interval: 3000,
        onProgress: (job) => setProgress({ done: job.done, total: job.total }),
      });
      setMessages(result);
    } finally {
      setGenerating(false);
      setProgress(null);
    }
  };

  const generateReport = async () => {
    setReporting(true);
    try {
      const started = await api.report();
      const result = await pollJob(started.job_id, { interval: 3000 });
      setReport(result);
    } finally {
      setReporting(false);
    }
  };

  const generateSummary = async () => {
    setSummarizing(true);
    try {
      const started = await api.aiSummary();
      const result = await pollJob(started.job_id, { interval: 2000 });
      setSummary(result);
    } finally {
      setSummarizing(false);
    }
  };

  const copyMessage = async (index, text) => {
    await navigator.clipboard.writeText(text);
    setCopied(index);
    setTimeout(() => setCopied(null), 1500);
  };

  const copyReport = async () => {
    await navigator.clipboard.writeText(report);
    setReportCopied(true);
    setTimeout(() => setReportCopied(false), 2000);
  };

  return (
    <div className="space-y-6">
      <h1 className="text-3xl font-bold tracking-tight">Act on Results</h1>

      <Tabs
        tabs={[
          { key: "working_paper", label: "Export Working Paper" },
          { key: "follow_ups", label: "Supplier Follow-Ups" },
          { key: "ims", label: "IMS Actions" },
          { key: "report", label: "Client Report" },
          { key: "new_session", label: "New Session" },
        ]}
        active={tab}
        onChange={setTab}
      />

      {/* ── Working Paper ──────────────────────────────────────── */}
      {tab === "working_paper" && (
        <Card className="p-6 max-w-2xl">
          <div className="flex items-center gap-2 mb-3">
            <FileText size={18} className="text-matcha-accent" />
            <h3 className="text-xl font-semibold">CA-Ready Working Paper</h3>
          </div>
          <p className="text-sm text-matcha-text-secondary mb-6">
            All reconciliation data as structured exports. Ready for CA review.
          </p>

          {/* Stats summary */}
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 mb-6">
            {[
              { label: "Matched", value: stats.matched, color: "text-matcha-accent" },
              { label: "Mismatched", value: stats.review, color: "text-matcha-amber" },
              { label: "Only Books", value: stats.only_books, color: "text-matcha-red" },
              { label: "Only Portal", value: stats.only_gstr2b, color: "text-matcha-blue" },
            ].map((s) => (
              <div key={s.label} className="bg-matcha-bg rounded-lg p-3 text-center">
                <p className={`text-xl font-bold font-mono ${s.color}`}>{s.value ?? 0}</p>
                <p className="text-xs text-matcha-text-tertiary mt-1">{s.label}</p>
              </div>
            ))}
          </div>

          <div className="flex flex-wrap items-center gap-3 mb-6">
            <Button onClick={() => api.download("/api/export/csv")}>
              <Download size={16} /> Download CSV
            </Button>
            <Button variant="outline" onClick={() => api.download("/api/export/working-paper.xlsx")}>
              <FileText size={16} /> Working Paper (.xlsx)
            </Button>
            <Button variant="outline" onClick={() => api.download("/api/export/itc-register.xlsx")}>
              <ClipboardCheck size={16} /> ITC Register (.xlsx)
            </Button>
          </div>

          {/* AI Summary */}
          {!summary && (
            <Button variant="outline" onClick={generateSummary} disabled={summarizing}>
              {summarizing ? <Loader2 size={16} className="animate-spin" /> : <Sparkles size={16} />}
              {summarizing ? "Generating AI Summary…" : "Generate AI Summary"}
            </Button>
          )}
          {summary && (
            <div className="bg-matcha-bg border border-matcha-border rounded-lg p-4 mt-3">
              <p className="text-xs uppercase tracking-wider text-matcha-text-tertiary mb-2 flex items-center gap-1.5">
                <Sparkles size={12} className="text-matcha-accent" /> AI Summary
              </p>
              <p className="text-sm text-matcha-text-secondary leading-relaxed">{summary}</p>
              <button
                onClick={generateSummary}
                className="mt-3 text-xs text-matcha-text-tertiary hover:text-matcha-text-primary flex items-center gap-1"
              >
                <RotateCcw size={11} /> Regenerate
              </button>
            </div>
          )}
        </Card>
      )}

      {/* ── Supplier Follow-Ups ───────────────────────────────── */}
      {tab === "follow_ups" && (
        <Card className="p-6 max-w-3xl">
          <div className="flex items-center gap-2 mb-3">
            <MessageSquare size={18} className="text-matcha-accent" />
            <h3 className="text-xl font-semibold">Supplier Follow-Up Messages</h3>
          </div>
          <p className="text-sm text-matcha-text-secondary mb-6">
            AI drafts professional follow-up emails for{" "}
            <span className="text-matcha-red font-semibold">{stats.only_books}</span> suppliers
            whose invoices are missing from GSTR-2B. Each email cites Section 16(2)(aa) CGST Act.
          </p>
          {!messages && (
            <div>
              <Button onClick={generateMessages} disabled={generating}>
                {generating ? <Loader2 size={16} className="animate-spin" /> : <Sparkles size={16} />}
                {generating ? "Drafting messages with AI…" : "Generate Follow-Up Messages"}
              </Button>
              {generating && progress && (
                <div className="mt-4">
                  <div className="flex items-center justify-between text-xs text-matcha-text-tertiary mb-1">
                    <span>Drafting {progress.done} of {progress.total} messages…</span>
                    <span>{Math.round((progress.done / progress.total) * 100)}%</span>
                  </div>
                  <div className="h-1.5 bg-matcha-bg rounded-full overflow-hidden">
                    <div
                      className="h-full bg-matcha-accent rounded-full transition-all"
                      style={{ width: `${(progress.done / progress.total) * 100}%` }}
                    />
                  </div>
                </div>
              )}
            </div>
          )}
          {messages && (
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <p className="text-sm text-matcha-text-tertiary">{messages.length} messages generated</p>
                <button
                  onClick={generateMessages}
                  className="text-xs text-matcha-text-tertiary hover:text-matcha-text-primary flex items-center gap-1"
                >
                  <RotateCcw size={11} /> Regenerate All
                </button>
              </div>
              {messages.map((m, i) => (
                <div key={i} className="bg-matcha-bg border border-matcha-border rounded-lg p-4">
                  <div className="flex items-center justify-between mb-3">
                    <div>
                      <p className="font-medium">{m.supplier}</p>
                      <p className="text-xs font-mono text-matcha-text-tertiary">
                        {m.gstin} · {m.invoice_no} · {inrFormat(m.itc)}
                      </p>
                    </div>
                    <Button
                      variant="outline"
                      className="!px-3 !py-1.5"
                      onClick={() => copyMessage(i, m.message)}
                    >
                      {copied === i ? (
                        <Check size={14} className="text-matcha-accent" />
                      ) : (
                        <Copy size={14} />
                      )}
                      {copied === i ? "Copied!" : "Copy"}
                    </Button>
                  </div>
                  <pre className="whitespace-pre-wrap text-xs text-matcha-text-secondary font-sans leading-relaxed">
                    {m.message}
                  </pre>
                </div>
              ))}
            </div>
          )}
        </Card>
      )}

      {/* ── IMS Actions ──────────────────────────────────────── */}
      {tab === "ims" && (
        <div className="max-w-3xl space-y-4">
          <div className="flex items-center gap-2 mb-2">
            <Landmark size={18} className="text-matcha-accent" />
            <h3 className="text-xl font-semibold">IMS Action Recommendations</h3>
          </div>
          <p className="text-sm text-matcha-text-secondary">
            Based on reconciliation results — what to do on the GST portal IMS dashboard
          </p>

          <InfoBox title="Section 16(4) Deadline">
            ITC for {period.month} {period.year} invoices must be claimed by 30 November{" "}
            {deadlineYear} or the annual return filing date, whichever is earlier. Invoices kept
            pending in IMS beyond this date cannot be claimed.
          </InfoBox>

          <InfoBox title="IMS Rules">
            No action = Deemed Accepted (ITC flows automatically). Rejection is reversible before
            filing GSTR-3B. After filing, supplier can re-furnish via GSTR-1A. Credit notes can be
            kept pending for one tax period only (from Oct 2025). Draft GSTR-2B generates on the
            14th — actions can be changed after but must recompute.
          </InfoBox>

          {imsActions.map((action) => (
            <Card key={action.key} className="p-5">
              <div className="flex items-center justify-between mb-2">
                <h4 className="font-semibold">{action.title}</h4>
                <span className="text-sm font-mono text-matcha-text-tertiary">
                  {action.count} invoices · {inrFormat(action.itc)}
                </span>
              </div>
              <p className="text-sm text-matcha-text-secondary">{action.action}</p>
              <p className="text-sm text-matcha-accent mt-2">{action.note}</p>
              {action.invoices && action.invoices.length > 0 && (
                <div className="mt-3 flex flex-wrap gap-1">
                  {action.invoices.slice(0, 8).map((inv, j) => (
                    <span
                      key={j}
                      className="text-xs font-mono bg-matcha-bg border border-matcha-border rounded px-2 py-0.5 text-matcha-text-tertiary"
                    >
                      {inv}
                    </span>
                  ))}
                  {action.invoices.length > 8 && (
                    <span className="text-xs text-matcha-text-tertiary px-2 py-0.5">
                      +{action.invoices.length - 8} more
                    </span>
                  )}
                </div>
              )}
            </Card>
          ))}
        </div>
      )}

      {/* ── Client Report ─────────────────────────────────────── */}
      {tab === "report" && (
        <div className="max-w-3xl space-y-4">
          <Card className="p-6">
            <div className="flex items-center justify-between mb-3">
              <div className="flex items-center gap-2">
                <FileText size={18} className="text-matcha-accent" />
                <h3 className="text-xl font-semibold">Client Report</h3>
              </div>
              {report && (
                <div className="flex items-center gap-2">
                  <Button
                    variant="outline"
                    className="!px-3 !py-1.5 text-xs"
                    onClick={copyReport}
                  >
                    {reportCopied ? (
                      <Check size={13} className="text-matcha-accent" />
                    ) : (
                      <Copy size={13} />
                    )}
                    {reportCopied ? "Copied!" : "Copy"}
                  </Button>
                  <Button
                    variant="outline"
                    className="!px-3 !py-1.5 text-xs"
                    onClick={() => downloadMarkdown(report, period)}
                  >
                    <Download size={13} /> Download .md
                  </Button>
                  <Button
                    variant="outline"
                    className="!px-3 !py-1.5 text-xs"
                    onClick={() => downloadTextAsHtml(report, period)}
                  >
                    <Printer size={13} /> Print / PDF
                  </Button>
                  <button
                    onClick={generateReport}
                    className="text-xs text-matcha-text-tertiary hover:text-matcha-text-primary flex items-center gap-1 px-2"
                  >
                    <RotateCcw size={11} /> Regenerate
                  </button>
                </div>
              )}
            </div>

            {!report && (
              <>
                <p className="text-sm text-matcha-text-secondary mb-6">
                  AI will generate a professional, plain-language report summarizing your
                  reconciliation findings, risk assessment, and recommended actions. Ready to share
                  with your client.
                </p>
                <Button onClick={generateReport} disabled={reporting}>
                  {reporting ? (
                    <Loader2 size={16} className="animate-spin" />
                  ) : (
                    <Sparkles size={16} />
                  )}
                  {reporting ? "Generating with AI…" : "Generate Client Report"}
                </Button>
              </>
            )}

            {reporting && (
              <div className="mt-4 flex items-center gap-3 text-sm text-matcha-text-secondary">
                <Loader2 size={16} className="animate-spin text-matcha-accent" />
                AI is writing your report — this usually takes 15–30 seconds…
              </div>
            )}

            {report && (
              <div className="bg-matcha-bg border border-matcha-border rounded-lg p-5 mt-2">
                <Markdown text={report} />
              </div>
            )}
          </Card>

          {/* Also allow downloading from server */}
          {report && (
            <div className="flex items-center gap-2 text-sm text-matcha-text-tertiary">
              <span>Also available via API:</span>
              <button
                onClick={() => api.download("/api/export/report.md")}
                className="text-matcha-accent hover:underline text-xs font-mono"
              >
                GET /api/export/report.md
              </button>
            </div>
          )}
        </div>
      )}

      {/* ── New Session ───────────────────────────────────────── */}
      {tab === "new_session" && (
        <Card className="p-6 max-w-2xl">
          <div className="flex items-center gap-2 mb-3">
            <RefreshCw size={18} className="text-matcha-amber" />
            <h3 className="text-xl font-semibold">Start Fresh</h3>
          </div>
          <p className="text-sm text-matcha-text-secondary mb-6">
            Clear all data and start a new reconciliation for a different period or jurisdiction.
            This will reset everything — uploads, mappings, and results.
          </p>
          <Button variant="outline" onClick={onRestart}>
            <AlertTriangle size={16} /> Reset &amp; Start Over
          </Button>
        </Card>
      )}

      <div className="flex items-center gap-3 pt-2">
        <Button variant="outline" onClick={onBack}>
          <ArrowLeft size={16} /> Back to Intelligence
        </Button>
      </div>
    </div>
  );
}
