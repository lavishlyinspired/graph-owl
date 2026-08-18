import { useEffect, useMemo, useState } from "react";
import { layoutTermGraph } from "../../lib/vocabulary/vocabularyGraph";
import { addTermRelation, fetchGlossaryTerms, fetchTermRelations, type GlossaryTerm, type SkosRelation, type SkosRelationKind } from "../../lib/api";
import { strings } from "../../lib/strings";

const RELATION_KINDS: readonly SkosRelationKind[] = ["broader", "narrower", "related", "exactMatch", "closeMatch"];
const SIZE = 500;

export function GraphTab({ glossaryId }: { readonly glossaryId: string }) {
  const [terms, setTerms] = useState<readonly GlossaryTerm[] | null>(null);
  const [relationsByTerm, setRelationsByTerm] = useState<ReadonlyMap<string, readonly SkosRelation[]>>(new Map());
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [kind, setKind] = useState<SkosRelationKind>("related");
  const [busy, setBusy] = useState(false);

  const load = () => {
    fetchGlossaryTerms(glossaryId).then(async (fetchedTerms) => {
      setTerms(fetchedTerms);
      const entries = await Promise.all(
        fetchedTerms.map(async (term) => [term.id, await fetchTermRelations(term.id)] as const),
      );
      setRelationsByTerm(new Map(entries));
    });
  };

  useEffect(load, [glossaryId]);

  const graph = useMemo(() => {
    if (!terms) return null;
    return layoutTermGraph(
      terms.map((t) => ({ id: t.id, name: t.name })),
      relationsByTerm,
      SIZE / 2 - 60,
      { x: SIZE / 2, y: SIZE / 2 },
    );
  }, [terms, relationsByTerm]);

  if (!terms) {
    return <div className="text-[13px] text-gowl-t5">{strings.studioLoading}</div>;
  }

  const connect = async () => {
    if (!from || !to) return;
    setBusy(true);
    try {
      await addTermRelation(from, kind, to);
      load();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="grid grid-cols-[1fr_280px] gap-4">
      <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
        {!graph || graph.nodes.length === 0 ? (
          <p className="p-6 text-[13px] text-gowl-t5">{strings.graphEmpty}</p>
        ) : (
          <svg width={SIZE} height={SIZE} viewBox={`0 0 ${SIZE} ${SIZE}`} role="img" aria-label={strings.studioTabGraph}>
            {graph.edges.map((edge, index) => {
              const source = graph.nodes.find((n) => n.id === edge.from);
              const target = graph.nodes.find((n) => n.id === edge.to);
              if (!source || !target) return null;
              return (
                <line
                  key={index}
                  x1={source.x}
                  y1={source.y}
                  x2={target.x}
                  y2={target.y}
                  stroke="var(--gowl-line-3)"
                  strokeWidth={1}
                />
              );
            })}
            {graph.nodes.map((node) => (
              <g key={node.id}>
                <circle cx={node.x} cy={node.y} r={22} fill="var(--gowl-accent-bg)" stroke="var(--gowl-accent)" strokeWidth={1.5} />
                <text x={node.x} y={node.y + 34} textAnchor="middle" fontSize={11} fill="var(--gowl-t2)">
                  {node.name.length > 14 ? `${node.name.slice(0, 13)}…` : node.name}
                </text>
              </g>
            ))}
          </svg>
        )}
      </div>

      <div className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
        <div className="mb-3 font-mono text-[9.5px] tracking-widest text-gowl-t6">{strings.graphConnectTitle}</div>
        <div className="mb-2">
          <div className="mb-1 text-[11px] text-gowl-t5">{strings.graphConnectFrom}</div>
          <select
            value={from}
            onChange={(e) => setFrom(e.target.value)}
            aria-label={strings.graphConnectFrom}
            className="w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[12px] text-gowl-t1"
          >
            <option value="" />
            {terms.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        </div>
        <div className="mb-2">
          <div className="mb-1 text-[11px] text-gowl-t5">{strings.graphConnectKind}</div>
          <select
            value={kind}
            onChange={(e) => setKind(e.target.value as SkosRelationKind)}
            aria-label={strings.graphConnectKind}
            className="w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[12px] text-gowl-t1"
          >
            {RELATION_KINDS.map((k) => (
              <option key={k} value={k}>
                {k}
              </option>
            ))}
          </select>
        </div>
        <div className="mb-3">
          <div className="mb-1 text-[11px] text-gowl-t5">{strings.graphConnectTo}</div>
          <select
            value={to}
            onChange={(e) => setTo(e.target.value)}
            aria-label={strings.graphConnectTo}
            className="w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[12px] text-gowl-t1"
          >
            <option value="" />
            {terms.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        </div>
        <button
          type="button"
          disabled={busy || !from || !to}
          onClick={connect}
          className="w-full rounded-md bg-gowl-accent px-3 py-1.5 text-[12px] font-semibold text-gowl-accent-on disabled:opacity-40"
        >
          {strings.graphConnectSubmit}
        </button>
      </div>
    </div>
  );
}
