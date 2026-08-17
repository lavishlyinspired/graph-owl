import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { strings } from "../lib/strings";
import { kindLabel, pairsFor, type ResolvedPair } from "../lib/memory/contradictions";
import { relativeTime } from "../lib/format";
import {
  fetchAsset,
  fetchAssetGraph,
  fetchAssetVersions,
  fetchContradictions,
  pinToInvestigation,
  recallMemories,
  reviewContradiction,
  type Asset,
  type AssetVersion,
  type Memory,
} from "../lib/api";

interface Fact {
  readonly relationship: string;
  readonly target: string;
  readonly derived: boolean;
}

interface ImpactRow {
  readonly label: string;
  readonly n: number;
}

function Side({ memory }: { readonly memory: Memory | null }) {
  if (memory === null) {
    return <div className="text-[12px] italic text-gowl-t6">{strings.entityContradictionSideMissing}</div>;
  }
  return (
    <div className="rounded-md border border-gowl-amber-border bg-gowl-panel p-3">
      <div className="text-[14px] text-gowl-t1">{memory.content}</div>
      <div className="mt-1.5 font-mono text-[10px] text-gowl-t7">
        {`${new Date(memory.asOf).toLocaleDateString()} · ${memory.confidence.toFixed(2)}`}
      </div>
    </div>
  );
}

export default function EntityRoute() {
  const { id } = useParams<{ id?: string }>();

  const [asset, setAsset] = useState<Asset | null>(null);
  const [facts, setFacts] = useState<readonly Fact[] | null>(null);
  const [versions, setVersions] = useState<readonly AssetVersion[] | null>(null);
  const [impact, setImpact] = useState<readonly ImpactRow[] | null>(null);
  const [pairs, setPairs] = useState<readonly ResolvedPair[] | null>(null);
  const [error, setError] = useState(false);
  const [decided, setDecided] = useState<ReadonlySet<string>>(new Set());
  const [confirmingDismiss, setConfirmingDismiss] = useState<string | null>(null);
  const [pinStatus, setPinStatus] = useState<"idle" | "pinning" | "done" | "failed">("idle");

  useEffect(() => {
    setAsset(null);
    setFacts(null);
    setVersions(null);
    setImpact(null);
    setPairs(null);
    setError(false);
    setDecided(new Set());
    setPinStatus("idle");
    if (!id) return;

    let live = true;
    void (async () => {
      try {
        const [assetData, outgoing, versionsData, incoming, contradictions] = await Promise.all([
          fetchAsset(id),
          fetchAssetGraph(id, { direction: "outgoing", hops: 1 }),
          fetchAssetVersions(id),
          fetchAssetGraph(id, { direction: "incoming", hops: 1 }),
          fetchContradictions(id),
        ]);
        if (!live) return;

        setAsset(assetData);
        const byId = new Map(outgoing.nodes.map((node) => [node.id, node.name]));
        setFacts(
          outgoing.edges.map((edge) => ({
            relationship: edge.relationship,
            target: byId.get(edge.to) ?? edge.to,
            derived: edge.derived === true,
          })),
        );
        setVersions(versionsData);

        const grouped = new Map<string, number>();
        for (const edge of incoming.edges) {
          grouped.set(edge.relationship, (grouped.get(edge.relationship) ?? 0) + 1);
        }
        setImpact([...grouped.entries()].map(([label, n]) => ({ label, n })));

        if (contradictions.length === 0) {
          setPairs([]);
        } else {
          const recalled = await recallMemories(id);
          if (!live) return;
          setPairs(pairsFor(contradictions, recalled.map((r) => r.memory)));
        }
      } catch {
        if (live) setError(true);
      }
    })();

    return () => {
      live = false;
    };
  }, [id]);

  const decide = async (pair: ResolvedPair, verdict: "confirmed" | "dismissed") => {
    const key = `${pair.id.a}-${pair.id.b}`;
    setConfirmingDismiss(null);
    try {
      await reviewContradiction({ a: pair.id.a, b: pair.id.b, verdict });
      // Only a dismissal leaves the queue. A confirmed pair stays, and
      // removing it here would tell a lie the server does not tell.
      if (verdict === "dismissed") setDecided((seen) => new Set(seen).add(key));
    } catch {
      // Left in the open list; the reviewer can simply try again.
    }
  };

  const handlePin = async () => {
    if (!id || !asset) return;
    setPinStatus("pinning");
    try {
      await pinToInvestigation(id, `Pinned from Entity: ${asset.name}`);
      setPinStatus("done");
    } catch {
      setPinStatus("failed");
    }
  };

  if (!id) {
    return <div className="p-8 text-[13px] text-gowl-t5">{strings.entityNoId}</div>;
  }
  if (error) {
    return <div className="p-8 text-[13px] text-gowl-bad">{strings.entityError}</div>;
  }
  if (!asset) {
    return <div className="p-8 text-[13px] text-gowl-t5">{strings.entityLoading}</div>;
  }

  const openPairs = (pairs ?? []).filter((pair) => !decided.has(`${pair.id.a}-${pair.id.b}`));

  return (
    <div className="flex h-full flex-col overflow-auto">
      <div className="border-b border-gowl-line bg-gowl-panel px-8 py-5">
        <div className="flex items-start gap-4">
          <div className="flex-1">
            <div className="mb-1.5 flex items-center gap-2">
              <h1 className="text-[21px] font-semibold text-gowl-t1">{asset.name}</h1>
              {openPairs.length > 0 && (
                <span className="rounded border border-gowl-amber-border bg-gowl-amber-bg px-1.5 py-0.5 font-mono text-[9.5px] text-gowl-amber">
                  {`${openPairs.length} CONTRADICTION${openPairs.length === 1 ? "" : "S"}`}
                </span>
              )}
            </div>
            <div className="font-mono text-[11.5px] text-gowl-t6">{asset.fullyQualifiedName}</div>
          </div>
          <div className="flex flex-none gap-1.5">
            <Link
              to={`/explore/${encodeURIComponent(asset.id)}`}
              className="rounded-md border border-gowl-line-2 px-3 py-1.5 text-[12px] text-gowl-t3 hover:border-gowl-hover"
            >
              {strings.entityOpenInExplorer}
            </Link>
            <button
              type="button"
              onClick={() => void handlePin()}
              disabled={pinStatus === "pinning"}
              className="rounded-md bg-gowl-accent px-3 py-1.5 text-[12px] font-semibold text-gowl-accent-on disabled:opacity-60"
            >
              {strings.entityPinToInvestigation}
            </button>
          </div>
        </div>
      </div>

      <div className="grid grid-cols-[1fr_320px] gap-6 p-8">
        <div className="flex flex-col gap-5">
          <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
            <div className="border-b border-gowl-line px-4 py-2.5 font-mono text-[9.5px] tracking-widest text-gowl-t6">
              {strings.entityFacts}
            </div>
            {(facts ?? []).length === 0 ? (
              <div className="p-4 text-[12px] text-gowl-t5">{strings.entityFactsEmpty}</div>
            ) : (
              (facts ?? []).map((fact, index) => (
                <div
                  key={`${fact.relationship}-${fact.target}-${index}`}
                  className="grid grid-cols-[150px_1fr_74px] items-center gap-3 border-b border-gowl-row px-4 py-2.5 last:border-b-0"
                >
                  <span className="font-mono text-[11px] text-gowl-t5">{fact.relationship}</span>
                  <span className="text-[12.5px] text-gowl-t1">{fact.target}</span>
                  <span
                    className={`text-right font-mono text-[9.5px] ${fact.derived ? "text-gowl-accent" : "text-gowl-t6"}`}
                  >
                    {fact.derived ? strings.entityFactDerived : strings.entityFactAsserted}
                  </span>
                </div>
              ))
            )}
          </div>

          {openPairs.map((pair) => {
            const key = `${pair.id.a}-${pair.id.b}`;
            return (
              <div key={key} className="rounded-lg border border-gowl-amber-border bg-gowl-amber-deep p-4">
                <div className="mb-3 font-mono text-[9.5px] tracking-widest text-gowl-amber">
                  {`${strings.entityContradictionTitle} · ${kindLabel(pair.kind)}`}
                </div>
                <div className="grid grid-cols-[1fr_34px_1fr] items-center gap-3">
                  <Side memory={pair.a} />
                  <div className="text-center text-[14px] text-gowl-amber">{strings.entityContradictionDivider}</div>
                  <Side memory={pair.b} />
                </div>
                <div className="mt-3.5 flex items-center gap-1.5">
                  <button
                    type="button"
                    onClick={() => void decide(pair, "confirmed")}
                    className="rounded-md border border-gowl-amber-border px-3 py-1.5 text-[11.5px] text-gowl-amber"
                  >
                    {strings.entityConfirm}
                  </button>
                  {confirmingDismiss === key ? (
                    <>
                      <span className="text-[11.5px] text-gowl-t5">{strings.entityDismissBody}</span>
                      <button
                        type="button"
                        onClick={() => void decide(pair, "dismissed")}
                        className="rounded-md border border-gowl-line-3 px-3 py-1.5 text-[11.5px] text-gowl-t4"
                      >
                        {strings.entityDismiss}
                      </button>
                      <button
                        type="button"
                        onClick={() => setConfirmingDismiss(null)}
                        className="text-[11.5px] text-gowl-t6"
                      >
                        {strings.entityDismissCancel}
                      </button>
                    </>
                  ) : (
                    <button
                      type="button"
                      onClick={() => setConfirmingDismiss(key)}
                      className="rounded-md border border-gowl-line-3 px-3 py-1.5 text-[11.5px] text-gowl-t4"
                    >
                      {strings.entityDismiss}
                    </button>
                  )}
                  <span className="ml-auto text-[11.5px] text-gowl-t6">{strings.entityContradictionHint}</span>
                </div>
              </div>
            );
          })}
        </div>

        <div className="flex flex-col gap-4">
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
            <div className="mb-3 font-mono text-[9.5px] tracking-widest text-gowl-t6">{strings.entityHistory}</div>
            {(versions ?? []).length === 0 ? (
              <div className="text-[12px] text-gowl-t5">{strings.entityHistoryEmpty}</div>
            ) : (
              (versions ?? []).map((version) => (
                <div key={`${version.version.major}.${version.version.minor}`} className="flex gap-2.5 pb-3">
                  <div className="w-14 flex-none pt-0.5 font-mono text-[10px] text-gowl-t7">
                    {relativeTime(version.updatedAt, new Date())}
                  </div>
                  <div className="flex-1 text-[12px] text-gowl-t2">
                    {version.changeDescription?.summary ?? version.updatedBy}
                  </div>
                </div>
              ))
            )}
          </div>

          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
            <div className="mb-3 font-mono text-[9.5px] tracking-widest text-gowl-t6">{strings.entityImpact}</div>
            {(impact ?? []).length === 0 ? (
              <div className="text-[12px] text-gowl-t5">{strings.entityImpactEmpty}</div>
            ) : (
              (impact ?? []).map((row) => (
                <div key={row.label} className="flex justify-between border-b border-gowl-row py-1.5 last:border-b-0">
                  <span className="text-[12px] text-gowl-t4">{row.label}</span>
                  <span className="font-mono text-[12px] text-gowl-t1">{row.n}</span>
                </div>
              ))
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
