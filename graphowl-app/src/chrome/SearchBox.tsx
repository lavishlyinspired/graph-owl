import { useEffect, useRef, useState } from "react";
import { strings } from "../lib/strings";
import { search, type SearchResult } from "../lib/api";

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
  const [open, setOpen] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    clearTimeout(timer.current);
    if (query.trim().length === 0) {
      setResults([]);
      return;
    }
    timer.current = setTimeout(() => {
      search(query)
        .then(setResults)
        .catch(() => setResults([]));
    }, DEBOUNCE_MS);
    return () => clearTimeout(timer.current);
  }, [query]);

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
          className="flex-1 bg-transparent text-[13px] text-gowl-t1 outline-none placeholder:text-gowl-t5"
        />
        <kbd className="rounded bg-gowl-kbd px-1.5 py-0.5 font-mono text-[11px] text-gowl-t4">
          {strings.searchShortcut}
        </kbd>
      </div>

      {open && query.trim().length > 0 && (
        <div className="absolute top-full left-0 z-40 mt-1 max-h-80 w-full overflow-y-auto rounded-md border border-gowl-line bg-gowl-panel shadow-2xl">
          {results.length === 0 && <div className="p-3 text-[12px] text-gowl-t5">{strings.searchNoResults}</div>}
          {results.map((r) => (
            <div key={`${r.kind}-${r.id}`} className="border-b border-gowl-line p-3 last:border-b-0">
              <div className="font-mono text-[10px] tracking-widest text-gowl-t6">{kindLabel(r.kind)}</div>
              <div className="mt-1 text-[13px] text-gowl-t1">{r.label}</div>
              <div className="mt-0.5 font-mono text-[11px] text-gowl-t5">{r.fqn}</div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
