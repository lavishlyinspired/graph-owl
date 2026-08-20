import { useState } from "react";
import { strings } from "../lib/strings";

interface Dataset {
  readonly name: string;
  readonly meta: string;
  readonly mapped: number;
  readonly total: number;
}

interface MappingRow {
  readonly property: string;
  readonly className: string;
  readonly required: boolean;
  readonly sourceColumn: string;
  readonly sample: string;
  readonly confidence: number;
}

const DATASETS: readonly Dataset[] = [
  { name: "supplier_master.csv", meta: "27 columns · 8,412 rows", mapped: 24, total: 27 },
  { name: "gst_returns_jul.json", meta: "18 columns · 1,204 rows", mapped: 18, total: 18 },
  { name: "erp_invoices.xlsx", meta: "32 columns · 14,891 rows", mapped: 21, total: 32 },
];

const MAPPINGS: readonly MappingRow[] = [
  { property: "name", className: "Organization", required: true, sourceColumn: "supplier_name", sample: "Supplier ABC", confidence: 0.98 },
  { property: "pan", className: "Organization", required: true, sourceColumn: "pan_number", sample: "AABCU9603R", confidence: 0.97 },
  { property: "gstin", className: "Organization", required: false, sourceColumn: "gstin", sample: "27AABCU9603R1ZM", confidence: 0.99 },
  { property: "state", className: "Organization", required: true, sourceColumn: "supplier_state", sample: "Maharashtra", confidence: 0.84 },
  { property: "invoiceDate", className: "Invoice", required: true, sourceColumn: "inv_date", sample: "2026-07-15", confidence: 0.95 },
  { property: "totalAmount", className: "Invoice", required: true, sourceColumn: "total", sample: "₹42,000", confidence: 0.92 },
  { property: "igst", className: "Invoice", required: false, sourceColumn: "igst_amount", sample: "₹7,560", confidence: 0.88 },
];

const STEPS = [
  { key: "source", label: strings.pipelineStepSource, done: true },
  { key: "columns", label: strings.pipelineStepColumns, done: true },
  { key: "ontology", label: strings.pipelineStepOntology, done: false },
  { key: "rules", label: strings.pipelineStepRules, done: false },
  { key: "build", label: strings.pipelineStepBuild, done: false },
];

export default function PipelineRoute() {
  const [selectedDataset, setSelectedDataset] = useState(0);
  const ds: Dataset = DATASETS[selectedDataset] ?? DATASETS[0]!;

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Header with 5-step progress */}
      <div className="flex-none border-b border-gowl-line bg-gowl-panel px-6 py-4">
        <div className="mb-4 flex items-end justify-between">
          <div>
            <h1 className="mb-1 text-[21.5px] font-semibold text-gowl-t1">{strings.pipelineTitle}</h1>
            <p className="text-[14px] text-gowl-t5">{strings.pipelineSubtitle}</p>
          </div>
          <div className="flex gap-2">
            <button type="button" className="rounded-md border border-gowl-line-2 px-3 py-1.5 text-[13.5px] text-gowl-t3 hover:border-gowl-hover">
              {strings.pipelineAddSource}
            </button>
            <button type="button" className="rounded-md bg-gowl-accent px-3 py-1.5 text-[13.5px] font-semibold text-gowl-accent-on">
              {strings.pipelineBuild}
            </button>
          </div>
        </div>

        {/* 5-step progress bar */}
        <div className="flex items-start">
          {STEPS.map((step, i) => (
            <div key={step.key} className="flex flex-1 items-start">
              <div className="flex flex-col gap-1.5">
                <div className="flex items-center gap-2">
                  <span className={`flex h-[18px] w-[18px] flex-none items-center justify-center rounded-full border text-[10.5px] ${
                    step.done
                      ? "border-gowl-accent bg-gowl-accent text-gowl-accent-on"
                      : i === 2
                        ? "border-gowl-accent bg-gowl-accent-bg text-gowl-accent"
                        : "border-gowl-line-2 bg-gowl-panel-2 text-gowl-t6"
                  }`}>
                    {step.done ? "✓" : i + 1}
                  </span>
                  <span className={`whitespace-nowrap text-[14px] font-medium ${
                    step.done ? "text-gowl-accent" : i === 2 ? "text-gowl-accent" : "text-gowl-t5"
                  }`}>
                    {step.label}
                  </span>
                </div>
              </div>
              {i < STEPS.length - 1 && (
                <div className={`mt-2.5 mx-3 h-px flex-1 ${step.done ? "bg-gowl-accent" : "bg-gowl-line-2"}`} />
              )}
            </div>
          ))}
        </div>
      </div>

      {/* Content: sidebar + mapping table */}
      <div className="flex flex-1 min-h-0">
        {/* Dataset sidebar */}
        <div className="w-[236px] flex-none border-r border-gowl-line bg-gowl-panel p-3">
          <div className="mb-2.5 px-2 font-mono text-[10.5px] tracking-widest text-gowl-t7">{strings.pipelineDatasetsTitle}</div>
          {DATASETS.map((d, i) => (
            <button
              key={d.name}
              type="button"
              onClick={() => setSelectedDataset(i)}
              className={`mb-0.5 w-full rounded-md px-2.5 py-2.5 text-left ${
                selectedDataset === i ? "bg-gowl-panel-2 border-l-2 border-gowl-accent" : "border-l-2 border-transparent hover:bg-gowl-row"
              }`}
            >
              <div className="flex items-center gap-2">
                <span className={`text-[14px] ${selectedDataset === i ? "text-gowl-accent" : "text-gowl-t3"}`}>{d.name}</span>
                <span className={`ml-auto text-[11.5px] ${d.mapped === d.total ? "text-gowl-ok" : "text-gowl-amber"}`}>
                  {d.mapped}/{d.total}
                </span>
              </div>
              <div className="mt-0.5 font-mono text-[11px] text-gowl-t7">{d.meta}</div>
            </button>
          ))}
          <div className="mt-3.5 border-t border-gowl-line px-1 pt-3.5 text-[13px] leading-relaxed text-gowl-t6">
            {strings.pipelineDatasetNote}
          </div>
        </div>

        {/* Main content */}
        <div className="flex-1 min-w-0 overflow-auto p-5">
          {/* Dataset header */}
          <div className="mb-3.5 flex items-center gap-3">
            <span className="text-[15.5px] font-semibold text-gowl-t1">{ds.name}</span>
            <span className="font-mono text-[12px] text-gowl-t6">{ds.meta}</span>
            <button type="button" className="ml-auto rounded border border-gowl-accent-border bg-gowl-accent-bg px-2.5 py-1 text-[13px] text-gowl-accent">
              {strings.pipelineAutoMap}
            </button>
          </div>

          {/* Mapping table */}
          <div className="mb-3.5 overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
            <div className="grid grid-cols-[1.5fr_1.2fr_1.2fr_96px] gap-3 border-b border-gowl-line bg-gowl-panel-2 px-4 py-2.5 font-mono text-[11px] tracking-wider text-gowl-t6">
              <span>{strings.pipelineMapHeaderProperty}</span>
              <span>{strings.pipelineMapHeaderColumn}</span>
              <span>{strings.pipelineMapHeaderValue}</span>
              <span className="text-right">{strings.pipelineMapHeaderConfidence}</span>
            </div>
            {MAPPINGS.map((m) => (
              <div
                key={m.property}
                className="grid grid-cols-[1.5fr_1.2fr_1.2fr_96px] items-center gap-3 border-b border-gowl-row px-4 py-2.5 last:border-b-0 hover:bg-gowl-panel-2"
              >
                <div>
                  <div className="font-mono text-[13px] text-gowl-accent">
                    {m.property}{m.required && <span className="text-gowl-bad">*</span>}
                  </div>
                  <div className="mt-0.5 font-mono text-[11px] text-gowl-t7">{m.className}</div>
                </div>
                <div className="flex items-center gap-2 rounded border border-gowl-line-2 px-2 py-1">
                  <span className="font-mono text-[12.5px] text-gowl-t3">{m.sourceColumn}</span>
                  <span className="ml-auto text-[11.5px] text-gowl-t7">▾</span>
                </div>
                <span className="font-mono text-[12.5px] text-gowl-t3">{m.sample}</span>
                <span className={`font-mono text-[12px] text-right ${
                  m.confidence >= 0.95 ? "text-gowl-ok" : m.confidence >= 0.80 ? "text-gowl-t5" : "text-gowl-amber"
                }`}>
                  {m.confidence.toFixed(2)}
                </span>
              </div>
            ))}
            <div className="flex items-center justify-between px-4 py-2.5">
              <span className="text-[13px] text-gowl-t6">{MAPPINGS.length} mappings</span>
              <button type="button" className="rounded-md border border-gowl-accent-border bg-gowl-accent-bg px-3 py-1.5 text-[13.5px] text-gowl-accent">
                {strings.pipelineConfirmDataset}
              </button>
            </div>
          </div>

          {/* Bottom info panels */}
          <div className="grid grid-cols-2 gap-3.5">
            <div className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
              <div className="mb-2.5 font-mono text-[11px] tracking-widest text-gowl-t6">{strings.pipelineUnmappedTitle}</div>
              <div className="mb-2.5 flex flex-wrap gap-1.5">
                {["created_at", "updated_by", "internal_code"].map((col) => (
                  <span key={col} className="rounded bg-gowl-input border border-gowl-line-2 px-1.5 py-0.5 font-mono text-[11.5px] text-gowl-t5">
                    {col}
                  </span>
                ))}
              </div>
              <div className="text-[13px] leading-relaxed text-gowl-t6">{strings.pipelineUnmappedNote}</div>
            </div>
            <div className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
              <div className="mb-2.5 font-mono text-[11px] tracking-widest text-gowl-t6">{strings.pipelineWhatDecidesTitle}</div>
              <div className="text-[13.5px] leading-relaxed text-gowl-t4">{strings.pipelineWhatDecidesNote}</div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
