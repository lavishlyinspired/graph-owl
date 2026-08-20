import { useState } from "react";
import { runCypher, runSparql, type SparqlResult } from "../../lib/api";
import { strings } from "../../lib/strings";

type Lang = "sparql" | "cypher";

/** Folded in from the former standalone `/workbench` page — same
 *  SPARQL/Cypher runner, now where the rest of graph-owl's authoring
 *  surfaces already live. SPARQL and Cypher share one result envelope
 *  (`SparqlOutcome` server-side, `/sparql` and `/cypher` alike), so this
 *  is one runner parameterized by which endpoint it calls, not two tabs. */
export function QueriesTab() {
  const [lang, setLang] = useState<Lang>("sparql");
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<SparqlResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const run = async () => {
    if (query.trim().length === 0) return;
    setBusy(true);
    setError(null);
    try {
      const outcome = await (lang === "sparql" ? runSparql(query.trim()) : runCypher(query.trim()));
      setResult(outcome);
    } catch {
      setError(strings.studioError);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <p className="mb-3 text-[16.5px] text-gowl-t5">{strings.workbenchDescription}</p>

      <div className="mb-3 flex gap-1 border-b border-gowl-line">
        {(["sparql", "cypher"] as const).map((l) => (
          <button
            key={l}
            type="button"
            onClick={() => {
              setLang(l);
              setResult(null);
              setError(null);
            }}
            className={`px-3 py-2 text-[16.5px] ${
              lang === l ? "border-b-2 border-gowl-accent text-gowl-accent" : "text-gowl-t5 hover:text-gowl-t2"
            }`}
          >
            {l === "sparql" ? strings.workbenchTabSparql : strings.workbenchTabCypher}
          </button>
        ))}
      </div>

      <textarea
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={lang === "sparql" ? strings.sparqlPlaceholder : strings.cypherPlaceholder}
        rows={6}
        className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input p-3 font-mono text-[16.5px] text-gowl-t1"
      />
      <button
        type="button"
        disabled={busy || query.trim().length === 0}
        onClick={() => void run()}
        className="mb-4 rounded-md bg-gowl-accent px-4 py-1.5 text-[16px] font-semibold text-gowl-accent-on disabled:opacity-40"
      >
        {strings.sparqlRun}
      </button>

      {error && <p className="text-[17px] text-gowl-bad">{error}</p>}

      {result && (
        <div>
          <div className="mb-3 flex gap-6 font-mono text-[16px] text-gowl-t1">
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
            <p className="text-[17px] text-gowl-t5">{strings.sparqlNoRows}</p>
          ) : (
            <div className="overflow-x-auto rounded-lg border border-gowl-line bg-gowl-panel">
              <table className="w-full text-left text-[16px]">
                <thead>
                  <tr className="border-b border-gowl-line bg-gowl-panel-2">
                    {result.variables.map((variable) => (
                      <th key={variable} className="px-3 py-2 font-mono text-[13.5px] tracking-wider text-gowl-t6">
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

      {!result && !error && <p className="text-[17px] text-gowl-t5">{strings.sparqlEmpty}</p>}
    </div>
  );
}
