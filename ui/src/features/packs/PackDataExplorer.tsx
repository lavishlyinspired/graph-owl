/** The Explore sider's "Pack data" block — Plan 115 Slice B1.
 *
 *  **Why this block exists at all.** Explore's hierarchy is the catalog asset
 *  tree (`database → schema → table`), and imported pack data deliberately
 *  does not live there — it is graph flakes in named import graphs, not
 *  relational assets (see `plans/115`). But "not in the tree" was landing as
 *  "not findable": a CA who uploads a GST file gets a toast naming
 *  `gst-gstr2b-2025-07` and then has no idea where to see that name again.
 *  This block is the answer: the installed packs, and the graph's own named
 *  import graphs filed under them.
 *
 *  **It reads the graph, not the tree.** One `GROUP BY` over the named graphs
 *  returns every import source with its triple count, and `/namespaces`
 *  returns which packs are installed — the same two sources of truth the
 *  reconciliation workspace uses, so this block can never report a source
 *  the reconciliation does not hold. A connector's imports are real import
 *  graphs and are parsed, but file under their own (non-pack) id and are
 *  not offered as a pack's data.
 *
 *  **The asset tree is untouched.** This is a sibling, not a subtree: a pack
 *  subject is not an `Asset`, and shoehorning it into the hierarchy would
 *  blur two different concepts (`plans/115` rejected that path).
 *
 *  **Absent is the default, not an error.** A deployment with no pack renders
 *  nothing at all — a heading with nothing under it reads as broken. */

import { useEffect, useState } from "react";
import { Space, Tag, Typography } from "./../../components/ui/antd-compat";
import { DatabaseOutlined } from "@ant-design/icons";
import { api } from "../../api";
import {
  loadedSourcesFromSparql,
  NAMED_GRAPHS_QUERY,
  sourcesForPack,
  type LoadedSource,
} from "./packData";
import { installedPacks, type InstalledPack } from "./packSurfaces";

const { Text, Paragraph } = Typography;

const COPY = {
  heading: "PACK DATA",
  nothingLoaded: "Nothing imported yet — upload from the Reconciliation section.",
  hint: "Pack data lives in the graph, outside the asset tree above. Open a source to see its subjects here; the Reconciliation is where you work a period.",
};

export function PackDataExplorer({
  onOpen,
}: {
  /** Called with the clicked source and its pack's label, so the caller can
   *  show that source's data — the caller decides where the click lands, not
   *  this block. */
  onOpen: (source: LoadedSource, packLabel: string) => void;
}) {
  const [packs, setPacks] = useState<readonly InstalledPack[] | null>(null);
  const [sources, setSources] = useState<readonly LoadedSource[]>([]);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const namespaces = await api.namespaces();
        const graphs = await api.sparql(NAMED_GRAPHS_QUERY);
        if (!live) return;
        setPacks(installedPacks(namespaces));
        setSources(loadedSourcesFromSparql(graphs.rows));
      } catch {
        if (live) setFailed(true);
      }
    })();
    return () => {
      live = false;
    };
  }, []);

  // A deployment with no pack, or a graph this block could not read, renders
  // nothing: the block is an aid to navigation, and a broken aid is worse
  // than none — it would sit at the bottom of the tree looking like the tree
  // is failing.
  if (failed || packs === null || packs.length === 0) return null;

  return (
    <div style={{ marginTop: 24 }}>
      <Text type="secondary" style={{ fontSize: 11, fontWeight: 600, letterSpacing: "0.06em" }}>
        {COPY.heading}
      </Text>
      <Space direction="vertical" size={12} style={{ marginTop: 10, width: "100%" }}>
        {packs.map((pack) => {
          const mine = sourcesForPack(sources, pack.packId);
          return (
            <div key={pack.packId}>
              <Text strong style={{ fontSize: 13 }}>
                {pack.label}
              </Text>
              {mine.length === 0 ? (
                <Text type="secondary" style={{ display: "block", fontSize: 12 }}>
                  {COPY.nothingLoaded}
                </Text>
              ) : (
                <Space direction="vertical" size={2} style={{ width: "100%", marginTop: 4 }}>
                  {mine.map((source) => (
                    // The source name a successful upload printed in its toast
                    // (C1) — matched here to the same graph, one period per
                    // line, so "where did my upload go" has an answer.
                    <Text
                      key={source.name}
                      type="secondary"
                      style={{ cursor: "pointer", display: "block", fontSize: 12 }}
                      onClick={() => onOpen(source, pack.label)}
                    >
                      <DatabaseOutlined style={{ marginRight: 6 }} />
                      {source.name}
                      <Tag style={{ marginLeft: 8, fontSize: 11 }}>{source.triples}</Tag>
                    </Text>
                  ))}
                </Space>
              )}
            </div>
          );
        })}
      </Space>
      <Paragraph type="secondary" style={{ fontSize: 11, marginTop: 8, marginBottom: 0 }}>
        {COPY.hint}
      </Paragraph>
    </div>
  );
}
