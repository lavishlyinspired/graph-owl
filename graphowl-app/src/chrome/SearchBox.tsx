import { useEffect, useRef, useState } from "react";
import { strings } from "../lib/strings";
import { askGraphOwl, search, type AskResult, type SearchResult } from "../lib/api";

const DEBOUNCE_MS = 200;

function kindLabel(kind: SearchResult["kind"]): string {
  switch (kind) {
    case "asset":
      return strings.searchAsset;
    case "glossary-term":
      return strings.searchGlossaryTerm;
    case "business-metric":
      return strings.searchBusinessMetric;
  }
}

export function SearchBox() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<readonly SearchResult[]>([]);
  const [failed, setFailed] = useState(false);
  const [open, setOpen] = useState(false);
  const [asking, setAsking] = useState(false);
  const [askResult, setAskResult] = useState<AskResult | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    clearTimeout(timer.current);
    setAskResult(null);
    if (query.trim().length === 0) {
      setResults([]);
      setFailed(false);
      return;
    }
    timer.current = setTimeout(() => {
      search(query)
        .then((found) => {
          setResults(found);
          setFailed(false);
        })
        .catch(() => {
          // A failed search must not render identically to a genuine "no
          // matches" — the same distinction `ui/`'s SearchBox already drew
          // (Epic 39 Slice F's RED), and the one graphowl-app's own
          // first-run Playwright spec caught missing before this fix.
          setResults([]);
          setFailed(true);
        });
    }, DEBOUNCE_MS);
    return () => clearTimeout(timer.current);
  }, [query]);

  const runAsk = () => {
    setAsking(true);
    setAskResult(null);
    askGraphOwl(query)
      .then(setAskResult)
      .catch(() => setAskResult({ kind: "error", message: strings.searchAskFailed }))
      .finally(() => setAsking(false));
  };

  return (
    <div className="relative flex-1">
      <div className="flex items-center gap-2 rounded-md border border-gowl-line bg-gowl-input px-3 py-1.5 text-gowl-t5">
        <span aria-hidden="true">{strings.searchIcon}</span>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onFocus={() => setOpen(true)}
          onBlur={() => setTimeout(() => setOpen(false), 150)}
          placeholder={strings.searchPlaceholder}
          className="flex-1 bg-transparent text-[17px] text-gowl-t1 outline-none placeholder:text-gowl-t5"
        />
        <kbd className="rounded bg-gowl-kbd px-1.5 py-0.5 font-mono text-[15px] text-gowl-t4">
          {strings.searchShortcut}
        </kbd>
      </div>

      {(open || asking || askResult !== null) && query.trim().length > 0 && (
        <div className="absolute top-full left-0 z-40 mt-1 max-h-96 w-full overflow-y-auto rounded-md border border-gowl-line bg-gowl-panel shadow-2xl">
          {failed && <div className="p-3 text-[16px] text-gowl-bad">{strings.searchFailed}</div>}
          {!failed && results.length === 0 && (
            <div className="p-3 text-[16px] text-gowl-t5">{strings.searchNoResults}</div>
          )}
          {results.map((r) => (
            <div key={`${r.kind}-${r.id}`} className="border-b border-gowl-line p-3 last:border-b-0">
              <div className="font-mono text-[14px] tracking-widest text-gowl-t6">{kindLabel(r.kind)}</div>
              <div className="mt-1 text-[17px] text-gowl-t1">{r.label}</div>
              <div className="mt-0.5 font-mono text-[15px] text-gowl-t5">{r.fqn}</div>
            </div>
          ))}

          <div className="border-t border-gowl-line p-3">
            <button
              type="button"
              onMouseDown={(e) => e.preventDefault()}
              onClick={runAsk}
              disabled={asking}
              className="text-[16.5px] text-gowl-accent disabled:opacity-60"
            >
              {asking ? strings.searchAsking : `${strings.searchAskPrefix} "${query}"`}
            </button>
            {!askResult && !asking && <p className="mt-1 text-[14.5px] text-gowl-t6">{strings.searchAskScopeNote}</p>}

            {askResult?.kind === "noMatch" && (
              <p className="mt-2 text-[16px] text-gowl-t5">{askResult.message}</p>
            )}
            {askResult?.kind === "error" && (
              <p className="mt-2 text-[16px] text-gowl-bad">{askResult.message}</p>
            )}
            {askResult?.kind === "answered" && (
              <div className="mt-2">
                <div className="font-mono text-[13.5px] tracking-widest text-gowl-t6">
                  {strings.searchAnswerHeading}
                </div>
                <p className="mt-1 whitespace-pre-wrap text-[16.5px] text-gowl-t1">
                  {askResult.narration ?? askResult.answer}
                </p>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
