import { KpiGrid } from "./KpiGrid";
import { strings } from "../lib/strings";

/** Mockup's audit events — the admin screen shows a flat audit log with
 *  both human and agent actors in the same grid, plus a time-series
 *  sidebar chart. No backend exists yet, so this uses mock data
 *  matching the mockup's `V.admin` structure exactly. */

interface AuditEvent {
  readonly actor: string;
  readonly action: string;
  readonly object: string;
  readonly time: string;
  readonly result: "OK" | "DENIED";
}

const AUDIT_EVENTS: readonly AuditEvent[] = [
  { actor: "akash", action: "Accepted contradiction A", object: "Supplier ABC · registeredState", time: "16 Aug 20:14", result: "OK" },
  { actor: "reco-service", action: "Deep-linked investigation", object: "gst:Invoice/INV-1025", time: "16 Aug 19:58", result: "OK" },
  { actor: "agent:investigator", action: "Called find_path", object: "Supplier ABC → Company XYZ", time: "16 Aug 19:58", result: "OK" },
  { actor: "priya", action: "Published ontology version", object: "gst pack 1.4.2", time: "16 Aug 18:02", result: "OK" },
  { actor: "agent:drafter", action: "Attempted write to graph", object: "gst:Supplier/27AAB…", time: "16 Aug 17:41", result: "DENIED" },
];

interface HourBar {
  readonly hour: string;
  readonly value: string;
  readonly height: string;
  readonly isPeak: boolean;
}

const AUDIT_HOURLY: readonly HourBar[] = [
  { hour: "14", value: "96", height: "40%", isPeak: false },
  { hour: "15", value: "128", height: "53%", isPeak: false },
  { hour: "16", value: "204", height: "85%", isPeak: false },
  { hour: "17", value: "241", height: "100%", isPeak: true },
  { hour: "18", value: "188", height: "78%", isPeak: false },
  { hour: "19", value: "142", height: "59%", isPeak: false },
  { hour: "20", value: "88", height: "37%", isPeak: false },
];

export default function AdminRoute() {
  return (
    <div className="flex h-full">
      <div className="flex-1 overflow-y-auto p-8">
        <div className="mb-5 flex items-end justify-between">
          <div>
            <h1 className="mb-1 text-[22.5px] font-semibold text-gowl-t1">{strings.adminTitle}</h1>
            <p className="text-[14px] text-gowl-t5">{strings.adminDescription}</p>
          </div>
          <div className="flex gap-2">
            <button
              type="button"
              className="rounded-md bg-gowl-accent px-4 py-1.5 text-[13.5px] font-semibold text-gowl-accent-on"
            >
              {strings.adminPrimaryAction}
            </button>
            <button
              type="button"
              className="rounded-md border border-gowl-line-2 px-4 py-1.5 text-[13.5px] text-gowl-t2"
            >
              {strings.adminSecondaryAction}
            </button>
          </div>
        </div>

        <KpiGrid
          kpis={[
            { label: strings.adminKpiUsers, value: "42", sub: "6 admins" },
            { label: strings.adminKpiApiKeys, value: "9", sub: "2 expiring" },
            { label: strings.adminKpiSso, value: "OIDC", sub: "Okta, enforced" },
            { label: strings.adminKpiAuditEvents, value: "1,884", sub: "retained 400 d" },
          ]}
        />

        <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
          <div className="grid grid-cols-[120px_1.2fr_1.2fr_130px_96px] gap-3 border-b border-gowl-line bg-gowl-panel-2 px-4 py-2 font-mono text-[11px] tracking-wider text-gowl-t6">
            <span>{strings.adminColActor}</span>
            <span>{strings.adminColAction}</span>
            <span>{strings.adminColObject}</span>
            <span>{strings.adminColTime}</span>
            <span>{strings.adminColResult}</span>
          </div>
          {AUDIT_EVENTS.map((event, index) => (
            <div
              key={index}
              className="grid grid-cols-[120px_1.2fr_1.2fr_130px_96px] items-center gap-3 border-b border-gowl-row px-4 py-2.5 last:border-b-0 hover:bg-gowl-row"
            >
              <span className="truncate text-[14px] text-gowl-t1">{event.actor}</span>
              <span className="truncate text-[14px] text-gowl-t2">{event.action}</span>
              <span className="truncate font-mono text-[13px] text-gowl-t2">{event.object}</span>
              <span className="font-mono text-[13px] text-gowl-t5">{event.time}</span>
              <span
                className={`font-mono text-[13px] ${
                  event.result === "OK" ? "text-gowl-ok" : "text-gowl-bad"
                }`}
              >
                {event.result}
              </span>
            </div>
          ))}
        </div>
      </div>

      <div className="w-[280px] flex-none overflow-y-auto border-l border-gowl-line bg-gowl-panel p-5">
        <div className="mb-5">
          <div className="mb-2 font-mono text-[11px] tracking-widest text-gowl-t6">
            {strings.adminChartTitle}
          </div>
          <p className="mb-3 text-[12.5px] text-gowl-t5">{strings.adminChartSubtitle}</p>
          <div className="flex items-end gap-1.5" style={{ height: 100 }}>
            {AUDIT_HOURLY.map((bar) => (
              <div key={bar.hour} className="flex flex-1 flex-col items-center gap-1">
                <div
                  className={`w-full rounded-sm ${
                    bar.isPeak ? "bg-gowl-accent" : "bg-gowl-line-3"
                  }`}
                  style={{ height: bar.height }}
                />
                <span className="font-mono text-[10.5px] text-gowl-t6">{bar.hour}</span>
              </div>
            ))}
          </div>
        </div>

        <div className="mb-5 rounded-md border border-gowl-accent-border bg-gowl-accent-deep p-3">
          <div className="mb-1 font-mono text-[11px] tracking-widest text-gowl-accent">
            {strings.adminCalloutTitle}
          </div>
          <p className="text-[13px] leading-relaxed text-gowl-t3">
            {strings.adminCalloutBody}
          </p>
        </div>

        <div>
          <div className="mb-2 font-mono text-[11px] tracking-widest text-gowl-t6">
            {strings.relatedTitle}
          </div>
          <div className="space-y-1">
            {[
              { label: "Agents", route: "agents", detail: "identities" },
              { label: "MCP", route: "mcp-tools", detail: "sessions" },
              { label: "Governance", route: "governance", detail: "policies" },
            ].map((link) => (
              <a
                key={link.route}
                href={`/${link.route}`}
                className="flex items-center justify-between rounded-md px-2.5 py-1.5 text-[14px] text-gowl-t2 hover:bg-gowl-row"
              >
                <span>{link.label}</span>
                <span className="font-mono text-[11.5px] text-gowl-t5">{link.detail}</span>
              </a>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
