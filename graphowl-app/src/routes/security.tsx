import { strings } from "../lib/strings";

interface SecurityEvent {
  readonly id: string;
  readonly type: string;
  readonly actor: string;
  readonly detail: string;
  readonly time: string;
  readonly severity: "ok" | "warn" | "bad";
}

const EVENTS: readonly SecurityEvent[] = [
  { id: "s1", type: "Auth failure", actor: "unknown (IP 203.0.113.42)", detail: "3 failed login attempts for admin@graphowl.dev", time: "12 min ago", severity: "warn" },
  { id: "s2", type: "Token rotated", actor: "Platform Admin", detail: "API key for Snowflake connector rotated", time: "2 hours ago", severity: "ok" },
  { id: "s3", type: "Access denied", actor: "Agent: Ontology suggester", detail: "Write attempt blocked — agents may not write the graph", time: "4 hours ago", severity: "bad" },
  { id: "s4", type: "Policy updated", actor: "Governance Lead", detail: "Row-level security policy 'team-finance' updated", time: "1 day ago", severity: "ok" },
  { id: "s5", type: "Erasure request", actor: "Data Steward", detail: "Right-to-erasure request ER-003 submitted for entity SUP-4521", time: "2 days ago", severity: "warn" },
];

const SEVERITY_STYLES: Record<SecurityEvent["severity"], { bg: string; text: string }> = {
  ok: { bg: "bg-gowl-ok-bg", text: "text-gowl-ok" },
  warn: { bg: "bg-gowl-amber-bg", text: "text-gowl-amber" },
  bad: { bg: "bg-gowl-bad-bg", text: "text-gowl-bad" },
};

export default function SecurityRoute() {
  return (
    <div className="p-8">
      <div className="mb-5 flex items-end justify-between">
        <div>
          <h1 className="mb-1 text-[22.5px] font-semibold text-gowl-t1">{strings.securityTitle}</h1>
          <p className="text-[14px] text-gowl-t5">{strings.securityDescription}</p>
        </div>
      </div>

      <div className="mb-6 grid grid-cols-4 gap-px overflow-hidden rounded-lg border border-gowl-line bg-gowl-line">
        {[
          { label: "FAILED LOGINS 24H", value: "7", color: "text-gowl-amber" },
          { label: "ACCESS DENIED 24H", value: "3", color: "text-gowl-bad" },
          { label: "TOKENS ACTIVE", value: "14", color: "text-gowl-t1" },
          { label: "POLICIES ACTIVE", value: "12", color: "text-gowl-t1" },
        ].map((kpi) => (
          <div key={kpi.label} className="bg-gowl-panel p-4">
            <div className="mb-2 font-mono text-[10.5px] tracking-widest text-gowl-t6">{kpi.label}</div>
            <div className={`font-mono text-[21.5px] ${kpi.color}`}>{kpi.value}</div>
          </div>
        ))}
      </div>

      <div className="rounded-lg border border-gowl-line bg-gowl-panel">
        <div className="border-b border-gowl-line px-4 py-2.5 font-mono text-[11px] tracking-widest text-gowl-t6">
          SECURITY EVENTS
        </div>
        {EVENTS.map((event) => {
          const ss = SEVERITY_STYLES[event.severity];
          return (
            <div key={event.id} className="flex items-center gap-4 border-b border-gowl-row px-4 py-3 last:border-b-0">
              <span className={`flex-none h-2 w-2 rounded-full ${ss.bg}`} />
              <span className="flex-none w-[130px] text-[13px] text-gowl-t4">{event.type}</span>
              <span className="flex-none w-[180px] truncate text-[13px] text-gowl-t2">{event.actor}</span>
              <span className="flex-1 text-[13.5px] text-gowl-t1">{event.detail}</span>
              <span className="flex-none text-[12.5px] text-gowl-t5">{event.time}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
