/** The tree + detail pane shell — Epic 42 decision 1. Every difference
 *  between glossary, classification, domain and ontology pack lives in a
 *  `VocabularyConfig` (`vocabularies.ts`); this file names none of them.
 *  If a vocabulary-specific `if`/`switch` ever appears here, the pattern
 *  has failed and the fifth vocabulary is not actually free —
 *  `vocabularyStructure.test.ts`'s own structural test asserts exactly
 *  that, by reading this file's source rather than trusting intent.
 *
 *  **Every user-visible string not sourced from `config` is a `COPY`
 *  entry**, never a JSX literal — `eslint-rules/no-raw-jsx-text.mjs`
 *  enforces this on every file except `App.tsx`'s own pre-existing text,
 *  and this is new code. */

import { useCallback, useEffect, useMemo, useState } from "react";
import { Alert, Empty, Layout, Space, Spin, Tag, Tree, Typography } from "antd";
import type { DataNode } from "antd/es/tree";
import type { Key } from "react";
import { buildVocabularyTree, type VocabularyTreeNode } from "./vocabularyTree";
import type { VocabularyConfig, VocabularyData, VocabularyDetail } from "./vocabularies";
import { readParam, writeParam } from "../deepLink";

const { Sider } = Layout;
const { Text, Title, Paragraph } = Typography;

const COPY = {
  loading: "Loading vocabulary…",
  loadError: "This vocabulary could not be loaded.",
  cyclicSuffix: " (cycle)",
  cyclicNotice:
    "This term and one of its ancestors declare each other as broader — shown once, here, to break the loop rather than repeat forever.",
  detailPlaceholder: "Select an item to see its detail, relations, and where it is used.",
  noRelations: "No relations recorded.",
  noUsage: "Nothing recorded yet.",
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
  readonly config: VocabularyConfig;
}

export function VocabularyBrowser({ config }: VocabularyBrowserProps) {
  const [data, setData] = useState<VocabularyData | null>(null);
  const [selectedItemId, setSelectedItemId] = useState<string | null>(() => readParam("term"));
  const [detail, setDetail] = useState<VocabularyDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setData(null);
    setError(null);
    config
      .fetchData()
      .then((fetched) => {
        if (!cancelled) setData(fetched);
      })
      .catch(() => {
        if (!cancelled) setError(COPY.loadError);
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed by `config.key` at the call site, not by object identity
  }, [config.key]);

  const tree = useMemo(() => {
    if (data === null) return null;
    return buildVocabularyTree(data.items, data.relationsByItem);
  }, [data]);

  const byRenderKey = useMemo(() => (tree ? indexByRenderKey(tree.roots) : new Map()), [tree]);
  const treeData = useMemo(() => tree?.roots.map(toDataNode) ?? [], [tree]);

  // Every render position sharing the selected item's identity is
  // highlighted — the poly-hierarchy criterion applied to selection state,
  // not just to rendering: an item selected under one parent must not look
  // unselected under its other parent.
  const selectedKeys = useMemo(() => {
    if (selectedItemId === null) return [];
    return [...byRenderKey.values()]
      .filter((node) => node.termId === selectedItemId)
      .map((node) => node.renderKey);
  }, [byRenderKey, selectedItemId]);

  useEffect(() => {
    if (selectedItemId === null || data === null) {
      setDetail(null);
      return;
    }
    let cancelled = false;
    config
      .detailFor(selectedItemId, data)
      .then((fetched) => {
        if (!cancelled) setDetail(fetched);
      })
      .catch(() => {
        if (!cancelled) setDetail(null);
      });
    return () => {
      cancelled = true;
    };
  }, [config, selectedItemId, data]);

  const handleSelect = useCallback(
    (_keys: Key[], info: { node: { key: Key } }) => {
      // `info.node` — the exact node the click landed on — not `_keys[0]`.
      // With `multiple` set (needed so every occurrence of a poly-hierarchy
      // item highlights, not only the one that was clicked), antd's own
      // `selectedKeys` array can already include prior selections by the
      // time this fires, so its first entry is not reliably "the one just
      // clicked". `info.node` has no such ambiguity, and using it means
      // every click *replaces* the identity-based selection rather than
      // accumulating antd's own per-position bookkeeping.
      const key = info.node.key;
      if (typeof key !== "string") return;
      const node = byRenderKey.get(key);
      if (!node) return;
      setSelectedItemId(node.termId);
      writeParam("term", node.termId);
    },
    [byRenderKey],
  );

  if (error) return <Alert type="error" message={error} showIcon />;

  if (data === null) {
    return (
      <Space direction="vertical" align="center" style={{ width: "100%", padding: 48 }}>
        <Spin />
        <Text>{COPY.loading}</Text>
      </Space>
    );
  }

  if (data.items.length === 0) {
    return <Empty description={<Text>{config.emptyDescription}</Text>}>{config.emptyTitle}</Empty>;
  }

  return (
    <Layout style={{ background: "transparent" }}>
      <Sider width={320} style={{ background: "transparent" }}>
        {config.readOnlyNotice && (
          <Alert
            type="info"
            showIcon
            message={config.readOnlyNotice}
            style={{ marginBottom: 12 }}
          />
        )}
        <Tree
          showLine
          multiple
          treeData={treeData}
          selectedKeys={selectedKeys}
          onSelect={handleSelect}
          aria-label={config.treeLabel}
        />
      </Sider>
      {/* A plain `div`, not antd's `Layout.Content` — `Content` renders a
          semantic `<main>`, and this panel sits inside `App.tsx`'s own
          `<main>` for whichever section is active. A nested `<main>` is an
          axe `landmark-is-unique` violation (found running Slice A's own
          Playwright journey against a real page, not assumed): two
          landmarks of the same assistive-technology role, neither named,
          nested one inside the other. */}
      <div style={{ flex: "auto", paddingLeft: 24 }}>
        {detail ? (
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            {/* `level={2}`, not the visually-matching `4` — the chrome's own
                `<h1>graph-owl</h1>` is the only heading above this one on
                the page, and axe's `heading-order` rule flags any jump
                that skips a level. `fontSize` pins the pre-existing visual
                size explicitly, the same way `App.tsx`'s own `level={2}`
                titles already decouple semantic level from appearance. */}
            <Title level={2} style={{ margin: 0, fontSize: 20, fontWeight: 600 }}>
              {detail.title}
            </Title>
            {byRenderKey.get(selectedKeys[0] ?? "")?.isCyclic && (
              <Alert type="warning" showIcon message={COPY.cyclicNotice} />
            )}
            {detail.fields.map((field) => (
              <div key={field.label}>
                <Text strong>{field.label}</Text>
                <Paragraph>{field.value}</Paragraph>
              </div>
            ))}
            {detail.relationsLabel && (
              <div>
                <Text strong>{detail.relationsLabel}</Text>
                {detail.relations.length === 0 ? (
                  <Paragraph type="secondary">{COPY.noRelations}</Paragraph>
                ) : (
                  <ul>
                    {detail.relations.map((relation) => (
                      <li key={`${relation.label}:${relation.target}`}>
                        <Tag>{relation.label}</Tag> {relation.target}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
            {detail.usageLabel && (
              <div>
                <Text strong>{detail.usageLabel}</Text>
                {detail.usage.length === 0 ? (
                  <Paragraph type="secondary">{COPY.noUsage}</Paragraph>
                ) : (
                  <ul>
                    {detail.usage.map((entry) => (
                      <li key={entry}>{entry}</li>
                    ))}
                  </ul>
                )}
              </div>
            )}
          </Space>
        ) : (
          <Empty description={<Text>{COPY.detailPlaceholder}</Text>} />
        )}
      </div>
    </Layout>
  );
}
