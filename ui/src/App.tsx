import { useCallback, useEffect, useMemo, useState } from "react";
import {
  App as AntApp,
  Breadcrumb,
  Button,
  Card,
  ConfigProvider,
  Descriptions,
  Empty,
  Flex,
  Form,
  Input,
  Layout,
  Space,
  Statistic,
  Table,
  Tag,
  Tree,
  Typography,
} from "antd";
import type { DataNode } from "antd/es/tree";
import ApartmentOutlined from "@ant-design/icons/es/icons/ApartmentOutlined";
import ClockCircleOutlined from "@ant-design/icons/es/icons/ClockCircleOutlined";
import CloudServerOutlined from "@ant-design/icons/es/icons/CloudServerOutlined";
import DatabaseOutlined from "@ant-design/icons/es/icons/DatabaseOutlined";
import FolderOutlined from "@ant-design/icons/es/icons/FolderOutlined";
import SafetyCertificateOutlined from "@ant-design/icons/es/icons/SafetyCertificateOutlined";
import SearchOutlined from "@ant-design/icons/es/icons/SearchOutlined";
import TableOutlined from "@ant-design/icons/es/icons/TableOutlined";
import TagOutlined from "@ant-design/icons/es/icons/TagOutlined";
import { type Asset, type AssetKind, ApiError, api } from "./api";
import { darkTheme, lightTheme } from "./theme";

const { Header, Sider, Content } = Layout;
const { Text, Title, Paragraph } = Typography;

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

function Fqn({ children }: { children: string }) {
  return (
    <Text code style={{ fontSize: 12 }}>
      {children}
    </Text>
  );
}

function useDarkMode() {
  const [dark, setDark] = useState(
    () => window.matchMedia("(prefers-color-scheme: dark)").matches,
  );
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = (e: MediaQueryListEvent) => setDark(e.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);
  return dark;
}

function AssetDetail({ asset }: { asset: Asset }) {
  const [ancestors, setAncestors] = useState<Asset[]>([]);
  const [children, setChildren] = useState<Asset[]>([]);

  useEffect(() => {
    api.ancestors(asset.id).then(setAncestors).catch(() => setAncestors([]));
    if (asset.kind === "column") setChildren([]);
    else api.children(asset.id).then(setChildren).catch(() => setChildren([]));
  }, [asset.id, asset.kind]);

  const properties = Object.entries(asset.properties ?? {});

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Breadcrumb items={ancestors.map((a) => ({ title: a.name }))} />

      <Flex align="center" gap={12} wrap>
        <Title level={3} style={{ margin: 0 }}>
          {asset.name}
        </Title>
        <Tag color={KIND_COLOR[asset.kind]} icon={KIND_ICON[asset.kind]}>
          {asset.kind}
        </Tag>
      </Flex>
      <Fqn>{asset.fullyQualifiedName}</Fqn>

      {/* States what it does not know yet rather than rendering a confident
          blank. Epic 3 fills version, Epic 26 certification, Epic 29 lineage. */}
      <Card size="small" styles={{ body: { padding: "10px 16px" } }}>
        <Space size="large" wrap>
          <Text type="secondary">
            <ClockCircleOutlined /> no version history yet
          </Text>
          <Text type="secondary">
            <SafetyCertificateOutlined /> uncertified
          </Text>
          <Text type="secondary">
            <ApartmentOutlined /> lineage not captured
          </Text>
        </Space>
      </Card>

      <Paragraph
        type={asset.description ? undefined : "secondary"}
        italic={!asset.description}
      >
        {asset.description ??
          "No description. A connector reported this asset structurally; nobody has described it."}
      </Paragraph>

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
              { title: "Name", dataIndex: "name", key: "name" },
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
                width: 240,
                render: (_: unknown, row: Asset) => (
                  <Text code>
                    {String(
                      row.properties?.["dataType"] ??
                        row.properties?.["tableType"] ??
                        "—",
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
}

function ConnectorCard({ onDone }: { onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  const run = async (values: { connectionString: string; serviceName: string }) => {
    setBusy(true);
    setStatus(null);
    try {
      const result = await api.runPostgresConnector(values);
      setStatus(
        `Catalogued ${result.created} assets${result.failed ? `, ${result.failed} failed` : ""}.`,
      );
      onDone();
    } catch (error) {
      setStatus(error instanceof ApiError ? error.problem.detail : String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card title="Catalog a Postgres source" size="small" style={{ maxWidth: 640 }}>
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
          rules={[{ required: true }]}
        >
          <Input style={{ fontFamily: "ui-monospace, monospace", fontSize: 12 }} />
        </Form.Item>
        <Form.Item label="Service name" name="serviceName" rules={[{ required: true }]}>
          <Input />
        </Form.Item>
        <Button type="primary" htmlType="submit" loading={busy}>
          Run connector
        </Button>
        {status && (
          <Paragraph style={{ marginTop: 12, marginBottom: 0 }} type="success">
            {status}
          </Paragraph>
        )}
      </Form>
    </Card>
  );
}

export default function App() {
  const dark = useDarkMode();
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

  // Children are fetched on expand. Loading the hierarchy whole would pull
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
  const borderColor = dark ? "#262b36" : "#eef1f6";

  return (
    <ConfigProvider theme={dark ? darkTheme : lightTheme}>
      <AntApp>
        <Layout style={{ minHeight: "100vh" }}>
          <Header
            style={{
              display: "flex",
              alignItems: "center",
              gap: 24,
              padding: "0 20px",
              borderBottom: `1px solid ${borderColor}`,
            }}
          >
            <Space size={8}>
              <ApartmentOutlined style={{ color: "#1570ef", fontSize: 18 }} />
              <Text strong style={{ fontSize: 15 }}>
                graph-owl
              </Text>
            </Space>
            <Input
              prefix={<SearchOutlined />}
              placeholder="Search assets, schemas, columns…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              style={{ maxWidth: 520 }}
              allowClear
            />
            {/* Time travel arrives with Epic 4, but the control belongs in the
                chrome from the start — it is a session-wide property. */}
            <Tag color="cyan" style={{ marginLeft: "auto" }} icon={<ClockCircleOutlined />}>
              now
            </Tag>
          </Header>

          <Layout>
            <Sider
              width={300}
              theme={dark ? "dark" : "light"}
              style={{ borderRight: `1px solid ${borderColor}`, overflow: "auto" }}
            >
              <div style={{ padding: 16 }}>
                <Flex gap={8} wrap style={{ marginBottom: 16 }}>
                  {stats.map((s) => (
                    <Card key={s.kind} size="small" styles={{ body: { padding: "6px 12px" } }}>
                      <Statistic
                        title={`${s.kind}s`}
                        value={s.count}
                        valueStyle={{ fontSize: 18, lineHeight: 1.2 }}
                      />
                    </Card>
                  ))}
                </Flex>
                <Text type="secondary" style={{ fontSize: 11, letterSpacing: "0.06em" }}>
                  HIERARCHY
                </Text>
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
              </div>
            </Sider>

            <Content style={{ padding: 24, overflow: "auto" }}>
              {results !== null ? (
                <Space direction="vertical" style={{ width: "100%" }} size="middle">
                  <Title level={5} style={{ margin: 0 }}>
                    {results.length} result{results.length === 1 ? "" : "s"} for “{query}”
                  </Title>
                  {results.length === 0 ? (
                    <Empty description="Nothing matched" />
                  ) : (
                    <Table
                      size="small"
                      rowKey="id"
                      dataSource={results}
                      pagination={{ pageSize: 15 }}
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
                          render: (name: string) => <Text strong>{name}</Text>,
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
                <AssetDetail asset={selected} />
              ) : (
                <Space direction="vertical" size="large" style={{ width: "100%" }}>
                  <div>
                    <Title level={4} style={{ marginBottom: 4 }}>
                      {total === 0 ? "Nothing catalogued yet" : `${total} assets catalogued`}
                    </Title>
                    <Text type="secondary">
                      {total === 0
                        ? "graph-owl reads a source's structure and builds a browsable hierarchy from it."
                        : "Pick something from the hierarchy, or search above."}
                    </Text>
                  </div>
                  <ConnectorCard onDone={refresh} />
                </Space>
              )}
            </Content>
          </Layout>
        </Layout>
      </AntApp>
    </ConfigProvider>
  );
}
