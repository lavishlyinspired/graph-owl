import { useEffect, useMemo, useState } from "react";
import { buildVocabularyTree, type VocabularyTreeNode } from "../../lib/vocabulary/vocabularyTree";
import {
  addTermRelation,
  createGlossaryTerm,
  deleteGlossaryTerm,
  deleteTermRelation,
  fetchGlossaryTerms,
  fetchTermRelations,
  fetchTermUsage,
  type GlossaryTerm,
  type SkosRelation,
  type SkosRelationKind,
} from "../../lib/api";
import { strings } from "../../lib/strings";

const RELATION_KINDS: readonly SkosRelationKind[] = ["broader", "narrower", "related", "exactMatch", "closeMatch"];

function indexByRenderKey(roots: readonly VocabularyTreeNode[]): Map<string, VocabularyTreeNode> {
  const index = new Map<string, VocabularyTreeNode>();
  const walk = (nodes: readonly VocabularyTreeNode[]) => {
    for (const node of nodes) {
      index.set(node.renderKey, node);
      walk(node.children);
    }
  };
  walk(roots);
  return index;
}

function TreeNodeRow({
  node,
  selectedTermId,
  onSelect,
}: {
  readonly node: VocabularyTreeNode;
  readonly selectedTermId: string | null;
  readonly onSelect: (renderKey: string) => void;
}) {
  return (
    <div>
      <button
        type="button"
        onClick={() => onSelect(node.renderKey)}
        style={{ paddingLeft: `${8 + node.depth * 16}px` }}
        className={`block w-full truncate py-1 text-left text-[14px] hover:text-gowl-accent ${
          node.termId === selectedTermId ? "text-gowl-accent" : "text-gowl-t2"
        }`}
      >
        {node.term.name}
        {node.isCyclic && strings.buildCyclicSuffix}
      </button>
      {node.children.map((child) => (
        <TreeNodeRow key={child.renderKey} node={child} selectedTermId={selectedTermId} onSelect={onSelect} />
      ))}
    </div>
  );
}

export function BuildTab({ glossaryId }: { readonly glossaryId: string }) {
  const [terms, setTerms] = useState<readonly GlossaryTerm[] | null>(null);
  const [relationsByTerm, setRelationsByTerm] = useState<ReadonlyMap<string, readonly SkosRelation[]>>(new Map());
  const [selectedTermId, setSelectedTermId] = useState<string | null>(null);
  const [selectedRenderKey, setSelectedRenderKey] = useState<string | null>(null);
  const [relations, setRelations] = useState<readonly SkosRelation[] | null>(null);
  const [usage, setUsage] = useState<readonly string[] | null>(null);
  const [newTermName, setNewTermName] = useState("");
  const [relationKind, setRelationKind] = useState<SkosRelationKind>("broader");
  const [relationTarget, setRelationTarget] = useState("");
  const [busy, setBusy] = useState(false);

  const load = () => {
    setSelectedTermId(null);
    setSelectedRenderKey(null);
    fetchGlossaryTerms(glossaryId).then(async (fetchedTerms) => {
      setTerms(fetchedTerms);
      // The tree needs every term's `broader` edges up front — no bulk
      // "relations for this glossary" endpoint exists, so this is one fetch
      // per term, matching what `vocabularyTree.ts`'s own doc comment
      // expects a caller to have populated before calling it.
      const entries = await Promise.all(
        fetchedTerms.map(async (term) => [term.id, await fetchTermRelations(term.id)] as const),
      );
      setRelationsByTerm(new Map(entries));
    });
  };

  useEffect(load, [glossaryId]);

  useEffect(() => {
    if (!selectedTermId) {
      setRelations(null);
      setUsage(null);
      return;
    }
    fetchTermRelations(selectedTermId).then(setRelations);
    fetchTermUsage(selectedTermId).then((page) => setUsage(page.data));
  }, [selectedTermId]);

  const tree = useMemo(() => {
    if (!terms) return null;
    return buildVocabularyTree(
      terms.map((t) => ({ id: t.id, name: t.name })),
      relationsByTerm,
    );
  }, [terms, relationsByTerm]);

  const byRenderKey = useMemo(() => (tree ? indexByRenderKey(tree.roots) : new Map()), [tree]);

  if (!terms) {
    return <div className="text-[14.5px] text-gowl-t5">{strings.studioLoading}</div>;
  }

  const selected = terms.find((t) => t.id === selectedTermId) ?? null;
  const selectedNode = selectedRenderKey ? byRenderKey.get(selectedRenderKey) : undefined;

  const select = (renderKey: string) => {
    const node = byRenderKey.get(renderKey);
    if (!node) return;
    setSelectedRenderKey(renderKey);
    setSelectedTermId(node.termId);
  };

  const runCreate = async () => {
    if (newTermName.trim().length === 0) return;
    setBusy(true);
    try {
      await createGlossaryTerm(glossaryId, { name: newTermName.trim() });
      setNewTermName("");
      load();
    } finally {
      setBusy(false);
    }
  };

  const runDelete = async () => {
    if (!selected) return;
    setBusy(true);
    try {
      await deleteGlossaryTerm(selected.id);
      load();
    } finally {
      setBusy(false);
    }
  };

  const runAddRelation = async () => {
    if (!selected || relationTarget.trim().length === 0) return;
    setBusy(true);
    try {
      await addTermRelation(selected.id, relationKind, relationTarget.trim());
      setRelationTarget("");
      fetchTermRelations(selected.id).then(setRelations);
    } finally {
      setBusy(false);
    }
  };

  const runDeleteRelation = async (relation: SkosRelation) => {
    if (!selected) return;
    setBusy(true);
    try {
      await deleteTermRelation(selected.id, relation.kind, relation.target);
      fetchTermRelations(selected.id).then(setRelations);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="grid grid-cols-[280px_1fr] gap-4">
      <div className="rounded-lg border border-gowl-line bg-gowl-panel p-3">
        <div className="mb-2 flex gap-1">
          <input
            value={newTermName}
            onChange={(e) => setNewTermName(e.target.value)}
            placeholder={strings.buildTermNamePlaceholder}
            className="flex-1 rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1 text-[13.5px] text-gowl-t1"
          />
          <button
            type="button"
            disabled={busy || newTermName.trim().length === 0}
            onClick={runCreate}
            className="rounded-md bg-gowl-accent px-2 py-1 text-[12.5px] font-semibold text-gowl-accent-on disabled:opacity-40"
          >
            {strings.buildNewTerm}
          </button>
        </div>
        {tree && tree.roots.length === 0 ? (
          <p className="text-[13.5px] text-gowl-t5">{strings.buildTreeEmpty}</p>
        ) : (
          tree?.roots.map((root) => (
            <TreeNodeRow key={root.renderKey} node={root} selectedTermId={selectedTermId} onSelect={select} />
          ))
        )}
      </div>

      <div className="rounded-lg border border-gowl-line bg-gowl-panel p-5">
        {!selected ? (
          <p className="text-[14.5px] text-gowl-t5">{strings.buildDetailPlaceholder}</p>
        ) : (
          <div>
            <div className="mb-3 flex items-start justify-between">
              <div className="text-[17.5px] font-semibold text-gowl-t1">{selected.name}</div>
              <button type="button" disabled={busy} onClick={runDelete} className="text-[13.5px] text-gowl-bad">
                {strings.buildDeleteTerm}
              </button>
            </div>
            {selectedNode?.isCyclic && (
              <p className="mb-3 rounded-md border border-gowl-amber-border bg-gowl-amber-bg p-2 text-[13.5px] text-gowl-amber">
                {strings.buildCyclicNotice}
              </p>
            )}

            <div className="mb-3">
              <div className="mb-1 font-mono text-[11px] tracking-widest text-gowl-t6">{strings.buildDefinition}</div>
              <p className="text-[14px] text-gowl-t2">{selected.definition || "—"}</p>
            </div>
            <div className="mb-3 grid grid-cols-3 gap-3">
              <div>
                <div className="mb-1 font-mono text-[11px] tracking-widest text-gowl-t6">{strings.buildStatus}</div>
                <p className="text-[14px] text-gowl-t2">{selected.status}</p>
              </div>
              <div>
                <div className="mb-1 font-mono text-[11px] tracking-widest text-gowl-t6">{strings.buildSynonyms}</div>
                <p className="text-[14px] text-gowl-t2">{selected.synonyms.join(", ") || "—"}</p>
              </div>
              <div>
                <div className="mb-1 font-mono text-[11px] tracking-widest text-gowl-t6">{strings.buildAbbreviations}</div>
                <p className="text-[14px] text-gowl-t2">{selected.abbreviations.join(", ") || "—"}</p>
              </div>
            </div>

            <div className="mb-3">
              <div className="mb-1 font-mono text-[11px] tracking-widest text-gowl-t6">{strings.buildRelations}</div>
              {relations && relations.length === 0 && <p className="text-[13.5px] text-gowl-t5">{strings.buildNoRelations}</p>}
              {relations?.map((relation, index) => (
                <div key={index} className="flex items-center justify-between border-b border-gowl-row py-1 text-[13.5px]">
                  <span className="text-gowl-t2">
                    <span className="font-mono text-gowl-t6">{relation.kind}</span> {relation.target}
                  </span>
                  <button type="button" onClick={() => runDeleteRelation(relation)} className="text-gowl-bad">
                    {strings.buildRemoveRelation}
                  </button>
                </div>
              ))}
              <div className="mt-2 flex gap-1">
                <select
                  value={relationKind}
                  onChange={(e) => setRelationKind(e.target.value as SkosRelationKind)}
                  aria-label={strings.buildRelationKind}
                  className="rounded-md border border-gowl-line-2 bg-gowl-input px-1.5 py-1 text-[12.5px] text-gowl-t1"
                >
                  {RELATION_KINDS.map((kind) => (
                    <option key={kind} value={kind}>
                      {kind}
                    </option>
                  ))}
                </select>
                <input
                  value={relationTarget}
                  onChange={(e) => setRelationTarget(e.target.value)}
                  placeholder={strings.buildRelationTarget}
                  className="flex-1 rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1 text-[12.5px] text-gowl-t1"
                />
                <button
                  type="button"
                  disabled={busy || relationTarget.trim().length === 0}
                  onClick={runAddRelation}
                  className="rounded-md border border-gowl-line-2 px-2 py-1 text-[12.5px] text-gowl-t2 disabled:opacity-40"
                >
                  {strings.buildAddRelation}
                </button>
              </div>
            </div>

            <div>
              <div className="mb-1 font-mono text-[11px] tracking-widest text-gowl-t6">{strings.buildUsage}</div>
              {usage && usage.length === 0 && <p className="text-[13.5px] text-gowl-t5">{strings.buildNoUsage}</p>}
              <ul>
                {usage?.map((fqn) => (
                  <li key={fqn} className="font-mono text-[13px] text-gowl-t2">
                    {fqn}
                  </li>
                ))}
              </ul>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
