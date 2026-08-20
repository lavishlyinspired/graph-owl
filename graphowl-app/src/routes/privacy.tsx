import { strings } from "../lib/strings";

interface PrivacyControl {
  readonly label: string;
  readonly value: string;
  readonly detail: string;
  readonly action?: string;
}

const CONTROLS: readonly PrivacyControl[] = [
  { label: "Entities with PII tags", value: "1,247", detail: "32% of total entities are tagged as containing personally identifiable information" },
  { label: "Column-level masks active", value: "89", detail: "Dynamic masks applied to PII columns across all connected sources" },
  { label: "Row-level security policies", value: "12", detail: "Active policies restricting entity visibility by team or role" },
  { label: "Erasure requests pending", value: "3", detail: "Right-to-erasure requests awaiting graph impact assessment" },
  { label: "Data retention policies", value: "7 active", detail: "Automated cleanup rules for ephemeral entities and expired evidence" },
  { label: "Audit trail coverage", value: "100%", detail: "Every entity mutation is logged with actor, timestamp, and diff" },
];

export default function PrivacyRoute() {
  return (
    <div className="p-8">
      <div className="mb-5">
        <h1 className="mb-1 text-[25px] font-semibold text-gowl-t1">{strings.privacyTitle}</h1>
        <p className="text-[16.5px] text-gowl-t5">{strings.privacyDescription}</p>
      </div>

      <div className="grid grid-cols-3 gap-4">
        {CONTROLS.map((ctrl) => (
          <div key={ctrl.label} className="rounded-lg border border-gowl-line bg-gowl-panel p-5">
            <div className="mb-1 font-mono text-[13.5px] tracking-widest text-gowl-t6">{ctrl.label.toUpperCase()}</div>
            <div className="mb-2 font-mono text-[24px] text-gowl-t1">{ctrl.value}</div>
            <p className="text-[15.5px] text-gowl-t5">{ctrl.detail}</p>
          </div>
        ))}
      </div>
    </div>
  );
}
