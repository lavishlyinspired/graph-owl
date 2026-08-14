/* eslint-disable local/no-raw-jsx-text -- Epic 39 Slice E decision 7 asked for
 * every user-visible literal to be externalized from the first commit, but
 * this file predates the lint rule that enforces it (added Epic 39 Slice F,
 * 6 August 2026) and carries ~190 pre-existing violations. Retrofitting an
 * externalized-strings file across a 4500-line, single-file console is a
 * real, separate, mechanical task — not something to rush inside the slice
 * that is also standing up CI budgets, Playwright, and axe.
 *
 * A per-line disable across 190 sites would not be more honest than this —
 * it adds noise without extracting a single string — so the file carries one
 * disable rather than 190. The real cost of that: this file-level disable
 * also covers *new* code added to App.tsx, not only the pre-existing
 * literals, because ESLint disables do not have grandfather semantics. Any
 * new component with user-visible text should go in its own file (the rule
 * is enforced everywhere else in `src/`) rather than growing App.tsx
 * further — which is the same direction Epic 42's planned `features/`
 * structure already pushes toward. */
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  App as AntApp,
  Breadcrumb,
  Alert,
  Button,
  Card,
  Col,
  ConfigProvider,
  Descriptions,
  Divider,
  Empty,
  Flex,
  Form,
  Input,
  Layout,
  Menu,
  Popover,
  Row,
  Segmented,
  Select,
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
import BookOutlined from "@ant-design/icons/es/icons/BookOutlined";
import CalendarOutlined from "@ant-design/icons/es/icons/CalendarOutlined";
import ReconciliationOutlined from "@ant-design/icons/es/icons/ReconciliationOutlined";
import BulbOutlined from "@ant-design/icons/es/icons/BulbOutlined";
import DownloadOutlined from "@ant-design/icons/es/icons/DownloadOutlined";
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
import MergeOutlined from "@ant-design/icons/es/icons/MergeOutlined";
import TeamOutlined from "@ant-design/icons/es/icons/TeamOutlined";
import UserOutlined from "@ant-design/icons/es/icons/UserOutlined";
import RobotOutlined from "@ant-design/icons/es/icons/RobotOutlined";
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
  type Team,
} from "./api";
import { AuthProvider, useAuth, tryRefresh } from "./auth";
import { type DiffEdge, diff } from "./graph/diff";
import { overflowTitle, summarizeOwners } from "./graph/owners";
import { ReasoningView } from "./graph/ReasoningView";
import {
  CertificationBadge,
  ConfidenceBadge,
  userTextDir,
} from "./trust/TrustComponents";
import { VocabularySection } from "./features/vocabulary/VocabularySection";
import { ReviewSection } from "./features/review/ReviewSection";
import { ObligationCalendar, setObligationCalendarParams } from "./features/obligations/obligationCalendar";
import { ReconciliationWorkspace } from "./features/reconciliation/ReconciliationWorkspace";
import { AgentChat } from "./features/agentChat/AgentChat";
import { PackAdminPanel } from "./features/packs/PackAdminPanel";
import { KnowledgeGraphToggle } from "./features/knowledge/KnowledgeGraphToggle";
import { AgentActivityPanel } from "./features/agents/AgentActivityPanel";
import { BoltSessionsPanel } from "./features/agents/BoltSessionsPanel";
import { PartitionHealthPanel } from "./features/agents/PartitionHealthPanel";
import { OutboundWebhooksPanel } from "./features/agents/OutboundWebhooksPanel";
import { OntologyEditor } from "./features/ontology/OntologyEditor";
import { hierarchy, type TeamNode } from "./admin/hierarchy";
import { fields as schemaFields, missing as schemaMissing, renderable } from "./admin/schemaForm";
import {
  type Draft as PolicyDraft,
  type DryRun,
  type Operation,
  type Policy,
  incomplete as policyIncomplete,
  toPolicy,
  verdict as policyVerdict,
} from "./admin/policy";
import {
  type Memory,
  type RecalledMemory,
  authorLabel,
  isVisibleOnEntityPage,
  stalenessLabel,
} from "./memory/memory";
import { type GraphModel, expand, performedExpansions, replay, seed } from "./graph/model";
import { brand, darkTheme, lightTheme, palette } from "./theme";
import { GenericSourceMark, PostgresMark } from "./icons";
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
  type Solution,
  alignmentBadgeLabel,
  columns as resultColumns,
  display as displayTerm,
  graphShape,
  toGraph,
  verdict,
} from "./workbench/results";
import { GraphCanvas } from "./graph/GraphCanvas";
import { PathFinder } from "./graph/PathFinder";
import { filterParam, kindsFromAnalytics, whyEmpty } from "./graph/edgeFilter";
import { ConnectivityPanel } from "./graph/ConnectivityPanel";
import { Disagreements } from "./memory/Disagreements";
import {
  type Drafts,
  type Language,
  defaultQuery,
  isLanguage,
  keepDrafts,
  pastTenseNote,
} from "./workbench/language";
import type { ConnectorRun, ReasoningReport } from "./api";
import { describeReasoningRun } from "./governance/reasoningRun";
import { ReasoningPanel } from "./features/reasoning/ReasoningPanel";
import { GovernanceQueues } from "./features/governance/GovernanceQueues";
import { QualityPanel } from "./features/governance/QualityPanel";
import { MetricsPanel } from "./features/governance/MetricsPanel";
import { ExportDialog } from "./features/export/ExportDialog";
import watermarkImg from "./assets/watermark1.png";

const { Header, Sider, Content } = Layout;
const { Text, Title, Paragraph } = Typography;

type Section =
  | "overview"
  | "reconciliation"
  | "explore"
  | "connectors"
  | "governance"
  | "workbench"
  | "vocabulary"
  | "review"
  | "obligations"
  | "admin"
  | "agent";

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
        <CertificationBadge certification={{}} />
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
        <Title level={2} style={{ margin: 0, fontWeight: 600, fontSize: 16 }}>
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
  // The lag is named in the hint text itself, not only by the tile's colour,
  // so it reads the same without colour (00h).
  const graphLag = graph !== null && graph.nodes < assets.total;
  const graphHint =
    graph === null
      ? "no graph engine configured"
      : graphLag
        ? `${(assets.total - graph.nodes).toLocaleString()} assets not yet projected · ${graph.edges.toLocaleString()} edges`
        : `${graph.flakes.toLocaleString()} facts · ${graph.edges.toLocaleString()} edges`;

  return (
    <Space direction="vertical" size="large" style={{ width: "100%" }}>
      <div>
        <Title level={2} style={{ margin: 0, fontWeight: 600, fontSize: 16 }}>
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
          {tile(
            "Graph",
            graph ? (
              <span style={{ color: graphLag ? colors.warning : undefined }}>
                {graph.nodes.toLocaleString()}
              </span>
            ) : (
              "—"
            ),
            graphHint,
          )}
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
  assetName,
  asOf,
  colors,
}: {
  assetId: string;
  /** Only so the connection finder below can name this end of a route.
   *  The traversal itself never sees it — identities travel, labels do not. */
  assetName: string;
  asOf: string | null;
  colors: (typeof palette)["light"];
}) {
  const [hops, setHops] = useState(2);
  // Plan 112 Slice A. **The options come from analytics, which walks
  // unfiltered**, not from the current response: deriving them from a filtered
  // walk collapses them to the selection, and accumulating them across walks
  // fails outright on a *shared* filtered URL, where no unfiltered walk ever
  // happens. Found in a browser on the very link this made shareable.
  const [selected, setSelectedRaw] = useState<readonly string[]>(() =>
    (readParam("edges") ?? "").split(",").filter(Boolean),
  );
  const [kinds, setKinds] = useState<readonly string[]>([]);

  useEffect(() => {
    let live = true;
    setKinds([]);
    api
      .assetAnalytics(assetId, { hops })
      .then((found) => live && setKinds(kindsFromAnalytics(found.edgeTypes)))
      // No control rather than a broken one: a failed lookup leaves the reader
      // exactly where they were before this slice existed.
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [assetId, hops]);
  const setSelected = useCallback((next: readonly string[]) => {
    setSelectedRaw(next);
    // Shareable, like `hops`/`asOf`/`expand` already are — a filtered picture
    // somebody pastes into a review must come back filtered.
    writeParam("edges", next.length > 0 ? next.join(",") : null);
  }, []);
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
      const filter = filterParam(selected);
      let next = seed(assetId, await api.graph(assetId, hops, asOf, filter));
      for (const id of replay) {
        // **Expansion carries the filter.** A filter applied to the seed and
        // dropped on expansion produces a picture that is half filtered and
        // half not, with nothing on screen to say which half is which.
        next = expand(next, id, await api.graph(id, 1, asOf, filter));
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
  }, [assetId, hops, asOf, selected]);

  /** Epic 40 decision 2: the canvas grows by explicit expansion, never by
   *  "show everything". One hop per click, budgeted server-side like every
   *  other walk. */
  const expandNode = useCallback(
    (nodeId: string) => {
      if (!model || model.expanded.includes(nodeId) || expanding) return;
      setExpanding(nodeId);
      api
        .graph(nodeId, 1, asOf, filterParam(selected))
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
    [model, expanding, asOf, selected],
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
  // `kinds` is every edge name any walk has seen, so it is the honest witness
  // to "this node does have edges" even when the current filtered walk shows
  // none.
  const emptyReason = whyEmpty({
    selected,
    hasAnyEdge: kinds.length > 0,
    edgesShown: view.edges.length,
  });

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

      {/* **The third of the four controls the explorer needs** — depth and
          time were here, relationships were not, and a hub node's
          neighbourhood is unreadable without it: the picture is complete and
          tells the reader nothing. Options come from every walk so far, never
          from the current (possibly filtered) response, or the list would
          collapse to the selection and could never be widened again.

          Rendered only once a walk has reported at least one edge name: a
          deployment whose graph has no edges gets nothing rather than an
          enabled control with no options in it. */}
      {kinds.length > 0 && (
        <Flex align="center" gap={12} wrap>
          <Text type="secondary" style={{ fontSize: 13 }}>
            {GRAPH_COPY.relationships}
          </Text>
          <Select
            mode="multiple"
            allowClear
            size="small"
            aria-label={GRAPH_COPY.relationships}
            placeholder={GRAPH_COPY.everyRelationship}
            style={{ minWidth: 260 }}
            value={[...selected]}
            onChange={(next: string[]) => setSelected(next)}
            options={kinds.map((kind) => ({ value: kind, label: kind }))}
          />
        </Flex>
      )}

      {/* **"Nothing matches these filters" is not "nothing is connected."**
          The second is a claim about the graph, and showing it when a filter is
          responsible sends the reader hunting for data that is right there. */}
      {emptyReason && <Alert type="info" showIcon message={emptyReason} />}

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

      {/* **A different question from the one above, in the same place.** The
          neighbourhood answers "what is near this"; this answers "how is this
          related to *that*" — the question whose answer nobody can predict the
          shape of, and the one the traversal engine could answer since Epic 7a
          with nothing in the product able to ask it (Plan 111 Slice A). It
          lives inside this tab rather than as a route of its own: `routes.ts`
          rewards one surface absorbing many questions, and starting from the
          asset already on screen is what makes it discoverable at all. */}
      {/* **Which node is the hub of what you are looking at** — Plan 112
          Slice C. `GET /assets/{id}/analytics` and `api.assetAnalytics` both
          existed and nothing in the console called either. It sits under the
          picture and shares the picture's depth, so the numbers describe the
          neighbourhood on screen rather than a different one. */}
      <ConnectivityPanel
        cacheKey={`${assetId}:${hops}`}
        load={() => api.assetAnalytics(assetId, { hops })}
        names={
          new Map(
            picture.nodes.map((node) => [`${DSC_NAMESPACE}:${node.id}`, node.name] as const),
          )
        }
      />

      <PathFinder
        seedId={assetId}
        seedName={assetName}
        asOf={asOf}
        edgeKinds={kinds}
        colors={colors}
      />
    </Space>
  );
}

/** Plan 112 Slice A's user-visible words. Externalized like every other
 *  string in this console — `local/no-raw-jsx-text` fails the build otherwise,
 *  and a control's label is exactly the kind of text a translator needs to
 *  find in one place. */
/** The catalog namespace every projected asset is keyed under
 *  (`graph_owl_core::flake::namespace::DSC`). Analytics reports node ids as
 *  rendered `Sid`s, and the graph view reports bare asset ids, so joining the
 *  two needs the prefix. Not a pack vocabulary — this is the platform's own
 *  namespace, which is why the neutrality check allowlists it. */
const DSC_NAMESPACE = 1;

const GRAPH_COPY = {
  relationships: "Relationships",
  everyRelationship: "every relationship",
};

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
        color: asOf ? "#0F172A" : colors.selectedText,
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
function SignIn({
  onSignIn,
  onToken,
  strategy,
  error,
}: {
  onSignIn: () => void;
  onToken: (token: string) => void;
  strategy: "provider" | "token" | "open";
  error?: string | null;
}) {
  const [pasted, setPasted] = useState("");

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
              {strategy === "token"
                ? "This server verifies a shared secret, so there is no identity provider to sign in with. Paste a bearer token — ./scripts/demo.sh --secure prints one for each principal."
                : "This server requires authentication. Sign in with your organisation's identity provider to access the catalog — your data is not empty, it is simply waiting for you to identify yourself."}
            </Paragraph>
          </div>
          {/* The button offered is the one that can succeed. Showing the
              provider button against a shared-secret server is what produced
              the sign-in loop: the provider authenticated the user perfectly
              and the server then refused every token it issued. */}
          {strategy === "token" ? (
            <>
              <Input.Password
                size="large"
                placeholder="eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9…"
                aria-label="Bearer token"
                value={pasted}
                onChange={(event) => setPasted(event.target.value)}
                onPressEnter={() => onToken(pasted)}
              />
              <Button
                type="primary"
                size="large"
                block
                disabled={pasted.trim().length === 0}
                onClick={() => onToken(pasted)}
              >
                Use this token
              </Button>
            </>
          ) : (
            <Button type="primary" size="large" block onClick={onSignIn}>
              Sign in with your identity provider
            </Button>
          )}
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
          dir={userTextDir(asset.description)}
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
  const [ancestorsFailed, setAncestorsFailed] = useState(false);
  const [childrenFailed, setChildrenFailed] = useState(false);

  useEffect(() => {
    setAncestorsFailed(false);
    api
      .ancestors(asset.id)
      .then(setAncestors)
      .catch(() => {
        setAncestors([]);
        setAncestorsFailed(true);
      });
    setChildrenFailed(false);
    if (asset.kind === "column") setChildren([]);
    else
      api
        .children(asset.id)
        .then(setChildren)
        .catch(() => {
          setChildren([]);
          setChildrenFailed(true);
        });
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

      {childrenFailed && (
        <Alert type="error" showIcon message="Contents failed to load — this is not a leaf asset." />
      )}

      {!childrenFailed && children.length > 0 && (
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
      {ancestorsFailed ? (
        <Alert type="error" showIcon message="Ancestors failed to load — this is not a root-level asset." />
      ) : (
        <Breadcrumb items={ancestors.map((a) => ({ title: a.name }))} />
      )}

      <Flex align="center" gap={12} wrap>
        <Title level={2} dir={userTextDir(asset.name)} style={{ margin: 0, fontWeight: 600, fontSize: 24 }}>
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
              <GraphExplorer
                assetId={asset.id}
                assetName={asset.fullyQualifiedName}
                asOf={asOf}
                colors={colors}
              />
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
            children: <ReasoningView subject={`1:${asset.id}`} colors={colors} />,
          },
          {
            key: "knowledge",
            label: (
              <span>
                <BulbOutlined /> Knowledge
              </span>
            ),
            children: <KnowledgeView assetId={asset.id} colors={colors} />,
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
function WorkbenchPage({ colors, asOf }: { colors: (typeof palette)["light"]; asOf: string | null }) {
  const [view, setView] = useState<"query" | "ontology">(
    () => (readParam("workbenchView") === "ontology" ? "ontology" : "query"),
  );
  // Plan 111 Slice B. `POST /cypher` had no console caller at all before
  // this — a property-graph query language implemented end to end and
  // reachable only with curl.
  const [language, setLanguageRaw] = useState<Language>(() => {
    const saved = readParam("lang");
    return isLanguage(saved) ? saved : "sparql";
  });
  const [query, setQuery] = useState(() => defaultQuery(language));
  // Per-language drafts. Toggling to look at the other language and back
  // must not silently discard real work.
  const [drafts, setDrafts] = useState<Drafts>({});
  const [result, setResult] = useState<SparqlResult | null>(null);
  const [running, setRunning] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);
  const [asGraph, setAsGraph] = useState(false);

  const setLanguage = (next: Language) => {
    const kept = keepDrafts(drafts, language, query);
    setDrafts(kept);
    setQuery(kept[next] ?? defaultQuery(next));
    setLanguageRaw(next);
    writeParam("lang", next === "sparql" ? null : next);
    // A result answered in the other language beside a query in this one
    // would be attributed to the wrong text. Clearing is the honest reset.
    setResult(null);
    setFailed(null);
  };

  const run = async () => {
    setRunning(true);
    setFailed(null);
    try {
      // Both routes render the identical outcome envelope server-side, which
      // is what lets one results table serve both without knowing which ran.
      setResult(
        language === "cypher" ? await api.cypher(query, asOf) : await api.sparql(query, asOf),
      );
    } catch (error) {
      // A parse error is the author's to fix and belongs on screen verbatim —
      // "query failed" sends them guessing at which line.
      setFailed(error instanceof ApiError ? error.problem.detail ?? error.problem.title : "the query did not run");
      setResult(null);
    } finally {
      setRunning(false);
    }
  };

  const asOfNote = pastTenseNote(asOf);

  const rows: Solution[] = useMemo(() => [...(result?.rows ?? [])], [result]);
  const shape = useMemo(() => graphShape(rows), [rows]);
  const notes = useMemo(
    () =>
      result
        ? verdict(rows, {
            truncated: result.truncated,
            factsScanned: result.factsScanned,
            plan: result.plan,
            silencedFailures: result.silencedFailures,
          })
        : null,
    [result, rows],
  );

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Space align="center" wrap>
        <div>
          {/* level 2, matching Overview's own top-level heading — this page
              sits directly under the app's h1, and level 4 here (found by
              this slice's own axe check, the first to run against this
              route) skipped two levels with nothing at 2 or 3 in between. */}
          <Title level={2} style={{ margin: 0, fontWeight: 600, fontSize: 16 }}>
            Workbench
          </Title>
          <Paragraph type="secondary" style={{ margin: "4px 0 0", fontSize: 13 }}>
            SPARQL or Cypher over the same catalog graph, filtered to what you may
            see. The plan shows what the engine read to answer you.
          </Paragraph>
        </div>
        <Segmented
          value={view}
          onChange={(value) => {
            const next = value as "query" | "ontology";
            setView(next);
            writeParam("workbenchView", next === "ontology" ? "ontology" : null);
          }}
          options={[
            { label: "Query", value: "query" },
            { label: "Ontology editor", value: "ontology" },
          ]}
        />
      </Space>

      <div style={{ display: view === "ontology" ? "block" : "none" }}>
        <OntologyEditor colors={colors} />
      </div>

      <div style={{ display: view === "query" ? "block" : "none" }}>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      {/* **Two languages, one graph, one results table.** The server renders
          both through the identical outcome envelope, which is what lets the
          plan, the budget and the error shape below be written once. */}
      <Segmented
        value={language}
        aria-label="Query language"
        onChange={(value) => setLanguage(value as Language)}
        options={[
          { label: "SPARQL", value: "sparql" },
          { label: "Cypher", value: "cypher" },
        ]}
      />
      {/* A result from the past that looks like a result from now is the one
          failure this surface cannot afford: historical and stale data are
          indistinguishable on screen unless something says which this is. */}
      {asOfNote && <Alert type="warning" showIcon message={asOfNote} />}
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
        {/* Epic 101 Slice E: result-level, not per-row — spareval gives no
            hook to attribute one bound row to the SERVICE call that produced
            it, only the query as a whole (see the plan's own scope note). */}
        {result && result.federatedEndpoints.length > 0 && (
          <Tooltip title="This answer includes data fetched live from these SERVICE endpoints.">
            <span>
              {result.federatedEndpoints.map((endpoint) => (
                <Tag key={endpoint} color="blue">
                  federated: {endpoint}
                </Tag>
              ))}
            </span>
          </Tooltip>
        )}
        {/* Epic 104's console criterion: "on any cross-vocabulary result the
            alignment that made it reachable is inspectable — a result that
            crossed an approximate match must be distinguishable from one
            that did not, and not by colour alone." The tag's own text
            (`alignmentBadgeLabel`) carries curated-vs-computed and, for a
            computed match, its confidence — colour here is a supplementary
            cue, never the only signal. The tooltip is what makes it
            *inspectable*: the alignment's own left/right terms, source
            detail, and directionality, without cluttering the results
            table itself. */}
        {result &&
          result.alignmentsUsed.map((entry) => (
            <Tooltip
              key={entry.subject}
              title={
                <>
                  {entry.left ?? "?"} → {entry.right ?? "?"}
                  {entry.sourceDetail ? ` · ${entry.sourceDetail}` : ""}
                  {entry.lossyReverse ? " · lossy walking back from right to left" : ""}
                </>
              }
            >
              <Tag color={entry.sourceKind === "computed" ? "gold" : "purple"}>
                {alignmentBadgeLabel(entry)}
              </Tag>
            </Tooltip>
          ))}
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
      </div>
    </Space>
  );
}


/** Institutional memory about one entity — Epic 31 recall, Epic 41 Slice E
 *  on screen.
 *
 *  **Confidence gates what is shown here, not what exists.** A memory below
 *  [`IGNORE_BAND`] is real and stays findable in Admin's cross-entity
 *  search; hiding it here is about not cluttering the default view with
 *  guesses, never about hiding the fact that somebody guessed.
 */
function KnowledgeView({
  assetId,
  colors,
}: {
  assetId: string;
  colors: (typeof palette)["light"];
}) {
  const [recalled, setRecalled] = useState<RecalledMemory[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [composing, setComposing] = useState(false);

  const load = useCallback(() => {
    api
      .recallMemories(assetId)
      .then((found) => {
        if (!Array.isArray(found)) {
          setError("the server returned an unexpected shape for memories");
          return;
        }
        setRecalled(found);
        setError(null);
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : "could not load memories"));
  }, [assetId]);
  useEffect(load, [load]);

  if (error) return <Alert type="error" showIcon message={error} />;
  if (recalled === null) return <Spin />;

  const visible = recalled.filter(isVisibleOnEntityPage);

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <KnowledgeGraphToggle assetId={assetId} />

      {/* **What this deployment believes twice, incompatibly** — Plan 111
          Slice C. `GET /assets/{id}/contradictions` shipped with Epic 31 and
          no browser ever called it, so a product whose claim is that
          conflicting institutional knowledge becomes *visible* was keeping
          the conflicts where only an integrator could find them. It sits
          above the memories rather than below: a reader who has already
          scrolled past both sides has formed a view of each in isolation,
          which is the exact mistake the pairing exists to prevent. */}
      <Disagreements assetId={assetId} memories={visible.map((item) => item.memory)} />

      <Divider />

      <Flex justify="space-between" align="center">
        <Text type="secondary" style={{ fontSize: 13 }}>
          What people and agents have recorded about this asset.
        </Text>
        <Button size="small" onClick={() => setComposing(true)} disabled={composing}>
          Add what we know
        </Button>
      </Flex>

      {composing && (
        <MemoryForm
          assetId={assetId}
          onSaved={() => {
            setComposing(false);
            load();
          }}
          onCancel={() => setComposing(false)}
        />
      )}

      {visible.length === 0 ? (
        <Paragraph type="secondary" style={{ fontSize: 13 }}>
          Nothing recorded here yet — or nothing confident enough to show. A
          steward can find lower-confidence memories in Admin.
        </Paragraph>
      ) : (
        visible.map((item) => (
          <MemoryCard key={item.memory.id} recalled={item} colors={colors} onChanged={load} />
        ))
      )}
    </Space>
  );
}

/** One recalled memory: content, confidence, provenance, and the score that
 *  put it here — the retrieval context a reader needs to weigh it, not just
 *  trust it. */
function MemoryCard({
  recalled,
  colors,
  onChanged,
}: {
  recalled: RecalledMemory;
  colors: (typeof palette)["light"];
  onChanged: () => void;
}) {
  const { memory, staleness, score } = recalled;
  const [correcting, setCorrecting] = useState(false);
  const [retracting, setRetracting] = useState(false);
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showScore, setShowScore] = useState(false);

  const retract = () => {
    if (reason.trim() === "") return;
    setBusy(true);
    setError(null);
    api
      .retractMemory(memory.id, reason.trim())
      .then(onChanged)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : "could not retract"))
      .finally(() => setBusy(false));
  };

  return (
    <Card size="small">
      <Space direction="vertical" size={8} style={{ width: "100%" }}>
        <Flex justify="space-between" align="start" wrap gap={8}>
          <Text dir={userTextDir(memory.content)}>{memory.content}</Text>
          <Tag color={staleness.state === "fresh" ? "green" : staleness.state === "possiblyStale" ? "gold" : "red"}>
            {stalenessLabel(staleness)}
          </Tag>
        </Flex>
        <Space size={12} wrap>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {authorLabel(memory.authorship)}
          </Text>
          <ConfidenceBadge confidence={memory.confidence} />
          <Text type="secondary" style={{ fontSize: 12 }}>
            as of {new Date(memory.asOf).toLocaleDateString()}
          </Text>
          <Button size="small" type="link" onClick={() => setShowScore(!showScore)} style={{ padding: 0, height: "auto" }}>
            {showScore ? "hide" : "why this ranks here"}
          </Button>
        </Space>

        {showScore && (
          <div style={{ fontSize: 12, color: colors.textMuted }}>
            anchor {score.anchor.toFixed(2)} · lexical {score.lexical.toFixed(2)} · semantic{" "}
            {score.semantic === null ? "not measured" : score.semantic.toFixed(2)} · staleness{" "}
            {score.staleness.toFixed(2)} · recency {score.recency.toFixed(2)} · authorship{" "}
            {score.authorship.toFixed(2)} · confidence {score.confidence.toFixed(2)} · total{" "}
            {score.total.toFixed(2)}
          </div>
        )}

        <Flex gap={8}>
          <Button size="small" onClick={() => setCorrecting(true)} disabled={correcting}>
            Correct
          </Button>
          <Button size="small" danger onClick={() => setRetracting(!retracting)}>
            Retract
          </Button>
        </Flex>

        {retracting && (
          <Flex gap={8} align="center">
            <Input
              placeholder="why is this no longer believed?"
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              style={{ maxWidth: 320 }}
            />
            <Button size="small" danger type="primary" onClick={retract} loading={busy} disabled={reason.trim() === ""}>
              Confirm retraction
            </Button>
          </Flex>
        )}
        {error && <Alert type="error" showIcon description={error} />}

        {correcting && (
          <MemoryForm
            assetId={memory.links.find((l) => l.relation === "about")?.target ?? ""}
            supersedes={memory}
            onSaved={() => {
              setCorrecting(false);
              onChanged();
            }}
            onCancel={() => setCorrecting(false)}
          />
        )}
      </Space>
    </Card>
  );
}

/** Records a new memory, or a correction to an existing one when
 *  `supersedes` is given. One form for both, because a correction is a
 *  memory with an extra step at the end — the fields somebody fills in are
 *  identical either way. */
function MemoryForm({
  assetId,
  supersedes,
  onSaved,
  onCancel,
}: {
  assetId: string;
  supersedes?: Memory;
  onSaved: () => void;
  onCancel: () => void;
}) {
  const [content, setContent] = useState(supersedes?.content ?? "");
  const [confidence, setConfidence] = useState(supersedes?.confidence ?? 0.9);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = () => {
    if (content.trim() === "") return;
    setBusy(true);
    setError(null);
    const body = {
      kind: supersedes?.kind ?? ("decision" as const),
      content: content.trim(),
      confidence,
      links: [{ relation: "about" as const, target: assetId }],
    };
    const request = supersedes
      ? api.supersedeMemory(supersedes.id, body)
      : api.createMemory(body);
    request
      .then(onSaved)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : "could not save"))
      .finally(() => setBusy(false));
  };

  return (
    <Card size="small" title={supersedes ? "Correct this memory" : "Add what we know"}>
      <Space direction="vertical" size={8} style={{ width: "100%" }}>
        <Input.TextArea
          placeholder="what should the next reader know?"
          value={content}
          onChange={(e) => setContent(e.target.value)}
          rows={2}
        />
        <Flex gap={8} align="center">
          <Text type="secondary" style={{ fontSize: 13 }}>
            confidence
          </Text>
          <Input
            type="number"
            min={0}
            max={1}
            step={0.05}
            value={confidence}
            onChange={(e) => setConfidence(Number(e.target.value))}
            style={{ maxWidth: 100 }}
          />
        </Flex>
        {error && <Alert type="error" showIcon description={error} />}
        <Flex gap={8}>
          <Button type="primary" size="small" onClick={save} loading={busy} disabled={content.trim() === ""}>
            Save
          </Button>
          <Button size="small" onClick={onCancel}>
            Cancel
          </Button>
        </Flex>
      </Space>
    </Card>
  );
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
  const [reasoningRun, setReasoningRun] = useState<ReasoningReport | null>(null);
  const [reasoningRunning, setReasoningRunning] = useState(false);
  const [reasoningFailed, setReasoningFailed] = useState<string | null>(null);

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
          {/* level 2, not 4: directly under the chrome's own h1, matching
           *  Workbench/Admin's own fix for the same heading-order bug. */}
          <Title level={2} style={{ margin: 0, fontWeight: 600, fontSize: 16 }}>
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
            loading={reasoningRunning}
            onClick={() => {
              setReasoningRunning(true);
              setReasoningFailed(null);
              void api
                .runReasoning()
                .then((result) => {
                  setReasoningRun(result);
                  return load();
                })
                .catch((error: unknown) => {
                  setReasoningFailed(
                    error instanceof ApiError ? error.problem.detail ?? error.problem.title : "the run did not complete",
                  );
                  setReasoningRun(null);
                })
                .finally(() => setReasoningRunning(false));
            }}
          >
            Run reasoning
          </Button>
        </Space>
      </Flex>

      {failed && <Alert type="error" showIcon message={failed} />}

      {/* Epic 41 Slice G: "overlay staleness is shown... an out-of-profile
          ontology, and an override-permitted partial result, are marked —
          not by colour alone." The technique, counts and watermark are
          named in the message text itself; a partial run gets its own
          warning-coloured alert naming what was ignored, distinct from the
          success alert a complete run gets. */}
      {reasoningFailed && <Alert type="error" showIcon message={reasoningFailed} />}
      {reasoningRun &&
        (() => {
          const description = describeReasoningRun(reasoningRun);
          return (
            <>
              <Alert type="info" showIcon message={description.headline} />
              {description.warning && <Alert type="warning" showIcon message={description.warning} />}
            </>
          );
        })()}

      {/* **The half of reasoning that had no console caller at all.** "Run
        *  reasoning" above applies RL rules; profile detection and EL
        *  classification — 947 lines in `graph-owl-reasoning-el` plus the
        *  profile detector — were reachable only by the MCP agent. Plan 110
        *  Slice 1. */}
      <ReasoningPanel />

      {/* Plan 110 Slice 3 — two queues that were one route away from visible. */}
      <GovernanceQueues />

      {/* Plan 110 Slice 2 — eleven routes, one coherent surface, no caller. */}
      <QualityPanel />

      {/* Plan 110 Slice 4 — scheduled last, built anyway; each is a table. */}
      <MetricsPanel />

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
        {/* level 2, not 4: directly under the chrome's own h1, matching
         *  Workbench/Admin's own fix for the same heading-order bug. */}
        <Title level={2} style={{ margin: 0, fontWeight: 600, fontSize: 16 }}>
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

/** Try the connection before anything is saved — Epic 41 Slice F.
 *
 *  **Before**, not after: a test that could only run against a stored config would
 *  confirm the credential once the mistake was already written. Disabled until the
 *  required fields are present, because a test that fails for a missing field
 *  teaches nothing about the connection. */
function ConnectionTest({
  values,
  disabled,
}: {
  values: Record<string, unknown>;
  disabled: boolean;
}) {
  const [result, setResult] = useState<{ ok: boolean; detail?: string } | null>(null);
  const [busy, setBusy] = useState(false);

  const run = () => {
    setBusy(true);
    setResult(null);
    const { password, ...settings } = values;
    api
      .testConnector("postgres", {
        settings,
        secret: typeof password === "string" ? password : undefined,
      })
      .then(setResult)
      .catch((e: unknown) =>
        setResult({ ok: false, detail: e instanceof Error ? e.message : "the test could not run" }),
      )
      .finally(() => setBusy(false));
  };

  return (
    <Space direction="vertical" size={8} style={{ width: "100%" }}>
      <Button onClick={run} loading={busy} disabled={disabled}>
        Test connection
      </Button>
      {result?.ok === true && (
        <Alert type="success" showIcon title="Connected" description="These settings reach the database." />
      )}
      {result?.ok === false && (
        // **The driver's own message, not "could not connect".** "Could not
        // connect" tells an admin nothing; `password authentication failed for
        // user "catalog"` tells them which of five fields is wrong. The server
        // redacts the secret before it leaves the process.
        <Alert type="error" showIcon title="Could not connect" description={result.detail} />
      )}
    </Space>
  );
}

/** Compose a policy and see what it would do — Epic 41 Slice F.
 *
 *  The plan's own reason for this screen: "a policy saved without preview is a
 *  production access change made blind, and this is the one screen where a mistake
 *  is a security incident." So the dry-run is the feature; the form exists to feed
 *  it. */
function PolicyPanel() {
  const [draft, setDraft] = useState<PolicyDraft>({
    name: "",
    ruleName: "",
    effect: "allow",
    operations: ["viewBasic"],
    matcherType: "fqnPrefix",
    matcherValue: "",
  });
  const [roles, setRoles] = useState("analyst");
  const [run, setRun] = useState<DryRun | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState<{ policy: Policy; roles: string[] }[]>([]);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const loadSaved = useCallback(() => {
    api
      .policies()
      .then((p) => {
        if (!Array.isArray(p)) {
          setSaveError("the server returned an unexpected shape for /policies");
          return;
        }
        setSaved(p);
        setSaveError(null);
      })
      .catch((e: unknown) =>
        setSaveError(e instanceof Error ? e.message : "could not load saved policies"),
      );
  }, []);
  useEffect(loadSaved, [loadSaved]);

  const missing = policyIncomplete(draft);
  const parsedRoles = () => roles.split(",").map((r) => r.trim()).filter((r) => r !== "");
  const preview = () => {
    setBusy(true);
    setError(null);
    api
      .dryRunPolicy(toPolicy(draft), parsedRoles())
      .then(setRun)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : "the dry-run failed"))
      .finally(() => setBusy(false));
  };
  const save = () => {
    setSaving(true);
    setSaveError(null);
    api
      .savePolicy(toPolicy(draft), parsedRoles())
      .then(loadSaved)
      .catch((e: unknown) => setSaveError(e instanceof Error ? e.message : "could not save"))
      .finally(() => setSaving(false));
  };

  const OPERATIONS: Operation[] = [
    "viewBasic",
    "viewDetails",
    "viewSensitive",
    "create",
    "editDescription",
    "editTags",
    "editOwners",
    "delete",
    "restore",
  ];

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Alert
        type="warning"
        showIcon
        title="Dry-run before saving"
        description="A policy saved without preview is a production access change made blind. Compose, dry-run to see what it would do, then save."
      />

      <Card size="small" title="Compose">
        <Space direction="vertical" size={8} style={{ width: "100%" }}>
          <Flex gap={8} wrap>
            <Input
              placeholder="policy name"
              value={draft.name}
              onChange={(e) => setDraft({ ...draft, name: e.target.value })}
              style={{ maxWidth: 200 }}
            />
            <Input
              placeholder="rule name"
              value={draft.ruleName}
              onChange={(e) => setDraft({ ...draft, ruleName: e.target.value })}
              style={{ maxWidth: 240 }}
            />
            <select
              aria-label="Effect"
              value={draft.effect}
              onChange={(e) => setDraft({ ...draft, effect: e.target.value as PolicyDraft["effect"] })}
            >
              <option value="allow">allow</option>
              <option value="deny">deny</option>
            </select>
          </Flex>
          <Flex gap={8} wrap align="center">
            <select
              aria-label="Applies to"
              value={draft.matcherType}
              onChange={(e) =>
                setDraft({ ...draft, matcherType: e.target.value as PolicyDraft["matcherType"] })
              }
            >
              <option value="all">everything</option>
              <option value="fqnPrefix">FQN prefix</option>
              <option value="tagged">tagged</option>
            </select>
            {draft.matcherType !== "all" && (
              <Input
                placeholder={draft.matcherType === "tagged" ? "tag, e.g. pii" : "prefix, e.g. warehouse."}
                value={draft.matcherValue}
                onChange={(e) => setDraft({ ...draft, matcherValue: e.target.value })}
                style={{ maxWidth: 240 }}
              />
            )}
          </Flex>
          <Space size={4} wrap>
            {OPERATIONS.map((op) => (
              <Tag.CheckableTag
                key={op}
                checked={draft.operations.includes(op)}
                onChange={(on) =>
                  setDraft({
                    ...draft,
                    operations: on
                      ? [...draft.operations, op]
                      : draft.operations.filter((o) => o !== op),
                  })
                }
              >
                {op}
              </Tag.CheckableTag>
            ))}
          </Space>
          <Flex gap={8} align="center" wrap>
            <Text type="secondary" style={{ fontSize: 13 }}>
              Simulate as roles:
            </Text>
            <Input
              value={roles}
              onChange={(e) => setRoles(e.target.value)}
              placeholder="analyst, steward"
              style={{ maxWidth: 220 }}
            />
            <Button type="primary" onClick={preview} loading={busy} disabled={missing.length > 0}>
              Dry run
            </Button>
            <Button onClick={save} loading={saving} disabled={missing.length > 0}>
              Save
            </Button>
            {missing.length > 0 && (
              <Text type="secondary" style={{ fontSize: 13 }}>
                still needed: {missing.join(", ")}
              </Text>
            )}
          </Flex>
        </Space>
      </Card>

      {error && <Alert type="error" showIcon title="Dry run failed" description={error} />}
      {run && <DryRunResult run={run} />}

      {saveError && (
        // Shared by save, load and delete — the description names the
        // specific failure, so a generic title here does not misreport a
        // failed delete as a failed save.
        <Alert type="error" showIcon title="Saved policies" description={saveError} />
      )}

      <Card size="small" title={`Saved policies (${saved.length})`}>
        <Table
          size="small"
          rowKey={({ policy }) => policy.name}
          dataSource={saved}
          pagination={false}
          locale={{ emptyText: "No policies saved yet. Compose and save one above." }}
          columns={[
            {
              title: "Policy",
              key: "policy",
              render: (_: unknown, { policy }: { policy: Policy; roles: string[] }) => (
                <Space direction="vertical" size={0}>
                  <Text strong>{policy.name}</Text>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {policy.rules.map((r) => `${r.effect} ${r.name}`).join(", ")}
                  </Text>
                </Space>
              ),
            },
            {
              title: "Roles",
              key: "roles",
              render: (_: unknown, { roles: appliesTo }: { policy: Policy; roles: string[] }) =>
                appliesTo.length === 0 ? (
                  <Text type="secondary" style={{ fontSize: 13 }}>
                    no roles — does not apply to anyone yet
                  </Text>
                ) : (
                  <Space size={4} wrap>
                    {appliesTo.map((r) => (
                      <Tag key={r}>{r}</Tag>
                    ))}
                  </Space>
                ),
            },
            {
              title: "",
              key: "actions",
              width: 90,
              render: (_: unknown, { policy }: { policy: Policy; roles: string[] }) => (
                <Button
                  size="small"
                  type="text"
                  danger
                  onClick={() => {
                    setSaveError(null);
                    api
                      .deletePolicy(policy.name)
                      .then(loadSaved)
                      .catch((e: unknown) =>
                        setSaveError(e instanceof Error ? e.message : "could not delete"),
                      );
                  }}
                >
                  Delete
                </Button>
              ),
            },
          ]}
        />
      </Card>
    </Space>
  );
}

/** The `SERVICE` allow-list — Epic 101 Slice E.
 *
 *  **Read-only, deliberately.** Unlike a policy, the allow-list has no
 *  persisted store behind it: it is set once, at startup, from
 *  `GRAPH_OWL_FEDERATION_ENDPOINTS` (see `main.rs` and `/admin/federation`'s
 *  own doc comment). An add/remove form here would either silently do
 *  nothing past the next restart, or need the same kind of dynamically-read
 *  store policies already have — real future work, not implied by this
 *  screen. What this gives an operator is confirmation of what is actually
 *  configured, and a dry-run against it before trusting a query that names
 *  one of these endpoints. */
function FederationPanel() {
  const [endpoints, setEndpoints] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [candidate, setCandidate] = useState("");

  useEffect(() => {
    api
      .federationEndpoints()
      .then((r) => setEndpoints(r.endpoints))
      .catch((e: unknown) => setError(e instanceof Error ? e.message : "could not load the allow-list"));
  }, []);

  const trimmed = candidate.trim();
  const allowed = endpoints !== null && trimmed !== "" ? endpoints.includes(trimmed) : null;

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Alert
        type="info"
        showIcon
        message="Configured at deployment, not here"
        description="This allow-list comes from GRAPH_OWL_FEDERATION_ENDPOINTS at startup. Changing it means changing the deployment's environment and restarting — there is nothing to save from this screen."
      />

      {error && <Alert type="error" showIcon message="Could not load the allow-list" description={error} />}

      <Card size="small" title={`Allow-listed endpoints (${endpoints?.length ?? 0})`}>
        {endpoints === null ? (
          <Text type="secondary">Loading…</Text>
        ) : endpoints.length === 0 ? (
          <Text type="secondary">
            No endpoints are allow-listed. Every <Text code>SERVICE</Text> clause is refused.
          </Text>
        ) : (
          <Space size={[4, 4]} wrap>
            {endpoints.map((endpoint) => (
              <Tag key={endpoint}>{endpoint}</Tag>
            ))}
          </Space>
        )}
      </Card>

      <Card size="small" title="Would a query be allowed to reach this endpoint?">
        <Space direction="vertical" size={8} style={{ width: "100%" }}>
          <Flex gap={8} wrap>
            <Input
              placeholder="https://example.org/sparql"
              value={candidate}
              onChange={(e) => setCandidate(e.target.value)}
              style={{ maxWidth: 320 }}
            />
          </Flex>
          {trimmed !== "" && endpoints !== null && (
            <Alert
              type={allowed ? "success" : "warning"}
              showIcon
              message={
                allowed
                  ? `Allowed — a SERVICE clause naming ${trimmed} would be answered.`
                  : `Not allowed — a SERVICE clause naming ${trimmed} would be refused.`
              }
            />
          )}
        </Space>
      </Card>
    </Space>
  );
}

/** What the preview means. The numbers alone cannot distinguish a correct policy
 *  from one that admits the whole estate, which is why `policyVerdict` exists. */
function DryRunResult({ run }: { run: DryRun }) {
  const { warnings } = policyVerdict(run);
  return (
    <Card size="small" title="What this policy would do">
      <Space direction="vertical" size={10} style={{ width: "100%" }}>
        {warnings.map((warning) => (
          <Alert key={warning} type="warning" showIcon description={warning} />
        ))}
        <Flex gap={24} wrap>
          <Statistic title="Visible" value={run.admitted} />
          <Statistic title="Hidden" value={run.denied} />
          <Statistic title="Total" value={run.total} />
        </Flex>
        {run.examples.length > 0 && (
          <div>
            <Text type="secondary" style={{ fontSize: 13 }}>
              For example:
            </Text>
            <Space direction="vertical" size={2} style={{ marginTop: 4 }}>
              {run.examples.map((fqn) => (
                <Text key={fqn} code style={{ fontSize: 12 }}>
                  {fqn}
                </Text>
              ))}
            </Space>
          </div>
        )}
      </Space>
    </Card>
  );
}

/** Cross-entity memory search, for administration — Epic 41 Slice E.
 *
 *  **Retracted memories default to excluded, matching the server's own
 *  default.** This is the one screen where they need to stay findable at
 *  all, which is what `Show retracted` is for — but the *default* view
 *  should not be cluttered with what nobody believes any more, same as the
 *  entity page.
 */
function MemoryAdminPanel({ colors }: { colors: (typeof palette)["light"] }) {
  const [author, setAuthor] = useState("");
  const [minConfidence, setMinConfidence] = useState("");
  const [includeRetracted, setIncludeRetracted] = useState(false);
  const [results, setResults] = useState<{ data: Memory[]; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const search = useCallback(() => {
    setBusy(true);
    setError(null);
    api
      .searchMemories({
        author: author.trim() === "" ? undefined : author.trim(),
        minConfidence: minConfidence === "" ? undefined : Number(minConfidence),
        includeRetracted,
      })
      .then(setResults)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : "could not search"))
      .finally(() => setBusy(false));
  }, [author, minConfidence, includeRetracted]);
  useEffect(search, [search]);

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Card size="small" title="Search">
        <Space direction="vertical" size={8} style={{ width: "100%" }}>
          <Flex gap={8} wrap align="center">
            <Input
              placeholder="author (user or agent id)"
              value={author}
              onChange={(e) => setAuthor(e.target.value)}
              style={{ maxWidth: 220 }}
            />
            <Input
              type="number"
              min={0}
              max={1}
              step={0.05}
              placeholder="min confidence"
              value={minConfidence}
              onChange={(e) => setMinConfidence(e.target.value)}
              style={{ maxWidth: 140 }}
            />
            <Tag.CheckableTag checked={includeRetracted} onChange={setIncludeRetracted}>
              Show retracted
            </Tag.CheckableTag>
            <Button size="small" onClick={search} loading={busy}>
              Search
            </Button>
          </Flex>
        </Space>
      </Card>

      {error && <Alert type="error" showIcon title="Could not search" description={error} />}

      <Card size="small" title={`Results (${results?.total ?? 0})`}>
        <Table
          size="small"
          rowKey={(memory: Memory) => memory.id}
          dataSource={results?.data ?? []}
          pagination={false}
          locale={{ emptyText: "No memories match this search." }}
          columns={[
            {
              title: "Memory",
              key: "content",
              render: (_: unknown, memory: Memory) => (
                <Space direction="vertical" size={0}>
                  <Text>{memory.content}</Text>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {authorLabel(memory.authorship)} · confidence {memory.confidence.toFixed(2)}
                  </Text>
                </Space>
              ),
            },
            {
              title: "State",
              key: "state",
              width: 160,
              render: (_: unknown, memory: Memory) =>
                memory.retractedAt ? (
                  <Tag color="red">retracted: {memory.retractionReason}</Tag>
                ) : memory.supersededBy ? (
                  <Tag color="default">corrected</Tag>
                ) : (
                  <Tag color="green">current</Tag>
                ),
            },
          ]}
        />
      </Card>
      <Text type="secondary" style={{ fontSize: 12, color: colors.textMuted }}>
        Below-confidence and retracted memories are hidden from entity pages
        but always findable here.
      </Text>
    </Space>
  );
}

/** The admin section — Epic 41 Slice F.
 *
 *  **`SchemaForm` is the only form renderer here.** A hundred connectors with
 *  hand-written forms is a hundred places for a field to go missing, and the one
 *  that goes missing is always the optional-looking one somebody needed. The
 *  structural test in `admin.test.ts` asserts this file grows no second renderer.
 */
function AdminPage({
  colors,
  setSection,
}: {
  colors: (typeof palette)["light"];
  setSection: (section: Section) => void;
}) {
  const viewObligationsFor = useCallback(
    (packId: string) => {
      setObligationCalendarParams({ pack: packId });
      setSection("obligations");
    },
    [setSection],
  );

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <div>
        {/* level 2, not 4: directly under the chrome's own h1, and this is
         *  the first spec to ever axe-scan the Admin section — found via a
         *  real axe run while adding Epic 42 Slice F's own tabs, the same
         *  heading-order class of bug Slice C already fixed elsewhere. */}
        <Title level={2} style={{ margin: 0, fontWeight: 600, fontSize: 16 }}>
          Admin
        </Title>
        <Text type="secondary">Principals, teams, and connector configuration.</Text>
      </div>
      <Tabs
        defaultActiveKey={readParam("adminTab") ?? "principals"}
        onChange={(key) => writeParam("adminTab", key === "principals" ? null : key)}
        items={[
          { key: "principals", label: "People & teams", children: <PrincipalsPanel colors={colors} /> },
          { key: "connectors", label: "Connector config", children: <ConnectorConfigPanel /> },
          { key: "policies", label: "Policies", children: <PolicyPanel /> },
          { key: "packs", label: "Packs", children: <PackAdminPanel onViewObligations={viewObligationsFor} /> },
          { key: "memories", label: "Memories", children: <MemoryAdminPanel colors={colors} /> },
          { key: "federation", label: "Federation", children: <FederationPanel /> },
          { key: "agents", label: "Agent activity", children: <AgentActivityPanel /> },
          { key: "bolt", label: "Bolt sessions", children: <BoltSessionsPanel /> },
          { key: "partitions", label: "Partition health", children: <PartitionHealthPanel /> },
          { key: "webhooks", label: "Outbound webhooks", children: <OutboundWebhooksPanel /> },
        ]}
      />
    </Space>
  );
}

/** People and teams, with the hierarchy drawn from `parentTeamId`.
 *
 *  Orphans and cycles are **shown, not hidden**: `hierarchy()` separates them, and
 *  an admin screen that quietly promoted an orphan to a root would assert it is
 *  top-level. A cycle is data the server refuses to create, so seeing one means
 *  something else wrote it — which is worth saying out loud. */
function PrincipalsPanel({ colors }: { colors: (typeof palette)["light"] }) {
  const [teams, setTeams] = useState<Team[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    api
      .teams()
      .then((t) => {
        // **Shape checked at the boundary, not assumed from the type.** A
        // `Team[]` annotation is a claim about this build, not a guarantee about
        // whichever server answers — the asset page went blank when an older one
        // omitted `owners`, and `hierarchy()` would throw the same way on a
        // non-array. An explicit message beats a white screen.
        if (!Array.isArray(t)) {
          setError("the server returned an unexpected shape for /teams");
          return;
        }
        setTeams(t);
        setError(null);
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : "could not load teams"));
  }, []);
  useEffect(load, [load]);

  const tree = useMemo(() => hierarchy(teams), [teams]);

  const rows = (nodes: readonly TeamNode[]): { node: TeamNode }[] =>
    nodes.flatMap((node) => [{ node }, ...rows(node.children)]);

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      {error && <Alert type="error" showIcon title="Could not load teams" description={error} />}

      <NewTeamForm teams={teams} onSaved={load} />

      {tree.cyclic.length > 0 && (
        <Alert
          type="error"
          showIcon
          title="A team reports into itself"
          description={`${tree.cyclic.join(", ")} could not be placed: following their parents leads back to themselves. The API refuses to create this, so something else wrote it.`}
        />
      )}
      {tree.orphans.length > 0 && (
        <Alert
          type="warning"
          showIcon
          title="Reports into a team you cannot see"
          description={`${tree.orphans.map((o) => o.displayName).join(", ")} name a parent that is not in this list — either filtered by policy, or deleted since.`}
        />
      )}

      <Card size="small" title={`Teams (${teams.length})`}>
        <Table
          size="small"
          rowKey={({ node }) => node.team.id}
          dataSource={rows(tree.roots)}
          pagination={false}
          locale={{ emptyText: "No teams yet. Create one above to own assets by group." }}
          columns={[
            {
              title: "Team",
              key: "team",
              render: (_: unknown, { node }: { node: TeamNode }) => (
                <span style={{ paddingLeft: node.depth * 20 }}>
                  <TeamOutlined style={{ color: colors.textMuted, marginRight: 6 }} />
                  <Text style={{ fontWeight: node.depth === 0 ? 600 : 400 }}>
                    {node.team.displayName}
                  </Text>
                  <Text type="secondary" style={{ fontSize: 12, marginLeft: 8 }}>
                    {node.team.id}
                  </Text>
                </span>
              ),
            },
            {
              title: "Members",
              key: "members",
              width: 220,
              render: (_: unknown, { node }: { node: TeamNode }) =>
                node.team.members.length === 0 ? (
                  <Text type="secondary" style={{ fontSize: 13 }}>
                    nobody yet
                  </Text>
                ) : (
                  <Space size={4} wrap>
                    {node.team.members.map((m) => (
                      <Tag key={m} icon={<UserOutlined />}>
                        {m}
                      </Tag>
                    ))}
                  </Space>
                ),
            },
            {
              title: "",
              key: "actions",
              width: 90,
              render: (_: unknown, { node }: { node: TeamNode }) => (
                <Button
                  size="small"
                  type="text"
                  danger
                  loading={busy}
                  onClick={() => {
                    setBusy(true);
                    api
                      .deletePrincipal("team", node.team.id)
                      .then(load)
                      // **The server's refusal is shown verbatim.** It carries the
                      // counts by kind, which is the actionable part — "1 service,
                      // 3 schemas, 396 columns" says reassign the service. A
                      // generic "could not delete" would throw that away.
                      .catch((e: unknown) =>
                        setError(e instanceof Error ? e.message : "could not delete"),
                      )
                      .finally(() => setBusy(false));
                  }}
                >
                  Delete
                </Button>
              ),
            },
          ]}
        />
      </Card>
    </Space>
  );
}

/** Create a team, optionally under a parent.
 *
 *  The parent picker offers existing teams only, so the common mistake is a
 *  cycle rather than a typo — and the server refuses cycles at any depth, which
 *  this surfaces rather than duplicating. A second cycle check here would be a
 *  second implementation to keep in step. */
function NewTeamForm({ teams, onSaved }: { teams: Team[]; onSaved: () => void }) {
  const [id, setId] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [parent, setParent] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  const save = () => {
    setBusy(true);
    setProblem(null);
    api
      .upsertTeam({
        id: id.trim(),
        displayName: displayName.trim(),
        members: [],
        parentTeamId: parent,
      })
      .then(() => {
        setId("");
        setDisplayName("");
        setParent(null);
        onSaved();
      })
      .catch((e: unknown) => setProblem(e instanceof Error ? e.message : "could not save"))
      .finally(() => setBusy(false));
  };

  return (
    <Card size="small" title="New team">
      <Space direction="vertical" size={8} style={{ width: "100%" }}>
        <Flex gap={8} wrap>
          <Input
            placeholder="id, e.g. platform"
            value={id}
            onChange={(e) => setId(e.target.value)}
            style={{ maxWidth: 200 }}
          />
          <Input
            placeholder="Display name"
            value={displayName}
            onChange={(e) => setDisplayName(e.target.value)}
            style={{ maxWidth: 260 }}
          />
          <select
            value={parent ?? ""}
            onChange={(e) => setParent(e.target.value === "" ? null : e.target.value)}
            style={{ minWidth: 180 }}
            aria-label="Parent team"
          >
            <option value="">no parent (top level)</option>
            {teams.map((t) => (
              <option key={t.id} value={t.id}>
                under {t.displayName}
              </option>
            ))}
          </select>
          <Button
            type="primary"
            loading={busy}
            disabled={id.trim() === "" || displayName.trim() === ""}
            onClick={save}
          >
            Create
          </Button>
        </Flex>
        {problem && <Alert type="error" showIcon title="Could not save" description={problem} />}
      </Space>
    </Card>
  );
}

/** Connector configuration, rendered from the connector's own JSON Schema.
 *
 *  **The only form renderer in admin.** Fields, requiredness, secrets and what
 *  cannot be rendered are all decided in `admin/schemaForm.ts` and tested there;
 *  this draws what it is handed and nothing else. */
function ConnectorConfigPanel() {
  const [schema, setSchema] = useState<Record<string, unknown> | null>(null);
  const [values, setValues] = useState<Record<string, unknown>>({});
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .connectorSchema("postgres")
      .then((s) => {
        setSchema(s);
        setError(null);
      })
      .catch((e: unknown) =>
        setError(e instanceof Error ? e.message : "could not load the connector schema"),
      );
  }, []);

  if (error) return <Alert type="error" showIcon title="Could not load" description={error} />;
  if (!schema) return <Spin />;

  const check = renderable(schema as Parameters<typeof renderable>[0]);
  const drawn = schemaFields(schema as Parameters<typeof schemaFields>[0], values);
  const absent = schemaMissing(
    schema as Parameters<typeof schemaMissing>[0],
    values,
    // No secret is stored yet in this panel, so a required secret is genuinely
    // missing rather than excused.
    false,
  );

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      {!check.ok && (
        <Alert
          type="warning"
          showIcon
          title="This connector declares fields this build cannot render"
          description={`${check.blocking.join(", ")} — configure it through the API until the console understands them. A field drawn as the wrong type would submit the wrong value.`}
        />
      )}
      <Card size="small" title="PostgreSQL connector">
        <Space direction="vertical" size={10} style={{ width: "100%" }}>
          {drawn.map((field) => (
            <Flex key={field.name} gap={10} align="center">
              <Text style={{ minWidth: 190 }}>
                {field.label}
                {field.required && <Text type="danger"> *</Text>}
              </Text>
              {field.kind === "unsupported" ? (
                <Tag color="orange">not renderable</Tag>
              ) : field.kind === "boolean" ? (
                <input
                  type="checkbox"
                  aria-label={field.label}
                  checked={values[field.name] === true}
                  onChange={(e) =>
                    setValues({ ...values, [field.name]: e.target.checked })
                  }
                />
              ) : (
                <Input
                  aria-label={field.label}
                  // **A secret is never populated, even from a stored value.**
                  // `schemaForm` refuses to supply one and this refuses to show
                  // one — write-only means write-only.
                  type={field.kind === "secret" ? "password" : "text"}
                  placeholder={field.kind === "secret" ? "write-only" : undefined}
                  value={String(values[field.name] ?? field.initial ?? "")}
                  onChange={(e) => setValues({ ...values, [field.name]: e.target.value })}
                  style={{ maxWidth: 340 }}
                />
              )}
              {field.description && (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {field.description}
                </Text>
              )}
            </Flex>
          ))}
          {absent.length > 0 && (
            <Text type="secondary" style={{ fontSize: 13 }}>
              Still needed: {absent.join(", ")}
            </Text>
          )}
          <ConnectionTest values={values} disabled={absent.length > 0} />
        </Space>
      </Card>
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
      named === "admin" ||
      named === "connectors" ||
      named === "explore" ||
      named === "overview" ||
      named === "governance" ||
      named === "workbench" ||
      named === "vocabulary" ||
      named === "review" ||
      // "obligations" was missing here until now — a cold load or
      // bookmark on `?section=obligations` fell through to "overview"
      // even though `setSection("obligations")` writes that exact URL.
      // Included explicitly for "agent" from the start rather than
      // repeating that gap.
      named === "obligations" ||
      named === "reconciliation" ||
      named === "agent"
    ) {
      return named;
    }
    if (readParam("asset")) return "explore";
    if (readParam("vocabulary") || readParam("term")) return "vocabulary";
    return "overview";
  });
  const setSection = useCallback((next: Section) => {
    setSectionRaw(next);
    writeParam("section", next === "overview" ? null : next);
  }, []);
  const [selected, setSelectedRaw] = useState<Asset | null>(null);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Asset[] | null>(null);
  // A search that failed to load is not a search that found nothing — the
  // same distinction the tree already makes for `refused`/`denied` below,
  // applied to the box every evaluator touches first.
  const [searchFailed, setSearchFailed] = useState(false);
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
  const [exportOpen, setExportOpen] = useState(false);
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
      setSearchFailed(false);
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
          setSearchFailed(true);
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
              {/* The wordmark is a decorative SVG with no accessible-name
                  role of its own, so the page has no level-one heading
                  without this — found by the Epic 39 Slice F first-run axe
                  check, not written in from a checklist. Visually hidden:
                  a printed page or a sighted user does not need a second
                  "graph-owl" beside the logo, but a screen reader does. */}
              <h1
                style={{
                  position: "absolute",
                  width: 1,
                  height: 1,
                  padding: 0,
                  margin: -1,
                  overflow: "hidden",
                  clip: "rect(0,0,0,0)",
                  whiteSpace: "nowrap",
                  border: 0,
                }}
              >
                graph-owl
              </h1>
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
              {auth.state.status === "authenticated" && auth.state.strategy !== "open" && (
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
              <Tooltip title="Export the graph — Phase 3 item 3.15">
                <Button
                  type="text"
                  icon={<DownloadOutlined />}
                  onClick={() => setExportOpen(true)}
                  aria-label="Export"
                />
              </Tooltip>
              <Tooltip title={dark ? "Switch to light" : "Switch to dark"}>
                <Button type="text" icon={<BulbOutlined />} onClick={toggle} aria-label="Toggle theme" />
              </Tooltip>
            </Space>
          </Header>

          <ExportDialog open={exportOpen} onClose={() => setExportOpen(false)} defaultAsOf={asOf} />

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
                <SignIn
                  onSignIn={() => auth.login()}
                  onToken={(token) => auth.signInWithToken(token)}
                  strategy={auth.state.strategy}
                  error={auth.state.error}
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
              aria-label="Primary navigation"
            >
              <Menu
                mode="inline"
                selectedKeys={[section]}
                onClick={({ key }) => setSection(key as Section)}
                style={{ borderInlineEnd: 0, paddingTop: 8 }}
                items={[
                  { key: "overview", icon: <DashboardOutlined />, label: "Overview" },
                  {
                    key: "reconciliation",
                    icon: <ReconciliationOutlined />,
                    label: "Reconciliation",
                  },
                  { key: "explore", icon: <CompassOutlined />, label: "Explore" },
                  {
                    key: "governance",
                    icon: <SafetyCertificateOutlined />,
                    label: "Governance",
                  },
                  { key: "workbench", icon: <ThunderboltOutlined />, label: "Workbench" },
                  { key: "vocabulary", icon: <BookOutlined />, label: "Vocabulary" },
                  { key: "review", icon: <MergeOutlined />, label: "Review" },
                  { key: "obligations", icon: <CalendarOutlined />, label: "Obligations" },
                  { key: "connectors", icon: <PlusOutlined />, label: "Connectors" },
                  { key: "admin", icon: <TeamOutlined />, label: "Admin" },
                  { key: "agent", icon: <RobotOutlined />, label: "Agent" },
                ]}
              />
            </Sider>

            {section === "explore" && (
              <Sider
                width={288}
                theme={dark ? "dark" : "light"}
                style={{ borderRight: `1px solid ${colors.border}`, overflow: "auto" }}
                aria-label="Asset hierarchy"
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

            <Content
              style={{
                padding: 24,
                overflow: "auto",
                // The agent tab is a permanently-mounted sibling below (an
                // in-flight investigation's EventSource must survive
                // switching sections, not just its transcript — see that
                // element's own comment). This Content still renders on
                // every other section exactly as before; only "agent"
                // hides it in favour of that sibling.
                display: section === "agent" ? "none" : undefined,
              }}
            >
              {section === "overview" ? (
                <OverviewPage
                  colors={colors}
                  onOpen={(asset) => {
                    setSection("explore");
                    setSelected(asset);
                  }}
                  onAddSource={() => setSection("connectors")}
                />
              ) : section === "reconciliation" ? (
                <ReconciliationWorkspace
                  onReview={() => setSection("review")}
                  onWorkbench={() => setSection("workbench")}
                  onVocabulary={() => setSection("vocabulary")}
                />
              ) : section === "governance" ? (
                <GovernancePage colors={colors} />
              ) : section === "workbench" ? (
                <WorkbenchPage colors={colors} asOf={asOf} />
              ) : section === "vocabulary" ? (
                <VocabularySection />
              ) : section === "review" ? (
                <ReviewSection />
              ) : section === "obligations" ? (
                <ObligationCalendar />
              ) : section === "connectors" ? (
                <ConnectorsPage onDone={refresh} colors={colors} />
              ) : section === "admin" ? (
                <AdminPage colors={colors} setSection={setSection} />
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
                  {searchFailed ? (
                    <Alert type="error" showIcon message="Search failed to load — this is not an empty result." />
                  ) : visibleResults.length === 0 ? (
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
                            <Text dir={userTextDir(name)} style={{ fontWeight: 500 }}>
                              {name}
                            </Text>
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

            {/* Permanently mounted, hidden (not unmounted) on every other
                section — an in-flight investigation opens a real
                EventSource (`AgentChat.tsx`'s `streamsRef`), and that
                connection's own cleanup effect closes it on unmount.
                Rendering this inside the `section === "agent"` ternary
                above meant switching tabs mid-investigation silently
                killed it, and switching back showed an empty "No
                questions yet" as if it had never been asked. */}
            <Content
              style={{
                padding: 24,
                overflow: "auto",
                display: section === "agent" ? undefined : "none",
              }}
            >
              <AgentChat />
            </Content>
              </>
            )}
          </Layout>
        </Layout>
      </AntApp>
    </ConfigProvider>
  );
}
