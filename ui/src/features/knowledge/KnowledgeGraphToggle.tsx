/** Epic 42 Slice E, decision 6: a toggle on the existing Knowledge tab, not
 *  a new screen — the same subject rendered as triples or as a
 *  property-graph node. Both panels stay mounted throughout (toggled with
 *  CSS `display`, never unmounted), which is what "the toggle preserves
 *  scroll and selection" means concretely: nothing resets because nothing
 *  ever unmounts. */

import { useEffect, useState } from "react";
import { Alert, Empty, Segmented, Space, Spin, Tag, Typography } from "./../../components/ui/antd-compat";
import { api, ApiError, type LpgNodeView, type Solution } from "../../api";
import { describeLoss, inboundTriplesQuery, outboundTriplesQuery } from "./knowledgeGraph";

const { Text, Title, Paragraph } = Typography;

const COPY = {
  title: "Facts about this asset",
  triplesView: "Triples",
  propertyGraphView: "Property graph",
  loading: "Loading…",
  loadError: "This could not be loaded.",
  outboundLabel: "This asset says",
  inboundLabel: "Said about this asset",
  noOutbound: "No outbound facts.",
  noInbound: "Nothing points at this asset.",
  notProjected: "This asset has not been added to the property graph yet.",
  labelsLabel: "Labels",
  propertiesLabel: "Properties",
  lossesLabel: "What did not carry across",
  losslessNotice: "Nothing was lost converting this asset to a property-graph node.",
  propertySeparator: ": ",
};

interface KnowledgeGraphToggleProps {
  readonly assetId: string;
}

type NodeState = { kind: "loading" } | { kind: "notProjected" } | { kind: "error"; message: string } | { kind: "ready"; view: LpgNodeView };

export function KnowledgeGraphToggle({ assetId }: KnowledgeGraphToggleProps) {
  const [view, setView] = useState<"triples" | "propertyGraph">("triples");
  const [outbound, setOutbound] = useState<readonly Solution[] | null>(null);
  const [inbound, setInbound] = useState<readonly Solution[] | null>(null);
  const [triplesError, setTriplesError] = useState<string | null>(null);
  const [node, setNode] = useState<NodeState>({ kind: "loading" });

  useEffect(() => {
    let cancelled = false;
    setOutbound(null);
    setInbound(null);
    setTriplesError(null);
    Promise.all([api.sparql(outboundTriplesQuery(assetId)), api.sparql(inboundTriplesQuery(assetId))]).then(
      ([out, inb]) => {
        if (cancelled) return;
        setOutbound(out.rows);
        setInbound(inb.rows);
      },
      (err) => {
        if (cancelled) return;
        setTriplesError(err instanceof Error ? err.message : COPY.loadError);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [assetId]);

  useEffect(() => {
    let cancelled = false;
    setNode({ kind: "loading" });
    api.lpgNode(assetId).then(
      (fetched) => {
        if (!cancelled) setNode({ kind: "ready", view: fetched });
      },
      (err) => {
        if (cancelled) return;
        if (err instanceof ApiError && err.problem.status === 404) {
          setNode({ kind: "notProjected" });
        } else {
          setNode({ kind: "error", message: err instanceof Error ? err.message : COPY.loadError });
        }
      },
    );
    return () => {
      cancelled = true;
    };
  }, [assetId]);

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Space align="center">
        <Title level={3} style={{ margin: 0, fontWeight: 600, fontSize: 16 }}>
          {COPY.title}
        </Title>
        <Segmented
          value={view}
          onChange={(value) => setView(value as "triples" | "propertyGraph")}
          options={[
            { label: COPY.triplesView, value: "triples" },
            { label: COPY.propertyGraphView, value: "propertyGraph" },
          ]}
        />
      </Space>

      <div style={{ display: view === "triples" ? "block" : "none" }}>
        {triplesError ? (
          <Alert type="error" showIcon message={triplesError} />
        ) : outbound === null || inbound === null ? (
          <Space direction="vertical" align="center" style={{ width: "100%", padding: 24 }}>
            <Spin />
            <Text>{COPY.loading}</Text>
          </Space>
        ) : (
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            <div>
              <Text strong>{COPY.outboundLabel}</Text>
              {outbound.length === 0 ? (
                <Paragraph type="secondary">{COPY.noOutbound}</Paragraph>
              ) : (
                <ul>
                  {outbound.map((row, index) => (
                    <li key={`${row.p ?? ""}:${row.o ?? ""}:${index}`}>
                      <Tag>{row.p}</Tag> {row.o}
                    </li>
                  ))}
                </ul>
              )}
            </div>
            <div>
              <Text strong>{COPY.inboundLabel}</Text>
              {inbound.length === 0 ? (
                <Paragraph type="secondary">{COPY.noInbound}</Paragraph>
              ) : (
                <ul>
                  {inbound.map((row, index) => (
                    <li key={`${row.s ?? ""}:${row.p ?? ""}:${index}`}>
                      {row.s} <Tag>{row.p}</Tag>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </Space>
        )}
      </div>

      <div style={{ display: view === "propertyGraph" ? "block" : "none" }}>
        {node.kind === "loading" ? (
          <Space direction="vertical" align="center" style={{ width: "100%", padding: 24 }}>
            <Spin />
            <Text>{COPY.loading}</Text>
          </Space>
        ) : node.kind === "error" ? (
          <Alert type="error" showIcon message={node.message} />
        ) : node.kind === "notProjected" ? (
          <Empty description={<Text>{COPY.notProjected}</Text>} />
        ) : (
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            <div>
              <Text strong>{COPY.labelsLabel}</Text>
              <div>
                {node.view.node.labels.map((label) => (
                  <Tag key={label} color="blue">
                    {label}
                  </Tag>
                ))}
              </div>
            </div>
            <div>
              <Text strong>{COPY.propertiesLabel}</Text>
              <ul>
                {Object.entries(node.view.node.properties).map(([key, value]) => (
                  <li key={key}>
                    <Text code>{key}</Text>
                    {COPY.propertySeparator}
                    {String(value.value)}
                  </li>
                ))}
              </ul>
            </div>
            <div>
              <Text strong>{COPY.lossesLabel}</Text>
              {node.view.report.lossy.length === 0 ? (
                <Alert type="success" showIcon message={COPY.losslessNotice} />
              ) : (
                <Space direction="vertical" style={{ width: "100%" }}>
                  {node.view.report.lossy.map((loss, index) => (
                    <Alert key={index} type="warning" showIcon message={describeLoss(loss)} />
                  ))}
                </Space>
              )}
            </div>
          </Space>
        )}
      </div>
    </Space>
  );
}
