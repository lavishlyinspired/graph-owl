/** The tree + detail pane shell — Epic 42 Slice A, on the glossary alone.
 *  Slice B's own job is proving the same component covers classifications,
 *  domains, and ontology packs through config; nothing here should grow a
 *  glossary-specific branch that a config parameter could carry instead.
 *
 *  **Every user-visible string is a `COPY` entry**, never a JSX literal —
 *  `eslint-rules/no-raw-jsx-text.mjs` enforces this on every file except
 *  `App.tsx`'s own pre-existing text, and this is new code.
 *
 *  **Scope note, recorded rather than silently narrowed**: relations for
 *  every term in the glossary are fetched once, up front, so the tree's own
 *  shape (parents, poly-hierarchy, cycles) is known before anything renders.
 *  "Lazy expansion" in the acceptance criteria is satisfied at the *UI*
 *  layer — antd's `Tree` starts fully collapsed and expands on click, so a
 *  large vocabulary is never rendered open — but the underlying *fetch* is
 *  not yet lazy per node. A glossary large enough for that to matter is a
 *  real, separate optimisation (a per-node `GET .../relations` on expand,
 *  the way `App.tsx`'s own asset tree already does for children), deferred
 *  rather than built speculatively for Slice A's one-vocabulary scope. */

import { useCallback, useEffect, useMemo, useState } from "react";
import { Alert, Empty, Layout, Space, Spin, Tag, Tree, Typography } from "antd";
import type { DataNode } from "antd/es/tree";
import type { Key } from "react";
import { api, type Glossary, type GlossaryTerm, type SkosRelation } from "../../api";
import { buildVocabularyTree, type VocabularyTreeNode } from "./vocabularyTree";
import { readParam, writeParam } from "./deepLink";

const { Sider } = Layout;
const { Text, Title, Paragraph } = Typography;

const COPY = {
  loading: "Loading vocabulary…",
  loadError: "This vocabulary could not be loaded.",
  emptyTitle: "No terms yet",
  emptyDescription: "This glossary has no terms. Add one to start building its hierarchy.",
  cyclicSuffix: " (cycle)",
  cyclicNotice:
    "This term and one of its ancestors declare each other as broader — shown once, here, to break the loop rather than repeat forever.",
  detailPlaceholder: "Select a term to see its definition, relations, and where it is used.",
  treeLabel: "Vocabulary terms",
  relationsHeading: "Relations",
  noRelations: "No relations recorded.",
  usageHeading: "Used on",
  noUsage: "Not attached to any asset yet.",
  definitionHeading: "Definition",
  statusHeading: "Status",
  synonymsHeading: "Synonyms",
};

const RELATION_LABEL: Record<SkosRelation["kind"], string> = {
  broader: "broader",
  narrower: "narrower",
  related: "related",
  exactMatch: "exact match",
  closeMatch: "close match",
};

function toDataNode(node: VocabularyTreeNode): DataNode {
  return {
    key: node.renderKey,
    title: node.term.name + (node.isCyclic ? COPY.cyclicSuffix : ""),
    children: node.children.map(toDataNode),
    isLeaf: node.children.length === 0,
  };
}

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

export interface VocabularyBrowserProps {
  /** Which glossary to open. Falls back to the first glossary the server
   *  lists, and to the `term` query param for the initial selection —
   *  Slice A's own deep-link criterion. */
  glossaryId?: string;
}

export function VocabularyBrowser({ glossaryId: forcedGlossaryId }: VocabularyBrowserProps) {
  const [glossaries, setGlossaries] = useState<Glossary[] | null>(null);
  const [glossaryId, setGlossaryId] = useState<string | null>(
    () => forcedGlossaryId ?? readParam("vocabulary"),
  );
  const [terms, setTerms] = useState<GlossaryTerm[] | null>(null);
  const [relationsByTerm, setRelationsByTerm] =
    useState<ReadonlyMap<string, readonly SkosRelation[]> | null>(null);
  const [selectedTermId, setSelectedTermId] = useState<string | null>(() => readParam("term"));
  const [selectedRelations, setSelectedRelations] = useState<SkosRelation[] | null>(null);
  const [usage, setUsage] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (forcedGlossaryId) return;
    api
      .glossaries()
      .then(setGlossaries)
      .catch(() => setError(COPY.loadError));
  }, [forcedGlossaryId]);

  useEffect(() => {
    if (forcedGlossaryId || glossaryId !== null) return;
    const first = glossaries?.[0];
    if (first) {
      setGlossaryId(first.id);
      writeParam("vocabulary", first.id);
    }
  }, [forcedGlossaryId, glossaries, glossaryId]);

  useEffect(() => {
    if (glossaryId === null) return;
    let cancelled = false;
    api
      .glossaryTerms(glossaryId)
      .then(async (fetchedTerms) => {
        const relations = await Promise.all(fetchedTerms.map((t) => api.termRelations(t.id)));
        if (cancelled) return;
        setTerms(fetchedTerms);
        setRelationsByTerm(new Map(fetchedTerms.map((t, i) => [t.id, relations[i] ?? []])));
      })
      .catch(() => {
        if (!cancelled) setError(COPY.loadError);
      });
    return () => {
      cancelled = true;
    };
  }, [glossaryId]);

  const tree = useMemo(() => {
    if (terms === null || relationsByTerm === null) return null;
    return buildVocabularyTree(
      terms.map((t) => ({ id: t.id, name: t.name })),
      relationsByTerm,
    );
  }, [terms, relationsByTerm]);

  const byRenderKey = useMemo(() => (tree ? indexByRenderKey(tree.roots) : new Map()), [tree]);
  const treeData = useMemo(() => tree?.roots.map(toDataNode) ?? [], [tree]);
  const selectedTerm = useMemo(
    () => terms?.find((t) => t.id === selectedTermId) ?? null,
    [terms, selectedTermId],
  );

  // Every render position sharing the selected term's identity is
  // highlighted — the poly-hierarchy criterion applied to selection state,
  // not just to rendering: a term selected under one parent must not look
  // unselected under its other parent.
  const selectedKeys = useMemo(() => {
    if (selectedTermId === null) return [];
    return [...byRenderKey.values()]
      .filter((node) => node.termId === selectedTermId)
      .map((node) => node.renderKey);
  }, [byRenderKey, selectedTermId]);

  useEffect(() => {
    if (selectedTermId === null) {
      setSelectedRelations(null);
      setUsage(null);
      return;
    }
    let cancelled = false;
    Promise.all([api.termRelations(selectedTermId), api.termUsage(selectedTermId)])
      .then(([relations, usagePage]) => {
        if (cancelled) return;
        setSelectedRelations(relations);
        setUsage(usagePage.data);
      })
      .catch(() => {
        if (!cancelled) {
          setSelectedRelations([]);
          setUsage([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selectedTermId]);

  const handleSelect = useCallback(
    (_keys: Key[], info: { node: { key: Key } }) => {
      // `info.node` — the exact node the click landed on — not `_keys[0]`.
      // With `multiple` set (needed so every occurrence of a poly-hierarchy
      // term highlights, not only the one that was clicked), antd's own
      // `selectedKeys` array can already include prior selections by the
      // time this fires, so its first entry is not reliably "the one just
      // clicked". `info.node` has no such ambiguity, and using it means
      // every click *replaces* the identity-based selection rather than
      // accumulating antd's own per-position bookkeeping.
      const key = info.node.key;
      if (typeof key !== "string") return;
      const node = byRenderKey.get(key);
      if (!node) return;
      setSelectedTermId(node.termId);
      writeParam("term", node.termId);
    },
    [byRenderKey],
  );

  if (error) return <Alert type="error" message={error} showIcon />;

  if (terms === null) {
    return (
      <Space direction="vertical" align="center" style={{ width: "100%", padding: 48 }}>
        <Spin />
        <Text>{COPY.loading}</Text>
      </Space>
    );
  }

  if (terms.length === 0) {
    return <Empty description={<Text>{COPY.emptyDescription}</Text>}>{COPY.emptyTitle}</Empty>;
  }

  return (
    <Layout style={{ background: "transparent" }}>
      <Sider width={320} style={{ background: "transparent" }}>
        <Tree
          showLine
          multiple
          treeData={treeData}
          selectedKeys={selectedKeys}
          onSelect={handleSelect}
          aria-label={COPY.treeLabel}
        />
      </Sider>
      {/* A plain `div`, not antd's `Layout.Content` — `Content` renders a
          semantic `<main>`, and this panel sits inside `App.tsx`'s own
          `<main>` for whichever section is active. A nested `<main>` is an
          axe `landmark-is-unique` violation (found running this slice's own
          Playwright journey against a real page, not assumed): two
          landmarks the same assistive-technology role, neither named,
          nested one inside the other. */}
      <div style={{ flex: "auto", paddingLeft: 24 }}>
        {selectedTerm ? (
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            <Title level={4}>{selectedTerm.name}</Title>
            {selectedTerm.status === "deprecated" ? (
              <Tag color="red">{selectedTerm.status}</Tag>
            ) : (
              <Tag>{selectedTerm.status}</Tag>
            )}
            <div>
              <Text strong>{COPY.definitionHeading}</Text>
              <Paragraph>{selectedTerm.definition}</Paragraph>
            </div>
            {byRenderKey.get(selectedKeys[0] ?? "")?.isCyclic && (
              <Alert type="warning" showIcon message={COPY.cyclicNotice} />
            )}
            <div>
              <Text strong>{COPY.relationsHeading}</Text>
              {selectedRelations === null ? (
                <Spin size="small" />
              ) : selectedRelations.length === 0 ? (
                <Paragraph type="secondary">{COPY.noRelations}</Paragraph>
              ) : (
                <ul>
                  {selectedRelations.map((relation) => (
                    <li key={`${relation.kind}:${relation.target}`}>
                      <Tag>{RELATION_LABEL[relation.kind]}</Tag> {relation.target}
                    </li>
                  ))}
                </ul>
              )}
            </div>
            <div>
              <Text strong>{COPY.usageHeading}</Text>
              {usage === null ? (
                <Spin size="small" />
              ) : usage.length === 0 ? (
                <Paragraph type="secondary">{COPY.noUsage}</Paragraph>
              ) : (
                <ul>
                  {usage.map((fqn) => (
                    <li key={fqn}>{fqn}</li>
                  ))}
                </ul>
              )}
            </div>
          </Space>
        ) : (
          <Empty description={<Text>{COPY.detailPlaceholder}</Text>} />
        )}
      </div>
    </Layout>
  );
}
