import { useState } from "react";

interface ExportFormat {
  readonly id: string;
  readonly name: string;
  readonly description: string;
  readonly icon: string;
}

const EXPORT_FORMATS: readonly ExportFormat[] = [
  { id: "csv", name: "CSV", description: "Comma-separated values for spreadsheets and BI tools", icon: "📄" },
  { id: "json", name: "JSON-LD", description: "Linked Data format for graph-aware applications", icon: "🔗" },
  { id: "rdf", name: "RDF (Turtle)", description: "W3C standard ontology serialization", icon: "🐢" },
  { id: "skos", name: "SKOS", description: "Simple Knowledge Organization System for concept schemes", icon: "📚" },
  { id: "xlsx", name: "Excel", description: "Native spreadsheet format with formatting preserved", icon: "📊" },
];

interface ExportJob {
  readonly id: string;
  readonly format: string;
  readonly startedAt: string;
  readonly status: "running" | "complete" | "failed";
  readonly recordCount?: number;
  readonly fileSize?: string;
}

const MOCK_JOBS: readonly ExportJob[] = [
  { id: "exp-001", format: "CSV", startedAt: "12 min ago", status: "complete", recordCount: 268, fileSize: "14.2 KB" },
  { id: "exp-002", format: "SKOS", startedAt: "3 hours ago", status: "complete", recordCount: 268, fileSize: "42.1 KB" },
];

const STATUS_STYLES: Record<ExportJob["status"], { bg: string; text: string }> = {
  running: { bg: "bg-gowl-amber-bg", text: "text-gowl-amber" },
  complete: { bg: "bg-gowl-ok-bg", text: "text-gowl-ok" },
  failed: { bg: "bg-gowl-bad-bg", text: "text-gowl-bad" },
};

export function ExportTab({ glossaryId: _glossaryId }: { readonly glossaryId: string }) {
  const [selected, setSelected] = useState<string>("csv");
  const [jobs, setJobs] = useState<readonly ExportJob[]>(MOCK_JOBS);

  const startExport = () => {
    const newJob: ExportJob = {
      id: `exp-${String(jobs.length + 1).padStart(3, "0")}`,
      format: EXPORT_FORMATS.find((f) => f.id === selected)?.name ?? selected,
      startedAt: "just now",
      status: "running",
    };
    setJobs([newJob, ...jobs]);
  };

  return (
    <div>
      <div className="mb-6 grid grid-cols-5 gap-3">
        {EXPORT_FORMATS.map((fmt) => (
          <button
            key={fmt.id}
            type="button"
            onClick={() => setSelected(fmt.id)}
            className={`rounded-md border p-4 text-left transition-colors ${
              selected === fmt.id
                ? "border-gowl-accent-border bg-gowl-accent-deep"
                : "border-gowl-line bg-gowl-panel hover:border-gowl-line-3"
            }`}
          >
            <div className="mb-1 text-[18px]">{fmt.icon}</div>
            <div className="text-[12.5px] font-semibold text-gowl-t1">{fmt.name}</div>
            <div className="mt-0.5 text-[10.5px] text-gowl-t5">{fmt.description}</div>
          </button>
        ))}
      </div>

      <div className="mb-6 flex items-center gap-3">
        <button
          type="button"
          onClick={startExport}
          className="rounded-md bg-gowl-accent px-5 py-2 text-[12.5px] font-semibold text-gowl-accent-on"
        >
          Export as {EXPORT_FORMATS.find((f) => f.id === selected)?.name}
        </button>
        <span className="text-[11.5px] text-gowl-t5">
          268 terms · includes definitions, domain mappings, and cross-references
        </span>
      </div>

      <div className="rounded-md border border-gowl-line bg-gowl-panel">
        <div className="border-b border-gowl-line px-4 py-2.5 font-mono text-[9px] tracking-widest text-gowl-t6">
          EXPORT HISTORY
        </div>
        {jobs.length === 0 ? (
          <div className="p-8 text-center text-[12.5px] text-gowl-t5">No exports yet.</div>
        ) : (
          <div className="divide-y divide-gowl-row">
            {jobs.map((job) => {
              const ss = STATUS_STYLES[job.status];
              return (
                <div key={job.id} className="flex items-center justify-between px-4 py-3">
                  <div className="flex items-center gap-3">
                    <span className="font-mono text-[11px] text-gowl-t4">{job.format}</span>
                    <span className={`rounded-full px-2 py-0.5 font-mono text-[8.5px] ${ss.bg} ${ss.text}`}>
                      {job.status.toUpperCase()}
                    </span>
                  </div>
                  <div className="flex items-center gap-4 text-[11px] text-gowl-t5">
                    {job.recordCount !== undefined && <span>{job.recordCount} records</span>}
                    {job.fileSize !== undefined && <span>{job.fileSize}</span>}
                    <span>{job.startedAt}</span>
                    {job.status === "complete" && (
                      <button type="button" className="text-gowl-accent">Download</button>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
