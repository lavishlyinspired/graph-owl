import { useEffect, useState } from "react";
import { KpiGrid } from "./KpiGrid";
import { ShapesPanel } from "./ShapesPanel";
import { validationKpis } from "../lib/govern";
import {
  assignFinding,
  fetchValidationReport,
  revokeWaiver,
  unassignFinding,
  waiveFinding,
  type Severity,
  type ValidationFinding,
} from "../lib/api";
import { strings } from "../lib/strings";

const SEVERITY_COLOR: Record<Severity, string> = {
  violation: "text-gowl-bad",
  warning: "text-gowl-amber",
  info: "text-gowl-t5",
};

function statusOf(finding: ValidationFinding): string {
  if (finding.waiver && !finding.waiver.expired) return strings.validationStatusWaived;
  if (finding.assignment) return strings.validationStatusAssigned;
  return strings.validationStatusOpen;
}

function defaultExpiry(): string {
  const date = new Date();
  date.setDate(date.getDate() + 30);
  return date.toISOString().slice(0, 10);
}

export default function ValidationRoute() {
  const [findings, setFindings] = useState<readonly ValidationFinding[] | null>(null);
  const [computedAtT, setComputedAtT] = useState(0);
  const [error, setError] = useState(false);
  const [selected, setSelected] = useState<ValidationFinding | null>(null);
  const [waiveReason, setWaiveReason] = useState("");
  const [waiveExpiry, setWaiveExpiry] = useState(defaultExpiry());
  const [assignee, setAssignee] = useState("");
  const [busy, setBusy] = useState(false);

  const load = () => {
    fetchValidationReport()
      .then((report) => {
        setFindings(report.data);
        setComputedAtT(report.computedAtT);
      })
      .catch(() => setError(true));
  };

  useEffect(load, []);

  useEffect(() => {
    if (!selected) return;
    const fresh = findings?.find((f) => f.id === selected.id) ?? null;
    setSelected(fresh);
  }, [findings, selected]);

  if (error) {
    return <div className="p-8 text-[17px] text-gowl-bad">{strings.governError}</div>;
  }
  if (!findings) {
    return <div className="p-8 text-[17px] text-gowl-t5">{strings.governLoading}</div>;
  }

  const kpis = validationKpis(findings);
  const waivedCount = findings.filter((f) => f.waiver && !f.waiver.expired).length;

  const runWaive = async () => {
    if (!selected || waiveReason.trim().length === 0) return;
    setBusy(true);
    try {
      await waiveFinding({
        shape: selected.shape,
        focusNode: selected.focusNode,
        path: selected.path,
        constraint: selected.constraint,
        reason: waiveReason.trim(),
        expiresAt: new Date(waiveExpiry).toISOString(),
      });
      setWaiveReason("");
      load();
    } finally {
      setBusy(false);
    }
  };

  const runRevokeWaiver = async () => {
    if (!selected?.waiver) return;
    setBusy(true);
    try {
      await revokeWaiver(selected.waiver.id);
      load();
    } finally {
      setBusy(false);
    }
  };

  const runAssign = async () => {
    if (!selected || assignee.trim().length === 0) return;
    setBusy(true);
    try {
      await assignFinding({
        shape: selected.shape,
        focusNode: selected.focusNode,
        path: selected.path,
        constraint: selected.constraint,
        assignee: assignee.trim(),
      });
      setAssignee("");
      load();
    } finally {
      setBusy(false);
    }
  };

  const runUnassign = async () => {
    if (!selected?.assignment) return;
    setBusy(true);
    try {
      await unassignFinding(selected.assignment.id);
      load();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full">
      <div className="flex-1 overflow-y-auto p-8">
        <h1 className="mb-1 text-[25px] font-semibold text-gowl-t1">{strings.validationTitle}</h1>
        <p className="mb-5 text-[16.5px] text-gowl-t5">{strings.validationDescription}</p>

        <ShapesPanel computedAtT={computedAtT} onValidated={load} />

        <KpiGrid
          kpis={[
            { label: strings.validationKpiViolations, value: String(kpis.violations) },
            { label: strings.validationKpiWarnings, value: String(kpis.warnings) },
            { label: strings.validationKpiAffected, value: String(kpis.affected) },
            { label: strings.validationKpiWaived, value: String(waivedCount) },
          ]}
        />

        <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
          <div className="grid grid-cols-5 gap-3 border-b border-gowl-line bg-gowl-panel-2 px-4 py-2 font-mono text-[13.5px] tracking-wider text-gowl-t6">
            <span>{strings.validationColShape}</span>
            <span>{strings.validationColFocusNode}</span>
            <span>{strings.validationColConstraint}</span>
            <span>{strings.validationColSeverity}</span>
            <span>{strings.validationColStatus}</span>
          </div>
          {findings.length === 0 ? (
            <div className="p-6 text-[16.5px] text-gowl-t5">{strings.validationEmpty}</div>
          ) : (
            findings.map((finding) => (
              <button
                key={finding.id}
                type="button"
                onClick={() => setSelected(finding)}
                className="grid w-full grid-cols-5 items-center gap-3 border-b border-gowl-row px-4 py-2.5 text-left last:border-b-0 hover:bg-gowl-row"
              >
                <span className="truncate font-mono text-[15.5px] text-gowl-t2">{finding.shape}</span>
                <span className="truncate text-[16.5px] text-gowl-t1">{finding.focusNode}</span>
                <span className="truncate font-mono text-[15.5px] text-gowl-t2">{finding.constraint}</span>
                <span className={`text-[16px] font-semibold ${SEVERITY_COLOR[finding.severity]}`}>
                  {finding.severity}
                </span>
                <span className="text-[16px] text-gowl-t5">{statusOf(finding)}</span>
              </button>
            ))
          )}
        </div>
      </div>

      {selected && (
        <div className="w-[380px] flex-none overflow-y-auto border-l border-gowl-line bg-gowl-panel p-5">
          <div className="mb-4 flex items-start justify-between">
            <div>
              <div className="font-mono text-[15px] text-gowl-t6">{selected.shape}</div>
              <div className="text-[19px] font-semibold text-gowl-t1">{selected.focusNode}</div>
            </div>
            <button type="button" onClick={() => setSelected(null)} className="text-[16px] text-gowl-t5">
              {strings.governClose}
            </button>
          </div>
          <p className="mb-4 text-[16.5px] text-gowl-t3">{selected.message}</p>
          {selected.suggestion && (
            <p className="mb-4 text-[16px] text-gowl-accent">{selected.suggestion}</p>
          )}

          {selected.waiver && !selected.waiver.expired ? (
            <div className="mb-4 rounded-md border border-gowl-line-2 bg-gowl-panel-2 p-3">
              <div className="mb-1 text-[15px] text-gowl-t5">{selected.waiver.reason}</div>
              <button
                type="button"
                disabled={busy}
                onClick={runRevokeWaiver}
                className="text-[16px] text-gowl-accent"
              >
                {strings.validationRevokeWaiver}
              </button>
            </div>
          ) : (
            <div className="mb-4 rounded-md border border-gowl-line-2 p-3">
              <div className="mb-2 text-[15px] tracking-widest text-gowl-t6">{strings.validationWaiveTitle}</div>
              <input
                value={waiveReason}
                onChange={(e) => setWaiveReason(e.target.value)}
                placeholder={strings.validationWaiveReason}
                className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
              />
              <input
                type="date"
                value={waiveExpiry}
                onChange={(e) => setWaiveExpiry(e.target.value)}
                className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
              />
              <button
                type="button"
                disabled={busy || waiveReason.trim().length === 0}
                onClick={runWaive}
                className="rounded-md bg-gowl-accent px-3 py-1.5 text-[16px] font-semibold text-gowl-accent-on disabled:opacity-40"
              >
                {strings.validationWaiveSubmit}
              </button>
            </div>
          )}

          {selected.assignment ? (
            <div className="rounded-md border border-gowl-line-2 bg-gowl-panel-2 p-3">
              <div className="mb-1 text-[15px] text-gowl-t5">{selected.assignment.assignee}</div>
              <button type="button" disabled={busy} onClick={runUnassign} className="text-[16px] text-gowl-accent">
                {strings.validationUnassign}
              </button>
            </div>
          ) : (
            <div className="rounded-md border border-gowl-line-2 p-3">
              <div className="mb-2 text-[15px] tracking-widest text-gowl-t6">{strings.validationAssignTitle}</div>
              <input
                value={assignee}
                onChange={(e) => setAssignee(e.target.value)}
                placeholder={strings.validationAssigneePlaceholder}
                className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 font-mono text-[16px] text-gowl-t1"
              />
              <button
                type="button"
                disabled={busy || assignee.trim().length === 0}
                onClick={runAssign}
                className="rounded-md bg-gowl-accent px-3 py-1.5 text-[16px] font-semibold text-gowl-accent-on disabled:opacity-40"
              >
                {strings.validationAssignSubmit}
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
