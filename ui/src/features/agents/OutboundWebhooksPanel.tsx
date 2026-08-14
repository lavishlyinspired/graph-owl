/** Epic 42's recorded gap, closed 9 August 2026: "there is no catalog-wide
 *  webhook-delivery listing endpoint" — the backend (`GET
 *  /admin/outbound-webhooks`, `GET
 *  /admin/outbound-webhooks/{id}/deliveries`) actually shipped earlier
 *  this session (Epic 14 Slice F); what was missing was a console panel to
 *  read it, the same "the design working, but with no consumer" shape
 *  `PartitionHealthPanel`'s own header note already recorded once.
 *
 *  Read-only, mirroring `AgentActivityPanel`'s own decision 5: this file
 *  makes no mutating request — subscriptions are registered elsewhere
 *  (there is no register-a-webhook form here), only `api.outboundWebhooks()`
 *  and `api.outboundWebhookDeliveries()`, both `GET`. */

import { type KeyboardEvent as ReactKeyboardEvent, useEffect, useState } from "react";
import { Alert, Empty, Space, Spin, Table, Tag, Typography } from "./../../components/ui/antd-compat";
import { ApiError, api, type OutboundWebhook, type OutboundWebhookDelivery } from "../../api";
import { readParam, writeParam } from "../deepLink";
import { describeDeliveryStatus } from "./outboundWebhooks";

const { Text, Title, Paragraph } = Typography;

const COPY = {
  title: "Outbound webhooks",
  intro: "Every registered subscription, and what is still pending or has given up delivering.",
  loading: "Loading…",
  loadError: "Could not load outbound webhooks.",
  noWebhooks: "No outbound webhook is registered yet.",
  selectWebhook: "Select a subscription to see its queue.",
  deliveriesTitle: "Deliveries",
  deliveriesLoadError: "Could not load this subscription's deliveries.",
  deliveriesEmpty:
    "Nothing queued or dead-lettered for this subscription — a delivered row is removed once acknowledged, so an empty queue is the healthy state, not evidence nothing was ever sent.",
  disabled: "Disabled",
  enabled: "Enabled",
  everyEvent: "every event",
};

function statusColor(tone: "warning" | "error"): string {
  return tone === "error" ? "red" : "orange";
}

export function OutboundWebhooksPanel() {
  const [webhooks, setWebhooks] = useState<OutboundWebhook[] | null>(null);
  const [webhooksError, setWebhooksError] = useState<string | null>(null);
  const [selected, setSelectedRaw] = useState<string | null>(() => readParam("webhook"));
  const [deliveries, setDeliveries] = useState<OutboundWebhookDelivery[] | null>(null);
  const [deliveriesError, setDeliveriesError] = useState<string | null>(null);

  const setSelected = (id: string | null) => {
    setSelectedRaw(id);
    writeParam("webhook", id);
  };

  useEffect(() => {
    api
      .outboundWebhooks()
      .then((w) => setWebhooks(w))
      .catch((e: unknown) => setWebhooksError(e instanceof ApiError ? e.message : COPY.loadError));
  }, []);

  useEffect(() => {
    if (!selected) {
      setDeliveries(null);
      return;
    }
    let cancelled = false;
    setDeliveries(null);
    setDeliveriesError(null);
    api.outboundWebhookDeliveries(selected).then(
      (d) => {
        if (!cancelled) setDeliveries(d);
      },
      (e: unknown) => {
        if (!cancelled) setDeliveriesError(e instanceof ApiError ? e.message : COPY.deliveriesLoadError);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [selected]);

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <div>
        <Title level={3} style={{ margin: 0, fontWeight: 600, fontSize: 16 }}>
          {COPY.title}
        </Title>
        <Text type="secondary">{COPY.intro}</Text>
      </div>

      {webhooksError && <Alert type="error" showIcon message={COPY.loadError} description={webhooksError} />}

      {webhooks === null ? (
        <Spin />
      ) : webhooks.length === 0 ? (
        <Empty description={<Text>{COPY.noWebhooks}</Text>} />
      ) : (
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <Table
            size="small"
            rowKey={(webhook: OutboundWebhook) => webhook.id}
            dataSource={webhooks}
            pagination={false}
            onRow={(webhook: OutboundWebhook) => ({
              onClick: () => setSelected(webhook.id),
              tabIndex: 0,
              "aria-selected": selected === webhook.id,
              onKeyDown: (event: ReactKeyboardEvent) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  setSelected(webhook.id);
                }
              },
              style: { cursor: "pointer" },
            })}
            columns={[
              {
                title: "URL",
                key: "url",
                render: (_: unknown, webhook: OutboundWebhook) => (
                  <Text strong={selected === webhook.id} code>
                    {webhook.url}
                  </Text>
                ),
              },
              {
                title: "Event types",
                key: "eventTypes",
                render: (_: unknown, webhook: OutboundWebhook) => (
                  <Space size={[4, 4]} wrap>
                    {webhook.eventTypes.length === 0 ? (
                      <Text type="secondary">{COPY.everyEvent}</Text>
                    ) : (
                      webhook.eventTypes.map((kind) => <Tag key={kind}>{kind}</Tag>)
                    )}
                  </Space>
                ),
              },
              {
                title: "State",
                key: "enabled",
                render: (_: unknown, webhook: OutboundWebhook) =>
                  webhook.enabled ? (
                    <Tag color="green">{COPY.enabled}</Tag>
                  ) : (
                    <Tag color="default">{COPY.disabled}</Tag>
                  ),
              },
            ]}
          />

          {!selected ? (
            <Paragraph type="secondary">{COPY.selectWebhook}</Paragraph>
          ) : (
            <Space direction="vertical" size="small" style={{ width: "100%" }}>
              <Title level={4} style={{ margin: 0, fontWeight: 600 }}>
                {COPY.deliveriesTitle}
              </Title>

              {deliveriesError && (
                <Alert type="error" showIcon message={COPY.deliveriesLoadError} description={deliveriesError} />
              )}
              {deliveries === null ? (
                <Spin />
              ) : deliveries.length === 0 ? (
                <Paragraph type="secondary">{COPY.deliveriesEmpty}</Paragraph>
              ) : (
                <Table
                  size="small"
                  rowKey={(delivery: OutboundWebhookDelivery) => delivery.id}
                  dataSource={deliveries}
                  pagination={false}
                  columns={[
                    {
                      title: "Status",
                      key: "status",
                      render: (_: unknown, delivery: OutboundWebhookDelivery) => {
                        const described = describeDeliveryStatus(delivery);
                        return (
                          <Space direction="vertical" size={0}>
                            <Tag color={statusColor(described.tone)}>{described.label}</Tag>
                            <Text type="secondary" style={{ fontSize: 12 }}>
                              {described.detail}
                            </Text>
                          </Space>
                        );
                      },
                    },
                    {
                      title: "Attempt",
                      key: "attempt",
                      render: (_: unknown, delivery: OutboundWebhookDelivery) => delivery.attempt,
                    },
                    {
                      title: "Next attempt",
                      key: "nextAttemptAt",
                      render: (_: unknown, delivery: OutboundWebhookDelivery) => (
                        <Text type="secondary">{new Date(delivery.nextAttemptAt).toLocaleString()}</Text>
                      ),
                    },
                    {
                      title: "Queued",
                      key: "createdAt",
                      render: (_: unknown, delivery: OutboundWebhookDelivery) => (
                        <Text type="secondary">{new Date(delivery.createdAt).toLocaleString()}</Text>
                      ),
                    },
                  ]}
                />
              )}
            </Space>
          )}
        </Space>
      )}
    </Space>
  );
}
