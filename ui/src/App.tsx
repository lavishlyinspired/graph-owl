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
import type { DataNode } from "antd/es/tree";
import ApartmentOutlined from "@ant-design/icons/es/icons/ApartmentOutlined";
import ArrowLeftOutlined from "@ant-design/icons/es/icons/ArrowLeftOutlined";
import BulbOutlined from "@ant-design/icons/es/icons/BulbOutlined";
import CheckCircleFilled from "@ant-design/icons/es/icons/CheckCircleFilled";
import ClockCircleOutlined from "@ant-design/icons/es/icons/ClockCircleOutlined";
import CloudServerOutlined from "@ant-design/icons/es/icons/CloudServerOutlined";
import CompassOutlined from "@ant-design/icons/es/icons/CompassOutlined";
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
  ApiError,
  api,
} from "./api";
import { darkTheme, lightTheme, palette } from "./theme";
import { GenericSourceMark, PostgresMark } from "./icons";

const { Header, Sider, Content } = Layout;
const { Text, Title, Paragraph } = Typography;

type Section = "explore" | "connectors";

const KIND_ICON: Record<AssetKind, React.ReactNode> = {
  service: <CloudServerOutlined />,
  database: <DatabaseOutlined />,
  schema: <FolderOutlined />,
  table: <TableOutlined />,
  column: <TagOutlined />,
};

const KIND_COLOR: Record<AssetKind, string> = {
  service: "blue",
  database: "geekblue",
  schema: "purple",
  table: "green",
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

function useTheme() {
  // Light by default, persisted, and overridable from the URL. The URL matters
  // beyond convenience: it makes a theme deep-linkable, so a screenshot in a
  // bug report can be reproduced exactly.
  const [dark, setDark] = useState(
    () => (readParam("theme") ?? localStorage.getItem("theme")) === "dark",
  );
  useEffect(() => {
    localStorage.setItem("theme", dark ? "dark" : "light");
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
}: {
  asset: Asset;
  onChanged: (a: Asset) => void;
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
        ]}
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
            <CheckCircleFilled style={{ color: "#059669", fontSize: 18 }} />
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
  const [section, setSectionRaw] = useState<Section>(
    () => (readParam("section") === "connectors" ? "connectors" : "explore"),
  );
  const setSection = useCallback((next: Section) => {
    setSectionRaw(next);
    writeParam("section", next === "explore" ? null : next);
  }, []);
  const [selected, setSelectedRaw] = useState<Asset | null>(null);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Asset[] | null>(null);
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
        setIndex((i) => ({ ...i, ...Object.fromEntries(roots.map((r) => [r.id, r])) }));
        setNodes(roots.map(toNode));
      })
      .catch(() => setNodes([]));
    api
      .stats()
      .then((s) => setStats(s.byKind))
      .catch(() => setStats([]));
  }, [toNode]);

  useEffect(refresh, [refresh]);

  useEffect(() => {
    const id = new URLSearchParams(window.location.search).get("asset");
    if (id) api.asset(id).then(setSelectedRaw).catch(() => undefined);
  }, []);

  useEffect(() => {
    if (query.trim().length < 2) {
      setResults(null);
      return;
    }
    const timer = setTimeout(() => {
      api
        .search(query)
        .then((page) => setResults(page.data))
        .catch(() => setResults([]));
    }, 150);
    return () => clearTimeout(timer);
  }, [query]);

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
            <Space size={8}>
              <ApartmentOutlined style={{ color: "#1570ef", fontSize: 18 }} />
              <Text style={{ fontSize: 15, fontWeight: 600 }}>graph-owl</Text>
            </Space>
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
              {/* Time travel arrives with Epic 4, but the control belongs in the
                  chrome from the start — it is a session-wide property. */}
              <Tooltip title="Time travel arrives with the graph engine">
                <Tag color="cyan" icon={<ClockCircleOutlined />} style={{ marginInlineEnd: 0 }}>
                  now
                </Tag>
              </Tooltip>
              <Tooltip title={dark ? "Switch to light" : "Switch to dark"}>
                <Button type="text" icon={<BulbOutlined />} onClick={toggle} aria-label="Toggle theme" />
              </Tooltip>
            </Space>
          </Header>

          <Layout style={{ minHeight: 0 }}>
            {/* Navigation and the hierarchy are different things. The rail is
                where you are in the product; the tree is where you are in the
                data. Conflating them is why the tree previously had nowhere
                for a second section to go. */}
            <Sider
              width={64}
              theme={dark ? "dark" : "light"}
              style={{ borderRight: `1px solid ${colors.border}` }}
            >
              <Menu
                mode="inline"
                inlineCollapsed
                selectedKeys={[section]}
                onClick={({ key }) => setSection(key as Section)}
                style={{ borderInlineEnd: 0, paddingTop: 8 }}
                items={[
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
              {section === "connectors" ? (
                <ConnectorsPage onDone={refresh} />
              ) : results !== null ? (
                <Space direction="vertical" style={{ width: "100%" }} size="middle">
                  <Title level={5} style={{ margin: 0, fontWeight: 600 }}>
                    {results.length} result{results.length === 1 ? "" : "s"} for “{query}”
                  </Title>
                  {results.length === 0 ? (
                    <Empty description="Nothing matched" />
                  ) : (
                    <Table
                      size="small"
                      rowKey="id"
                      dataSource={results}
                      pagination={{ pageSize: 15, size: "small" }}
                      onRow={(row) => ({
                        onClick: () => {
                          setSelected(row);
                          setQuery("");
                        },
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
              ) : selected ? (
                <AssetDetail asset={selected} onChanged={setSelectedRaw} />
              ) : (
                <Space direction="vertical" size="large" style={{ width: "100%" }}>
                  <div>
                    <Title level={4} style={{ marginBottom: 4, fontWeight: 600 }}>
                      {total === 0 ? "Nothing catalogued yet" : `${total} assets catalogued`}
                    </Title>
                    <Text type="secondary">
                      {total === 0
                        ? "graph-owl reads a source's structure and builds a browsable hierarchy from it."
                        : "Pick something from the hierarchy, or search above."}
                    </Text>
                  </div>
                  {/* The empty-database first run offers the next action rather
                      than a blank page — 39-ui-foundation.md Slice F. */}
                  <Button type="primary" icon={<PlusOutlined />} onClick={() => setSection("connectors")}>
                    {total === 0 ? "Catalogue a source" : "Add another source"}
                  </Button>
                </Space>
              )}
            </Content>
          </Layout>
        </Layout>
      </AntApp>
    </ConfigProvider>
  );
}
