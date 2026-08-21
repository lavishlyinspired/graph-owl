import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { strings } from "../lib/strings";
import { fetchFindings, performInboxAction, type Finding } from "../lib/api";
import { resolveInboxAction, INBOX_REJECT_REQUIRES_REASON } from "../lib/inboxActions";
import { packsIn, sortForReview, subjectDisplayLabel } from "../lib/findingsQueue";
import { relativeTime } from "../lib/format";
import { KpiGrid } from "./KpiGrid";

const STATUS_LABEL: Record<Finding["status"], string> = {
  pending: strings.contradictionsFilterStatusPending,
  accepted: strings.contradictionsFilterStatusAccepted,
  rejected: strings.contradictionsFilterStatusRejected,
};

const REJECT_REQUIRES_REASON = INBOX_REJECT_REQUIRES_REASON.has("finding");

/** The detail + decide panel for one selected finding — the "what is this,
 *  why should I believe it, what do I do" a business admin needs before
 *  acting, not just a one-line summary. */
function FindingDetail({
  finding,
  onClose,
  onDecide,
}: {
  readonly finding: Finding;
  readonly onClose: () => void;
  readonly onDecide: (finding: Finding, verdict: "approve" | "reject", reason?: string) => Promise<void>;
}) {
  const [rejecting, setRejecting] = useState(false);
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setRejecting(false);
    setReason("");
    setFailed(false);
  }, [finding.id]);

  const accept = async () => {
    setBusy(true);
    setFailed(false);
    try {
      await onDecide(finding, "approve");
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  };

  const confirmReject = async () => {
    setBusy(true);
    setFailed(false);
    try {
      await onDecide(finding, "reject", reason.trim());
      setRejecting(false);
      setReason("");
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="w-[380px] flex-none overflow-y-auto border-l border-gowl-line bg-gowl-panel p-5">
      <div className="mb-4 flex items-start justify-between">
        <div>
          <div className="font-mono text-[15px] text-gowl-t6">
            {finding.pack}
            {finding.priority != null ? ` · P${finding.priority}` : ""}
          </div>
          <div className="text-[19px] font-semibold text-gowl-t1">{subjectDisplayLabel(finding)}</div>
          <div className="mt-0.5 font-mono text-[13.5px] text-gowl-amber">{finding.label}</div>
        </div>
        <button type="button" onClick={onClose} className="text-[16px] text-gowl-t5">
          {strings.governClose}
        </button>
      </div>

      <p className="mb-4 text-[16.5px] leading-relaxed text-gowl-t2">{finding.summary}</p>

      <div className="mb-4">
        <div className="mb-1 font-mono text-[13.5px] tracking-widest text-gowl-t6">
          {strings.contradictionsDetailGovernedBy}
        </div>
        <div className="font-mono text-[15.5px] text-gowl-t3">{finding.governedBy}</div>
      </div>

      <div className="mb-4 overflow-hidden rounded-md border border-gowl-line-2">
        <div className="border-b border-gowl-line-2 bg-gowl-panel-2 px-3 py-2 font-mono text-[13.5px] tracking-widest text-gowl-t5">
          {`${strings.contradictionsDetailEvidence} · ${finding.evidence.length}`}
        </div>
        {finding.evidence.map((item, i) => (
          <div
            key={`${item.predicate}-${i}`}
            className="flex items-center justify-between gap-2 border-b border-gowl-row px-3 py-2 last:border-b-0"
          >
            <span className="font-mono text-[15px] text-gowl-t2">{item.predicate}</span>
            <span className="font-mono text-[14px] text-gowl-ok">{item.value}</span>
          </div>
        ))}
      </div>

      <Link
        to={`/entity/${encodeURIComponent(finding.subject)}`}
        className="mb-4 inline-block text-[16px] text-gowl-accent underline"
      >
        {strings.contradictionsOpenEntity}
      </Link>

      {failed && <div className="mb-3 text-[15.5px] text-gowl-bad">{strings.contradictionsActionFailed}</div>}

      {finding.status === "pending" ? (
        rejecting ? (
          <div className="rounded-md border border-gowl-line-2 p-3">
            <input
              autoFocus
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              placeholder={strings.contradictionsReasonPlaceholder}
              className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
            />
            <div className="flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setRejecting(false)}
                className="rounded-md border border-gowl-line px-3 py-1.5 text-[15.5px] text-gowl-t3"
              >
                {strings.contradictionsReasonCancel}
              </button>
              <button
                type="button"
                disabled={busy || (REJECT_REQUIRES_REASON && reason.trim().length === 0)}
                onClick={() => void confirmReject()}
                className="rounded-md border border-gowl-bad bg-gowl-bad-bg px-3 py-1.5 text-[15.5px] text-gowl-bad disabled:opacity-40"
              >
                {strings.contradictionsReasonConfirm}
              </button>
            </div>
          </div>
        ) : (
          <div className="flex gap-1.5">
            <button
              type="button"
              disabled={busy}
              onClick={() => void accept()}
              className="rounded-md bg-gowl-accent px-3 py-1.5 text-[15.5px] font-semibold text-gowl-accent-on disabled:opacity-40"
            >
              {strings.contradictionsAccept}
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => setRejecting(true)}
              className="rounded-md border border-gowl-line-3 px-3 py-1.5 text-[15.5px] text-gowl-t4"
            >
              {strings.contradictionsReject}
            </button>
          </div>
        )
      ) : (
        <div className="rounded-md border border-gowl-line-2 bg-gowl-panel-2 p-3 text-[15.5px] text-gowl-t4">
          {`${STATUS_LABEL[finding.status]}${finding.decidedBy ? ` · ${strings.contradictionsDetailDecidedBy} ${finding.decidedBy}` : ""}`}
          {finding.reason && <div className="mt-1 text-gowl-t5">{finding.reason}</div>}
        </div>
      )}
    </div>
  );
}

/** GOVERN's Contradictions tab — Plan 122a A5, reworked.
 *
 *  **Two genuinely different features, shown honestly as two.** The queue
 *  above is a domain pack's rule-derived findings — evidence-backed, cited
 *  to a rule. The search box below is `Memory`-based institutional
 *  disagreement — a person's claim, not a rule's conclusion. Both now work
 *  for a catalog asset or a graph-native subject: `/assets/{id}/memories`
 *  and `/assets/{id}/contradictions` used to 400 ("UUID parsing failed")
 *  for any subject that was not a catalog asset's own `Uuid` — verified
 *  against the running deployment before the backend was extended to
 *  accept a subject IRI on this same path. Collapsing the two into one
 *  "Contradictions" queue would still be wrong: one is a machine's
 *  conclusion from evidence, the other is a person's word, and a reviewer's
 *  next move differs for each. */
export default function ContradictionsRoute() {
  const navigate = useNavigate();
  const [entityId, setEntityId] = useState("");

  const [findings, setFindings] = useState<readonly Finding[] | null>(null);
  const [allPacks, setAllPacks] = useState<readonly string[]>([]);
  const [error, setError] = useState(false);
  const [pack, setPack] = useState("");
  const [status, setStatus] = useState<"" | Finding["status"]>("");
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const load = () => {
    fetchFindings({ pack: pack || undefined, status: status || undefined })
      .then((data) => {
        setFindings(sortForReview(data));
        setAllPacks((prev) => (prev.length === 0 ? packsIn(data) : prev));
      })
      .catch(() => setError(true));
  };

  useEffect(load, [pack, status]);

  const decide = async (finding: Finding, verdict: "approve" | "reject", reason?: string) => {
    await performInboxAction(resolveInboxAction("finding", finding.id, verdict, reason));
    load();
  };

  const openEntitySearch = () => {
    const trimmed = entityId.trim();
    if (trimmed.length === 0) return;
    navigate(`/entity/${encodeURIComponent(trimmed)}`);
  };

  const selected = findings?.find((f) => f.id === selectedId) ?? null;

  return (
    <div className="flex h-full">
      <div className="flex-1 overflow-y-auto p-8">
        <h1 className="mb-1 text-[25px] font-semibold text-gowl-t1">{strings.contradictionsTitle}</h1>
        <p className="mb-5 text-[16.5px] text-gowl-t5">{strings.contradictionsDescription}</p>

        {error ? (
          <div className="text-[17px] text-gowl-bad">{strings.contradictionsLoadError}</div>
        ) : !findings ? (
          <div className="text-[17px] text-gowl-t5">{strings.governLoading}</div>
        ) : (
          <>
            <KpiGrid
              kpis={[
                { label: strings.contradictionsKpiPending, value: String(findings.filter((f) => f.status === "pending").length) },
                { label: strings.contradictionsKpiAccepted, value: String(findings.filter((f) => f.status === "accepted").length) },
                { label: strings.contradictionsKpiRejected, value: String(findings.filter((f) => f.status === "rejected").length) },
                { label: strings.contradictionsKpiPacks, value: String(allPacks.length) },
              ]}
            />

            <div className="mb-3 flex gap-2">
              <select
                value={pack}
                onChange={(e) => setPack(e.target.value)}
                className="rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 text-[15.5px] text-gowl-t2"
              >
                <option value="">{strings.contradictionsFilterPackAll}</option>
                {allPacks.map((p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ))}
              </select>
              <select
                value={status}
                onChange={(e) => setStatus(e.target.value as "" | Finding["status"])}
                className="rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 text-[15.5px] text-gowl-t2"
              >
                <option value="">{strings.contradictionsFilterStatusAll}</option>
                <option value="pending">{strings.contradictionsFilterStatusPending}</option>
                <option value="accepted">{strings.contradictionsFilterStatusAccepted}</option>
                <option value="rejected">{strings.contradictionsFilterStatusRejected}</option>
              </select>
            </div>

            <div className="mb-8 overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
              <div className="grid grid-cols-[1.4fr_1.4fr_100px_110px_110px] gap-3 border-b border-gowl-line bg-gowl-panel-2 px-4 py-2 font-mono text-[13.5px] tracking-wider text-gowl-t6">
                <span>{strings.contradictionsColSubject}</span>
                <span>{strings.contradictionsColFinding}</span>
                <span>{strings.contradictionsColPack}</span>
                <span>{strings.contradictionsColStatus}</span>
                <span>{strings.contradictionsColDetected}</span>
              </div>
              {findings.length === 0 ? (
                <div className="p-6 text-[16.5px] text-gowl-t5">{strings.contradictionsEmpty}</div>
              ) : (
                findings.map((finding) => (
                  <button
                    key={finding.id}
                    type="button"
                    onClick={() => setSelectedId(finding.id)}
                    className={`grid w-full grid-cols-[1.4fr_1.4fr_100px_110px_110px] items-center gap-3 border-b border-gowl-row px-4 py-2.5 text-left last:border-b-0 hover:bg-gowl-row ${
                      selectedId === finding.id ? "bg-gowl-row" : ""
                    }`}
                  >
                    <span className="truncate text-[16.5px] text-gowl-t1">{subjectDisplayLabel(finding)}</span>
                    <span className="truncate font-mono text-[15px] text-gowl-t2">{finding.label}</span>
                    <span className="truncate font-mono text-[14.5px] text-gowl-t5">{finding.pack}</span>
                    <span className="text-[15px] text-gowl-t5">{STATUS_LABEL[finding.status]}</span>
                    <span className="font-mono text-[14px] text-gowl-t7">
                      {relativeTime(finding.detectedAt, new Date())}
                    </span>
                  </button>
                ))
              )}
            </div>
          </>
        )}

        <div className="max-w-[560px] rounded-lg border border-gowl-line bg-gowl-panel p-5">
          <div className="mb-2 font-mono text-[13.5px] tracking-widest text-gowl-t6">
            {strings.contradictionsMemorySectionTitle}
          </div>
          <p className="mb-4 text-[15.5px] leading-relaxed text-gowl-t4">{strings.contradictionsMemorySectionNote}</p>
          <div className="flex gap-2">
            <input
              value={entityId}
              onChange={(e) => setEntityId(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") openEntitySearch();
              }}
              placeholder={strings.contradictionsSearchPlaceholder}
              className="flex-1 rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 font-mono text-[16px] text-gowl-t1"
            />
            <button
              type="button"
              onClick={openEntitySearch}
              disabled={entityId.trim().length === 0}
              className="rounded-md bg-gowl-accent px-4 py-1.5 text-[16px] font-semibold text-gowl-accent-on disabled:opacity-40"
            >
              {strings.contradictionsSearchSubmit}
            </button>
          </div>
        </div>
      </div>

      {selected && (
        <FindingDetail finding={selected} onClose={() => setSelectedId(null)} onDecide={decide} />
      )}
    </div>
  );
}
