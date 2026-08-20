/** The shared renderer for a trace-style detail view — Plan 122a A4,
 *  narrowed to Paths alone (see `lib/trace.ts`'s own doc comment for why
 *  Lineage/History/Evidence no longer share this shape). Owns none of the
 *  domain logic (that lives in `lib/trace.ts`'s pure config builder) and
 *  none of the data fetching (that lives wherever renders it — Explore's
 *  own Entity tab, for Paths). It only draws whatever `TraceConfig` it is
 *  handed. */

import { Link } from "react-router-dom";
import type { TraceConfig } from "../lib/trace";
import { strings } from "../lib/strings";

export function TraceDetail({ config, id }: { readonly config: TraceConfig; readonly id?: string }) {
  return (
    <div className="p-8">
      <div className="mb-5 flex items-end justify-between">
        <div>
          <h1 className="mb-1 text-[25px] font-semibold text-gowl-t1">{config.title}</h1>
          <p className="text-[16.5px] text-gowl-t5">{config.description}</p>
        </div>
      </div>

      <div className="mb-4 grid grid-cols-4 gap-px overflow-hidden rounded-lg border border-gowl-line bg-gowl-line">
        {config.kpis.map((kpi) => (
          <div key={kpi.label} className="bg-gowl-panel p-4">
            <div className="mb-2 font-mono text-[13.5px] tracking-widest text-gowl-t6">{kpi.label}</div>
            <div className="font-mono text-[24px] text-gowl-t1">{kpi.value}</div>
            {kpi.sub && <div className="mt-1 text-[15px] text-gowl-t7">{kpi.sub}</div>}
          </div>
        ))}
      </div>

      <div className="grid grid-cols-[1fr_300px] items-start gap-4">
        <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
          <div
            className="grid gap-3 border-b border-gowl-line bg-gowl-panel-2 px-4 py-2 font-mono text-[13.5px] tracking-wider text-gowl-t6"
            style={{ gridTemplateColumns: `repeat(${config.columns.length}, 1fr)` }}
          >
            {config.columns.map((col) => (
              <span key={col}>{col}</span>
            ))}
          </div>
          {config.rows.length === 0 ? (
            <div className="p-6 text-[16.5px] text-gowl-t5">{config.emptyMessage}</div>
          ) : (
            config.rows.map((row) => (
              <div
                key={row.key}
                className="grid items-center gap-3 border-b border-gowl-row px-4 py-2.5 last:border-b-0"
                style={{ gridTemplateColumns: `repeat(${config.columns.length}, 1fr)` }}
              >
                {row.cells.map((cell, index) => (
                  <div key={index}>
                    <div className={cell.mono ? "truncate font-mono text-[15.5px] text-gowl-t2" : "truncate text-[16.5px] text-gowl-t1"}>
                      {cell.text}
                    </div>
                    {cell.sub && <div className="mt-0.5 truncate font-mono text-[14px] text-gowl-t7">{cell.sub}</div>}
                  </div>
                ))}
              </div>
            ))
          )}
        </div>

        <div className="flex flex-col gap-3.5">
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
            <div className="mb-2.5 font-mono text-[13.5px] tracking-widest text-gowl-t6">{config.noteTitle}</div>
            <div className="text-[16.5px] leading-relaxed text-gowl-t3">{config.noteBody}</div>
          </div>
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
            <div className="mb-2.5 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.relatedTitle}</div>
            {config.related.map((related) => (
              <RelatedLink key={related.route} label={related.label} route={related.route} id={id ?? related.id} />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

function RelatedLink({ label, route, id }: { readonly label: string; readonly route: string; readonly id?: string }) {
  const to = id ? `/${route}/${encodeURIComponent(id)}` : `/${route}`;
  return (
    <Link
      to={to}
      className="flex items-center justify-between border-b border-gowl-row py-1.5 text-[16px] text-gowl-accent last:border-b-0"
    >
      <span>{label}</span>
    </Link>
  );
}
