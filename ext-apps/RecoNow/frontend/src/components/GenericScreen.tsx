import { useState } from "react";
import { useNavigate } from "react-router-dom";
import type {
  ScreenConfig,
  VizData,
  KpiItem,
  CopilotData,
  BarItem,
  SeriesItem,
  FlowChain,
  CellData,
  RowData,
} from "../lib/screenConfigs";

interface GenericScreenProps {
  readonly config: ScreenConfig;
  readonly liveRows?: readonly RowData[];
  readonly liveKpis?: readonly KpiItem[];
  readonly liveViz?: VizData;
  readonly liveCopilot?: CopilotData;
  /** Column headings for `liveRows`. A route knows what it is rendering; the
   *  config knows the mockup's shape. Where they differed, rows rendered
   *  under the wrong headings — a rupee figure beneath "MISMATCH" and a count
   *  beneath "ITC AT RISK" on the suppliers screen. Supplying them together
   *  with the rows is what stops the two drifting apart. */
  readonly liveCols?: readonly string[];
  readonly liveGrid?: string;
  readonly loading?: boolean;
  readonly error?: string | null;
  /** What the header's two buttons do.
   *
   *  **They did nothing at all until now.** The labels come from the mockup
   *  config, so every generic screen rendered a primary and a secondary
   *  action that looked live and was inert — the worst kind of broken,
   *  because a button that does nothing is indistinguishable from one whose
   *  work has not finished. A screen that supplies no handler now renders no
   *  button rather than a decorative one. */
  readonly onPrimary?: () => void;
  readonly onSecondary?: () => void;
}

/** The screen config supplies *layout* — titles, column headings, grid widths,
 *  the shape of the chart. It also carries the delivered mockup's illustrative
 *  figures, and those are never rendered.
 *
 *  This used to be `liveRows ?? config.rows`, so any screen whose fetch failed,
 *  or that passed no rows at all, quietly rendered the mockup's invented
 *  suppliers and rupee amounts. For a product that reports a client's tax
 *  position that is not a degraded state, it is a fabricated one — and it is
 *  indistinguishable from a working screen, which is what made it dangerous.
 *  Quantities now come from the caller or they do not appear. */
export default function GenericScreen({
  config,
  liveRows,
  liveKpis,
  liveViz,
  liveCopilot,
  liveCols,
  liveGrid,
  loading,
  error,
  onPrimary,
  onSecondary,
}: GenericScreenProps) {
  const effectiveRows = liveRows ?? [];
  const effectiveKpis = liveKpis ?? [];
  const effectiveCols = liveCols ?? config.cols;
  const effectiveGrid = liveGrid ?? config.grid;
  const [drawerRow, setDrawerRow] = useState<number | null>(null);
  const navigate = useNavigate();

  // A row under the wrong headings is a chart with mislabelled axes: every
  // value is shown, and every value means something else. Silence is the
  // worst outcome, so say so loudly rather than rendering it.
  const widest = effectiveRows.reduce((n, r) => Math.max(n, r.cells.length), 0);
  if (widest > 0 && widest !== effectiveCols.length) {
    console.error(
      `GenericScreen("${config.title}"): ${effectiveCols.length} column headings for rows of ` +
        `${widest} cells. Values are being rendered under headings that do not describe them. ` +
        `Pass liveCols alongside liveRows.`,
    );
  }

  const handleRelated = (route: string) => {
    // Routes are mounted at `/${route}` by router.tsx, not under an `/r/`
    // prefix — every "NEXT" link used to 404.
    navigate(`/${route}`);
  };

  return (
    <div className="p-6 pb-11">
      <div className="mb-4 flex items-end justify-between">
        <div>
          <h1 className="mb-1 text-[20px] font-bold tracking-tight text-reco-t1">
            {config.title}
          </h1>
          <p className="text-[12.5px] text-reco-t4">{config.desc}</p>
        </div>
        <div className="flex gap-2">
          {/* Rendered only where the screen supplied a handler. An inert
              button is worse than an absent one: it promises an action and
              swallows the click. */}
          {onSecondary && (
            <button
              type="button"
              onClick={onSecondary}
              className="rounded-[7px] border border-reco-line-3 bg-white px-3.5 py-[7px] text-[12.5px] text-reco-t1 hover:border-reco-t5"
            >
              {config.secondary}
            </button>
          )}
          {onPrimary && (
            <button
              type="button"
              onClick={onPrimary}
              className="rounded-[7px] bg-reco-t0 px-3.5 py-[7px] text-[12.5px] font-semibold text-white"
            >
              {config.primary}
            </button>
          )}
        </div>
      </div>

      {loading ? (
        <div className="mb-4 flex h-[120px] items-center justify-center text-[13px] text-reco-t4">Loading…</div>
      ) : error ? (
        <div
          data-testid="generic-error"
          className="mb-4 rounded-[10px] border border-reco-bad-border bg-reco-bad-bg px-4 py-3.5 text-[12.5px] text-reco-bad"
        >
          {error}
        </div>
      ) : (
        <>
          {effectiveKpis.length > 0 && <KpiGrid kpis={effectiveKpis} />}

          {liveViz && <VizSection viz={liveViz} />}

          <div className="grid grid-cols-[1fr_300px] items-start gap-3.5">
            <div className="overflow-hidden rounded-[10px] border border-reco-line bg-white">
              <div
                className="grid gap-3 border-b border-reco-line bg-reco-panel-2 px-[18px] py-2.5 font-mono text-[9.5px] tracking-[0.1em] text-reco-t4"
                style={{ gridTemplateColumns: `28px ${effectiveGrid} 96px` }}
              >
                <span>☐</span>
                {effectiveCols.map((col) => (
                  <span key={col}>{col}</span>
                ))}
                <span />
              </div>
              {effectiveRows.length === 0 && (
                <div
                  data-testid="generic-empty"
                  className="px-[18px] py-9 text-center text-[12.5px] text-reco-t4"
                >
                  Nothing to show for this client and period yet.
                  <div className="mt-1 text-[11.5px] text-reco-t5">
                    Upload and reconcile a period, and this fills from the reconciled data.
                  </div>
                </div>
              )}
              {effectiveRows.map((row, i) => (
            <div
              key={i}
              className="grid cursor-pointer items-center gap-3 border-b border-reco-row px-[18px] py-3 hover:bg-reco-panel-2"
              style={{ gridTemplateColumns: `28px ${effectiveGrid} 96px` }}
              onClick={() => setDrawerRow(i)}
              onKeyDown={() => setDrawerRow(i)}
              role="button"
              tabIndex={0}
            >
              <span className="text-[12px] text-reco-t6">☐</span>
              {row.cells.map((cell, j) => (
                <Cell key={j} cell={cell} />
              ))}
              <span className="justify-self-end whitespace-nowrap rounded-[6px] border border-reco-line bg-white px-2.5 py-[3px] text-[11px] text-reco-accent-hi">
                Open
              </span>
            </div>
          ))}
        </div>

        <div className="flex flex-col gap-3.5">
          <div className="rounded-[10px] border border-reco-accent-border bg-white p-4">
            <div className="mb-2.5 flex items-center gap-2">
              <span className="h-[7px] w-[7px] rounded-[2px] bg-reco-accent" />
              <span className="font-mono text-[9.5px] tracking-[0.12em] text-reco-accent-hi">
                FROM THE GRAPH
              </span>
            </div>
            <div className="text-[12.5px] leading-relaxed text-reco-t2">
              {config.graphNote}
            </div>
          </div>

          <div className="rounded-[10px] border border-reco-line bg-white p-4">
            <div className="mb-2.5 font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
              NEXT
            </div>
            {config.related.map((rel) => (
              <button
                key={rel.route}
                type="button"
                className="flex w-full items-center justify-between border-b border-reco-row py-[7px] text-left last:border-b-0"
                onClick={() => handleRelated(rel.route)}
              >
                {/* `rel.meta` is deliberately not rendered: in the mockup it
                    carries counts ("24 pending", "₹3.42 Cr") that no query
                    produces. The destination is real navigation and stays. */}
                <span className="text-[12px] text-reco-accent">{rel.name}</span>
                <span className="text-[11px] text-reco-t5">→</span>
              </button>
            ))}
          </div>

          {/* The mockup's copilot copy quotes specific figures — "₹8.2 L sits
              inside the s.16(4) window", "24 IMS records with no action". No
              query produces them. A panel that says "built from graph facts"
              is the last place to show a number that came from a design file,
              so it renders only when a caller supplies a real one. */}
          {liveCopilot && (
            <div className="rounded-[10px] border border-reco-line bg-[#fbfaf8] p-4">
              <div className="mb-2.5 flex items-center gap-2">
                <span className="h-[7px] w-[7px] rounded-full bg-reco-purple" />
                <span className="font-mono text-[9.5px] tracking-[0.12em] text-reco-purple">
                  ASSISTANT · READ THIS SCREEN
                </span>
              </div>
              <div className="mb-3 text-[12.5px] leading-relaxed text-reco-t2">
                {liveCopilot.text}
              </div>
              <button
                type="button"
                className="w-full rounded-[7px] bg-reco-t0 py-2 text-[12px] text-center text-white"
              >
                {liveCopilot.action}
              </button>
              <div className="mt-2.5 text-[10.5px] leading-snug text-reco-t5">
                Suggestion only, built from graph facts. Nothing runs until you
                press it.
              </div>
            </div>
          )}
        </div>
      </div>

          {drawerRow !== null && effectiveRows[drawerRow] !== undefined && (
            <Drawer
              row={effectiveRows[drawerRow]!}
              cols={effectiveCols}
              title={config.title}
              graphNote={config.graphNote}
              actions={config.rowActs}
              onClose={() => setDrawerRow(null)}
            />
          )}
        </>
      )}
    </div>
  );
}

function KpiGrid({ kpis }: { readonly kpis: readonly KpiItem[] }) {
  return (
    <div className="mb-3.5 grid grid-cols-4 gap-3">
      {kpis.map((kpi) => (
        <div
          key={kpi.label}
          className="rounded-[10px] border border-reco-line bg-white p-4"
        >
          <div className="mb-2 font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
            {kpi.label}
          </div>
          <div
            className="font-mono text-[21px]"
            style={{ color: kpi.color }}
          >
            {kpi.value}
          </div>
          <div className="mt-1.5 text-[11px] text-reco-t4">{kpi.sub}</div>
        </div>
      ))}
    </div>
  );
}

function VizSection({ viz }: { readonly viz: VizData }) {
  return (
    <div className="mb-3.5 rounded-[10px] border border-reco-line bg-white p-4 pb-[18px]">
      <div className="mb-4 flex items-baseline justify-between">
        <span className="font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
          {viz.title}
        </span>
        <span className="text-[11.5px] text-reco-t5">{viz.hint}</span>
      </div>
      {viz.kind === "bars" && <BarsViz items={viz.items} />}
      {viz.kind === "series" && <SeriesViz items={viz.items} />}
      {viz.kind === "flow" && <FlowViz chains={viz.chains} />}
      {viz.kind === "split" && <SplitViz items={viz.items} />}
    </div>
  );
}

function BarsViz({ items }: { readonly items: readonly BarItem[] }) {
  return (
    <div className="flex flex-col gap-[11px]">
      {items.map((b) => (
        <div key={b.label} className="flex items-center gap-3.5">
          <span className="w-[170px] text-[12px] text-reco-t2">{b.label}</span>
          <div className="relative h-[9px] flex-1 overflow-hidden rounded-[5px] bg-reco-row">
            <div
              className="absolute inset-y-0 left-0 rounded-[5px]"
              style={{ width: b.pct, background: b.color }}
            />
          </div>
          <span className="w-[132px] text-right font-mono text-[11.5px] text-reco-t4">
            {b.value}
          </span>
        </div>
      ))}
    </div>
  );
}

function SeriesViz({ items }: { readonly items: readonly SeriesItem[] }) {
  return (
    <div className="flex h-[120px] items-end gap-2.5">
      {items.map((s) => (
        <div
          key={s.label}
          className="flex flex-1 flex-col items-center gap-[7px] justify-end h-full"
        >
          <span className="font-mono text-[10.5px] text-reco-t2">{s.value}</span>
          <div
            className="w-full rounded-t-[4px]"
            style={{ height: s.h, background: s.color }}
          />
          <span className="font-mono text-[10px] text-reco-t5">{s.label}</span>
        </div>
      ))}
    </div>
  );
}

function FlowViz({ chains }: { readonly chains: readonly FlowChain[] }) {
  return (
    <div className="flex flex-col gap-3.5">
      {chains.map((ch) => (
        <div key={ch.name}>
          <div className="mb-2.5 flex items-baseline gap-2.5">
            <span className="font-mono text-[9.5px] tracking-[0.12em] text-reco-accent-hi">
              {ch.name}
            </span>
            <span className="text-[11px] text-reco-t5">{ch.meta}</span>
          </div>
          <div className="flex flex-wrap items-center">
            {ch.steps.map((st, i) => (
              <div key={i} className="flex items-center">
                <div
                  className="rounded-lg border bg-[#fbfaf8] px-3 py-[9px]"
                  style={{ borderColor: st.color }}
                >
                  <div className="text-[12px] text-reco-t1">{st.label}</div>
                  <div className="mt-[3px] font-mono text-[9.5px] text-reco-t4">
                    {st.sub}
                  </div>
                </div>
                {i < ch.steps.length - 1 && (
                  <span className="w-6 text-center text-[12px] text-reco-t6">
                    →
                  </span>
                )}
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

function SplitViz({ items }: { readonly items: readonly BarItem[] }) {
  return (
    <div>
      <div className="mb-3.5 flex h-[26px] gap-[2px] overflow-hidden rounded-[6px]">
        {items.map((s) => (
          <div
            key={s.label}
            style={{ width: s.pct, background: s.color }}
          />
        ))}
      </div>
      <div className="flex flex-wrap gap-[18px]">
        {items.map((s) => (
          <div key={s.label} className="flex items-center gap-2">
            <span
              className="h-[9px] w-[9px] rounded-[2px]"
              style={{ background: s.color }}
            />
            <span className="text-[12px] text-reco-t2">{s.label}</span>
            <span className="font-mono text-[11.5px] text-reco-t4">{s.value}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function Cell({ cell }: { readonly cell: CellData }) {
  const isMono = cell.font === "mono";
  const fontClass = isMono ? "font-mono" : "";
  const sizeClass = isMono ? "text-[12px]" : "text-[12.5px]";
  return (
    <div>
      <div
        className={`${fontClass} ${sizeClass}`}
        style={{ color: cell.color ?? "#1c1b18" }}
      >
        {cell.t}
      </div>
      {cell.sub && (
        <div className="mt-[2px] font-mono text-[10px] text-reco-t5">
          {cell.sub}
        </div>
      )}
    </div>
  );
}

function Drawer({
  row,
  cols,
  title,
  graphNote,
  actions,
  onClose,
}: {
  readonly row: { readonly cells: readonly CellData[] };
  readonly cols: readonly string[];
  readonly title: string;
  readonly graphNote: string;
  readonly actions: readonly string[];
  readonly onClose: () => void;
}) {
  return (
    <div className="fixed inset-y-0 right-0 top-14 z-30 flex w-[392px] flex-col border-l border-reco-line bg-white shadow-[-18px_0_42px_rgba(28,27,24,.12)]">
      <div className="flex items-start gap-3 border-b border-reco-line-2 px-5 py-[18px]">
        <div className="flex-1">
          <div className="mb-1 text-[16px] font-bold text-reco-t1">
            {row.cells[0]?.t ?? "—"}
          </div>
          <div className="font-mono text-[10.5px] text-reco-t4">{title}</div>
        </div>
        <button
          type="button"
          className="text-[15px] text-reco-t5"
          onClick={onClose}
        >
          ×
        </button>
      </div>
      <div className="flex-1 overflow-auto px-5 py-4">
        <div className="mb-2.5 font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
          RECORD
        </div>
        {cols.map((col, i) => (
          <div
            key={col}
            className="grid grid-cols-[120px_1fr] gap-3 border-b border-reco-row py-[9px]"
          >
            <span className="font-mono text-[10px] text-reco-t5">{col}</span>
            <div>
              <div className="text-[12.5px] text-reco-t1">
                {row.cells[i]?.t ?? "—"}
              </div>
              {row.cells[i]?.sub && (
                <div className="mt-[2px] font-mono text-[10px] text-reco-t5">
                  {row.cells[i]!.sub}
                </div>
              )}
            </div>
          </div>
        ))}
        <div className="mt-[18px] rounded-lg border border-reco-accent-border bg-reco-accent-bg p-3.5 text-[12px] leading-relaxed text-reco-t2">
          {graphNote}
        </div>
      </div>
      <div className="flex flex-col gap-2 border-t border-reco-line-2 px-5 py-4">
        {actions.map((a, i) => (
          <button
            key={a}
            type="button"
            className={`rounded-[7px] border py-[9px] text-center text-[12.5px] ${
              i === 0
                ? "border-reco-t0 bg-reco-t0 text-white"
                : "border-reco-line-3 bg-white text-reco-t1"
            }`}
          >
            {a}
          </button>
        ))}
      </div>
    </div>
  );
}
