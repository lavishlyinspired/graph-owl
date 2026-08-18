import { useEffect, useState } from "react";
import { KpiGrid } from "./KpiGrid";
import {
  fetchAgentActivity,
  fetchAgentGrants,
  revokeAgentGrant,
  setAgentGrant,
  type AgentActivity,
  type AgentGrant,
} from "../lib/api";
import { relativeTime } from "../lib/format";
import { strings } from "../lib/strings";

export default function AgentsRoute() {
  const [grants, setGrants] = useState<readonly AgentGrant[] | null>(null);
  const [error, setError] = useState(false);
  const [selected, setSelected] = useState<AgentGrant | null>(null);
  const [activity, setActivity] = useState<readonly AgentActivity[] | null>(null);
  const [composing, setComposing] = useState(false);
  const [agentId, setAgentId] = useState("");
  const [scopeFqnPrefix, setScopeFqnPrefix] = useState("");
  const [busy, setBusy] = useState(false);

  const load = () => {
    fetchAgentGrants()
      .then(setGrants)
      .catch(() => setError(true));
  };

  useEffect(load, []);

  useEffect(() => {
    if (!selected) {
      setActivity(null);
      return;
    }
    fetchAgentActivity(selected.agent.id).then((page) => setActivity(page.data));
  }, [selected]);

  if (error) {
    return <div className="p-8 text-[13px] text-gowl-bad">{strings.governError}</div>;
  }
  if (!grants) {
    return <div className="p-8 text-[13px] text-gowl-t5">{strings.governLoading}</div>;
  }

  const now = new Date();
  const capabilitiesInUse = new Set(grants.flatMap((g) => g.capabilities)).size;
  const expiringSoon = grants.filter((g) => g.expiresAt && new Date(g.expiresAt).getTime() - now.getTime() < 7 * 24 * 60 * 60 * 1000).length;

  const runRevoke = async (grant: AgentGrant) => {
    setBusy(true);
    try {
      await revokeAgentGrant(grant.agent.id);
      setSelected(null);
      load();
    } finally {
      setBusy(false);
    }
  };

  const runGrant = async () => {
    if (agentId.trim().length === 0) return;
    setBusy(true);
    try {
      await setAgentGrant(agentId.trim(), {
        capabilities: ["proposeDescription"],
        scopeFqnPrefix: scopeFqnPrefix.trim() || undefined,
      });
      setAgentId("");
      setScopeFqnPrefix("");
      setComposing(false);
      load();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full">
      <div className="flex-1 overflow-y-auto p-8">
        <div className="mb-5 flex items-end justify-between">
          <div>
            <h1 className="mb-1 text-[21px] font-semibold text-gowl-t1">{strings.agentsTitle}</h1>
            <p className="text-[12.5px] text-gowl-t5">{strings.agentsDescription}</p>
          </div>
          <button
            type="button"
            onClick={() => setComposing(true)}
            className="rounded-md bg-gowl-accent px-4 py-1.5 text-[12px] font-semibold text-gowl-accent-on"
          >
            {strings.agentsNewGrant}
          </button>
        </div>

        <KpiGrid
          kpis={[
            { label: strings.agentsKpiGrants, value: String(grants.length) },
            { label: strings.agentsKpiCapabilities, value: String(capabilitiesInUse) },
            { label: strings.agentsKpiExpiring, value: String(expiringSoon) },
          ]}
        />

        {composing && (
          <div className="mb-4 flex gap-2 rounded-lg border border-gowl-line bg-gowl-panel p-4">
            <input
              value={agentId}
              onChange={(e) => setAgentId(e.target.value)}
              placeholder={strings.agentsGrantAgentId}
              className="flex-1 rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 font-mono text-[12px] text-gowl-t1"
            />
            <input
              value={scopeFqnPrefix}
              onChange={(e) => setScopeFqnPrefix(e.target.value)}
              placeholder={strings.agentsGrantScope}
              className="flex-1 rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 text-[12px] text-gowl-t1"
            />
            <button
              type="button"
              disabled={busy || agentId.trim().length === 0}
              onClick={runGrant}
              className="rounded-md bg-gowl-accent px-3 py-1.5 text-[12px] font-semibold text-gowl-accent-on disabled:opacity-40"
            >
              {strings.agentsGrantSubmit}
            </button>
          </div>
        )}

        <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
          <div className="grid grid-cols-5 gap-3 border-b border-gowl-line bg-gowl-panel-2 px-4 py-2 font-mono text-[9.5px] tracking-wider text-gowl-t6">
            <span>{strings.agentsColAgent}</span>
            <span>{strings.agentsColCapabilities}</span>
            <span>{strings.agentsColScope}</span>
            <span>{strings.agentsColRateLimit}</span>
            <span>{strings.agentsColExpires}</span>
          </div>
          {grants.length === 0 ? (
            <div className="p-6 text-[12.5px] text-gowl-t5">{strings.agentsEmpty}</div>
          ) : (
            grants.map((grant) => (
              <button
                key={grant.id}
                type="button"
                onClick={() => setSelected(grant)}
                className="grid w-full grid-cols-5 items-center gap-3 border-b border-gowl-row px-4 py-2.5 text-left last:border-b-0 hover:bg-gowl-row"
              >
                <span className="truncate text-[12.5px] text-gowl-t1">{grant.agent.displayName || grant.agent.id}</span>
                <span className="truncate font-mono text-[11px] text-gowl-t2">{grant.capabilities.join(", ")}</span>
                <span className="truncate text-[12px] text-gowl-t5">{grant.scope?.fqnPrefix ?? strings.agentsScopeAll}</span>
                <span className="font-mono text-[11.5px] text-gowl-t2">
                  {grant.rateLimit.maxWrites}
                  {strings.agentsRateLimitSeparator}
                  {grant.rateLimit.windowSeconds}
                  {strings.agentsRateLimitSuffix}
                </span>
                <span className="text-[12px] text-gowl-t5">
                  {grant.expiresAt ? relativeTime(grant.expiresAt, now) : strings.agentsNoExpiry}
                </span>
              </button>
            ))
          )}
        </div>
      </div>

      {selected && (
        <div className="w-[400px] flex-none overflow-y-auto border-l border-gowl-line bg-gowl-panel p-5">
          <div className="mb-4 flex items-start justify-between">
            <div className="text-[15px] font-semibold text-gowl-t1">{selected.agent.displayName || selected.agent.id}</div>
            <button type="button" onClick={() => setSelected(null)} className="text-[12px] text-gowl-t5">
              {strings.governClose}
            </button>
          </div>
          <button
            type="button"
            disabled={busy}
            onClick={() => runRevoke(selected)}
            className="mb-4 w-full rounded-md border border-gowl-bad px-3 py-1.5 text-[12px] text-gowl-bad disabled:opacity-40"
          >
            {strings.agentsRevoke}
          </button>

          <div className="mb-1 font-mono text-[9.5px] tracking-widest text-gowl-t6">{strings.agentsActivityTitle}</div>
          {activity && activity.length === 0 && <p className="text-[12px] text-gowl-t5">{strings.agentsActivityEmpty}</p>}
          <div className="overflow-hidden rounded-md border border-gowl-line-2">
            {activity?.map((entry) => (
              <div key={entry.id} className="border-b border-gowl-row px-3 py-2 text-[11.5px] last:border-b-0">
                <div className="flex justify-between text-gowl-t2">
                  <span className="font-mono">{entry.capability}</span>
                  <span className="text-gowl-t6">{relativeTime(entry.at, now)}</span>
                </div>
                <div className="mt-0.5 truncate font-mono text-[10.5px] text-gowl-t5">{entry.targetFqn}</div>
                <div className="mt-0.5 text-[10.5px] text-gowl-t6">
                  {entry.outcome}
                  {entry.refusal && ` — ${entry.refusal}`}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
