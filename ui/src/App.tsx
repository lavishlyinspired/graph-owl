import { useCallback, useEffect, useMemo, useState, useRef } from "react";
import {
  App as AntApp,
  Breadcrumb,
  Alert,
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
  Modal,
  Spin,
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
import TeamOutlined from "@ant-design/icons/es/icons/TeamOutlined";
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
  type SparqlResult,
  type ValidationRun,
  ApiError,
  api,
  isUnauthenticated,
  isForbidden,
  setRefreshHandler,
} from "./api";
import { AuthProvider, useAuth, tryRefresh } from "./auth";
import { type DiffEdge, diff } from "./graph/diff";
import { overflowTitle, summarizeOwners } from "./graph/owners";
import { type GraphModel, expand, performedExpansions, replay, seed } from "./graph/model";
import { brand, darkTheme, lightTheme, palette } from "./theme";
import { GenericSourceMark, PostgresMark } from "./icons";
import cytoscape from "cytoscape";
import { Background, Controls, Position, ReactFlow } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { type LineageGraph, positions } from "./graph/lineage";
import {
  type Finding,
  type Severity,
  currency,
  describeSuggestion,
  groupByAsset,
  localName,
} from "./governance/queue";
import {
  type Explanation,
  type Row as ChainRow,
  depthOf,
  flatten,
  rulesUsed,
} from "./governance/explanation";
import {
  type Solution,
  columns as resultColumns,
  display as displayTerm,
  graphShape,
  toGraph,
  verdict,
} from "./workbench/results";
import {
  type Picture as CyPicture,
  layoutOptions,
  toElements,
  wantsWebgl,
} from "./graph/cytoscape";
import type { ConnectorRun } from "./api";
import watermarkImg from "./assets/watermark1.png";

const { Header, Sider, Content } = Layout;
const { Text, Title, Paragraph } = Typography;

type Section = "overview" | "explore" | "connectors" | "governance" | "workbench";

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
/** Who owns this asset — Epic 39, the console half of Epic 11 Slice C/D. All
 *  the logic that decides *what* to show lives in `graph/owners.ts`, tested
 *  there without rendering anything; this only draws what it is handed. */
function OwnerChips({ owners }: { owners: Asset["owners"] }) {
  const summary = summarizeOwners(owners);

  // The server never mentioned owners — an older build, or a read that did not
  // include them. Rendering nothing is the only honest option: "no owner
  // recorded" would be a claim about an estate we were not told about.
  if (summary.unknown) return null;

  // Unowned is a real, reportable state per Epic 11 — not a loading gap — so
  // it is said plainly rather than left as a blank space a reader might
  // mistake for "not loaded yet".
  if (summary.unowned) {
    return (
      <Text type="secondary" style={{ fontSize: 13 }}>
        <UserOutlined /> no owner recorded
      </Text>
    );
  }

  return (
    <Space size={4} wrap>
      {summary.chips.map((chip) => (
        <Tooltip
          key={chip.key}
          title={chip.inherited ? "inherited from an ancestor — not recorded on this asset itself" : undefined}
        >
          <Tag
            icon={chip.kind === "team" ? <TeamOutlined /> : <UserOutlined />}
            // Dashed rather than solid for an inherited owner: the same visual
            // language a draft or a placeholder uses elsewhere in this
            // console, so "not recorded here" reads consistently.
            style={chip.inherited ? { borderStyle: "dashed" } : undefined}
          >
            {chip.label}
          </Tag>
        </Tooltip>
      ))}
      {summary.overflow > 0 && (
        <Tooltip title={overflowTitle(owners)}>
          <Tag>+{summary.overflow} more</Tag>
        </Tooltip>
      )}
      {/* **Words, not just a dashed border.** Verified in the browser that the
          border alone was indistinguishable at real size on both a parent and
          its child, which defeats the point of the flag: "nobody named an owner
          here" and "somebody did" are the two answers a steward needs, and one
          of them was invisible. */}
      {summary.inheritance !== "none" && (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {summary.inheritance === "all" ? "inherited" : "partly inherited"}
        </Text>
      )}
    </Space>
  );
}

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

/** Lineage, as a layered DAG read left to right.
 *
 *  A second renderer, deliberately (`00f`). Exploration is an arbitrary cyclic
 *  graph at scale where WebGL is the point; lineage is a DAG of modest size
 *  where the *layering* is the point, and a force layout is actively wrong for
 *  it — it would place a source and a consumer wherever the physics settled and
 *  destroy the one thing the picture is for.
 *
 *  Positions come from `graph/lineage.ts` and are tested there. This mounts
 *  them.
 */
function LineageView({
  assetId,
  colors,
}: {
  assetId: string;
  colors: (typeof palette)["light"];
}) {
  const [depth, setDepth] = useState(2);
  const [graph, setGraph] = useState<LineageGraph | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    setGraph(null);
    setError(null);
    api
      .lineage(assetId, depth, depth)
      .then((g) => {
        if (current) setGraph(g);
      })
      .catch((e: unknown) => {
        if (current) {
          setError(e instanceof ApiError ? e.problem.detail : "could not load lineage");
        }
      });
    return () => {
      current = false;
    };
  }, [assetId, depth]);

  const flow = useMemo(() => {
    if (!graph) return null;
    const placed = new Map(positions(graph).map((p) => [p.id, p]));
    const nodes = graph.nodes.map((node) => {
      const at = placed.get(node.id) ?? { x: 0, y: 0 };
      const isRoot = node.id === graph.rootId;
      return {
        id: node.id,
        position: { x: at.x, y: at.y },
        data: { label: node.deleted ? `${node.name} (deleted)` : node.name },
        // Source on the right, target on the left: the picture reads
        // left-to-right and the handles have to agree with it, or every edge
        // loops back on itself.
        sourcePosition: Position.Right,
        targetPosition: Position.Left,
        style: {
          padding: 8,
          borderRadius: 8,
          fontSize: 12,
          width: 160,
          border: `${isRoot ? 2 : 1}px solid ${isRoot ? colors.primary : colors.border}`,
          // A tombstoned node is marked by pattern and opacity, not colour
          // alone — a deletion shown only in red is invisible to a reader who
          // cannot see red, and this view exists to show breaks.
          borderStyle: node.deleted ? "dashed" : "solid",
          opacity: node.deleted ? 0.55 : 1,
          background: colors.raised,
          color: colors.text,
        },
      };
    });
    const edges = graph.edges.map((edge) => ({
      id: edge.id,
      source: edge.fromAssetId,
      target: edge.toAssetId,
      animated: false,
      label: edge.source === "connector" ? "observed" : "asserted",
      labelStyle: { fontSize: 10, fill: colors.textMuted },
      style: {
        stroke: colors.border,
        // Provenance and flow are different edges and must not look alike
        // (`00c`): `derivedFrom` explains *how*, `feeds` says *that*.
        strokeDasharray: edge.relationship === "derivedFrom" ? "4 3" : undefined,
      },
    }));
    return { nodes, edges };
  }, [graph, colors]);

  if (error) return <Empty description={error} />;
  if (!graph || !flow) return <Text type="secondary">Walking lineage…</Text>;

  const only = graph.nodes.length <= 1;

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Flex align="center" gap={12} wrap="wrap">
        <Text type="secondary" style={{ fontSize: 13 }}>
          Upstream feeds this asset; downstream depends on it.
        </Text>
        <Space.Compact>
          {[1, 2, 3].map((n) => (
            <Button key={n} size="small" type={depth === n ? "primary" : "default"} onClick={() => setDepth(n)}>
              {n} hop{n === 1 ? "" : "s"}
            </Button>
          ))}
        </Space.Compact>
      </Flex>

      {only ? (
        // Not an empty canvas. "No lineage recorded" and "this asset is a leaf"
        // are the same picture and different facts, and only one of them is a
        // reason to go and record something.
        <Empty
          description={
            <span>
              No lineage recorded for this asset yet. Lineage is asserted through{" "}
              <Text code>POST /lineage</Text>, by a person or by a connector.
            </span>
          }
        />
      ) : (
        <div
          style={{
            height: 420,
            border: `1px solid ${colors.border}`,
            borderRadius: 16,
            background: colors.raised,
          }}
        >
          <ReactFlow
            nodes={flow.nodes}
            edges={flow.edges}
            fitView
            proOptions={{ hideAttribution: false }}
            nodesDraggable={false}
            nodesConnectable={false}
          >
            <Background color={colors.border} gap={16} />
            <Controls showInteractive={false} />
          </ReactFlow>
        </div>
      )}
    </Space>
  );
}

/** The canvas.
 *
 *  Cytoscape rather than the hand-drawn SVG this replaced. The SVG was honest
 *  at demo scale and explicitly would not survive 10k nodes (`00f`); Cytoscape
 *  ships a WebGL renderer and a deterministic `breadthfirst` layout, which are
 *  the two things that decision needed.
 *
 *  **Everything decidable lives in `graph/cytoscape.ts`** — which elements
 *  exist, what classes they carry, whether the layout is deterministic — and is
 *  tested there. This component is the imperative shell: mount, feed, listen.
 *  `00f` requires graph tests to assert the model rather than the picture, and
 *  that is only possible if the picture is this thin.
 */
function GraphCanvas({
  picture,
  colors,
  onExpand,
  label,
}: {
  picture: CyPicture;
  colors: (typeof palette)["light"];
  onExpand: (id: string) => void;
  label: string;
}) {
  const host = useRef<HTMLDivElement | null>(null);
  const cy = useRef<cytoscape.Core | null>(null);
  const expand = useRef(onExpand);
  expand.current = onExpand;

  const elements = useMemo(() => toElements(picture), [picture]);

  useEffect(() => {
    if (!host.current) return undefined;
    const instance = cytoscape({
      container: host.current,
      elements,
      // Chosen once, at creation. `00f` rejects a hybrid that swaps renderers
      // mid-session: the swap discards the layout at the moment a reader most
      // needs it, because their mental map of where things are is the main
      // thing keeping a large graph legible.
      // `renderer` is not in Cytoscape's published option type, but it is the
      // documented way to select the WebGL backend, so the cast is narrow and
      // stated rather than an `any` on the whole options object.
      ...(wantsWebgl(picture.nodes.length)
        ? ({ renderer: { name: "canvas", webgl: true } } as unknown as cytoscape.CytoscapeOptions)
        : {}),
      style: [
        {
          selector: "node",
          style: {
            label: "data(label)",
            "font-size": 11,
            color: colors.text,
            "text-valign": "bottom",
            "text-margin-y": 4,
            "background-color": colors.primary,
            width: 18,
            height: 18,
          },
        },
        { selector: "node.seed", style: { width: 26, height: 26, "font-weight": "bold" } },
        // A ring, not a colour: the expandable marker has to survive a reader
        // who cannot distinguish the two hues.
        {
          selector: "node.expandable",
          style: { "border-width": 3, "border-color": colors.primary, "background-opacity": 0.35 },
        },
        {
          selector: "node.truncated",
          style: { "border-width": 3, "border-style": "dashed", "border-color": colors.text },
        },
        { selector: "node.hidden-kind", style: { "background-color": colors.border } },
        // Removed nodes stay in the picture, marked by shape *and* opacity
        // rather than colour alone — a deletion shown only in red is invisible
        // to a reader who cannot see red.
        {
          selector: "node.removed",
          style: { shape: "diamond", "background-opacity": 0.4, "border-style": "dashed", "border-width": 2, "border-color": colors.text },
        },
        { selector: "node.added", style: { shape: "star" } },
        { selector: "edge", style: { width: 1, "line-color": colors.border, "curve-style": "straight" } },
        {
          // **A conclusion, drawn as one.** Dashed and tinted, so it is legible
          // as inferred without colour alone carrying the meaning —
          // `00h-ui-design-system.md` requires a state to survive being unable
          // to tell two hues apart, and this is a state somebody acts on.
          selector: "edge.derived",
          style: {
            "line-style": "dashed",
            "line-color": brand.cyan400,
            "target-arrow-color": brand.cyan400,
          },
        },
        { selector: "edge.removed", style: { "line-style": "dashed", "line-color": colors.text } },
        { selector: "edge.added", style: { width: 2, "line-color": colors.primary } },
      ],
      layout: layoutOptions(picture.seedId),
      // The reader drives the picture; nothing moves on its own.
      autoungrabify: true,
    });
    instance.on("tap", "node.expandable", (event) => {
      expand.current(event.target.id());
    });
    cy.current = instance;
    return () => {
      instance.destroy();
      cy.current = null;
    };
    // Colours change only with the theme, which remounts cheaply; elements are
    // handled below so an expansion does not tear the canvas down.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [colors]);

  // Elements are replaced in place and re-laid out, rather than remounting.
  // A remount loses the reader's pan and zoom on every expand, which is the
  // one thing they were using to keep their place.
  useEffect(() => {
    const instance = cy.current;
    if (!instance) return;
    instance.elements().remove();
    instance.add(elements as cytoscape.ElementDefinition[]);
    instance.layout(layoutOptions(picture.seedId)).run();
  }, [elements, picture.seedId]);

  return (
    <div
      ref={host}
      role="img"
      aria-label={label}
      style={{
        height: 420,
        border: `1px solid ${colors.border}`,
        borderRadius: 16,
        background: colors.raised,
      }}
    />
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
  //
  // And at the same *expansions*. The picture on screen is a seed walk plus
  // everything the reader opened; comparing it against a bare seed walk would
  // report every expanded node as added no matter when it arrived — a diff
  // that invents changes, which is worse than one that misses them.
  const expansions = model ? performedExpansions(model) : null;
  const expansionKey = expansions?.join(",") ?? null;

  useEffect(() => {
    if (compareTo === null || expansionKey === null) {
      setBaseline(null);
      return undefined;
    }
    let current = true;
    setBaseline(null);
    (async () => {
      const seedView = await api.graph(assetId, hops, compareTo);
      const steps = await Promise.all(
        (expansionKey === "" ? [] : expansionKey.split(",")).map(async (id) => ({
          id,
          // A node the reader expanded today may not have existed then. That
          // is an answer, not a failure: `null` skips the step, and the node
          // is then correctly reported as added.
          view: await api.graph(id, 1, compareTo).catch(() => null),
        })),
      );
      if (current) setBaseline(replay(assetId, seedView, steps));
    })().catch((e: unknown) => {
      if (current) {
        setError(
          e instanceof ApiError ? e.problem.detail : "could not load the earlier graph",
        );
      }
    });
    return () => {
      current = false;
    };
  }, [assetId, hops, compareTo, expansionKey]);

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


  if (error) {
    return <Empty description={error} />;
  }
  if (!view || !picture) {
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

      <GraphCanvas
        picture={{
          seedId: view.seedId,
          nodes: picture.nodes,
          edges: picture.edges,
          expanded: view.expanded,
          truncatedAt: view.truncatedAt,
        }}
        colors={colors}
        onExpand={expandNode}
        label={
          comparison
            ? `Graph comparison: ${comparison.summary.added} added, ${comparison.summary.removed} removed, ${comparison.summary.changed} changed`
            : `Graph neighbourhood: ${picture.nodes.length} nodes, ${picture.edges.length} edges`
        }
      />

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
function SignIn({ onSignIn, error }: { onSignIn: () => void; error?: string | null }) {
  return (
    <Flex align="center" justify="center" style={{ height: "100%" }}>
      <Card style={{ maxWidth: 460, width: "100%" }}>
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          {/* A sign-in that *failed* must not look like one that never
              happened. Returning silently to this panel tells the user to try
              the thing they just tried, with no hint that the provider
              refused, that consent was declined, or that the callback could
              not be verified. */}
          {error && (
            <Alert type="error" showIcon message="Sign-in did not complete" description={error} />
          )}
          <div>
            <Title level={4} style={{ margin: 0, fontWeight: 600 }}>
              Sign in to graph-owl
            </Title>
            <Paragraph type="secondary" style={{ marginTop: 8, marginBottom: 0 }}>
              This server requires authentication. Sign in with your
              organisation's identity provider to access the catalog — your
              data is not empty, it is simply waiting for you to identify
              yourself.
            </Paragraph>
          </div>
          <Button type="primary" size="large" block onClick={onSignIn}>
            Sign in with Auth0
          </Button>
        </Space>
      </Card>
    </Flex>
  );
}

function Denied() {
  return (
    <Flex align="center" justify="center" style={{ height: "100%" }}>
      <Card style={{ maxWidth: 460, width: "100%" }}>
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <div>
            <Title level={4} style={{ margin: 0, fontWeight: 600 }}>
              Access denied
            </Title>
            <Paragraph type="secondary" style={{ marginTop: 8, marginBottom: 0 }}>
              Your account is authenticated but you do not have permission to
              access this catalog. Contact your administrator to request the
              required role or scope.
            </Paragraph>
          </div>
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
      <OwnerChips owners={asset.owners} />

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
          // Lineage only for the kinds that carry it. Offering the tab on a
          // schema would promise an answer the model refuses to hold: lineage
          // runs table-to-table or column-to-column, and a coarse edge standing
          // in for the specific one is worse than none because it looks like an
          // answer.
          ...(asset.kind === "table" || asset.kind === "column"
            ? [
                {
                  key: "lineage",
                  label: (
                    <span>
                      <ApartmentOutlined /> Lineage
                    </span>
                  ),
                  children: <LineageView assetId={asset.id} colors={colors} />,
                },
              ]
            : []),
          // Offered on every kind, unlike Lineage: a rule can conclude
          // something about any asset, and hiding the tab where it happens to
          // be empty would make "the reasoner said nothing" indistinguishable
          // from "this console does not show you".
          {
            key: "reasoning",
            label: (
              <span>
                <ThunderboltOutlined /> Reasoning
              </span>
            ),
            children: <ReasoningView assetId={asset.id} colors={colors} />,
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

/** What the connector actually did, kept after it finished.
 *
 *  A run previously reported into the HTTP response and nowhere else, so "did
 *  last night's sync work" was unanswerable the moment the caller closed the
 *  connection. `15-connectors.md` treats run history as a governance concern
 *  for that reason: a catalog whose freshness cannot be evidenced is one nobody
 *  can trust a decision to.
 */
function RunHistory({ colors }: { colors: (typeof palette)["light"] }) {
  const [runs, setRuns] = useState<ConnectorRun[] | null>(null);

  useEffect(() => {
    api
      .connectorRuns()
      .then(setRuns)
      .catch(() => setRuns([]));
  }, []);

  if (runs === null) return <Text type="secondary">Loading run history…</Text>;
  if (runs.length === 0) {
    return (
      <Text type="secondary">
        No runs recorded yet. Catalogue a source and its result will appear here.
      </Text>
    );
  }

  return (
    <Table
      size="small"
      rowKey="id"
      pagination={false}
      dataSource={runs}
      columns={[
        {
          title: "When",
          dataIndex: "startedAt",
          render: (at: string) => new Date(at).toLocaleString(),
        },
        { title: "Service", dataIndex: "serviceName" },
        {
          title: "Result",
          render: (_: unknown, run: ConnectorRun) => {
            // A run that never reported is not a fast success, and the table
            // must not let the two look alike.
            if (run.finishedAt === null) {
              return <Tag color="red">did not finish</Tag>;
            }
            if (run.failed > 0) {
              return <Tag color="red">{run.failed} failed</Tag>;
            }
            // A refusal is a *successful* run that deliberately did nothing —
            // reading it as a failure sends someone looking for a fault that is
            // not there.
            if (run.refusal) {
              return <Tag color="orange">refused</Tag>;
            }
            return <Tag color="green">ok</Tag>;
          },
        },
        {
          title: "Written",
          render: (_: unknown, run: ConnectorRun) =>
            run.created === 0 && run.skipped > 0 ? (
              // The distinction the whole `skipped` column exists for: nothing
              // written because nothing changed, not because nothing worked.
              <Text type="secondary">unchanged ({run.skipped} skipped)</Text>
            ) : (
              <span>
                {run.created} written
                {run.skipped > 0 && (
                  <Text type="secondary"> · {run.skipped} unchanged</Text>
                )}
              </span>
            ),
        },
        {
          title: "Removed",
          render: (_: unknown, run: ConnectorRun) =>
            run.refusal ? (
              <Tooltip title={run.refusal}>
                <Text type="warning">refused</Text>
              </Tooltip>
            ) : (
              <Text type={run.deleted > 0 ? undefined : "secondary"}>{run.deleted}</Text>
            ),
        },
        { title: "By", dataIndex: "triggeredBy" },
      ]}
      style={{ border: `1px solid ${colors.border}`, borderRadius: 12 }}
    />
  );
}

/** The SPARQL workbench — Epic 41.
 *
 *  Three things on one screen, and the second is the one that makes the others
 *  usable: the query, **what the engine decided to read**, and the results.
 *  An author who cannot see the plan cannot tell a query that is inherently
 *  expensive from one that is a single triple pattern away from being cheap.
 */
function WorkbenchPage({ colors }: { colors: (typeof palette)["light"] }) {
  const [query, setQuery] = useState(
    "SELECT ?s ?p ?o WHERE { ?s ?p ?o }\nLIMIT 50",
  );
  const [result, setResult] = useState<SparqlResult | null>(null);
  const [running, setRunning] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);
  const [asGraph, setAsGraph] = useState(false);

  const run = async () => {
    setRunning(true);
    setFailed(null);
    try {
      setResult(await api.sparql(query));
    } catch (error) {
      // A parse error is the author's to fix and belongs on screen verbatim —
      // "query failed" sends them guessing at which line.
      setFailed(error instanceof ApiError ? error.problem.detail ?? error.problem.title : "the query did not run");
      setResult(null);
    } finally {
      setRunning(false);
    }
  };

  const rows: Solution[] = useMemo(() => [...(result?.rows ?? [])], [result]);
  const shape = useMemo(() => graphShape(rows), [rows]);
  const notes = useMemo(
    () =>
      result
        ? verdict(rows, {
            truncated: result.truncated,
            factsScanned: result.factsScanned,
            plan: result.plan,
          })
        : null,
    [result, rows],
  );

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <div>
        <Title level={4} style={{ margin: 0, fontWeight: 600 }}>
          Workbench
        </Title>
        <Paragraph type="secondary" style={{ margin: "4px 0 0", fontSize: 13 }}>
          SPARQL over the catalog graph, filtered to what you may see. The plan
          shows what the engine read to answer you.
        </Paragraph>
      </div>

      <Input.TextArea
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        autoSize={{ minRows: 5, maxRows: 16 }}
        spellCheck={false}
        style={{ fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace", fontSize: 13 }}
      />

      <Space>
        <Button type="primary" loading={running} onClick={() => void run()}>
          Run
        </Button>
        {result && (
          <Text type="secondary" style={{ fontSize: 12 }}>
            {rows.length} row{rows.length === 1 ? "" : "s"} · {result.factsScanned} facts read
          </Text>
        )}
      </Space>

      {failed && <Alert type="error" showIcon message="The query did not run" description={failed} />}

      {notes?.warnings.map((warning) => (
        <Alert key={warning} type="warning" showIcon message={warning} />
      ))}

      {result && (
        <Card size="small" title="Plan">
          {/* One line per scan. A single `? ? ?` is the whole graph, and that
              is exactly the entry worth seeing. */}
          {result.plan.map((scan, i) => (
            <div key={`${scan}-${i}`}>
              <Text code style={{ fontSize: 12 }}>
                {scan}
              </Text>
            </div>
          ))}
          {result.plan.length === 0 && (
            <Text type="secondary" style={{ fontSize: 12 }}>
              No scan was needed.
            </Text>
          )}
        </Card>
      )}

      {result && rows.length > 0 && (
        <>
          <Space>
            <Button size="small" type={asGraph ? "default" : "primary"} onClick={() => setAsGraph(false)}>
              Table
            </Button>
            {/* Offered only when the results *are* triples. Drawing arbitrary
                columns as nodes asserts a relationship the query never
                returned, and a picture is believed more readily than a table. */}
            <Tooltip
              title={
                shape ? undefined : "These results are not triples, so there is no honest graph to draw."
              }
            >
              <Button
                size="small"
                disabled={!shape}
                type={asGraph ? "primary" : "default"}
                onClick={() => setAsGraph(true)}
              >
                Graph
              </Button>
            </Tooltip>
          </Space>

          {asGraph && shape ? (
            <Card size="small" styles={{ body: { height: 420, padding: 0 } }}>
              <ReactFlow
                nodes={toGraph(rows, shape).nodes.map((node, i) => ({
                  id: node.id,
                  position: { x: (i % 5) * 190, y: Math.floor(i / 5) * 110 },
                  data: { label: node.label },
                  style: {
                    background: colors.surface,
                    border: `1px solid ${colors.border}`,
                    borderRadius: 8,
                    fontSize: 12,
                    padding: 6,
                  },
                }))}
                edges={toGraph(rows, shape).edges.map((edge, i) => ({
                  id: `${edge.from}-${edge.to}-${i}`,
                  source: edge.from,
                  target: edge.to,
                  label: edge.label,
                }))}
                fitView
                proOptions={{ hideAttribution: true }}
              >
                <Background />
                <Controls />
              </ReactFlow>
            </Card>
          ) : (
            <Table
              size="small"
              rowKey={(_, i) => String(i)}
              dataSource={rows.map((row, i) => ({ ...row, __i: i }))}
              pagination={{ pageSize: 25, size: "small" }}
              columns={resultColumns(rows, result.variables).map((name) => ({
                title: name,
                dataIndex: name,
                key: name,
                render: (value: string | undefined) =>
                  value === undefined ? (
                    // An unbound variable is not an empty string. `OPTIONAL`
                    // produces exactly this, and rendering it blank makes
                    // "no value" indistinguishable from "the empty value".
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      unbound
                    </Text>
                  ) : (
                    <Tooltip title={value}>
                      <Text style={{ fontSize: 12 }}>{displayTerm(value)}</Text>
                    </Tooltip>
                  ),
              }))}
            />
          )}
        </>
      )}

      {result && rows.length === 0 && !failed && (
        <Card>
          <Paragraph type="secondary" style={{ margin: 0 }}>
            The query ran and matched nothing. That is an answer — it read{" "}
            {result.factsScanned} facts to establish it.
          </Paragraph>
        </Card>
      )}
    </Space>
  );
}

/** What the reasoner concluded about this asset, and why — Demo 4's second half.
 *
 *  A derived fact is **visibly marked**: `00b` decision 2 keeps conclusions in
 *  their own graph precisely so nobody mistakes one for something a person
 *  asserted, and the console has to honour that or the separation is invisible
 *  where it matters most.
 */
function ReasoningView({
  assetId,
  colors,
}: {
  assetId: string;
  colors: (typeof palette)["light"];
}) {
  const [facts, setFacts] = useState<{ s: string; p: string; o: string; t: number }[] | null>(
    null,
  );
  const [open, setOpen] = useState<string | null>(null);
  const [failed, setFailed] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setFacts(null);
    api
      .derivedAbout(`1:${assetId}`)
      .then((found) => live && setFacts(found))
      .catch((error) => {
        if (!live) return;
        setFailed(error instanceof ApiError ? error.problem.title : "could not load conclusions");
        setFacts([]);
      });
    return () => {
      live = false;
    };
  }, [assetId]);

  if (failed) return <Alert type="error" showIcon message={failed} />;
  if (facts === null) return <Spin />;

  if (facts.length === 0) {
    return (
      <Paragraph type="secondary" style={{ fontSize: 13 }}>
        The reasoner has concluded nothing about this asset. Either no rule
        applies, or no run has happened since the facts that would trigger one —
        run reasoning from <Text strong>Governance</Text>.
      </Paragraph>
    );
  }

  return (
    <Space direction="vertical" size="small" style={{ width: "100%" }}>
      <Alert
        type="info"
        showIcon
        message="These are conclusions, not assertions"
        description="Nobody stated them. They live in their own graph and are replaced on every run — open one to see what it rests on."
      />
      {facts.map((fact) => {
        const key = `${fact.s}|${fact.p}|${fact.o}`;
        return (
          <Card key={key} size="small">
            <Flex justify="space-between" align="center" wrap gap={8}>
              <Space size={6} wrap>
                <Tag color="purple">derived</Tag>
                <Text code style={{ fontSize: 12 }}>
                  {triple(fact)}
                </Text>
              </Space>
              <Button size="small" onClick={() => setOpen(open === key ? null : key)}>
                {open === key ? "Hide" : "Why?"}
              </Button>
            </Flex>
            {open === key && (
              <div style={{ marginTop: 10 }}>
                <DerivationChain fact={fact} colors={colors} />
              </div>
            )}
          </Card>
        );
      })}
    </Space>
  );
}

/** Why a fact holds, as an indented chain — Epic 6 Slice D on screen.
 *
 *  **The point of reasoning being explainable is that somebody reads it.** A
 *  derived fact with no visible derivation is an assertion the system made up,
 *  and the reason `00a` sells explainability is that a governance decision
 *  taken on an inference nobody can check is a governance decision nobody will
 *  take.
 *
 *  The chain is rendered to the assertions underneath, not one level down: a
 *  premise that is itself derived is the interesting half.
 */
function DerivationChain({
  fact,
  colors,
}: {
  fact: { s: string; p: string; o: string };
  colors: (typeof palette)["light"];
}) {
  const [explanation, setExplanation] = useState<Explanation | null>(null);
  const [missing, setMissing] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setExplanation(null);
    setMissing(false);
    setFailed(null);
    api
      .explain(fact.s, fact.p, fact.o)
      .then((found) => live && setExplanation(found))
      .catch((error) => {
        if (!live) return;
        // A 404 means nothing supports this fact — a different statement from
        // "the server is down", and only one of them is about the data.
        if (error instanceof ApiError && error.problem.status === 404) setMissing(true);
        else setFailed(error instanceof ApiError ? error.problem.title : "could not explain");
      });
    return () => {
      live = false;
    };
  }, [fact.s, fact.p, fact.o]);

  if (failed) return <Alert type="error" showIcon message={failed} />;
  if (missing) {
    return (
      <Alert
        type="info"
        showIcon
        message="Nothing supports this fact"
        description="It is neither asserted nor implied by anything the reasoner can see. That is a different answer from “it is false”."
      />
    );
  }
  if (!explanation) return <Spin />;

  const rows = flatten(explanation);
  const depth = depthOf(explanation);
  const rules = rulesUsed(explanation);

  return (
    <Space direction="vertical" size="small" style={{ width: "100%" }}>
      <Space wrap>
        {explanation.status === "asserted" ? (
          <Tag color="blue">Asserted</Tag>
        ) : (
          <>
            <Tag color="purple">Derived</Tag>
            {/* Depth is the one number that says whether an inference is a
                restatement or a genuine conclusion. */}
            <Text type="secondary" style={{ fontSize: 12 }}>
              {depth} step{depth === 1 ? "" : "s"} deep
            </Text>
            {rules.map((rule) => (
              <Tag key={rule}>{rule}</Tag>
            ))}
          </>
        )}
      </Space>

      <div style={{ fontSize: 13 }}>
        {rows.map((row: ChainRow, index) => (
          <div
            key={`${row.depth}-${index}`}
            style={{
              // Indentation *is* the chain. A flat list of the same rows says
              // which facts took part and not how they hang together.
              paddingLeft: row.depth * 18,
              borderLeft: row.depth > 0 ? `1px solid ${colors.border}` : undefined,
              marginLeft: row.depth > 0 ? 4 : 0,
              padding: "3px 0 3px 8px",
            }}
          >
            {row.kind === "rule" ? (
              <Space size={6}>
                <Tag color="purple" style={{ marginInlineEnd: 0 }}>
                  {row.rule}
                </Tag>
                {row.route !== undefined && (
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    route {row.route}
                  </Text>
                )}
              </Space>
            ) : row.kind === "asserted" ? (
              <Space size={6}>
                <Tag color="blue" style={{ marginInlineEnd: 0 }}>
                  asserted
                </Tag>
                <Text code style={{ fontSize: 12 }}>
                  {row.fact ? triple(row.fact) : ""}
                </Text>
              </Space>
            ) : row.kind === "circular" ? (
              // Only reachable through a cyclic ontology. Named rather than
              // truncated, or a modelling error reads as a short chain.
              <Text type="warning" style={{ fontSize: 12 }}>
                circular — {row.fact ? triple(row.fact) : ""}
              </Text>
            ) : (
              <Text type="secondary" style={{ fontSize: 12 }}>
                nothing supports this premise
              </Text>
            )}
          </div>
        ))}
      </div>
    </Space>
  );
}

/** A fact as a reader reads it, without the namespace codes. */
function triple(fact: { s: string; p: string; o: string }): string {
  return `${localName(fact.s)} ${localName(fact.p)} ${localName(fact.o)}`;
}

/** Severity, as a colour and a word.
 *
 *  Never colour alone: `00h-ui-design-system.md` requires a state to be legible
 *  without it, because a red dot and an amber dot are the same dot to a reader
 *  who cannot tell them apart.
 */
const SEVERITY_TAG: Record<Severity, { color: string; label: string }> = {
  violation: { color: "error", label: "Violation" },
  warning: { color: "warning", label: "Warning" },
  info: { color: "default", label: "Info" },
};

/** The violations queue, and the two engines that fill it — Epic 41.
 *
 *  One page for both because they answer the same question from two sides:
 *  validation says what is broken, reasoning says what the catalog believes and
 *  why. A steward opens this to decide what to do next.
 */
function GovernancePage({ colors }: { colors: (typeof palette)["light"] }) {
  const [findings, setFindings] = useState<readonly Finding[] | null>(null);
  const [computedAtT, setComputedAtT] = useState(0);
  const [currentT, setCurrentT] = useState(0);
  const [running, setRunning] = useState(false);
  const [lastRun, setLastRun] = useState<ValidationRun | null>(null);
  const [severity, setSeverity] = useState<Severity | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [waiving, setWaiving] = useState<Finding | null>(null);
  const [reason, setReason] = useState("");

  const load = useCallback(async () => {
    try {
      const report = await api.validationReport(severity ? { severity } : {});
      setFindings(report.data);
      setComputedAtT(report.computedAtT);
      setFailed(null);
    } catch (error) {
      // An empty queue and an unreachable one look identical, and only one of
      // them means "nothing is wrong".
      setFailed(error instanceof ApiError ? error.problem.title : "could not load the queue");
      setFindings([]);
    }
  }, [severity]);

  useEffect(() => {
    void load();
  }, [load]);

  const run = async () => {
    setRunning(true);
    try {
      const result = await api.runValidation();
      setLastRun(result);
      setCurrentT(result.computedAtT);
      await load();
    } catch (error) {
      setFailed(error instanceof ApiError ? error.problem.title : "the pass did not run");
    } finally {
      setRunning(false);
    }
  };

  const groups = useMemo(() => groupByAsset(findings ?? []), [findings]);
  // `currentT` is only known after a pass, so before one the report is judged
  // against itself — which reads as "current" and is honest: nothing newer is
  // known to exist.
  const age = currency(computedAtT, Math.max(currentT, computedAtT));

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Flex justify="space-between" align="flex-start" wrap gap={12}>
        <div>
          <Title level={4} style={{ margin: 0, fontWeight: 600 }}>
            Governance
          </Title>
          <Paragraph type="secondary" style={{ margin: "4px 0 0", fontSize: 13 }}>
            What the shapes say is broken, and what the reasoner concluded.
            Validation never blocks a write — it reports.
          </Paragraph>
        </div>
        <Space>
          <Tag color={age.stale ? "warning" : "success"}>{age.label}</Tag>
          <Button type="primary" loading={running} onClick={() => void run()}>
            Run validation
          </Button>
          <Button
            onClick={() => {
              void api.runReasoning().then(() => load());
            }}
          >
            Run reasoning
          </Button>
        </Space>
      </Flex>

      {failed && <Alert type="error" showIcon message={failed} />}

      {lastRun && (
        <Alert
          type={lastRun.conforms ? "success" : "warning"}
          showIcon
          message={
            lastRun.conforms
              ? `${lastRun.shapes} shape(s) ran, nothing violated`
              : `${lastRun.violations} violation(s), ${lastRun.warnings} warning(s) across ${lastRun.shapes} shape(s)`
          }
          description={
            // A pass over eighteen of twenty shapes produces a clean-looking
            // report for the two that did not run. Saying so is the point.
            lastRun.refusedShapes > 0
              ? `${lastRun.refusedShapes} shape(s) could not be compiled and did not run.`
              : undefined
          }
        />
      )}

      <Space>
        {(["violation", "warning", "info"] as const).map((s) => (
          <Button
            key={s}
            size="small"
            type={severity === s ? "primary" : "default"}
            onClick={() => setSeverity(severity === s ? null : s)}
          >
            {SEVERITY_TAG[s].label}
          </Button>
        ))}
      </Space>

      {findings === null ? (
        <Spin />
      ) : groups.length === 0 ? (
        <Card>
          <Paragraph type="secondary" style={{ margin: 0 }}>
            {computedAtT === 0
              ? "No validation pass has run yet. An empty queue is only good news once something has looked."
              : "Nothing violated. Every shape ran and every asset it targets conforms."}
          </Paragraph>
        </Card>
      ) : (
        <Space direction="vertical" size="small" style={{ width: "100%" }}>
          {groups.map((group) => (
            <Card
              key={group.focusNode}
              size="small"
              title={
                <Space>
                  <Tag color={SEVERITY_TAG[group.severity].color}>
                    {SEVERITY_TAG[group.severity].label}
                  </Tag>
                  <Text strong>{localName(group.focusNode)}</Text>
                  {/* Shown, never hidden: an accepted finding removed from the
                      queue is one nobody reviews — including nobody noticing
                      the acceptance is about to lapse. */}
                  {group.fullyWaived && <Tag color="processing">Accepted</Tag>}
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {group.findings.length} finding
                    {group.findings.length === 1 ? "" : "s"}
                  </Text>
                </Space>
              }
            >
              <Space direction="vertical" size={6} style={{ width: "100%" }}>
                {group.findings.map((finding) => {
                  const fix = describeSuggestion(finding.suggestion);
                  return (
                    <div key={finding.id}>
                      <Space size={8} wrap>
                        <Tag>{finding.constraint}</Tag>
                        <Text>{finding.message}</Text>
                        {finding.actual && (
                          <Text code style={{ fontSize: 12 }}>
                            {finding.actual}
                          </Text>
                        )}
                      </Space>
                      {fix && (
                        <div style={{ marginTop: 2 }}>
                          <Text type="secondary" style={{ fontSize: 12 }}>
                            {/* Suggested, never applied: the catalog does not
                                know whether a missing owner means "assign one"
                                or "this is deprecated". */}
                            Suggested — {fix}
                          </Text>
                        </div>
                      )}
                      <div>
                        <Text type="secondary" style={{ fontSize: 11 }}>
                          {localName(finding.shape)}
                          {finding.path ? ` · ${localName(finding.path)}` : ""}
                        </Text>
                      </div>
                      {finding.waiver ? (
                        <div style={{ marginTop: 4 }}>
                          <Space size={6} wrap>
                            {/* An expired acceptance and none at all look
                                identical unless the lapse is said out loud,
                                and only one is somebody's to answer for. */}
                            <Tag color={finding.waiver.expired ? "warning" : "processing"}>
                              {finding.waiver.expired ? "Acceptance expired" : "Accepted"}
                            </Tag>
                            <Text type="secondary" style={{ fontSize: 12 }}>
                              {finding.waiver.reason} — {finding.waiver.waivedBy}, until{" "}
                              {new Date(finding.waiver.expiresAt).toLocaleDateString()}
                            </Text>
                            <Button
                              size="small"
                              onClick={() => {
                                const id = finding.waiver?.id;
                                if (id) void api.revokeWaiver(id).then(() => load());
                              }}
                            >
                              Revoke
                            </Button>
                          </Space>
                        </div>
                      ) : (
                        <Button
                          size="small"
                          style={{ marginTop: 4 }}
                          onClick={() => setWaiving(finding)}
                        >
                          Accept…
                        </Button>
                      )}
                    </div>
                  );
                })}
              </Space>
            </Card>
          ))}
        </Space>
      )}

      <Modal
        open={waiving !== null}
        title="Accept this violation"
        okText="Accept for 30 days"
        okButtonProps={{ disabled: reason.trim().length === 0 }}
        onCancel={() => {
          setWaiving(null);
          setReason("");
        }}
        onOk={() => {
          if (!waiving) return;
          const expires = new Date();
          expires.setDate(expires.getDate() + 30);
          void api
            .waiveFinding({
              shape: waiving.shape,
              focusNode: waiving.focusNode,
              path: waiving.path,
              constraint: waiving.constraint,
              reason: reason.trim(),
              expiresAt: expires.toISOString(),
            })
            .then(() => {
              setWaiving(null);
              setReason("");
              return load();
            })
            .catch((error) =>
              setFailed(
                error instanceof ApiError
                  ? (error.problem.detail ?? error.problem.title)
                  : "the waiver was refused",
              ),
            );
        }}
      >
        <Paragraph type="secondary" style={{ fontSize: 13 }}>
          {/* Both rules are enforced by the server; saying them here is what
              stops somebody discovering them by being refused. */}
          A waiver has to say why, and it expires. Without a reason nobody can
          review it later; without an expiry it is a rule switched off where
          nobody will see it again.
        </Paragraph>
        <Input.TextArea
          autoFocus
          rows={3}
          value={reason}
          placeholder="Why is this acceptable, and until what changes?"
          onChange={(e) => setReason(e.target.value)}
        />
      </Modal>

      <Text type="secondary" style={{ fontSize: 11, color: colors.border }}>
        Repairs are suggestions. Nothing on this page is applied automatically.
      </Text>
    </Space>
  );
}

function ConnectorsPage({ onDone, colors }: { onDone: () => void; colors: (typeof palette)["light"] }) {
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

      <div>
        <Title level={5} style={{ margin: "8px 0 4px", fontWeight: 600 }}>
          Recent runs
        </Title>
        <Text type="secondary" style={{ fontSize: 13 }}>
          What each catalogue run did, kept after it finished.
        </Text>
      </div>
      <RunHistory colors={colors} />
    </Space>
  );
}

export default function App() {
  return <AuthProvider><AppShell /></AuthProvider>;
}

function AppShell() {
  const { dark, toggle } = useTheme();
  const auth = useAuth();

  useEffect(() => {
    setRefreshHandler(tryRefresh);
  }, []);
  const colors = dark ? palette.dark : palette.light;
  // Overview answers "what is in here" without a click; Explore is where you
  // go once you know what you are looking for. A deep link to an asset lands
  // on Explore regardless, or the link would not open the thing it names.
  const [section, setSectionRaw] = useState<Section>(() => {
    const named = readParam("section");
    if (
      named === "connectors" ||
      named === "explore" ||
      named === "overview" ||
      named === "governance" ||
      named === "workbench"
    ) {
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
  // 403 Forbidden while signed in. Distinct from refused (unauthenticated) —
  // the user's credentials are valid but they lack a role/claim to do this.
  const [denied, setDenied] = useState(false);
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
        // Both cleared, not just `refused`. A reader who was denied and has
        // since been granted a role would otherwise keep the denial screen
        // until they reloaded — and reloading is the thing they were told the
        // problem was not.
        setRefused(false);
        setDenied(false);
        setIndex((i) => ({ ...i, ...Object.fromEntries(roots.map((r) => [r.id, r])) }));
        setNodes(roots.map(toNode));
      })
      .catch((error: unknown) => {
        // An auth failure is not an empty result. Anything else genuinely is
        // a failure to load, and still must not claim the catalog is empty.
        setRefused(isUnauthenticated(error));
        setDenied(isForbidden(error));
        setNodes([]);
      });
    api
      .stats()
      .then((s) => setStats(s.byKind))
      .catch(() => setStats([]));
  }, [toNode]);

  // **Gated on the sign-in having settled, and re-run when it does.**
  //
  // On the OIDC callback this component mounts *while the code is still being
  // exchanged*, so a request fired now goes out with no token, comes back 401,
  // and sets `refused`. Nothing cleared it: the exchange then succeeded, the
  // token arrived, and the console kept showing the sign-in screen to a user
  // who had just signed in — indistinguishable from the sign-in never having
  // worked.
  //
  // Waiting through `loading` avoids the doomed request; depending on the
  // status re-runs the load the moment the token exists.
  const authStatus = auth.state.status;
  useEffect(() => {
    if (authStatus === "loading") return;
    refresh();
  }, [refresh, authStatus]);

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
              {auth.state.status === "authenticated" && (
                <Tooltip title="Sign out and return to the login screen">
                  <Button
                    type="text"
                    icon={<UserOutlined />}
                    aria-label="Sign out"
                    onClick={() => auth.logout()}
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
                about their data rather than a cosmetic bug. A 403
                (denied) is likewise separate — the user is signed in but
                lacks a role, so telling them to sign in again is wrong. */}
            {denied ? (
              <Content style={{ padding: 24 }}>
                <Denied />
              </Content>
            ) : auth.state.status === "loading" ? (
              /* Mid token-exchange. Without this the callback lands on a
                 tokenless first render, the catalog 401s, and the user sees a
                 flash of the sign-in screen they just came back from. */
              <Content style={{ padding: 24 }}>
                <Flex align="center" justify="center" style={{ height: "100%" }}>
                  <Space direction="vertical" align="center" size="middle">
                    <Spin size="large" />
                    <Text type="secondary">Completing sign-in…</Text>
                  </Space>
                </Flex>
              </Content>
            ) : refused || auth.state.error ? (
              <Content style={{ padding: 24 }}>
                <SignIn onSignIn={() => auth.login()} error={auth.state.error} />
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
                  {
                    key: "governance",
                    icon: <SafetyCertificateOutlined />,
                    label: "Governance",
                  },
                  { key: "workbench", icon: <ThunderboltOutlined />, label: "Workbench" },
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
              ) : section === "governance" ? (
                <GovernancePage colors={colors} />
              ) : section === "workbench" ? (
                <WorkbenchPage colors={colors} />
              ) : section === "connectors" ? (
                <ConnectorsPage onDone={refresh} colors={colors} />
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
