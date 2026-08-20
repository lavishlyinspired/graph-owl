import { useState } from "react";
import { strings } from "../../lib/strings";

interface Proposal {
  readonly id: string;
  readonly term: string;
  readonly definition: string;
  readonly submittedBy: string;
  readonly submittedAt: string;
  readonly status: "pending" | "approved" | "rejected" | "needs-info";
  readonly reviewNote?: string;
}

const MOCK_PROPOSALS: readonly Proposal[] = [
  { id: "p1", term: "Master Vendor Record", definition: "The golden copy of vendor entity data, resolved from source systems and certified by the governance team.", submittedBy: "Data Steward", submittedAt: "2 days ago", status: "pending" },
  { id: "p2", term: "Exposure Threshold", definition: "The monetary value above which an entity relationship requires human review before certification.", submittedBy: "Business Analyst", submittedAt: "5 days ago", status: "approved", reviewNote: "Clear definition, matches the rule in the engine." },
  { id: "p3", term: "Ghost Entity", definition: "A node in the graph that has no outbound edges to any verified fact or document.", submittedBy: "Data Steward", submittedAt: "1 week ago", status: "needs-info", reviewNote: "Needs a reference to the dedup pipeline's criteria." },
  { id: "p4", term: "Dual Filing", definition: "When two distinct entities are linked to the same regulatory submission, indicating potential duplication.", submittedBy: "Analyst", submittedAt: "3 days ago", status: "pending" },
  { id: "p5", term: "Source Chain", definition: "The path from a certified fact back to the raw document or record that supports it.", submittedBy: "Data Steward", submittedAt: "1 week ago", status: "approved", reviewNote: "Approved — this matches the lineage vocabulary." },
];

const STATUS_STYLES: Record<Proposal["status"], { bg: string; text: string; label: string }> = {
  pending: { bg: "bg-gowl-panel-2", text: "text-gowl-t2", label: "PENDING" },
  approved: { bg: "bg-gowl-ok-bg", text: "text-gowl-ok", label: "APPROVED" },
  rejected: { bg: "bg-gowl-bad-bg", text: "text-gowl-bad", label: "REJECTED" },
  "needs-info": { bg: "bg-gowl-amber-bg", text: "text-gowl-amber", label: "NEEDS INFO" },
};

export function ProposalsTab({ glossaryId: _glossaryId }: { readonly glossaryId: string }) {
  const [proposals] = useState<readonly Proposal[]>(MOCK_PROPOSALS);
  const [filter, setFilter] = useState<"all" | Proposal["status"]>("all");

  const filtered = filter === "all" ? proposals : proposals.filter((p) => p.status === filter);

  const counts = {
    all: proposals.length,
    pending: proposals.filter((p) => p.status === "pending").length,
    approved: proposals.filter((p) => p.status === "approved").length,
    rejected: proposals.filter((p) => p.status === "rejected").length,
    "needs-info": proposals.filter((p) => p.status === "needs-info").length,
  };

  return (
    <div>
      <div className="mb-4 flex items-center justify-between">
        <div className="flex gap-1">
          {(["all", "pending", "approved", "rejected", "needs-info"] as const).map((f) => (
            <button
              key={f}
              type="button"
              onClick={() => setFilter(f)}
              className={`rounded px-2.5 py-1 text-[15px] ${
                filter === f
                  ? "bg-gowl-accent text-gowl-accent-on"
                  : "text-gowl-t5 hover:text-gowl-t2"
              }`}
            >
              {f === "all" ? "All" : f === "needs-info" ? "Needs info" : f.charAt(0).toUpperCase() + f.slice(1)} ({counts[f]})
            </button>
          ))}
        </div>
      </div>

      <div className="space-y-2">
        {filtered.length === 0 ? (
          <p className="py-8 text-center text-[16.5px] text-gowl-t5">
            {strings.studioNotYetBuiltProposals}
          </p>
        ) : (
          filtered.map((p) => {
            const ss = STATUS_STYLES[p.status];
            return (
              <div key={p.id} className="rounded-md border border-gowl-line bg-gowl-panel p-4">
                <div className="mb-2 flex items-start justify-between">
                  <div>
                    <span className="text-[17px] font-semibold text-gowl-t1">{p.term}</span>
                    <span className={`ml-2 rounded-full px-2 py-0.5 font-mono text-[12.5px] ${ss.bg} ${ss.text}`}>
                      {ss.label}
                    </span>
                  </div>
                  <span className="text-[15px] text-gowl-t5">{p.submittedBy} · {p.submittedAt}</span>
                </div>
                <p className="mb-2 text-[16px] leading-relaxed text-gowl-t3">{p.definition}</p>
                {p.reviewNote && (
                  <div className="rounded bg-gowl-panel-2 px-3 py-1.5 text-[15px] text-gowl-t5">
                    Review: {p.reviewNote}
                  </div>
                )}
                {p.status === "pending" && (
                  <div className="mt-3 flex gap-2">
                    <button type="button" className="rounded border border-gowl-ok-border bg-gowl-ok-bg px-2.5 py-1 text-[14.5px] text-gowl-ok">
                      Approve
                    </button>
                    <button type="button" className="rounded border border-gowl-line-2 px-2.5 py-1 text-[14.5px] text-gowl-t5">
                      Request info
                    </button>
                    <button type="button" className="rounded border border-gowl-bad-border bg-gowl-bad-bg px-2.5 py-1 text-[14.5px] text-gowl-bad">
                      Reject
                    </button>
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
