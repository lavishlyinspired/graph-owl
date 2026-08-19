import { useState } from "react";
import { strings } from "../lib/strings";

interface ApiKey {
  readonly id: string;
  readonly name: string;
  readonly scope: string;
  readonly createdAt: string;
  readonly lastUsed: string;
  readonly expires: string;
  readonly status: "active" | "expired" | "revoked";
}

const MOCK_KEYS: readonly ApiKey[] = [
  { id: "k1", name: "Snowflake Connector", scope: "sources:read, connectors:manage", createdAt: "2 months ago", lastUsed: "12 min ago", expires: "never", status: "active" },
  { id: "k2", name: "BI Dashboard", scope: "analytics:read, evidence:read", createdAt: "1 month ago", lastUsed: "3 hours ago", expires: "never", status: "active" },
  { id: "k3", name: "CI/CD Pipeline", scope: "validation:run, studio:export", createdAt: "3 months ago", lastUsed: "6 hours ago", expires: "in 30 days", status: "active" },
  { id: "k4", name: "Legacy Integration", scope: "entities:read", createdAt: "6 months ago", lastUsed: "2 weeks ago", expires: "expired", status: "expired" },
];

const STATUS_STYLES: Record<ApiKey["status"], { bg: string; text: string }> = {
  active: { bg: "bg-gowl-ok-bg", text: "text-gowl-ok" },
  expired: { bg: "bg-gowl-panel-2", text: "text-gowl-t5" },
  revoked: { bg: "bg-gowl-bad-bg", text: "text-gowl-bad" },
};

export default function ApiKeysRoute() {
  const [keys] = useState<readonly ApiKey[]>(MOCK_KEYS);

  return (
    <div className="p-8">
      <div className="mb-5 flex items-end justify-between">
        <div>
          <h1 className="mb-1 text-[21px] font-semibold text-gowl-t1">{strings.apiKeysTitle}</h1>
          <p className="text-[12.5px] text-gowl-t5">{strings.apiKeysDescription}</p>
        </div>
        <button
          type="button"
          className="rounded-md bg-gowl-accent px-4 py-1.5 text-[12px] font-semibold text-gowl-accent-on"
        >
          {strings.apiKeysGenerate}
        </button>
      </div>

      <div className="overflow-hidden rounded-lg border border-gowl-line">
        <div className="grid grid-cols-[1fr_200px_100px_100px_100px_90px] gap-2 border-b border-gowl-line bg-gowl-panel-2 px-4 py-2.5 font-mono text-[8.5px] tracking-wider text-gowl-t6">
          <span>NAME</span>
          <span>SCOPE</span>
          <span>CREATED</span>
          <span>LAST USED</span>
          <span>EXPIRES</span>
          <span>STATUS</span>
        </div>
        {keys.map((key) => {
          const ss = STATUS_STYLES[key.status];
          return (
            <div
              key={key.id}
              className="grid grid-cols-[1fr_200px_100px_100px_100px_90px] items-center gap-2 border-b border-gowl-row px-4 py-3 last:border-b-0"
            >
              <span className="text-[12.5px] text-gowl-t1">{key.name}</span>
              <span className="truncate font-mono text-[10.5px] text-gowl-t4">{key.scope}</span>
              <span className="text-[11px] text-gowl-t5">{key.createdAt}</span>
              <span className="text-[11px] text-gowl-t5">{key.lastUsed}</span>
              <span className="text-[11px] text-gowl-t5">{key.expires}</span>
              <span className={`rounded-full px-2 py-0.5 text-center font-mono text-[8.5px] ${ss.bg} ${ss.text}`}>
                {key.status.toUpperCase()}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
