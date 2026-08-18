/** Epic 42 Slice F: Bolt endpoint status and active sessions (Epic 7d),
 *  read-only. `{enabled: false}` is a real, legitimate state — the `bolt`
 *  Cargo feature is off by default — not an error to alarm about.
 *
 *  **A second, indistinguishable-to-the-caller disabled state, found
 *  verifying this against a real (default, no `bolt` feature) server**:
 *  `graph-owl-server/src/lib.rs` registers `/admin/bolt/status` only
 *  `#[cfg(feature = "bolt")]` — "compiled out entirely, not merely inert"
 *  is the server's own stated decision. Off this build, the request falls
 *  through to the SPA's static-file fallback, which answers `200` with
 *  `index.html` rather than a `404` — so `api.boltStatus()` throws a raw
 *  JSON-parse error, not an `ApiError`. Both states mean exactly the same
 *  thing to an admin reading this screen ("you cannot use Bolt here"), so
 *  a parse failure is treated as `disabled` too, rather than surfaced as a
 *  frightening "Unexpected token '<'" error alert for a state that is not
 *  wrong at all. A genuine `ApiError` (a real Problem response from a
 *  handler that does exist) still surfaces as an error. */

import { useEffect, useState } from "react";
import { Alert, Space, Spin, Statistic, Table, Typography } from "./../../components/ui/antd-compat";
import { ApiError, api, type BoltStatus } from "../../api";

const { Text, Title, Paragraph } = Typography;

const COPY = {
  title: "Bolt sessions",
  intro: "The Bolt endpoint's own connection state — Epic 7d, read-only.",
  loading: "Loading…",
  loadError: "Could not load Bolt status.",
  disabled: "Bolt is not enabled on this server.",
  noSessions: "No active sessions.",
};

type PanelState =
  | { kind: "loading" }
  | { kind: "disabled" }
  | { kind: "error"; message: string }
  | { kind: "ready"; status: Extract<BoltStatus, { enabled: true }> };

export function BoltSessionsPanel() {
  const [state, setState] = useState<PanelState>({ kind: "loading" });

  useEffect(() => {
    api.boltStatus().then(
      (status) => setState(status.enabled ? { kind: "ready", status } : { kind: "disabled" }),
      (e: unknown) => {
        if (e instanceof ApiError) {
          setState({ kind: "error", message: e.message });
        } else {
          setState({ kind: "disabled" });
        }
      },
    );
  }, []);

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <div>
        <Title level={3} style={{ margin: 0, fontWeight: 600, fontSize: 16 }}>
          {COPY.title}
        </Title>
        <Text type="secondary">{COPY.intro}</Text>
      </div>

      {state.kind === "loading" ? (
        <Spin />
      ) : state.kind === "disabled" ? (
        <Alert type="info" showIcon message={COPY.disabled} />
      ) : state.kind === "error" ? (
        <Alert type="error" showIcon message={COPY.loadError} description={state.message} />
      ) : (
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <Space size={32}>
            <Statistic title="Max connections" value={state.status.maxConnections} />
            <Statistic title="Active sessions" value={state.status.activeConnections} />
          </Space>
          {state.status.sessions.length === 0 ? (
            <Paragraph type="secondary">{COPY.noSessions}</Paragraph>
          ) : (
            <Table
              size="small"
              rowKey={(session) => `${session.principal}-${session.connectedAt}`}
              dataSource={state.status.sessions}
              pagination={false}
              columns={[
                { title: "Principal", dataIndex: "principal", key: "principal" },
                {
                  title: "Connected",
                  key: "connectedAt",
                  render: (_: unknown, session) => new Date(session.connectedAt).toLocaleString(),
                },
              ]}
            />
          )}
        </Space>
      )}
    </Space>
  );
}
