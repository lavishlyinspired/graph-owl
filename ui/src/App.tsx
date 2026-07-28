import { useCallback, useEffect, useMemo, useState } from "react";
import {
  App as AntApp,
  Breadcrumb,
  Button,
  Card,
  Col,
  ConfigProvider,
  Descriptions,
  Empty,
  Flex,
  Form,
  Input,
  Layout,
  Menu,
  Popover,
  Row,
  Space,
  Statistic,
  Table,
  Tag,
  Timeline,
  Tabs,
  Tooltip,
  Tree,
  Typography,
} from "antd";
import { theme as antdTheme } from "antd";
import type { DataNode } from "antd/es/tree";
import ApartmentOutlined from "@ant-design/icons/es/icons/ApartmentOutlined";
import ArrowLeftOutlined from "@ant-design/icons/es/icons/ArrowLeftOutlined";
import BulbOutlined from "@ant-design/icons/es/icons/BulbOutlined";
import CheckCircleFilled from "@ant-design/icons/es/icons/CheckCircleFilled";
import ClockCircleOutlined from "@ant-design/icons/es/icons/ClockCircleOutlined";
import CloudServerOutlined from "@ant-design/icons/es/icons/CloudServerOutlined";
import CompassOutlined from "@ant-design/icons/es/icons/CompassOutlined";
import DashboardOutlined from "@ant-design/icons/es/icons/DashboardOutlined";
import DatabaseOutlined from "@ant-design/icons/es/icons/DatabaseOutlined";
import FolderOutlined from "@ant-design/icons/es/icons/FolderOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import SafetyCertificateOutlined from "@ant-design/icons/es/icons/SafetyCertificateOutlined";
import SearchOutlined from "@ant-design/icons/es/icons/SearchOutlined";
import TableOutlined from "@ant-design/icons/es/icons/TableOutlined";
import TagOutlined from "@ant-design/icons/es/icons/TagOutlined";
import ThunderboltOutlined from "@ant-design/icons/es/icons/ThunderboltOutlined";
import EditOutlined from "@ant-design/icons/es/icons/EditOutlined";
import HistoryOutlined from "@ant-design/icons/es/icons/HistoryOutlined";
import UserOutlined from "@ant-design/icons/es/icons/UserOutlined";
import {
  type Asset,
  type AssetKind,
  type AssetVersion,
  type ChangeDescription,
  type Facet,
  type GraphEdge,
  type Overview,
  type SearchFacets,
  ApiError,
  api,
  authToken,
  isUnauthenticated,
  setAuthToken,
} from "./api";
import { type DiffEdge, diff } from "./graph/diff";
import { type GraphModel, expand, seed } from "./graph/model";
import { brand, darkTheme, lightTheme, palette } from "./theme";
import { GenericSourceMark, PostgresMark } from "./icons";
import watermarkImg from "./assets/watermark1.png";

const { Header, Sider, Content } = Layout;
const { Text, Title, Paragraph } = Typography;

type Section = "overview" | "explore" | "connectors";

const KIND_ICON: Record<AssetKind, React.ReactNode> = {
  service: <CloudServerOutlined />,
  database: <DatabaseOutlined />,
  schema: <FolderOutlined />,
  table: <TableOutlined />,
  column: <TagOutlined />,
};

/** Kind colours walk the logo's own gradient — navy through blue and indigo to
 *  teal — so depth in the hierarchy reads as a colour progression rather than
 *  five unrelated hues. */
const KIND_COLOR: Record<AssetKind, string> = {
  service: brand.navy800,
  database: brand.blue600,
  schema: brand.indigo400,
  table: brand.teal500,
  column: "default",
};

/** The connector catalogue. Postgres is implemented; the rest are listed as
 *  unavailable rather than hidden, because a buyer's first question is "do you
 *  support X" and an empty list answers it wrongly. */
const CONNECTORS = [
  { id: "postgres", name: "PostgreSQL", blurb: "Schemas, tables, views and columns via information_schema.", available: true },
  { id: "mysql", name: "MySQL", blurb: "Relational metadata from the MySQL catalog.", available: false },
  { id: "snowflake", name: "Snowflake", blurb: "Databases, schemas, tables and column types.", available: false },
  { id: "bigquery", name: "BigQuery", blurb: "Datasets, tables and partitioning metadata.", available: false },
  { id: "kafka", name: "Kafka", blurb: "Topics, partitions and registered schemas.", available: false },
  { id: "airflow", name: "Airflow", blurb: "DAGs, tasks and pipeline lineage.", available: false },
];

function Fqn({ children }: { children: string }) {
  return <Text code>{children}</Text>;
}

function readParam(name: string): string | null {
  return new URLSearchParams(window.location.search).get(name);
}

function writeParam(name: string, value: string | null) {
  const params = new URLSearchParams(window.location.search);
  if (value === null) params.delete(name);
  else params.set(name, value);
  const query = params.toString();
  window.history.replaceState(null, "", query ? `?${query}` : window.location.pathname);
}

// Versioned, because the key is the reset mechanism. A preference persisted by
// an older build — a dark theme left behind by a screenshot session, say — is
// not worth migrating, and asking someone to clear site data to get their
// background back is not an answer. Bumping the suffix retires the old value.
const THEME_KEY = "graphowl.theme.v2";

// The retired key is inert once nothing reads it, but leaving it behind means
// the next person to open devtools finds two theme keys and has to work out
// which one is live.
localStorage.removeItem("theme");

function useTheme() {
  // Light by default, persisted, and overridable from the URL. The URL matters
  // beyond convenience: it makes a theme deep-linkable, so a screenshot in a
  // bug report can be reproduced exactly.
  const [dark, setDark] = useState(
    () => (readParam("theme") ?? localStorage.getItem(THEME_KEY)) === "dark",
  );
  useEffect(() => {
    localStorage.setItem(THEME_KEY, dark ? "dark" : "light");
  }, [dark]);
  return { dark, toggle: () => setDark((d) => !d) };
}

/** What the catalog knows about an asset's trustworthiness. Each item either
 *  carries a fact or says plainly that nothing is known yet — a confident-
 *  looking blank is worse than an admission. */
function TrustBar({ asset }: { asset: Asset }) {
  const version = `v${asset.version.major}.${asset.version.minor}`;
  return (
    <Card size="small" styles={{ body: { padding: "8px 14px" } }}>
      <Space size="large" wrap>
        <Text style={{ fontSize: 13 }}>
          <ClockCircleOutlined /> <Text strong>{version}</Text>{" "}
          <Text type="secondary">
            {asset.version.minor === 1 && asset.version.major === 0
              ? "as catalogued"
              : "edited"}
          </Text>
        </Text>
        <Text style={{ fontSize: 13 }}>
          <UserOutlined /> <Text type="secondary">last change by</Text>{" "}
          <Text strong>{asset.updatedBy}</Text>
        </Text>
        {asset.deleted && <Tag color="red">deleted</Tag>}
        <Text type="secondary" style={{ fontSize: 13 }}>
          <SafetyCertificateOutlined /> uncertified
        </Text>
        <Text type="secondary" style={{ fontSize: 13 }}>
          <ApartmentOutlined /> lineage not captured
        </Text>
      </Space>
    </Card>
  );
}

function renderChange(change: ChangeDescription | null | undefined) {
  if (!change) return <Text type="secondary">created</Text>;
  const rows = [
    ...change.fieldsAdded.map((c) => ({ ...c, verb: "set" })),
    ...change.fieldsUpdated.map((c) => ({ ...c, verb: "changed" })),
    ...change.fieldsDeleted.map((c) => ({ ...c, verb: "cleared" })),
  ];
  if (rows.length === 0) return <Text type="secondary">no field changes</Text>;
  return (
    <Space direction="vertical" size={2}>
      {rows.map((row) => (
        <Text key={`${row.verb}-${row.field}`} style={{ fontSize: 13 }}>
          <Text strong>{row.field}</Text> <Text type="secondary">{row.verb}</Text>
          {/* Both sides, because an audit trail without the previous value
              cannot answer "what did it say before". */}
          {row.before != null && (
            <>
              {" "}
              <Text delete type="secondary">
                {String(row.before)}
              </Text>
            </>
          )}
          {row.after != null && <> → {String(row.after)}</>}
        </Text>
      ))}
    </Space>
  );
}

/** The landing page.
 *
 *  Every tile is a number the system already knows and can defend. That
 *  constraint is the design, not a limitation of it: a dashboard of
 *  plausible-looking numbers nobody can act on is worse than a smaller one
 *  that is true, and it is discovered by the first person who clicks a tile.
 *
 *  Certification, quality and lineage tiles are deliberately absent — they
 *  need Epics 22–38, and rendering them over invented data would make the
 *  console lie about the maturity of the product it fronts. */
function OverviewPage({
  colors,
  onOpen,
  onAddSource,
}: {
  colors: (typeof palette)["light"];
  onOpen: (asset: Asset) => void;
  onAddSource: () => void;
}) {
  const [data, setData] = useState<Overview | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    api
      .overview()
      .then(setData)
      .catch(() => setFailed(true));
  }, []);

  if (failed) return <Empty description="Could not load the overview" />;
  if (!data) return <Text type="secondary">Loading…</Text>;

  const { assets, documentation, graph, recentlyChanged } = data;
  const coverage =
    documentation.total === 0
      ? 0
      : Math.round((documentation.described / documentation.total) * 100);

  // A new deployment gets an action, not a chart of zeros.
  if (assets.total === 0) {
    return (
      <Flex vertical align="center" justify="center" style={{ height: "100%" }} gap={16}>
        <Title level={4} style={{ margin: 0, fontWeight: 600 }}>
          Nothing catalogued yet
        </Title>
        <Text type="secondary">
          graph-owl reads a source&apos;s structure and builds a browsable hierarchy from it.
        </Text>
        <Button type="primary" icon={<PlusOutlined />} onClick={onAddSource}>
          Catalogue a source
        </Button>
      </Flex>
    );
  }

  const tile = (label: string, value: React.ReactNode, hint?: React.ReactNode) => (
    <Card size="small" style={{ height: "100%" }}>
      <Text type="secondary" style={{ fontSize: 12 }}>
        {label}
      </Text>
      <div style={{ fontSize: 26, fontWeight: 600, lineHeight: 1.2, marginTop: 4 }}>{value}</div>
      {hint && (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {hint}
        </Text>
      )}
    </Card>
  );

  // Projection lag, surfaced. A node count trailing the asset total means the
  // graph view is behind — a number on the page beats a log line nobody reads.
  const graphHint =
    graph === null
      ? "no graph engine configured"
      : `${graph.flakes.toLocaleString()} facts projected`;

  return (
    <Space direction="vertical" size="large" style={{ width: "100%" }}>
      <div>
        <Title level={4} style={{ margin: 0, fontWeight: 600 }}>
          Overview
        </Title>
        <Text type="secondary">
          Everything below is filtered to what you are allowed to see.
        </Text>
      </div>

      <Row gutter={[16, 16]}>
        <Col xs={24} sm={12} lg={6}>
          {tile("Assets", assets.total.toLocaleString(), "across every source")}
        </Col>
        <Col xs={24} sm={12} lg={6}>
          {tile(
            "Documented",
            <span style={{ color: coverage < 50 ? colors.warning : colors.success }}>
              {coverage}%
            </span>,
            `${documentation.described.toLocaleString()} of ${documentation.total.toLocaleString()} have a description`,
          )}
        </Col>
        <Col xs={24} sm={12} lg={6}>
          {tile("Graph", graph ? graph.flakes.toLocaleString() : "—", graphHint)}
        </Col>
        <Col xs={24} sm={12} lg={6}>
          {tile(
            "Kinds",
            assets.byKind.length,
            assets.byKind.map((k) => k.kind).join(" · "),
          )}
        </Col>
      </Row>

      <Row gutter={[16, 16]}>
        <Col xs={24} lg={10}>
          <Card size="small" title="What is in the catalog">
            <Space direction="vertical" size={10} style={{ width: "100%" }}>
              {assets.byKind
                .slice()
                .sort((a, b) => b.count - a.count)
                .map((row) => {
                  const share = assets.total === 0 ? 0 : (row.count / assets.total) * 100;
                  return (
                    <div key={row.kind}>
                      <Flex justify="space-between" style={{ marginBottom: 4 }}>
                        {/* The label carries the meaning, not the bar colour —
                            a reader who cannot distinguish the hues still gets
                            the full answer. */}
                        <Text style={{ fontSize: 13 }}>{row.kind}</Text>
                        <Text style={{ fontSize: 13, fontVariantNumeric: "tabular-nums" }}>
                          {row.count.toLocaleString()}
                        </Text>
                      </Flex>
                      <div
                        style={{
                          height: 8,
                          borderRadius: 4,
                          background: colors.fill,
                          overflow: "hidden",
                        }}
                        role="img"
                        aria-label={`${row.kind}: ${row.count} of ${assets.total}`}
                      >
                        <div
                          style={{
                            width: `${share}%`,
                            height: "100%",
                            background:
                              KIND_COLOR[row.kind] === "default"
                                ? colors.textSubtle
                                : KIND_COLOR[row.kind],
                          }}
                        />
                      </div>
                    </div>
                  );
                })}
            </Space>
          </Card>
        </Col>

        <Col xs={24} lg={14}>
          <Card size="small" title="Recently changed">
            {recentlyChanged.length === 0 ? (
              <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="No changes yet" />
            ) : (
              <Table
                size="small"
                rowKey="id"
                dataSource={recentlyChanged}
                pagination={false}
                onRow={(row) => ({ onClick: () => onOpen(row), style: { cursor: "pointer" } })}
                columns={[
                  {
                    title: "Name",
                    dataIndex: "name",
                    key: "name",
                    render: (name: string, row: Asset) => (
                      <Space size={6}>
                        <Tag color={KIND_COLOR[row.kind]} icon={KIND_ICON[row.kind]}>
                          {row.kind}
                        </Tag>
                        <Text style={{ fontWeight: 500 }}>{name}</Text>
                      </Space>
                    ),
                  },
                  {
                    title: "Version",
                    key: "version",
                    width: 90,
                    render: (_, row: Asset) => (
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        v{row.version.major}.{row.version.minor}
                      </Text>
                    ),
                  },
                  {
                    title: "By",
                    dataIndex: "updatedBy",
                    key: "updatedBy",
                    width: 120,
                    render: (by: string) => (
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {by}
                      </Text>
                    ),
                  },
                ]}
              />
            )}
          </Card>
        </Col>
      </Row>
    </Space>
  );
}

/** The graph explorer.
 *
 *  A deterministic **radial** layout rather than a force simulation: rings by
 *  distance from the seed. Two reasons. A force layout settles somewhere
 *  slightly different on every run, so the same neighbourhood never looks the
 *  same twice and nobody can point at "the node on the left". And distance
 *  from the seed is the single most useful thing this picture can encode —
 *  a physics simulation encodes it only incidentally.
 *
 *  Hand-drawn SVG rather than a graph library: the console is served under a
 *  strict CSP from the binary itself, and rings-and-lines does not justify a
 *  WebGL dependency. When a real estate outgrows this — Epic 40's Sigma canvas
 *  — the `GraphView` shape it consumes is already the renderer-agnostic one.
 */
function GraphExplorer({
  assetId,
  asOf,
  colors,
}: {
  assetId: string;
  asOf: string | null;
  colors: (typeof palette)["light"];
}) {
  const [hops, setHops] = useState(2);
  const [model, setModel] = useState<GraphModel | null>(null);
  const [expanding, setExpanding] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The second instant of a comparison. Null is "not comparing" — distinct
  // from comparing against now, which is a real thing to ask for.
  const [compareTo, setCompareToRaw] = useState<string | null>(() =>
    readParam("compareTo"),
  );
  const [baseline, setBaseline] = useState<GraphModel | null>(null);

  const setCompareTo = useCallback((value: string | null) => {
    setCompareToRaw(value);
    writeParam("compareTo", value);
  }, []);

  // The earlier instant is fetched as its own walk from the same seed, at the
  // same depth. Diffing two walks taken at different depths would report the
  // depth difference as change in the estate.
  useEffect(() => {
    if (compareTo === null) {
      setBaseline(null);
      return undefined;
    }
    let current = true;
    setBaseline(null);
    api
      .graph(assetId, hops, compareTo)
      .then((view) => {
        if (current) setBaseline(seed(assetId, view));
      })
      .catch((e: unknown) => {
        if (current) {
          setError(
            e instanceof ApiError ? e.problem.detail : "could not load the earlier graph",
          );
        }
      });
    return () => {
      current = false;
    };
  }, [assetId, hops, compareTo]);

  useEffect(() => {
    let current = true;
    setModel(null);
    setError(null);
    // Expansions are replayed from the URL, in order, on top of the seed —
    // so a pasted link restores the picture the sender was looking at rather
    // than the seed they started from.
    const replay = (readParam("expand") ?? "").split(",").filter(Boolean);
    (async () => {
      let next = seed(assetId, await api.graph(assetId, hops, asOf));
      for (const id of replay) {
        next = expand(next, id, await api.graph(id, 1, asOf));
      }
      if (current) setModel(next);
    })().catch((e: unknown) => {
      if (current) {
        setError(e instanceof ApiError ? e.problem.detail : "could not load the graph");
      }
    });
    return () => {
      current = false;
    };
  }, [assetId, hops, asOf]);

  /** Epic 40 decision 2: the canvas grows by explicit expansion, never by
   *  "show everything". One hop per click, budgeted server-side like every
   *  other walk. */
  const expandNode = useCallback(
    (nodeId: string) => {
      if (!model || model.expanded.includes(nodeId) || expanding) return;
      setExpanding(nodeId);
      api
        .graph(nodeId, 1, asOf)
        .then((view) => {
          setModel((previous) => {
            if (!previous) return previous;
            const next = expand(previous, nodeId, view);
            const grown = next.expanded.filter((id) => id !== next.seedId);
            writeParam("expand", grown.length > 0 ? grown.join(",") : null);
            return next;
          });
        })
        .catch((e: unknown) => {
          setError(e instanceof ApiError ? e.problem.detail : "could not expand that node");
        })
        .finally(() => setExpanding(null));
    },
    [model, expanding, asOf],
  );

  const view = model;

  /** What changed between the two instants, or null when not comparing. */
  const comparison = useMemo(
    () => (baseline && model ? diff(baseline, model) : null),
    [baseline, model],
  );

  /** The nodes and edges to draw. When comparing this is the *union* of both
   *  instants — a node removed since the earlier one has to stay on the canvas
   *  to be shown as removed, which is the entire point of the mode. */
  const picture = useMemo(() => {
    if (comparison) {
      return {
        nodes: comparison.nodes.map((node) => ({
          id: node.id,
          name: node.name,
          kind: node.kind,
        })),
        edges: comparison.edges,
      };
    }
    return model ? { nodes: model.nodes, edges: model.edges } : null;
  }, [comparison, model]);

  /** Change per node, for the canvas. Empty when not comparing. */
  const changeOf = useMemo(
    () => new Map((comparison?.nodes ?? []).map((node) => [node.id, node.change])),
    [comparison],
  );

  const layout = useMemo(() => {
    if (!picture) return null;
    const width = 900;
    const height = 460;
    const centre = { x: width / 2, y: height / 2 };

    // Distance is recovered from the edge set rather than requested from the
    // server a second time: BFS over what was returned is cheap and cannot
    // disagree with the picture being drawn.
    const adjacency = new Map<string, string[]>();
    for (const edge of picture.edges) {
      adjacency.set(edge.from, [...(adjacency.get(edge.from) ?? []), edge.to]);
      adjacency.set(edge.to, [...(adjacency.get(edge.to) ?? []), edge.from]);
    }
    const depth = new Map<string, number>([[assetId, 0]]);
    let frontier = [assetId];
    while (frontier.length > 0) {
      const next: string[] = [];
      for (const node of frontier) {
        for (const neighbour of adjacency.get(node) ?? []) {
          if (!depth.has(neighbour)) {
            depth.set(neighbour, (depth.get(node) ?? 0) + 1);
            next.push(neighbour);
          }
        }
      }
      frontier = next;
    }

    const rings = new Map<number, string[]>();
    for (const node of picture.nodes) {
      // A node with no path in the returned edges still belongs somewhere;
      // the outer ring is honest about it being least connected.
      const d = depth.get(node.id) ?? Math.max(1, hops);
      rings.set(d, [...(rings.get(d) ?? []), node.id]);
    }

    const positions = new Map<string, { x: number; y: number }>();
    const maxRing = Math.max(...[...rings.keys()], 1);
    for (const [ring, ids] of rings) {
      if (ring === 0) {
        positions.set(ids[0] ?? assetId, centre);
        continue;
      }
      const radius = (Math.min(width, height) / 2 - 60) * (ring / maxRing);
      ids.forEach((id, i) => {
        // Offset each ring so successive rings do not align spokes, which
        // makes edges overlap and the picture read as fewer nodes than it has.
        const angle = (2 * Math.PI * i) / ids.length + ring * 0.4;
        positions.set(id, {
          x: centre.x + radius * Math.cos(angle),
          y: centre.y + radius * Math.sin(angle),
        });
      });
    }
    return { width, height, positions };
  }, [picture, assetId, hops]);

  if (error) {
    return <Empty description={error} />;
  }
  if (!view || !layout || !picture) {
    return <Text type="secondary">Walking the graph…</Text>;
  }

  const byId = new Map(picture.nodes.map((n) => [n.id, n]));

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Flex align="center" gap={12} wrap>
        <Text type="secondary" style={{ fontSize: 13 }}>
          {view.nodes.length} nodes · {view.edges.length} edges
        </Text>
        <Space size={4}>
          {[1, 2, 3].map((n) => (
            <Button
              key={n}
              size="small"
              type={hops === n ? "primary" : "default"}
              onClick={() => setHops(n)}
            >
              {n} hop{n === 1 ? "" : "s"}
            </Button>
          ))}
        </Space>
        {view.truncated && (
          <Tag color="warning">
            truncated — the neighbourhood is larger than shown
          </Tag>
        )}
        {asOf && <Tag color="warning">as of {new Date(asOf).toLocaleString()}</Tag>}
        <Text type="secondary" style={{ fontSize: 12 }}>
          {expanding ? "expanding…" : "click a node to expand it"}
        </Text>
      </Flex>

      {/* Diff mode. Two instants, and what moved between them — the thing this
          graph can show because `op = false` is a retraction rather than a
          delete, and a store that overwrote could not answer it at all. */}
      <Flex align="center" gap={12} wrap>
        <Text type="secondary" style={{ fontSize: 13 }}>
          Compare with
        </Text>
        <input
          type="datetime-local"
          aria-label="Compare the graph with an earlier instant"
          value={compareTo ? compareTo.slice(0, 16) : ""}
          onChange={(e) =>
            setCompareTo(
              e.target.value ? new Date(e.target.value).toISOString() : null,
            )
          }
          style={{
            background: "transparent",
            color: colors.text,
            border: `1px solid ${colors.border}`,
            borderRadius: 8,
            padding: "2px 8px",
            fontSize: 13,
          }}
        />
        {compareTo && (
          <Button size="small" onClick={() => setCompareTo(null)}>
            Stop comparing
          </Button>
        )}
        {comparison && (
          <Space size={4}>
            <Tag color="success">+{comparison.summary.added} added</Tag>
            <Tag color="error">−{comparison.summary.removed} removed</Tag>
            <Tag color="processing">~{comparison.summary.changed} changed</Tag>
          </Space>
        )}
        {/* A partial comparison presented as complete would invent deletions
            that never happened: a node "missing" at one instant may simply not
            have been fetched. */}
        {comparison?.partial && (
          <Tag color="warning">
            partial — one side was truncated, so an absence may be an omission
          </Tag>
        )}
        {compareTo && !comparison && !error && (
          <Text type="secondary" style={{ fontSize: 12 }}>
            loading the earlier graph…
          </Text>
        )}
      </Flex>

      <div style={{ overflowX: "auto", border: `1px solid ${colors.border}`, borderRadius: 16 }}>
        <svg
          viewBox={`0 0 ${layout.width} ${layout.height}`}
          style={{ width: "100%", minWidth: 640, display: "block", background: colors.raised }}
          role="img"
          aria-label={
            comparison
              ? `Graph comparison: ${comparison.summary.added} added, ${comparison.summary.removed} removed, ${comparison.summary.changed} changed`
              : `Graph neighbourhood: ${picture.nodes.length} nodes, ${picture.edges.length} edges`
          }
        >
          {picture.edges.map((edge, i) => {
            const a = layout.positions.get(edge.from);
            const b = layout.positions.get(edge.to);
            if (!a || !b) return null;
            const change = "change" in edge ? edge.change : "unchanged";
            return (
              <line
                key={`${edge.from}-${edge.to}-${i}`}
                x1={a.x}
                y1={a.y}
                x2={b.x}
                y2={b.y}
                stroke={change === "unchanged" ? colors.border : colors.text}
                strokeWidth={change === "unchanged" ? 1.5 : 2}
                // Removed edges are dashed and added ones solid-but-heavier,
                // so the distinction survives a greyscale print and a
                // screenshot pasted into a ticket. Not colour alone — Epic 40
                // decision 4.
                strokeDasharray={change === "removed" ? "5 4" : undefined}
                opacity={change === "removed" ? 0.6 : 1}
              />
            );
          })}
          {picture.nodes.map((node) => {
            const at = layout.positions.get(node.id);
            if (!at) return null;
            const isSeed = node.id === assetId;
            const isExpanded = view.expanded.includes(node.id);
            const hidesMore = view.truncatedAt.includes(node.id);
            const change = changeOf.get(node.id);
            const fill = isSeed
              ? colors.primary
              : node.kind
                ? KIND_COLOR[node.kind] === "default"
                  ? colors.textSubtle
                  : KIND_COLOR[node.kind]
                : colors.textDisabled;
            return (
              <g
                key={node.id}
                onClick={() => expandNode(node.id)}
                style={{ cursor: isExpanded ? "default" : "pointer" }}
              >
                <circle
                  cx={at.x}
                  cy={at.y}
                  r={isSeed ? 13 : 8}
                  fill={change === "removed" ? "none" : fill}
                  stroke={change === "removed" ? fill : colors.raised}
                  strokeWidth={2}
                  strokeDasharray={change === "removed" ? "3 3" : undefined}
                />
                {/* An unexpanded node is drawn with a ring, so what is still
                    unexplored is visible without hovering anything. Suppressed
                    while comparing: two dashed treatments on one canvas would
                    make "unexplored" and "removed" look like each other. */}
                {!isExpanded && !comparison && (
                  <circle
                    cx={at.x}
                    cy={at.y}
                    r={isSeed ? 17 : 12}
                    fill="none"
                    stroke={fill}
                    strokeWidth={1}
                    strokeDasharray="2 3"
                    opacity={0.7}
                  />
                )}
                {/* Truncation is marked on the node that is hiding something,
                    not only on the canvas — the shape of the omission is what
                    tells someone which conclusion they may not draw. */}
                {hidesMore && (
                  <text
                    x={at.x + (isSeed ? 15 : 11)}
                    y={at.y - (isSeed ? 9 : 6)}
                    fontSize={13}
                    fontWeight={700}
                    fill={colors.warning}
                  >
                    +
                  </text>
                )}
                {/* The change is written into the label as a sigil, not
                    signalled by colour. A reader who cannot distinguish the
                    palette — or who printed the page — still gets the answer.
                    Epic 40 decision 4. */}
                <text
                  x={at.x}
                  y={at.y - (isSeed ? 24 : 19)}
                  textAnchor="middle"
                  fontSize={11}
                  fontWeight={isSeed || change === "added" ? 600 : 400}
                  fill={colors.text}
                  textDecoration={change === "removed" ? "line-through" : undefined}
                >
                  {change === "added" ? "+ " : change === "removed" ? "− " : change === "changed" ? "~ " : ""}
                  {node.name.length > 22 ? `${node.name.slice(0, 21)}…` : node.name}
                </text>
              </g>
            );
          })}
        </svg>
      </div>

      {/* A picture is not an accessible interface on its own. The same data as
          a list, so the neighbourhood is reachable by keyboard and by a screen
          reader — `00f` treats this as a non-negotiable, not an extra.
          Expansion lives here too: a list that could only *read* the graph
          while the canvas could grow it would be a summary, not an equivalent. */}
      <details>
        <summary style={{ cursor: "pointer", color: colors.textMuted, fontSize: 13 }}>
          The same neighbourhood as a list
        </summary>
        <Table
          size="small"
          style={{ marginTop: 12 }}
          rowKey={(row) => row.id}
          dataSource={[...picture.nodes]}
          pagination={{ pageSize: 10, size: "small" }}
          columns={[
            {
              title: "Node",
              key: "name",
              render: (_, row) => (
                <Space size={6}>
                  {row.name}
                  {row.id === view.seedId && <Tag>seed</Tag>}
                  {view.truncatedAt.includes(row.id) && (
                    <Tag color="warning">more not shown</Tag>
                  )}
                </Space>
              ),
            },
            {
              title: "Kind",
              key: "kind",
              render: (_, row) => row.kind ?? "not visible to you",
            },
            // Present only while comparing, and carrying the same `change`
            // the canvas draws — a list that omitted it would be a summary of
            // the model rather than an equivalent of it.
            ...(comparison
              ? [
                  {
                    title: "Change",
                    key: "change",
                    render: (_: unknown, row: { id: string }) => {
                      const node = comparison.nodes.find((n) => n.id === row.id);
                      if (!node) return null;
                      return (
                        <Space size={4}>
                          {node.change}
                          {node.wasName !== undefined && (
                            <Text type="secondary">was {node.wasName}</Text>
                          )}
                        </Space>
                      );
                    },
                  },
                ]
              : []),
            {
              title: "Neighbours",
              key: "expand",
              render: (_, row) =>
                view.expanded.includes(row.id) ? (
                  <Text type="secondary">expanded</Text>
                ) : (
                  <Button
                    size="small"
                    onClick={() => expandNode(row.id)}
                    disabled={expanding !== null}
                  >
                    Expand
                  </Button>
                ),
            },
          ]}
        />
        <Table
          size="small"
          style={{ marginTop: 12 }}
          rowKey={(row) => `${row.from}-${row.to}-${row.relationship}`}
          dataSource={[...picture.edges]}
          pagination={{ pageSize: 10, size: "small" }}
          columns={[
            {
              title: "From",
              key: "from",
              render: (_, row) => byId.get(row.from)?.name ?? row.from,
            },
            { title: "Relationship", dataIndex: "relationship", key: "rel" },
            {
              title: "To",
              key: "to",
              render: (_, row) => byId.get(row.to)?.name ?? row.to,
            },
            ...(comparison
              ? [
                  {
                    title: "Change",
                    key: "change",
                    render: (_: unknown, row: GraphEdge | DiffEdge) =>
                      "change" in row ? row.change : "unchanged",
                  },
                ]
              : []),
          ]}
        />
      </details>
    </Space>
  );
}

/** The time control.
 *
 *  Reads as a chip saying "now" until a moment is chosen, then names the
 *  moment. That labelling is the honest part: a console silently showing
 *  historical data would be indistinguishable from one showing stale data,
 *  and the whole value of reconstructing a past state is knowing that is what
 *  you are looking at.
 *
 *  Scope is stated rather than implied. `?asOf=` is currently answered for a
 *  single asset read, so the *detail* pane time-travels and the tree and
 *  search do not. Pretending otherwise would be worse than the limitation. */
function TimeControl({
  asOf,
  onChange,
  colors,
}: {
  asOf: string | null;
  onChange: (value: string | null) => void;
  colors: (typeof palette)["light"];
}) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState("");

  const chip = (
    <Tag
      icon={<ClockCircleOutlined />}
      onClick={() => {
        setDraft(asOf ?? new Date().toISOString().slice(0, 16));
        setOpen((o) => !o);
      }}
      style={{
        marginInlineEnd: 0,
        cursor: "pointer",
        background: asOf ? colors.warning : colors.selected,
        borderColor: asOf ? colors.warning : colors.primary,
        color: asOf ? "#0F172A" : colors.primary,
        fontWeight: 500,
      }}
    >
      {asOf ? new Date(asOf).toLocaleString() : "now"}
    </Tag>
  );

  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      trigger="click"
      placement="bottomRight"
      content={
        <Space direction="vertical" size="small" style={{ width: 280 }}>
          <Text strong>View the catalog as it was</Text>
          <Text type="secondary" style={{ fontSize: 12 }}>
            The selected asset is reconstructed from the graph at that instant.
            The hierarchy and search still show the present.
          </Text>
          <Input
            type="datetime-local"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
          />
          <Flex gap={8}>
            <Button
              type="primary"
              size="small"
              disabled={draft.length === 0}
              onClick={() => {
                // `datetime-local` yields a local wall time with no zone; the
                // API takes RFC 3339, so the browser's own offset is what
                // makes "3pm" mean the user's 3pm rather than UTC's.
                onChange(new Date(draft).toISOString());
                setOpen(false);
              }}
            >
              Go to this moment
            </Button>
            {asOf && (
              <Button
                size="small"
                onClick={() => {
                  onChange(null);
                  setOpen(false);
                }}
              >
                Back to now
              </Button>
            )}
          </Flex>
        </Space>
      }
    >
      {chip}
    </Popover>
  );
}

/** What the console shows when the server refuses it.
 *
 *  This exists because the alternative is worse than useless: without it a
 *  401 falls through to the empty-catalog state, and a signed-out user is
 *  told their estate contains nothing. That is not a cosmetic bug — it is the
 *  console asserting something false about the customer's data.
 *
 *  Pasting a bearer token is a stopgap, and is labelled as one. Epic 12's
 *  OIDC/PKCE replaces this panel with a real flow; what does not change is
 *  that "refused" and "empty" stay different screens. */
function SignIn({ onToken }: { onToken: (token: string) => void }) {
  const [value, setValue] = useState("");
  return (
    <Flex align="center" justify="center" style={{ height: "100%" }}>
      <Card style={{ maxWidth: 460, width: "100%" }}>
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <div>
            <Title level={4} style={{ margin: 0, fontWeight: 600 }}>
              This server requires a token
            </Title>
            <Paragraph type="secondary" style={{ marginTop: 8, marginBottom: 0 }}>
              graph-owl is running with authentication enabled, so it will not
              answer until it knows who is asking. Your catalog is not empty —
              this console simply has not been told who you are.
            </Paragraph>
          </div>
          <Input.TextArea
            value={value}
            onChange={(e) => setValue(e.target.value)}
            placeholder="Paste the bearer token printed by ./scripts/demo.sh --secure"
            autoSize={{ minRows: 3, maxRows: 6 }}
            style={{ fontFamily: "JetBrains Mono, ui-monospace, monospace", fontSize: 12 }}
          />
          <Flex justify="space-between" align="center">
            <Text type="secondary" style={{ fontSize: 12 }}>
              Kept for this tab only — closing it discards the token.
            </Text>
            <Button
              type="primary"
              disabled={value.trim().length === 0}
              onClick={() => onToken(value.trim())}
            >
              Use this token
            </Button>
          </Flex>
        </Space>
      </Card>
    </Flex>
  );
}

/** Facet buckets for one dimension, as toggles.
 *
 *  Counts come from the server, which computes them *after* authorization —
 *  so a bucket can never disclose a schema the reader may not see, nor how
 *  many assets are in it. That is the property Demo 2 demonstrates, and
 *  rendering the counts unchanged is what makes it visible.
 *
 *  Selecting a bucket toggles rather than accumulates: with one active value
 *  per dimension there is no way to build a filter whose result set is empty
 *  for reasons the user cannot see. */
function FacetGroup({
  title,
  buckets,
  active,
  onToggle,
}: {
  title: string;
  buckets: Facet[];
  active: string | null;
  onToggle: (value: string | null) => void;
}) {
  if (buckets.length === 0) return null;
  return (
    <div>
      <Text
        type="secondary"
        style={{ fontSize: 11, letterSpacing: 0.6, textTransform: "uppercase" }}
      >
        {title}
      </Text>
      <Space direction="vertical" size={4} style={{ width: "100%", marginTop: 8 }}>
        {buckets.map((bucket) => {
          const selected = active === bucket.value;
          return (
            <Button
              key={bucket.value}
              size="small"
              type={selected ? "primary" : "text"}
              aria-pressed={selected}
              onClick={() => onToggle(selected ? null : bucket.value)}
              style={{
                width: "100%",
                textAlign: "left",
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
              }}
            >
              <span
                style={{
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {bucket.value}
              </span>
              <Text
                style={{
                  fontSize: 12,
                  fontVariantNumeric: "tabular-nums",
                  color: selected ? "inherit" : undefined,
                }}
                type={selected ? undefined : "secondary"}
              >
                {bucket.count}
              </Text>
            </Button>
          );
        })}
      </Space>
    </div>
  );
}

function VersionHistory({ assetId }: { assetId: string }) {
  const [versions, setVersions] = useState<AssetVersion[] | null>(null);

  useEffect(() => {
    setVersions(null);
    api.versions(assetId).then(setVersions).catch(() => setVersions([]));
  }, [assetId]);

  if (versions === null) return <Text type="secondary">Loading…</Text>;
  if (versions.length === 0) {
    return (
      <Empty
        image={Empty.PRESENTED_IMAGE_SIMPLE}
        description="No edits yet. This asset is exactly as the connector reported it."
      />
    );
  }

  return (
    <Timeline
      items={versions.map((v) => ({
        children: (
          <Space direction="vertical" size={2}>
            <Space size={8}>
              <Text strong>
                v{v.version.major}.{v.version.minor}
              </Text>
              <Text type="secondary" style={{ fontSize: 12 }}>
                {new Date(v.updatedAt).toLocaleString()} · {v.updatedBy}
              </Text>
            </Space>
            {renderChange(v.changeDescription)}
          </Space>
        ),
      }))}
    />
  );
}

function DescriptionEditor({
  asset,
  onSaved,
}: {
  asset: Asset;
  onSaved: (a: Asset) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(asset.description ?? "");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setDraft(asset.description ?? "");
    setEditing(false);
  }, [asset.id, asset.description]);

  const save = async () => {
    setBusy(true);
    try {
      // Blank means clear, which the API expects as explicit null — absence
      // would mean "not declared" and leave the old value in place.
      onSaved(await api.updateAsset(asset.id, { description: draft.trim() || null }));
      setEditing(false);
    } catch {
      /* surfaced by the disabled state; a fuller error path lands with Epic 39 */
    } finally {
      setBusy(false);
    }
  };

  if (!editing) {
    return (
      <Flex align="flex-start" gap={8}>
        <Paragraph
          type={asset.description ? undefined : "secondary"}
          italic={!asset.description}
          style={{ marginBottom: 0, flex: 1 }}
        >
          {asset.description ??
            "No description. A connector reported this asset structurally; nobody has described it."}
        </Paragraph>
        <Button size="small" type="text" icon={<EditOutlined />} onClick={() => setEditing(true)}>
          Edit
        </Button>
      </Flex>
    );
  }

  return (
    <Space direction="vertical" style={{ width: "100%" }} size={8}>
      <Input.TextArea
        rows={3}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        placeholder="Describe what this asset holds and who relies on it."
      />
      <Space>
        <Button type="primary" size="small" loading={busy} onClick={save}>
          Save
        </Button>
        <Button size="small" onClick={() => setEditing(false)}>
          Cancel
        </Button>
      </Space>
    </Space>
  );
}

function AssetDetail({
  asset,
  onChanged,
  asOf,
  colors,
}: {
  asset: Asset;
  onChanged: (a: Asset) => void;
  asOf: string | null;
  colors: (typeof palette)["light"];
}) {
  const [ancestors, setAncestors] = useState<Asset[]>([]);
  const [children, setChildren] = useState<Asset[]>([]);

  useEffect(() => {
    api.ancestors(asset.id).then(setAncestors).catch(() => setAncestors([]));
    if (asset.kind === "column") setChildren([]);
    else api.children(asset.id).then(setChildren).catch(() => setChildren([]));
  }, [asset.id, asset.kind]);

  const properties = Object.entries(asset.properties ?? {});

  const overview = (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <DescriptionEditor asset={asset} onSaved={onChanged} />

      {properties.length > 0 && (
        <Card size="small" title="Properties">
          <Descriptions
            column={1}
            size="small"
            bordered
            items={properties.map(([key, value]) => ({
              key,
              label: key,
              children: <Text code>{String(value)}</Text>,
            }))}
          />
        </Card>
      )}

      {children.length > 0 && (
        <Card
          size="small"
          title={`${children[0]!.kind === "column" ? "Columns" : "Contains"} (${children.length})`}
        >
          <Table
            size="small"
            rowKey="id"
            dataSource={children}
            pagination={false}
            columns={[
              {
                title: "Name",
                dataIndex: "name",
                key: "name",
                render: (name: string) => <Text style={{ fontWeight: 500 }}>{name}</Text>,
              },
              {
                title: "Kind",
                dataIndex: "kind",
                key: "kind",
                width: 120,
                render: (kind: AssetKind) => <Tag color={KIND_COLOR[kind]}>{kind}</Tag>,
              },
              {
                title: "Type",
                key: "type",
                width: 230,
                render: (_: unknown, row: Asset) => (
                  <Text code>
                    {String(
                      row.properties?.["dataType"] ?? row.properties?.["tableType"] ?? "—",
                    )}
                  </Text>
                ),
              },
              {
                title: "Nullable",
                key: "nullable",
                width: 110,
                render: (_: unknown, row: Asset) =>
                  row.properties?.["nullable"] === undefined ? (
                    <Text type="secondary">—</Text>
                  ) : row.properties["nullable"] ? (
                    <Text type="secondary">yes</Text>
                  ) : (
                    <Tag>required</Tag>
                  ),
              },
            ]}
          />
        </Card>
      )}
    </Space>
  );

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Breadcrumb items={ancestors.map((a) => ({ title: a.name }))} />

      <Flex align="center" gap={12} wrap>
        <Title level={3} style={{ margin: 0, fontWeight: 600 }}>
          {asset.name}
        </Title>
        <Tag color={KIND_COLOR[asset.kind]} icon={KIND_ICON[asset.kind]}>
          {asset.kind}
        </Tag>
      </Flex>
      <Fqn>{asset.fullyQualifiedName}</Fqn>

      <TrustBar asset={asset} />

      <Tabs
        defaultActiveKey={readParam("tab") ?? "overview"}
        onChange={(key) => writeParam("tab", key === "overview" ? null : key)}
        items={[
          { key: "overview", label: "Overview", children: overview },
          {
            key: "history",
            label: (
              <span>
                <HistoryOutlined /> History
              </span>
            ),
            children: <VersionHistory assetId={asset.id} />,
          },
          {
            key: "graph",
            label: (
              <span>
                <ApartmentOutlined /> Graph
              </span>
            ),
            // Mounted only when the tab is opened. A traversal is a real query
            // and running it for everyone who opens an asset would make the
            // graph the cost of every page view.
            children: (
              <GraphExplorer assetId={asset.id} asOf={asOf} colors={colors} />
            ),
          },
        ]}
        destroyOnHidden
      />
    </Space>
  );
}

function ConnectorRunForm({
  onBack,
  onDone,
}: {
  onBack: () => void;
  onDone: () => void;
}) {
  const { token } = antdTheme.useToken();
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ created: number; failed: number } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async (values: { connectionString: string; serviceName: string }) => {
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      setResult(await api.runPostgresConnector(values));
      onDone();
    } catch (e) {
      setError(e instanceof ApiError ? e.problem.detail : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%", maxWidth: 720 }}>
      <Button type="text" icon={<ArrowLeftOutlined />} onClick={onBack} style={{ paddingLeft: 0 }}>
        All connectors
      </Button>
      <Flex align="center" gap={12}>
        <PostgresMark size={36} />
        <div>
          <Title level={4} style={{ margin: 0, fontWeight: 600 }}>
            PostgreSQL
          </Title>
          <Text type="secondary">Read schemas, tables, views and columns.</Text>
        </div>
      </Flex>

      <Card size="small" title="Connection">
        <Form
          layout="vertical"
          onFinish={run}
          initialValues={{
            connectionString: "postgres://postgres:postgres@localhost:55432/postgres",
            serviceName: "hdfc-core",
          }}
        >
          <Form.Item
            label="Connection string"
            name="connectionString"
            rules={[{ required: true, message: "A connection string is required" }]}
            extra="Credentials are used for this run only and are not stored."
          >
            <Input style={{ fontFamily: "JetBrains Mono, ui-monospace, monospace", fontSize: 12 }} />
          </Form.Item>
          <Form.Item
            label="Service name"
            name="serviceName"
            rules={[{ required: true, message: "A service name is required" }]}
            extra="The root of the hierarchy. One server can be catalogued as two logical services."
          >
            <Input />
          </Form.Item>
          <Button type="primary" htmlType="submit" loading={busy} icon={<ThunderboltOutlined />}>
            Run connector
          </Button>
        </Form>
      </Card>

      {result && (
        <Card size="small">
          <Space>
            <CheckCircleFilled style={{ color: token.colorSuccess, fontSize: 18 }} />
            <Text style={{ fontWeight: 500 }}>
              Catalogued {result.created} assets
              {result.failed > 0 ? `, ${result.failed} failed` : ""}.
            </Text>
          </Space>
        </Card>
      )}
      {error && (
        <Card size="small">
          <Text type="danger">{error}</Text>
        </Card>
      )}
    </Space>
  );
}

function ConnectorsPage({ onDone }: { onDone: () => void }) {
  const [chosen, setChosen] = useState<string | null>(null);

  if (chosen === "postgres") {
    return <ConnectorRunForm onBack={() => setChosen(null)} onDone={onDone} />;
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <div>
        <Title level={4} style={{ margin: 0, fontWeight: 600 }}>
          Connectors
        </Title>
        <Text type="secondary">Choose a source to catalogue.</Text>
      </div>
      <Row gutter={[16, 16]}>
        {CONNECTORS.map((connector) => (
          <Col key={connector.id} xs={24} sm={12} lg={8} xl={6}>
            <Tooltip title={connector.available ? undefined : "Not implemented yet"}>
              <Card
                size="small"
                hoverable={connector.available}
                onClick={() => connector.available && setChosen(connector.id)}
                style={{ height: "100%", opacity: connector.available ? 1 : 0.55 }}
                styles={{ body: { padding: 16 } }}
              >
                <Space direction="vertical" size={8} style={{ width: "100%" }}>
                  <Flex align="center" gap={10}>
                    {connector.id === "postgres" ? (
                      <PostgresMark />
                    ) : (
                      <GenericSourceMark />
                    )}
                    <Text style={{ fontWeight: 600, fontSize: 15 }}>{connector.name}</Text>
                  </Flex>
                  <Text type="secondary" style={{ fontSize: 13 }}>
                    {connector.blurb}
                  </Text>
                  {connector.available ? (
                    <Tag color="green">available</Tag>
                  ) : (
                    <Tag>planned</Tag>
                  )}
                </Space>
              </Card>
            </Tooltip>
          </Col>
        ))}
      </Row>
    </Space>
  );
}

export default function App() {
  const { dark, toggle } = useTheme();
  const colors = dark ? palette.dark : palette.light;
  // Overview answers "what is in here" without a click; Explore is where you
  // go once you know what you are looking for. A deep link to an asset lands
  // on Explore regardless, or the link would not open the thing it names.
  const [section, setSectionRaw] = useState<Section>(() => {
    const named = readParam("section");
    if (named === "connectors" || named === "explore" || named === "overview") {
      return named;
    }
    return readParam("asset") ? "explore" : "overview";
  });
  const setSection = useCallback((next: Section) => {
    setSectionRaw(next);
    writeParam("section", next === "overview" ? null : next);
  }, []);
  const [selected, setSelectedRaw] = useState<Asset | null>(null);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Asset[] | null>(null);
  // Distinct from "no data". A refused request rendered as an empty catalog is
  // the single most misleading thing this console can do: it tells someone
  // their estate is empty when in fact they are simply not signed in.
  const [refused, setRefused] = useState(false);
  // Null is now. Deep-linkable, so a screenshot of a past state carries the
  // instant it was taken at rather than being unreproducible.
  const [asOf, setAsOf] = useState<string | null>(() => readParam("asOf"));
  const [facets, setFacets] = useState<SearchFacets | null>(null);
  // Which facet bucket is narrowing the results, if any. Filtering happens on
  // the client over the returned page: the server's facet counts are already
  // authorization-filtered, so narrowing locally cannot reveal anything the
  // server did not already send.
  const [activeSchema, setActiveSchema] = useState<string | null>(null);
  const [activeKind, setActiveKind] = useState<AssetKind | null>(null);
  // -1 is "nothing focused". Keyboard navigation is a `00f` non-negotiable:
  // a result list reachable only by mouse is not reachable by everyone.
  const [cursor, setCursor] = useState(-1);
  const [stats, setStats] = useState<{ kind: AssetKind; count: number }[]>([]);
  const [nodes, setNodes] = useState<DataNode[]>([]);
  const [index, setIndex] = useState<Record<string, Asset>>({});

  const setSelected = useCallback((asset: Asset | null) => {
    setSelectedRaw(asset);
    // Deep-linkable: an entity you cannot paste into a ticket is one nobody shares.
    window.history.replaceState(
      null,
      "",
      asset ? `?asset=${asset.id}` : window.location.pathname,
    );
  }, []);

  const toNode = useCallback(
    (asset: Asset): DataNode => ({
      key: asset.id,
      title: asset.name,
      icon: KIND_ICON[asset.kind],
      isLeaf: asset.kind === "column",
    }),
    [],
  );

  const refresh = useCallback(() => {
    api
      .roots()
      .then((roots) => {
        setRefused(false);
        setIndex((i) => ({ ...i, ...Object.fromEntries(roots.map((r) => [r.id, r])) }));
        setNodes(roots.map(toNode));
      })
      .catch((error: unknown) => {
        // An auth failure is not an empty result. Anything else genuinely is
        // a failure to load, and still must not claim the catalog is empty.
        setRefused(isUnauthenticated(error));
        setNodes([]);
      });
    api
      .stats()
      .then((s) => setStats(s.byKind))
      .catch(() => setStats([]));
  }, [toNode]);

  useEffect(refresh, [refresh]);

  useEffect(() => {
    const id = new URLSearchParams(window.location.search).get("asset");
    if (id) {
      api
        .asset(id, asOf)
        .then(setSelectedRaw)
        .catch(() => undefined);
    }
    // Deliberately depends on `asOf` too: moving the clock must re-read the
    // open asset. Without it the chip would say "March" over data from today,
    // which is exactly the confusion the chip exists to prevent.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asOf]);

  // Re-read the *selected* asset when the moment changes. A selection made by
  // clicking the tree carries current data; the chip has to be able to pull it
  // backwards without the user re-clicking.
  useEffect(() => {
    if (!selected) return;
    api
      .asset(selected.id, asOf)
      .then(setSelectedRaw)
      .catch(() => {
        // The asset did not exist at that instant. Honest: keep the chip where
        // the user put it and say so, rather than silently snapping to now.
        setSelectedRaw(null);
      });
    // Keyed on the id, not the object, or writing the result would re-trigger.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asOf, selected?.id]);

  useEffect(() => {
    if (query.trim().length < 2) {
      setResults(null);
      setFacets(null);
      return;
    }
    const timer = setTimeout(() => {
      api
        .search(query)
        .then((page) => {
          setResults(page.data);
          setFacets(page.facets);
          // A narrowing carried over from the previous query would silently
          // hide results for the new one.
          setActiveSchema(null);
          setActiveKind(null);
          setCursor(-1);
        })
        .catch(() => {
          setResults([]);
          setFacets(null);
        });
    }, 150);
    return () => clearTimeout(timer);
  }, [query]);

  /** The schema is the third FQN segment: service.database.schema.…
   *  Deliberately the same rule the server uses to build the facet buckets —
   *  if the two ever disagree, a bucket labelled `n` would filter to something
   *  other than `n` rows, which reads as a broken count rather than a broken
   *  parser. */
  const schemaOf = (asset: Asset) => asset.fullyQualifiedName.split(".")[2];

  const visibleResults = useMemo(
    () =>
      (results ?? []).filter(
        (asset) =>
          (activeKind === null || asset.kind === activeKind) &&
          (activeSchema === null || schemaOf(asset) === activeSchema),
      ),
    [results, activeKind, activeSchema],
  );

  // Arrow keys move a cursor through the results and Enter opens one, so the
  // list is operable without a pointer (`00f` non-negotiable). Bound at the
  // document rather than the input because the cursor must survive the search
  // box losing focus — otherwise Tab out of the box strands the selection.
  useEffect(() => {
    if (visibleResults.length === 0) return undefined;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        setCursor((c) => {
          const next = event.key === "ArrowDown" ? c + 1 : c - 1;
          // Clamp rather than wrap: wrapping past the end of a long result
          // list silently moves the eye to the opposite end of the page.
          return Math.max(0, Math.min(next, visibleResults.length - 1));
        });
      } else if (event.key === "Enter" && cursor >= 0) {
        event.preventDefault();
        // Guarded rather than indexed blind: a facet toggle can shrink the
        // list between the keypress and this handler's closure, and opening
        // `undefined` would blank the detail pane with no explanation.
        const row = visibleResults[cursor];
        if (row) {
          setSelected(row);
          setQuery("");
        }
      } else if (event.key === "Escape") {
        setQuery("");
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [visibleResults, cursor, setSelected]);

  // Children are fetched on expand; loading the hierarchy whole would pull
  // every column of every table before showing anything.
  const loadChildren = useCallback(
    async (node: DataNode) => {
      const children = await api.children(String(node.key));
      setIndex((i) => ({ ...i, ...Object.fromEntries(children.map((c) => [c.id, c])) }));
      setNodes((current) => {
        const attach = (list: DataNode[]): DataNode[] =>
          list.map((n) =>
            n.key === node.key
              ? { ...n, children: children.map(toNode) }
              : n.children
                ? { ...n, children: attach(n.children) }
                : n,
          );
        return attach(current);
      });
    },
    [toNode],
  );

  const total = useMemo(() => stats.reduce((sum, s) => sum + s.count, 0), [stats]);

  return (
    <ConfigProvider theme={dark ? darkTheme : lightTheme}>
      <AntApp>
        <Layout style={{ height: "100vh" }}>
          <Header
            style={{
              display: "flex",
              alignItems: "center",
              gap: 20,
              padding: "0 16px",
              borderBottom: `1px solid ${colors.border}`,
              flex: "0 0 auto",
            }}
          >
            <div style={{ display: "flex", alignItems: "center" }}>
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 450 120" style={{ height: 52, width: "auto" }}>
                <text x="10" y="85" fontFamily="Arial,Helvetica,sans-serif" fontSize="60" fontWeight="900" fill={dark ? "#FFFFFF" : "#000000"}>GRAPH</text>
                <g transform="translate(268 60)">
                  <path d="M-34 -6 Q-46 -24 -34 -36 Q-18 -26 -16 -6 Z" fill="#14C3CF" />
                  <circle cx="0" cy="0" r="31" fill="#14C3CF" />
                  <ellipse cx="0" cy="2" rx="22" ry="20" fill="#FFF8EF" />
                  <path d="M-18 -18 Q-8 -30 3 -18" fill="none" stroke="#F3D28A" strokeWidth="4" strokeLinecap="round" />
                  <path d="M18 -18 Q8 -30 -3 -18" fill="none" stroke="#F3D28A" strokeWidth="4" strokeLinecap="round" />
                  <circle cx="-10" cy="-2" r="10" fill="none" stroke="#20365E" strokeWidth="2.6" />
                  <circle cx="10" cy="-2" r="10" fill="none" stroke="#20365E" strokeWidth="2.6" />
                  <line x1="0" y1="-2" x2="0" y2="-2" stroke="#20365E" strokeWidth="2" />
                  <circle cx="-10" cy="-2" r="6.5" fill="#23C5F6" />
                  <circle cx="-10" cy="-2" r="3.5" fill="#111" />
                  <circle cx="-8" cy="-4" r="1.4" fill="#FFF" />
                  <circle cx="10" cy="-2" r="6.5" fill="#23C5F6" />
                  <circle cx="10" cy="-2" r="3.5" fill="#111" />
                  <circle cx="12" cy="-4" r="1.4" fill="#FFF" />
                  <path d="M0 2 L7 9 Q0 18 -7 9 Z" fill="#F6A600" />
                  <path d="M-5 9 Q0 15 5 9" fill="#D85C2C" />
                  <path d="M-8 29 h6" stroke="#E09A21" strokeWidth="2" strokeLinecap="round" />
                  <path d="M2 29 h6" stroke="#E09A21" strokeWidth="2" strokeLinecap="round" />
                </g>
                <text x="302" y="85" fontFamily="Arial,Helvetica,sans-serif" fontSize="60" fontWeight="900" fill="#14C3CF">WL</text>
              </svg>
            </div>
            <Input
              prefix={<SearchOutlined style={{ color: colors.textSubtle }} />}
              placeholder="Search assets, schemas, columns…"
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                if (e.target.value) setSection("explore");
              }}
              style={{ maxWidth: 480 }}
              allowClear
            />
            <Space style={{ marginLeft: "auto" }} size={12}>
              <TimeControl
                asOf={asOf}
                colors={colors}
                onChange={(value) => {
                  setAsOf(value);
                  writeParam("asOf", value);
                }}
              />
              {/* Only when a token is in play. On an open server there is no
                  identity to drop, and an inert control implying otherwise is
                  worse than no control. Switching principals is Demo 2's whole
                  moment: the same search, two identities, different results. */}
              {authToken() !== null && (
                <Tooltip title="Discard this token and sign in as someone else">
                  <Button
                    type="text"
                    icon={<UserOutlined />}
                    aria-label="Sign out"
                    onClick={() => {
                      setAuthToken(null);
                      refresh();
                    }}
                  >
                    Sign out
                  </Button>
                </Tooltip>
              )}
              <Tooltip title={dark ? "Switch to light" : "Switch to dark"}>
                <Button type="text" icon={<BulbOutlined />} onClick={toggle} aria-label="Toggle theme" />
              </Tooltip>
            </Space>
          </Header>

          <Layout style={{ minHeight: 0 }}>
            {/* A refused request and an empty catalog are different
                screens. Routing both to the same one tells a signed-out
                user their estate is empty, which is a false statement
                about their data rather than a cosmetic bug. */}
            {refused ? (
              <Content style={{ padding: 24 }}>
                <SignIn
                  onToken={(token) => {
                    setAuthToken(token);
                    refresh();
                  }}
                />
              </Content>
            ) : (
              <>
            {/* Navigation and the hierarchy are different things. The rail is
                where you are in the product; the tree is where you are in the
                data. Conflating them is why the tree previously had nowhere
                for a second section to go. */}
            <Sider
              width={196}
              theme={dark ? "dark" : "light"}
              style={{ borderRight: `1px solid ${colors.border}` }}
            >
              <Menu
                mode="inline"
                selectedKeys={[section]}
                onClick={({ key }) => setSection(key as Section)}
                style={{ borderInlineEnd: 0, paddingTop: 8 }}
                items={[
                  { key: "overview", icon: <DashboardOutlined />, label: "Overview" },
                  { key: "explore", icon: <CompassOutlined />, label: "Explore" },
                  { key: "connectors", icon: <PlusOutlined />, label: "Connectors" },
                ]}
              />
            </Sider>

            {section === "explore" && (
              <Sider
                width={288}
                theme={dark ? "dark" : "light"}
                style={{ borderRight: `1px solid ${colors.border}`, overflow: "auto" }}
              >
                <div style={{ padding: 14 }}>
                  <Flex gap={8} wrap style={{ marginBottom: 14 }}>
                    {stats.map((s) => (
                      <Card key={s.kind} size="small" styles={{ body: { padding: "4px 10px" } }}>
                        <Statistic
                          title={<span style={{ fontSize: 11 }}>{`${s.kind}s`}</span>}
                          value={s.count}
                          valueStyle={{ fontSize: 17, fontWeight: 600, lineHeight: 1.2 }}
                        />
                      </Card>
                    ))}
                  </Flex>
                  <Text
                    type="secondary"
                    style={{ fontSize: 11, fontWeight: 600, letterSpacing: "0.06em" }}
                  >
                    HIERARCHY
                  </Text>
                  {nodes.length === 0 ? (
                    <Paragraph type="secondary" style={{ marginTop: 12, fontSize: 13 }}>
                      Nothing catalogued yet.
                    </Paragraph>
                  ) : (
                    <Tree
                      showIcon
                      blockNode
                      style={{ marginTop: 8, background: "transparent" }}
                      treeData={nodes}
                      loadData={loadChildren}
                      onSelect={(keys) => {
                        const asset = index[String(keys[0])];
                        if (asset) setSelected(asset);
                      }}
                    />
                  )}
                </div>
              </Sider>
            )}

            <Content style={{ padding: 24, overflow: "auto" }}>
              {section === "overview" ? (
                <OverviewPage
                  colors={colors}
                  onOpen={(asset) => {
                    setSection("explore");
                    setSelected(asset);
                  }}
                  onAddSource={() => setSection("connectors")}
                />
              ) : section === "connectors" ? (
                <ConnectorsPage onDone={refresh} />
              ) : results !== null ? (
                <Row gutter={24} style={{ width: "100%" }}>
                  <Col flex="200px">
                    <Space direction="vertical" size="large" style={{ width: "100%" }}>
                      <FacetGroup
                        title="Kind"
                        buckets={facets?.kind ?? []}
                        active={activeKind}
                        onToggle={(v) => {
                          setActiveKind(v as AssetKind | null);
                          setCursor(-1);
                        }}
                      />
                      <FacetGroup
                        title="Schema"
                        buckets={facets?.schema ?? []}
                        active={activeSchema}
                        onToggle={(v) => {
                          setActiveSchema(v);
                          setCursor(-1);
                        }}
                      />
                    </Space>
                  </Col>
                  <Col flex="auto" style={{ minWidth: 0 }}>
                <Space direction="vertical" style={{ width: "100%" }} size="middle">
                  <Title level={5} style={{ margin: 0, fontWeight: 600 }}>
                    {visibleResults.length} result{visibleResults.length === 1 ? "" : "s"} for “{query}”
                    {(activeKind || activeSchema) && (
                      <>
                        {" "}
                        <Text type="secondary" style={{ fontWeight: 400, fontSize: 13 }}>
                          filtered from {results.length}
                        </Text>{" "}
                        <Button
                          size="small"
                          type="link"
                          style={{ padding: 0, fontSize: 13 }}
                          onClick={() => {
                            setActiveKind(null);
                            setActiveSchema(null);
                            setCursor(-1);
                          }}
                        >
                          clear
                        </Button>
                      </>
                    )}
                  </Title>
                  {visibleResults.length === 0 ? (
                    <Empty description="Nothing matched" />
                  ) : (
                    <Table
                      size="small"
                      rowKey="id"
                      dataSource={visibleResults}
                      pagination={{ pageSize: 15, size: "small" }}
                      rowClassName={(_row, i) => (i === cursor ? "gowl-row-cursor" : "")}
                      onRow={(row, i) => ({
                        onClick: () => {
                          setSelected(row);
                          setQuery("");
                        },
                        onMouseEnter: () => setCursor(i ?? -1),
                        style: { cursor: "pointer" },
                      })}
                      columns={[
                        {
                          title: "Name",
                          dataIndex: "name",
                          key: "name",
                          width: 220,
                          render: (name: string) => (
                            <Text style={{ fontWeight: 500 }}>{name}</Text>
                          ),
                        },
                        {
                          title: "Kind",
                          dataIndex: "kind",
                          key: "kind",
                          width: 130,
                          render: (kind: AssetKind) => (
                            <Tag color={KIND_COLOR[kind]} icon={KIND_ICON[kind]}>
                              {kind}
                            </Tag>
                          ),
                        },
                        {
                          title: "Fully-qualified name",
                          dataIndex: "fullyQualifiedName",
                          key: "fqn",
                          render: (fqn: string) => <Fqn>{fqn}</Fqn>,
                        },
                      ]}
                    />
                  )}
                </Space>
                  </Col>
                </Row>
              ) : selected ? (
                <>
                  {asOf && (
                    <Card
                      size="small"
                      style={{
                        marginBottom: 16,
                        borderColor: colors.warning,
                        background: colors.fillSubtle,
                      }}
                    >
                      <Space>
                        <ClockCircleOutlined style={{ color: colors.warning }} />
                        <Text style={{ fontSize: 13 }}>
                          Showing this asset as it stood at{" "}
                          <Text strong>{new Date(asOf).toLocaleString()}</Text>,
                          reconstructed from the graph.
                        </Text>
                      </Space>
                    </Card>
                  )}
                  <AssetDetail
                    asset={selected}
                    onChanged={setSelectedRaw}
                    asOf={asOf}
                    colors={colors}
                  />
                </>
              ) : (
                <div style={{ position: "relative", width: "100%", height: "100%", display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center" }}>
                  {total === 0 && (
                    <img
                      src={watermarkImg}
                      alt=""
                      style={{
                        position: "absolute",
                        bottom: 24,
                        right: 24,
                        width: 360,
                        opacity: 0.25,
                        pointerEvents: "none",
                      }}
                    />
                  )}
                  <div style={{ textAlign: "center", zIndex: 1 }}>
                    <Title level={4} style={{ marginBottom: 4, fontWeight: 600 }}>
                      {total === 0 ? "Nothing catalogued yet" : `${total} assets catalogued`}
                    </Title>
                    <Text type="secondary">
                      {total === 0
                        ? "graph-owl reads a source's structure and builds a browsable hierarchy from it."
                        : "Pick something from the hierarchy, or search above."}
                    </Text>
                  </div>
                  <Button
                    type="primary"
                    icon={<PlusOutlined />}
                    onClick={() => setSection("connectors")}
                    style={{ marginTop: 24, zIndex: 1 }}
                  >
                    {total === 0 ? "Catalogue a source" : "Add another source"}
                  </Button>
                </div>
              )}
            </Content>
              </>
            )}
          </Layout>
        </Layout>
      </AntApp>
    </ConfigProvider>
  );
}
