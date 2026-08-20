import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { strings } from "../lib/strings";
import { kindLabel, pairsFor, type ResolvedPair } from "../lib/memory/contradictions";
import { relativeTime } from "../lib/format";
import { SubjectFindings } from "../components/SubjectFindings";
import { findingsFor } from "../lib/exploreSeeds";
import { factsFromEdges, impactFromEdges, type EntityFact, type EntityImpactRow } from "../lib/entityFacts";
import { looksLikeAssetId } from "../lib/graph/graphContext";
import { incomingReferencesQuery, outgoingFactsQuery } from "../lib/graph/entityQueries";
import { toPathsConfig } from "../lib/trace";
import { TraceDetail } from "./TraceDetail";
import {
  fetchAsset,
  fetchAssetGraph,
  fetchAssetVersions,
  fetchContradictions,
  fetchFindings,
  fetchGraphContext,
  findPaths,
  pinToInvestigation,
  recallMemories,
  reviewContradiction,
  type Asset,
  type AssetVersion,
  type Finding,
  type Memory,
  type PathSearchResult,
} from "../lib/api";

const ENTITY_TABS = ["overview", "evidence", "lineage", "history", "paths", "queries"] as const;
type EntityTab = (typeof ENTITY_TABS)[number];

interface SubjectHeader {
  readonly name: string;
  readonly iri: string;
  readonly semanticType: string | undefined;
}

function Side({ memory }: { readonly memory: Memory | null }) {
  if (memory === null) {
    return <div className="text-[16px] italic text-gowl-t6">{strings.entityContradictionSideMissing}</div>;
  }
  return (
    <div className="rounded-md border border-gowl-amber-border bg-gowl-panel p-3">
      <div className="text-[18px] text-gowl-t1">{memory.content}</div>
      <div className="mt-1.5 font-mono text-[14px] text-gowl-t7">
        {`${new Date(memory.asOf).toLocaleDateString()} · ${memory.confidence.toFixed(2)}`}
      </div>
    </div>
  );
}

/** One history rendering, shared by the Overview panel and the History
 *  tab — a graph-only subject (a GST invoice, most of this console's real
 *  data) has no `/assets/{id}/versions` at all, so its history is its own
 *  findings' `detectedAt`, sorted newest first. Split into a dedicated
 *  History tab and a duplicated, asset-only copy embedded in Overview was
 *  exactly how the History *tab* ended up silently empty for every
 *  graph-only subject while Overview's own panel, right next to it, kept
 *  showing the real thing. */
function HistoryList({
  isAssetEntity,
  versions,
  subjectFindings,
}: {
  readonly isAssetEntity: boolean;
  readonly versions: readonly AssetVersion[] | null;
  readonly subjectFindings: readonly Finding[];
}) {
  if (isAssetEntity) {
    if ((versions ?? []).length === 0) {
      return <div className="text-[16px] text-gowl-t5">{strings.entityHistoryEmpty}</div>;
    }
    return (
      <>
        {(versions ?? []).map((version) => (
          <div key={`${version.version.major}.${version.version.minor}`} className="flex gap-2.5 pb-3">
            <div className="w-14 flex-none pt-0.5 font-mono text-[14px] text-gowl-t7">
              {relativeTime(version.updatedAt, new Date())}
            </div>
            <div className="flex-1 text-[16px] text-gowl-t2">
              {version.changeDescription?.summary ?? version.updatedBy}
            </div>
          </div>
        ))}
      </>
    );
  }
  if (subjectFindings.length === 0) {
    return <div className="text-[16px] text-gowl-t5">{strings.entityHistoryEmpty}</div>;
  }
  return (
    <>
      {[...subjectFindings]
        .sort((a, b) => b.detectedAt.localeCompare(a.detectedAt))
        .map((finding) => (
          <div key={finding.id} className="flex gap-2.5 pb-3">
            <div className="w-14 flex-none pt-0.5 font-mono text-[14px] text-gowl-t7">
              {relativeTime(finding.detectedAt, new Date())}
            </div>
            <div className="flex-1 text-[16px] text-gowl-t2">
              {`${finding.label} ${strings.entityFindingDetected}`}
            </div>
          </div>
        ))}
    </>
  );
}

function ImpactRows({ rows, empty }: { readonly rows: readonly EntityImpactRow[]; readonly empty: string }) {
  if (rows.length === 0) return <div className="text-[16px] text-gowl-t5">{empty}</div>;
  return (
    <>
      {rows.map((row) => (
        <div key={row.label} className="flex justify-between border-b border-gowl-row py-1.5 last:border-b-0">
          <span className="text-[16px] text-gowl-t4">{row.label}</span>
          <span className="font-mono text-[16px] text-gowl-t1">{row.n}</span>
        </div>
      ))}
    </>
  );
}

function CopyableQuery({ label, query }: { readonly label: string; readonly query: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="mb-3 last:mb-0">
      <div className="mb-1.5 flex items-center justify-between">
        <span className="text-[16px] text-gowl-t3">{label}</span>
        <button
          type="button"
          onClick={() => {
            void navigator.clipboard.writeText(query);
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          }}
          className="text-[15px] text-gowl-accent"
        >
          {copied ? strings.entityQueriesCopied : strings.entityQueriesCopy}
        </button>
      </div>
      <pre className="overflow-x-auto rounded-md border border-gowl-line-2 bg-gowl-input p-2.5 font-mono text-[15px] text-gowl-t2">
        {query}
      </pre>
    </div>
  );
}

/** Folded in from the former standalone `/paths?from=&to=` page — same
 *  `findPaths`/`toPathsConfig`/`TraceDetail`, just with `from` pre-filled
 *  to the entity already being viewed instead of a second text box. */
function PathsPanel({ from }: { readonly from: string }) {
  const [toInput, setToInput] = useState("");
  const [to, setTo] = useState<string | null>(null);
  const [result, setResult] = useState<PathSearchResult | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    setResult(null);
    setError(false);
    setToInput("");
    setTo(null);
  }, [from]);

  useEffect(() => {
    if (!to) return;
    let live = true;
    findPaths(from, to)
      .then((found) => {
        if (live) setResult(found);
      })
      .catch(() => {
        if (live) setError(true);
      });
    return () => {
      live = false;
    };
  }, [from, to]);

  const submit = () => {
    setResult(null);
    setError(false);
    if (toInput.trim()) setTo(toInput.trim());
  };

  return (
    <>
      <div className="flex flex-wrap items-end gap-3 border-b border-gowl-line bg-gowl-panel px-8 py-4">
        <div>
          <div className="mb-1 text-[15px] text-gowl-t5">{strings.pathsFromLabel}</div>
          <div className="rounded-md border border-gowl-line-2 bg-gowl-row px-3 py-1.5 font-mono text-[16px] text-gowl-t4">
            {from}
          </div>
        </div>
        <div>
          <div className="mb-1 text-[15px] text-gowl-t5">{strings.pathsToLabel}</div>
          <input
            value={toInput}
            onChange={(event) => setToInput(event.target.value)}
            className="rounded-md border border-gowl-line-2 bg-gowl-input px-3 py-1.5 font-mono text-[16px] text-gowl-t1"
          />
        </div>
        <button
          type="button"
          onClick={submit}
          className="rounded-md bg-gowl-accent px-4 py-1.5 text-[16px] font-semibold text-gowl-accent-on"
        >
          {strings.pathsSearch}
        </button>
      </div>

      {!to ? (
        <div className="p-8 text-[17px] text-gowl-t5">{strings.pathsMissingEnds}</div>
      ) : error ? (
        <div className="p-8 text-[17px] text-gowl-bad">{strings.traceError}</div>
      ) : !result ? (
        <div className="p-8 text-[17px] text-gowl-t5">{strings.traceLoading}</div>
      ) : (
        <TraceDetail config={toPathsConfig(result, from, to)} />
      )}
    </>
  );
}

/** Everything about one entity — extracted from the former standalone
 *  `/entity/:id` page so it can also live as Explore's own "Entity" tab,
 *  driven by the same picker. The Graph tab that page used to have is
 *  dropped here on purpose: it was a stub linking back to Explore, and
 *  Explore is where this component now lives. */
export function EntityPanel({ id }: { readonly id: string }) {
  const isAssetEntity = looksLikeAssetId(id);

  const [asset, setAsset] = useState<Asset | null>(null);
  const [subject, setSubject] = useState<SubjectHeader | null>(null);
  const [facts, setFacts] = useState<readonly EntityFact[] | null>(null);
  const [versions, setVersions] = useState<readonly AssetVersion[] | null>(null);
  const [impact, setImpact] = useState<readonly EntityImpactRow[] | null>(null);
  const [downstream, setDownstream] = useState<readonly EntityImpactRow[] | null>(null);
  const [sources, setSources] = useState<readonly string[]>([]);
  const [pairs, setPairs] = useState<readonly ResolvedPair[] | null>(null);
  const [subjectFindings, setSubjectFindings] = useState<readonly Finding[]>([]);
  const [error, setError] = useState(false);
  const [decided, setDecided] = useState<ReadonlySet<string>>(new Set());
  const [confirmingDismiss, setConfirmingDismiss] = useState<string | null>(null);
  const [pinStatus, setPinStatus] = useState<"idle" | "pinning" | "done" | "failed">("idle");
  const [activeTab, setActiveTab] = useState<EntityTab>("overview");

  useEffect(() => {
    setAsset(null);
    setSubject(null);
    setFacts(null);
    setVersions(null);
    setImpact(null);
    setDownstream(null);
    setSources([]);
    setPairs(null);
    setSubjectFindings([]);
    setError(false);
    setDecided(new Set());
    setPinStatus("idle");
    setActiveTab("overview");
    if (!id) return;

    let live = true;

    if (looksLikeAssetId(id)) {
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
          setFacts(factsFromEdges(outgoing));
          setVersions(versionsData);
          setImpact(impactFromEdges(incoming.edges));
          setDownstream(impactFromEdges(outgoing.edges));
          setSources(outgoing.nodes.find((node) => node.id === id)?.sources ?? []);

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
    } else {
      void (async () => {
        try {
          const [outgoing, incoming, allFindings] = await Promise.all([
            fetchGraphContext(id, { direction: "outgoing", hops: 1 }),
            fetchGraphContext(id, { direction: "incoming", hops: 1 }),
            fetchFindings(),
          ]);
          if (!live) return;

          const anchor = outgoing.nodes.find((node) => node.id === id);
          setSubject({
            name: anchor?.name ?? id,
            iri: id,
            semanticType: anchor?.semanticType ?? undefined,
          });
          setFacts(factsFromEdges(outgoing));
          setImpact(impactFromEdges(incoming.edges));
          setDownstream(impactFromEdges(outgoing.edges));
          setSources(anchor?.sources ?? []);
          setSubjectFindings(findingsFor(allFindings, id));
        } catch {
          if (live) setError(true);
        }
      })();
    }

    return () => {
      live = false;
    };
  }, [id]);

  const decide = async (pair: ResolvedPair, verdict: "confirmed" | "dismissed") => {
    const key = `${pair.id.a}-${pair.id.b}`;
    setConfirmingDismiss(null);
    try {
      await reviewContradiction({ a: pair.id.a, b: pair.id.b, verdict });
      if (verdict === "dismissed") setDecided((seen) => new Set(seen).add(key));
    } catch {
      // Left in the open list; the reviewer can simply try again.
    }
  };

  const handlePin = async () => {
    if (!id || (!asset && !subject)) return;
    setPinStatus("pinning");
    try {
      await pinToInvestigation(id, `Pinned from Entity: ${asset?.name ?? subject?.name ?? id}`);
      setPinStatus("done");
    } catch {
      setPinStatus("failed");
    }
  };

  if (error) {
    return <div className="p-8 text-[17px] text-gowl-bad">{strings.entityError}</div>;
  }
  if (!asset && !subject) {
    return <div className="p-8 text-[17px] text-gowl-t5">{strings.entityLoading}</div>;
  }

  const openPairs = (pairs ?? []).filter((pair) => !decided.has(`${pair.id.a}-${pair.id.b}`));
  const headerName = asset?.name ?? subject?.name ?? id;
  const headerSubtitle = asset?.fullyQualifiedName ?? subject?.iri ?? id;
  // `findPaths` accepts either id shape (`/graph/paths`'s own seed
  // parsing matches `/graph/context`'s) — an asset's own UUID for a
  // catalog entity, the full graph IRI otherwise.
  const entityIdentifier = asset?.id ?? subject?.iri ?? id;

  return (
    <div className="flex h-full flex-col overflow-auto">
      <div className="border-b border-gowl-line bg-gowl-panel px-8 py-5">
        <div className="flex items-start gap-4">
          <div className="flex-1">
            <div className="mb-1.5 flex items-center gap-2">
              <h1 className="text-[25px] font-semibold text-gowl-t1">{headerName}</h1>
              {subject?.semanticType && (
                <span className="rounded border border-gowl-accent-border bg-gowl-accent-bg px-1.5 py-0.5 font-mono text-[13.5px] text-gowl-accent">
                  {subject.semanticType}
                </span>
              )}
              {isAssetEntity && openPairs.length > 0 && (
                <span className="rounded border border-gowl-amber-border bg-gowl-amber-bg px-1.5 py-0.5 font-mono text-[13.5px] text-gowl-amber">
                  {`${openPairs.length} CONTRADICTION${openPairs.length === 1 ? "" : "S"}`}
                </span>
              )}
              {!isAssetEntity && subjectFindings.length > 0 && (
                <span className="rounded border border-gowl-amber-border bg-gowl-amber-bg px-1.5 py-0.5 font-mono text-[13.5px] text-gowl-amber">
                  {`${subjectFindings.length} FINDING${subjectFindings.length === 1 ? "" : "s"}`}
                </span>
              )}
            </div>
            <div className="font-mono text-[15.5px] text-gowl-t6">{headerSubtitle}</div>
          </div>
          <div className="flex flex-none gap-1.5">
            <button
              type="button"
              onClick={() => void handlePin()}
              disabled={pinStatus === "pinning"}
              className="rounded-md bg-gowl-accent px-3 py-1.5 text-[16px] font-semibold text-gowl-accent-on disabled:opacity-60"
            >
              {strings.entityPinToInvestigation}
            </button>
          </div>
        </div>
      </div>

      <div className="flex gap-1 border-b border-gowl-line bg-gowl-panel px-8">
        {ENTITY_TABS.map((tab) => {
          const label = tab === "overview" ? strings.entityTabOverview
            : tab === "evidence" ? strings.entityTabEvidence
            : tab === "lineage" ? strings.entityTabLineage
            : tab === "history" ? strings.entityTabHistory
            : tab === "paths" ? strings.entityTabPaths
            : strings.entityTabQueries;
          return (
            <button
              key={tab}
              type="button"
              onClick={() => setActiveTab(tab)}
              className={`px-3 py-2 text-[16px] ${
                activeTab === tab
                  ? "border-b-2 border-gowl-accent text-gowl-accent"
                  : "text-gowl-t5 hover:text-gowl-t2"
              }`}
            >
              {label}
            </button>
          );
        })}
      </div>

      {activeTab === "overview" && (
        <div className="grid grid-cols-[1fr_320px] gap-6 p-8">
        <div className="flex flex-col gap-5">
          <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
            <div className="border-b border-gowl-line px-4 py-2.5 font-mono text-[13.5px] tracking-widest text-gowl-t6">
              {strings.entityFacts}
            </div>
            {(facts ?? []).length === 0 ? (
              <div className="p-4 text-[16px] text-gowl-t5">{strings.entityFactsEmpty}</div>
            ) : (
              (facts ?? []).map((fact, index) => (
                <div
                  key={`${fact.relationship}-${fact.target}-${index}`}
                  className="grid grid-cols-[150px_1fr_74px] items-center gap-3 border-b border-gowl-row px-4 py-2.5 last:border-b-0"
                >
                  <span className="font-mono text-[15px] text-gowl-t5">{fact.relationship}</span>
                  <span className="text-[16.5px] text-gowl-t1">{fact.target}</span>
                  <span
                    className={`text-right font-mono text-[13.5px] ${fact.derived ? "text-gowl-accent" : "text-gowl-t6"}`}
                  >
                    {fact.derived ? strings.entityFactDerived : strings.entityFactAsserted}
                  </span>
                </div>
              ))
            )}
          </div>

          {isAssetEntity
            ? openPairs.map((pair) => {
                const key = `${pair.id.a}-${pair.id.b}`;
                return (
                  <div key={key} className="rounded-lg border border-gowl-amber-border bg-gowl-amber-deep p-4">
                    <div className="mb-3 font-mono text-[13.5px] tracking-widest text-gowl-amber">
                      {`${strings.entityContradictionTitle} · ${kindLabel(pair.kind)}`}
                    </div>
                    <div className="grid grid-cols-[1fr_34px_1fr] items-center gap-3">
                      <Side memory={pair.a} />
                      <div className="text-center text-[18px] text-gowl-amber">{strings.entityContradictionDivider}</div>
                      <Side memory={pair.b} />
                    </div>
                    <div className="mt-3.5 flex items-center gap-1.5">
                      <button
                        type="button"
                        onClick={() => void decide(pair, "confirmed")}
                        className="rounded-md border border-gowl-amber-border px-3 py-1.5 text-[15.5px] text-gowl-amber"
                      >
                        {strings.entityConfirm}
                      </button>
                      {confirmingDismiss === key ? (
                        <>
                          <span className="text-[15.5px] text-gowl-t5">{strings.entityDismissBody}</span>
                          <button
                            type="button"
                            onClick={() => void decide(pair, "dismissed")}
                            className="rounded-md border border-gowl-line-3 px-3 py-1.5 text-[15.5px] text-gowl-t4"
                          >
                            {strings.entityDismiss}
                          </button>
                          <button
                            type="button"
                            onClick={() => setConfirmingDismiss(null)}
                            className="text-[15.5px] text-gowl-t6"
                          >
                            {strings.entityDismissCancel}
                          </button>
                        </>
                      ) : (
                        <button
                          type="button"
                          onClick={() => setConfirmingDismiss(key)}
                          className="rounded-md border border-gowl-line-3 px-3 py-1.5 text-[15.5px] text-gowl-t4"
                        >
                          {strings.entityDismiss}
                        </button>
                      )}
                      <span className="ml-auto text-[15.5px] text-gowl-t6">{strings.entityContradictionHint}</span>
                    </div>
                  </div>
                );
              })
            : (
                <div className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
                  <SubjectFindings findings={subjectFindings} semanticType={subject?.semanticType} />
                </div>
              )}
        </div>

        <div className="flex flex-col gap-4">
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
            <div className="mb-3 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.entityHistory}</div>
            <HistoryList isAssetEntity={isAssetEntity} versions={versions} subjectFindings={subjectFindings} />
          </div>

          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
            <div className="mb-3 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.entityImpact}</div>
            <ImpactRows rows={impact ?? []} empty={strings.entityImpactEmpty} />
          </div>
        </div>
      </div>
      )}

      {activeTab === "evidence" && (
        <div className="p-8">
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-6">
            <div className="mb-3 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.entityEvidenceTitle}</div>
            {sources.length === 0 ? (
              <div className="text-[16px] text-gowl-t5">{strings.entityEvidenceEmpty}</div>
            ) : (
              <div className="flex flex-wrap gap-1.5">
                {sources.map((source) => (
                  <span
                    key={source}
                    className="rounded border border-gowl-line-2 bg-gowl-row px-2 py-1 font-mono text-[14.5px] text-gowl-t3"
                  >
                    {source}
                  </span>
                ))}
              </div>
            )}
          </div>
        </div>
      )}

      {activeTab === "lineage" && (
        <div className="p-8">
          <div className="grid grid-cols-2 gap-4">
            <div className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
              <div className="mb-3 font-mono text-[13.5px] tracking-widest text-gowl-t6">
                {strings.entityLineageUpstream}
              </div>
              <ImpactRows rows={impact ?? []} empty={strings.entityLineageEmpty} />
            </div>
            <div className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
              <div className="mb-3 font-mono text-[13.5px] tracking-widest text-gowl-t6">
                {strings.entityLineageDownstream}
              </div>
              <ImpactRows rows={downstream ?? []} empty={strings.entityLineageEmpty} />
            </div>
          </div>
          {isAssetEntity && (
            <Link
              to={`/lineage-view/${encodeURIComponent(id)}`}
              className="mt-4 inline-block text-[17px] text-gowl-accent underline"
            >
              {strings.entityLineageFullView}
            </Link>
          )}
        </div>
      )}

      {activeTab === "history" && (
        <div className="p-8">
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
            <div className="mb-3 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.entityHistory}</div>
            <HistoryList isAssetEntity={isAssetEntity} versions={versions} subjectFindings={subjectFindings} />
          </div>
        </div>
      )}

      {activeTab === "paths" && <PathsPanel from={entityIdentifier} />}

      {activeTab === "queries" && (
        <div className="p-8">
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-6">
            <div className="mb-3 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.entityQueriesTitle}</div>
            {subject ? (
              <>
                <CopyableQuery label={strings.entityQueriesOutgoingLabel} query={outgoingFactsQuery(subject.iri)} />
                <CopyableQuery label={strings.entityQueriesIncomingLabel} query={incomingReferencesQuery(subject.iri)} />
              </>
            ) : (
              // A catalog asset's own graph subject is keyed by an internal
              // namespace code this screen has no reliable way to construct
              // client-side — real templates stay scoped to graph-mapped
              // subjects (the IRI shape every pack entity already has)
              // rather than showing a query that would silently query the
              // wrong subject.
              <div className="mb-3 text-[16px] text-gowl-t5">{strings.entityQueriesAssetNote}</div>
            )}
            <Link to="/studio" className="mt-1 inline-block text-[16.5px] text-gowl-accent underline">
              {strings.entityQueriesOpenSparql}
            </Link>
          </div>
        </div>
      )}
    </div>
  );
}
