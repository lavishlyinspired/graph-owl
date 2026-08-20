import { useEffect, useState } from "react";
import {
  createPackOverride,
  deletePackOverride,
  fetchAvailablePacks,
  fetchInstalledPacks,
  fetchPackOverrides,
  fetchPackTerms,
  installPack,
  upgradePack,
  type AvailablePack,
  type InstalledPack,
  type PackOverride,
  type PackOverrideKind,
  type PackTermView,
  type PackUpgradeResult,
} from "../lib/api";
import { strings } from "../lib/strings";

const OVERRIDE_KINDS: readonly PackOverrideKind[] = ["hide", "relabel", "reparent"];

export default function PacksRoute() {
  const [available, setAvailable] = useState<readonly AvailablePack[] | null>(null);
  const [installed, setInstalled] = useState<readonly InstalledPack[] | null>(null);
  const [error, setError] = useState(false);
  const [selected, setSelected] = useState<InstalledPack | null>(null);
  const [terms, setTerms] = useState<readonly PackTermView[] | null>(null);
  const [overrides, setOverrides] = useState<readonly PackOverride[] | null>(null);
  const [installResult, setInstallResult] = useState<string | null>(null);
  const [termPath, setTermPath] = useState("");
  const [overrideKind, setOverrideKind] = useState<PackOverrideKind>("hide");
  const [upgradeVersion, setUpgradeVersion] = useState("");
  const [upgradeManifest, setUpgradeManifest] = useState("");
  const [upgradeResult, setUpgradeResult] = useState<PackUpgradeResult | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () => {
    Promise.all([fetchAvailablePacks(), fetchInstalledPacks()])
      .then(([availablePacks, installedPacks]) => {
        setAvailable(availablePacks);
        setInstalled(installedPacks);
      })
      .catch(() => setError(true));
  };

  useEffect(load, []);

  useEffect(() => {
    if (!selected) {
      setTerms(null);
      setOverrides(null);
      setUpgradeResult(null);
      return;
    }
    fetchPackTerms(selected.id).then(setTerms);
    fetchPackOverrides(selected.id).then(setOverrides);
  }, [selected]);

  if (error) {
    return <div className="p-8 text-[17px] text-gowl-bad">{strings.governError}</div>;
  }
  if (!available || !installed) {
    return <div className="p-8 text-[17px] text-gowl-t5">{strings.governLoading}</div>;
  }

  const runInstall = async (pack: AvailablePack) => {
    setBusy(true);
    try {
      const result = await installPack(pack.id);
      setInstallResult(result.output);
      load();
    } finally {
      setBusy(false);
    }
  };

  const runAddOverride = async () => {
    if (!selected || termPath.trim().length === 0) return;
    setBusy(true);
    try {
      await createPackOverride(selected.id, { termPath: termPath.trim(), kind: overrideKind });
      setTermPath("");
      fetchPackOverrides(selected.id).then(setOverrides);
    } finally {
      setBusy(false);
    }
  };

  const runDeleteOverride = async (override: PackOverride) => {
    if (!selected) return;
    setBusy(true);
    try {
      await deletePackOverride(selected.id, override.id);
      fetchPackOverrides(selected.id).then(setOverrides);
    } finally {
      setBusy(false);
    }
  };

  const runUpgrade = async (dryRun: boolean) => {
    if (!selected || upgradeVersion.trim().length === 0) return;
    setBusy(true);
    try {
      const result = await upgradePack(selected.id, upgradeVersion.trim(), upgradeManifest, dryRun);
      setUpgradeResult(result);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full">
      <div className="flex-1 overflow-y-auto p-8">
        <h1 className="mb-1 text-[25px] font-semibold text-gowl-t1">{strings.packsTitle}</h1>
        <p className="mb-5 text-[16.5px] text-gowl-t5">{strings.packsDescription}</p>

        <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
          {available.length === 0 ? (
            <div className="p-6 text-[16.5px] text-gowl-t5">{strings.packsEmpty}</div>
          ) : (
            available.map((pack) => (
              <div key={pack.id} className="flex items-center justify-between border-b border-gowl-row px-4 py-2.5 last:border-b-0">
                <div>
                  <div className="text-[17px] text-gowl-t1">{pack.id}</div>
                  <div className="text-[15.5px] text-gowl-t5">{pack.description}</div>
                </div>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => runInstall(pack)}
                  className="rounded-md bg-gowl-accent px-3 py-1.5 text-[16px] font-semibold text-gowl-accent-on disabled:opacity-40"
                >
                  {busy ? strings.packsInstalling : strings.packsInstall}
                </button>
              </div>
            ))
          )}
        </div>

        {installResult && (
          <div className="mt-4 rounded-md border border-gowl-line-2 bg-gowl-panel-2 p-3">
            <div className="mb-1 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.packsInstallResult}</div>
            <pre className="overflow-x-auto font-mono text-[15px] text-gowl-t2">{installResult}</pre>
          </div>
        )}

        <div className="mb-2 mt-6 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.packsInstalledTitle}</div>
        <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
          {installed.length === 0 ? (
            <div className="p-6 text-[16.5px] text-gowl-t5">{strings.packsInstalledEmpty}</div>
          ) : (
            installed.map((pack) => (
              <button
                key={pack.id}
                type="button"
                onClick={() => setSelected(pack)}
                className="flex w-full items-center justify-between border-b border-gowl-row px-4 py-2.5 text-left last:border-b-0 hover:bg-gowl-row"
              >
                <div>
                  <div className="text-[17px] text-gowl-t1">{pack.packId}</div>
                  <div className="text-[15.5px] text-gowl-t5">
                    {strings.packsVersionPrefix}
                    {pack.version} {strings.lineageChainSeparator} {pack.termCount} {strings.packsTermsSuffix}
                  </div>
                </div>
                <span className="text-[16px] text-gowl-accent">{strings.packsInspect}</span>
              </button>
            ))
          )}
        </div>
      </div>

      {selected && (
        <div className="w-[420px] flex-none overflow-y-auto border-l border-gowl-line bg-gowl-panel p-5">
          <div className="mb-4 flex items-start justify-between">
            <div className="text-[19px] font-semibold text-gowl-t1">{selected.packId}</div>
            <button type="button" onClick={() => setSelected(null)} className="text-[16px] text-gowl-t5">
              {strings.governClose}
            </button>
          </div>

          <div className="mb-4">
            <div className="mb-1 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.packsTerms}</div>
            {terms?.map((view) => (
              <div key={view.sourceIri} className="border-b border-gowl-row py-1 text-[16px] text-gowl-t2 last:border-b-0">
                {view.term.name}
              </div>
            ))}
          </div>

          <div className="mb-4">
            <div className="mb-1 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.packsOverrides}</div>
            {overrides && overrides.length === 0 && <p className="text-[16px] text-gowl-t5">{strings.packsOverridesEmpty}</p>}
            {overrides?.map((override) => (
              <div key={override.id} className="flex items-center justify-between border-b border-gowl-row py-1 text-[16px]">
                <span className="text-gowl-t2">
                  <span className="font-mono text-gowl-t6">{override.kind}</span> {override.termPath}
                </span>
                <button type="button" onClick={() => runDeleteOverride(override)} className="text-gowl-bad">
                  {strings.buildRemoveRelation}
                </button>
              </div>
            ))}
            <div className="mt-2 flex gap-1">
              <select
                value={overrideKind}
                onChange={(e) => setOverrideKind(e.target.value as PackOverrideKind)}
                aria-label={strings.packsOverrideKind}
                className="rounded-md border border-gowl-line-2 bg-gowl-input px-1.5 py-1 text-[15px] text-gowl-t1"
              >
                {OVERRIDE_KINDS.map((kind) => (
                  <option key={kind} value={kind}>
                    {kind}
                  </option>
                ))}
              </select>
              <input
                value={termPath}
                onChange={(e) => setTermPath(e.target.value)}
                placeholder={strings.packsOverrideTermPath}
                className="flex-1 rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1 text-[15px] text-gowl-t1"
              />
              <button
                type="button"
                disabled={busy || termPath.trim().length === 0}
                onClick={runAddOverride}
                className="rounded-md border border-gowl-line-2 px-2 py-1 text-[15px] text-gowl-t2 disabled:opacity-40"
              >
                {strings.packsAddOverride}
              </button>
            </div>
          </div>

          <div>
            <div className="mb-1 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.packsUpgrade}</div>
            <input
              value={upgradeVersion}
              onChange={(e) => setUpgradeVersion(e.target.value)}
              placeholder={strings.packsUpgradeVersion}
              className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
            />
            <textarea
              value={upgradeManifest}
              onChange={(e) => setUpgradeManifest(e.target.value)}
              placeholder={strings.packsUpgradeManifest}
              rows={4}
              className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input p-2 font-mono text-[15px] text-gowl-t1"
            />
            <div className="flex gap-2">
              <button
                type="button"
                disabled={busy || upgradeVersion.trim().length === 0}
                onClick={() => runUpgrade(true)}
                className="rounded-md border border-gowl-line-2 px-3 py-1.5 text-[16px] text-gowl-t2 disabled:opacity-40"
              >
                {strings.packsDryRun}
              </button>
              <button
                type="button"
                disabled={busy || upgradeVersion.trim().length === 0}
                onClick={() => runUpgrade(false)}
                className="rounded-md bg-gowl-accent px-3 py-1.5 text-[16px] font-semibold text-gowl-accent-on disabled:opacity-40"
              >
                {strings.packsApply}
              </button>
            </div>
            {upgradeResult && (
              <p className="mt-2 text-[16px] text-gowl-t3">
                {upgradeResult.applied ? strings.packsUpgradeApplied : strings.packsUpgradePreview}
              </p>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
