import { useState } from "react";
import { runSparql, type SparqlResult } from "../../lib/api";
import { strings } from "../../lib/strings";

/** "Run; generation deferred" (Plan 122a A7 AC) — NL-to-SPARQL is
 *  explicitly out of scope for A7 (deferred to A7b in the plan), so this
 *  is just a query box against the real `/sparql` endpoint. */
export function SparqlTab() {
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<SparqlResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const run = async () => {
    if (query.trim().length === 0) return;
    setBusy(true);
    setError(null);
    try {
      const outcome = await runSparql(query.trim());
      setResult(outcome);
    } catch {
      setError(strings.studioError);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <textarea
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={strings.sparqlPlaceholder}
        rows={6}
        className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input p-3 font-mono text-[12.5px] text-gowl-t1"
      />
      <button
        type="button"
        disabled={busy || query.trim().length === 0}
        onClick={run}
        className="mb-4 rounded-md bg-gowl-accent px-4 py-1.5 text-[12px] font-semibold text-gowl-accent-on disabled:opacity-40"
      >
        {strings.sparqlRun}
      </button>

      {error && <p className="text-[13px] text-gowl-bad">{error}</p>}

      {result && (
        <div>
          <div className="mb-3 flex gap-6 font-mono text-[12px] text-gowl-t1">
            <span>
              {strings.sparqlRows} {result.rows.length}
            </span>
            <span>
              {strings.sparqlFactsScanned} {result.factsScanned}
            </span>
            <span>
              {strings.sparqlTruncated} {result.truncated ? "yes" : "no"}
            </span>
          </div>
          {result.rows.length === 0 ? (
            <p className="text-[13px] text-gowl-t5">{strings.sparqlNoRows}</p>
          ) : (
            <div className="overflow-x-auto rounded-lg border border-gowl-line bg-gowl-panel">
              <table className="w-full text-left text-[12px]">
                <thead>
                  <tr className="border-b border-gowl-line bg-gowl-panel-2">
                    {result.variables.map((variable) => (
                      <th key={variable} className="px-3 py-2 font-mono text-[9.5px] tracking-wider text-gowl-t6">
                        {variable}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {result.rows.map((row, index) => (
                    <tr key={index} className="border-b border-gowl-row last:border-b-0">
                      {result.variables.map((variable) => (
                        <td key={variable} className="px-3 py-2 font-mono text-gowl-t2">
                          {row[variable] ?? "—"}
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {!result && !error && <p className="text-[13px] text-gowl-t5">{strings.sparqlEmpty}</p>}
    </div>
  );
}
