import { strings } from "../lib/strings";

interface Pack {
  readonly name: string;
  readonly namespace: string;
  readonly version: string;
  readonly contents: string;
  readonly updated: string;
  readonly state: "active" | "disabled" | "update";
}

const PACKS: readonly Pack[] = [
  { name: "GST", namespace: "in.gov.gst", version: "1.4.2", contents: "412 classes · 62 rules · 24 matchers", updated: "12 min ago", state: "active" },
  { name: "Finance", namespace: "core.finance", version: "2.0.1", contents: "188 classes · 14 rules", updated: "6 d ago", state: "active" },
  { name: "Healthcare", namespace: "core.health", version: "0.9.0", contents: "621 classes · 10 rules", updated: "1 mo ago", state: "disabled" },
];

const STATE_COLORS = {
  active: "text-gowl-ok",
  disabled: "text-gowl-t5",
  update: "text-gowl-amber",
} as const;

export default function KnowledgeRoute() {
  return (
    <div className="flex h-full flex-col overflow-auto">
      <div className="border-b border-gowl-line bg-gowl-panel px-8 py-5">
        <div className="mb-1 text-[25px] font-semibold text-gowl-t1">{strings.knowledgeTitle}</div>
        <div className="text-[16.5px] text-gowl-t5">{strings.knowledgeSubtitle}</div>
      </div>

      <div className="p-8">
        {/* KPIs */}
        <div className="mb-5 grid grid-cols-4 gap-px overflow-hidden rounded-lg border border-gowl-line">
          {[
            { label: strings.knowledgeInstalled, value: "3", color: "text-gowl-t1" },
            { label: strings.knowledgeRulesShipped, value: "86", color: "text-gowl-t1" },
            { label: strings.knowledgeConsumers, value: "2 apps", color: "text-gowl-t1" },
            { label: "GST PACK", value: "v1.4.2", color: "text-gowl-accent" },
          ].map((kpi) => (
            <div key={kpi.label} className="bg-gowl-panel p-4">
              <div className="mb-2 font-mono text-[13.5px] tracking-widest text-gowl-t6">{kpi.label}</div>
              <div className={`font-mono text-[24px] font-medium ${kpi.color}`}>{kpi.value}</div>
            </div>
          ))}
        </div>

        {/* Packs table */}
        <div className="mb-5 overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
          <div className="grid grid-cols-[1.3fr_100px_1fr_110px_96px] gap-3 border-b border-gowl-line px-4 py-2.5 font-mono text-[13.5px] tracking-widest text-gowl-t6">
            <span>{strings.knowledgeInstalled.replace("INSTALLED", "PACK")}</span>
            <span>{strings.knowledgePackVersion}</span>
            <span>{strings.knowledgePackContents}</span>
            <span>{strings.knowledgePackUpdated}</span>
            <span>{strings.knowledgePackState}</span>
          </div>
          {PACKS.map((pack) => (
            <div
              key={`${pack.namespace}-${pack.version}`}
              className="grid grid-cols-[1.3fr_100px_1fr_110px_96px] items-center gap-3 border-b border-gowl-row px-4 py-3 last:border-b-0"
            >
              <div>
                <div className="text-[16.5px] text-gowl-t1">{pack.name}</div>
                <div className="font-mono text-[14px] text-gowl-t6">{pack.namespace}</div>
              </div>
              <span className="font-mono text-[15px] text-gowl-t3">{pack.version}</span>
              <span className="text-[15.5px] text-gowl-t4">{pack.contents}</span>
              <span className="font-mono text-[15px] text-gowl-t5">{pack.updated}</span>
              <span className={`font-mono text-[15px] ${STATE_COLORS[pack.state]}`}>
                {pack.state === "active" ? strings.knowledgeActive
                  : pack.state === "disabled" ? strings.knowledgeDisabled
                  : strings.knowledgeUpdate}
              </span>
            </div>
          ))}
        </div>

        {/* Architecture rule note */}
        <div className="rounded-lg border border-gowl-line bg-gowl-panel p-5">
          <div className="mb-2 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.knowledgeArchitectureRule}</div>
          <div className="text-[16.5px] text-gowl-t4">{strings.knowledgeArchitectureBody}</div>
        </div>
      </div>
    </div>
  );
}
