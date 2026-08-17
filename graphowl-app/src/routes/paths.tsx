import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { TraceDetail } from "./TraceDetail";
import { toPathsConfig } from "../lib/trace";
import { findPaths, type PathSearchResult } from "../lib/api";
import { strings } from "../lib/strings";

export default function PathsRoute() {
  const [params, setParams] = useSearchParams();
  const from = params.get("from") ?? "";
  const to = params.get("to") ?? "";

  const [fromInput, setFromInput] = useState(from);
  const [toInput, setToInput] = useState(to);
  const [result, setResult] = useState<PathSearchResult | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    setFromInput(from);
    setToInput(to);
    setResult(null);
    setError(false);
    if (!from || !to) return;
    let live = true;
    findPaths(from, to)
      .then((found) => {
        if (live) setResult(found);
      })
      .catch(() => {
        if (live) setError(true);
      });
    return () => {
      live = false;
    };
  }, [from, to]);

  const submit = () => {
    if (!fromInput.trim() || !toInput.trim()) return;
    setParams({ from: fromInput.trim(), to: toInput.trim() });
  };

  return (
    <div className="p-8">
      <div className="mb-5 flex flex-wrap items-end gap-3">
        <div>
          <div className="mb-1 text-[11px] text-gowl-t5">{strings.pathsFromLabel}</div>
          <input
            value={fromInput}
            onChange={(event) => setFromInput(event.target.value)}
            className="rounded-md border border-gowl-line-2 bg-gowl-input px-3 py-1.5 font-mono text-[12px] text-gowl-t1"
          />
        </div>
        <div>
          <div className="mb-1 text-[11px] text-gowl-t5">{strings.pathsToLabel}</div>
          <input
            value={toInput}
            onChange={(event) => setToInput(event.target.value)}
            className="rounded-md border border-gowl-line-2 bg-gowl-input px-3 py-1.5 font-mono text-[12px] text-gowl-t1"
          />
        </div>
        <button
          type="button"
          onClick={submit}
          className="rounded-md bg-gowl-accent px-4 py-1.5 text-[12px] font-semibold text-gowl-accent-on"
        >
          {strings.pathsSearch}
        </button>
      </div>

      {!from || !to ? (
        <div className="text-[13px] text-gowl-t5">{strings.pathsMissingEnds}</div>
      ) : error ? (
        <div className="text-[13px] text-gowl-bad">{strings.traceError}</div>
      ) : !result ? (
        <div className="text-[13px] text-gowl-t5">{strings.traceLoading}</div>
      ) : (
        <TraceDetail config={toPathsConfig(result, from, to)} />
      )}
    </div>
  );
}
