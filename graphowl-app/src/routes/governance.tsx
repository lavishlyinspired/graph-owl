import { useEffect, useState } from "react";
import { KpiGrid } from "./KpiGrid";
import { deniesEverything, draftIncomplete, draftToPolicy, type PolicyDraft, type PolicyDraftRule } from "../lib/govern";
import {
  countAssets,
  deletePolicy,
  dryRunPolicy,
  fetchPolicies,
  fetchRecertificationQueue,
  upsertPolicy,
  type DryRunOutcome,
  type MetadataOperation,
  type PolicyBinding,
  type PolicyEffect,
  type ResourceMatcher,
} from "../lib/api";
import { strings } from "../lib/strings";

const OPERATIONS: readonly MetadataOperation[] = [
  "viewBasic",
  "viewDetails",
  "viewSensitive",
  "create",
  "editDescription",
  "editTags",
  "editOwners",
  "delete",
  "restore",
];
const RESOURCE_TYPES: readonly ResourceMatcher["type"][] = ["all", "fqnPrefix", "tagged", "namedGraph"];
const EFFECTS: readonly PolicyEffect[] = ["allow", "deny"];

function emptyRule(): PolicyDraftRule {
  return { name: "", effect: "allow", operations: [], resourceType: "all", resourceValue: "" };
}

function emptyDraft(): PolicyDraft {
  return { name: "", roles: [], rules: [emptyRule()] };
}

export default function GovernanceRoute() {
  const [policies, setPolicies] = useState<readonly PolicyBinding[] | null>(null);
  const [certified, setCertified] = useState<number | null>(null);
  const [unowned, setUnowned] = useState<number | null>(null);
  const [deprecated, setDeprecated] = useState<number | null>(null);
  const [retired, setRetired] = useState<number | null>(null);
  const [error, setError] = useState(false);

  const [composing, setComposing] = useState(false);
  const [draft, setDraft] = useState<PolicyDraft>(emptyDraft());
  const [rolesInput, setRolesInput] = useState("");
  const [dryRun, setDryRun] = useState<DryRunOutcome | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () => {
    Promise.all([
      fetchPolicies(),
      fetchRecertificationQueue(),
      countAssets({ unowned: true }),
      countAssets({ lifecycle: "deprecated" }),
      countAssets({ lifecycle: "retired" }),
    ])
      .then(([policyList, recert, unownedCount, deprecatedCount, retiredCount]) => {
        setPolicies(policyList);
        setCertified(recert.length);
        setUnowned(unownedCount.count);
        setDeprecated(deprecatedCount.count);
        setRetired(retiredCount.count);
      })
      .catch(() => setError(true));
  };

  useEffect(load, []);

  if (error) {
    return <div className="p-8 text-[17px] text-gowl-bad">{strings.governError}</div>;
  }
  if (!policies || certified === null || unowned === null || deprecated === null || retired === null) {
    return <div className="p-8 text-[17px] text-gowl-t5">{strings.governLoading}</div>;
  }

  const draftWithRoles: PolicyDraft = {
    ...draft,
    roles: rolesInput
      .split(",")
      .map((r) => r.trim())
      .filter((r) => r.length > 0),
  };

  const updateRule = (index: number, patch: Partial<PolicyDraftRule>) => {
    setDraft((prev) => ({
      ...prev,
      rules: prev.rules.map((rule, i) => (i === index ? { ...rule, ...patch } : rule)),
    }));
  };

  const toggleOperation = (index: number, op: MetadataOperation) => {
    const rule = draft.rules[index];
    if (!rule) return;
    const has = rule.operations.includes(op);
    updateRule(index, { operations: has ? rule.operations.filter((o) => o !== op) : [...rule.operations, op] });
  };

  const runDryRun = async () => {
    setBusy(true);
    try {
      const outcome = await dryRunPolicy(draftToPolicy(draftWithRoles), draftWithRoles.roles);
      setDryRun(outcome);
    } finally {
      setBusy(false);
    }
  };

  const runSave = async () => {
    setBusy(true);
    try {
      await upsertPolicy(draftToPolicy(draftWithRoles), draftWithRoles.roles);
      setComposing(false);
      setDraft(emptyDraft());
      setRolesInput("");
      setDryRun(null);
      load();
    } finally {
      setBusy(false);
    }
  };

  const runDelete = async (name: string) => {
    setBusy(true);
    try {
      await deletePolicy(name);
      load();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="p-8">
      <div className="mb-5 flex items-end justify-between">
        <div>
          <h1 className="mb-1 text-[25px] font-semibold text-gowl-t1">{strings.governanceTitle}</h1>
          <p className="text-[16.5px] text-gowl-t5">{strings.governanceDescription}</p>
        </div>
        <button
          type="button"
          onClick={() => {
            setComposing(true);
            setDraft(emptyDraft());
            setRolesInput("");
            setDryRun(null);
          }}
          className="rounded-md bg-gowl-accent px-4 py-1.5 text-[16px] font-semibold text-gowl-accent-on"
        >
          {strings.governanceNewPolicy}
        </button>
      </div>

      <KpiGrid
        kpis={[
          { label: strings.governanceKpiCertified, value: String(certified) },
          { label: strings.governanceKpiUnowned, value: String(unowned) },
          { label: strings.governanceKpiDeprecated, value: String(deprecated) },
          { label: strings.governanceKpiRetired, value: String(retired) },
        ]}
      />

      {composing && (
        <div className="mb-4 rounded-lg border border-gowl-line bg-gowl-panel p-4">
          <input
            value={draft.name}
            onChange={(e) => setDraft((prev) => ({ ...prev, name: e.target.value }))}
            placeholder={strings.governancePolicyName}
            className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16.5px] text-gowl-t1"
          />
          <input
            value={rolesInput}
            onChange={(e) => setRolesInput(e.target.value)}
            placeholder={strings.governanceRoles}
            className="mb-3 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
          />

          {draft.rules.map((rule, index) => (
            <div key={index} className="mb-2 rounded-md border border-gowl-line-2 p-3">
              <input
                value={rule.name}
                onChange={(e) => updateRule(index, { name: e.target.value })}
                placeholder={strings.governanceRuleName}
                className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
              />
              <div className="mb-2 flex gap-2">
                <select
                  value={rule.effect}
                  onChange={(e) => updateRule(index, { effect: e.target.value as PolicyEffect })}
                  aria-label={strings.governanceEffect}
                  className="rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
                >
                  {EFFECTS.map((effect) => (
                    <option key={effect} value={effect}>
                      {effect}
                    </option>
                  ))}
                </select>
                <select
                  value={rule.resourceType}
                  onChange={(e) => updateRule(index, { resourceType: e.target.value as ResourceMatcher["type"] })}
                  aria-label={strings.governanceResourceType}
                  className="rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
                >
                  {RESOURCE_TYPES.map((type) => (
                    <option key={type} value={type}>
                      {type}
                    </option>
                  ))}
                </select>
                {rule.resourceType !== "all" && (
                  <input
                    value={rule.resourceValue}
                    onChange={(e) => updateRule(index, { resourceValue: e.target.value })}
                    placeholder={strings.governanceResourceValue}
                    className="flex-1 rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
                  />
                )}
              </div>
              <div className="flex flex-wrap gap-2">
                {OPERATIONS.map((op) => (
                  <label key={op} className="flex items-center gap-1 text-[15px] text-gowl-t3">
                    <input
                      type="checkbox"
                      checked={rule.operations.includes(op)}
                      onChange={() => toggleOperation(index, op)}
                    />
                    {op}
                  </label>
                ))}
              </div>
            </div>
          ))}
          <button
            type="button"
            onClick={() => setDraft((prev) => ({ ...prev, rules: [...prev.rules, emptyRule()] }))}
            className="mb-3 text-[16px] text-gowl-accent"
          >
            {strings.governanceAddRule}
          </button>

          <div className="flex gap-2">
            <button
              type="button"
              disabled={busy || draftIncomplete(draftWithRoles)}
              onClick={runDryRun}
              className="rounded-md border border-gowl-line-2 px-3 py-1.5 text-[16px] text-gowl-t2 disabled:opacity-40"
            >
              {strings.governanceDryRun}
            </button>
            <button
              type="button"
              disabled={busy || draftIncomplete(draftWithRoles)}
              onClick={runSave}
              className="rounded-md bg-gowl-accent px-3 py-1.5 text-[16px] font-semibold text-gowl-accent-on disabled:opacity-40"
            >
              {strings.governanceSave}
            </button>
            <button
              type="button"
              onClick={() => setComposing(false)}
              className="rounded-md border border-gowl-line-2 px-3 py-1.5 text-[16px] text-gowl-t2"
            >
              {strings.governCancel}
            </button>
          </div>

          {dryRun && (
            <div className="mt-3 rounded-md border border-gowl-line-2 bg-gowl-panel-2 p-3">
              <div className="mb-2 flex gap-6 font-mono text-[16px] text-gowl-t1">
                <span>
                  {strings.governanceDryRunAdmitted} {dryRun.admitted}
                </span>
                <span>
                  {strings.governanceDryRunDenied} {dryRun.denied}
                </span>
                <span>
                  {strings.governanceDryRunTotal} {dryRun.total}
                </span>
              </div>
              {dryRun.admitsEverything && (
                <p className="text-[16px] text-gowl-amber">{strings.governanceAdmitsEverything}</p>
              )}
              {deniesEverything(dryRun) && (
                <p className="text-[16px] text-gowl-amber">{strings.governanceDeniesEverything}</p>
              )}
            </div>
          )}
        </div>
      )}

      <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
        {policies.length === 0 ? (
          <div className="p-6 text-[16.5px] text-gowl-t5">{strings.governanceEmpty}</div>
        ) : (
          policies.map((binding) => (
            <div
              key={binding.policy.name}
              className="flex items-center justify-between border-b border-gowl-row px-4 py-2.5 last:border-b-0"
            >
              <div>
                <div className="text-[17px] text-gowl-t1">{binding.policy.name}</div>
                <div className="text-[15px] text-gowl-t6">
                  {binding.policy.rules.length} {strings.governRulesSuffix} {strings.lineageChainSeparator}{" "}
                  {binding.roles.join(", ") || strings.governNoRoles}
                </div>
              </div>
              <button
                type="button"
                disabled={busy}
                onClick={() => runDelete(binding.policy.name)}
                className="text-[16px] text-gowl-bad"
              >
                {strings.governanceDelete}
              </button>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
