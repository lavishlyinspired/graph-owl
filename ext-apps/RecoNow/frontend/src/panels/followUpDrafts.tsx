import { useState } from "react";
import { useOutletContext } from "react-router-dom";
import { GeneratedBadge } from "../components/GeneratedBadge";
import { generateFollowUps, type FollowUpGroup } from "../lib/api";
import { formatRupees } from "../lib/format";
import type { WorkspaceState } from "../lib/workspace";

/** Supplier chase messages — mockups 4 and 5.
 *
 *  **The vendor agent has been drafting these and nothing rendered them.**
 *  It runs on every reconciliation, produces a grounded message per unfiled
 *  invoice, and the drafts went into a run record nobody opened.
 *
 *  One card per **supplier**, not per invoice: you send one email to a
 *  supplier, and three separate emails about three invoices is how a working
 *  relationship gets damaged by software. */
export default function FollowUpDraftsPanel() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [state, setState] = useState<"idle" | "working" | "done" | "failed">("idle");
  const [groups, setGroups] = useState<readonly FollowUpGroup[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  const generate = () => {
    if (!clientId || !periodId) return;
    setState("working");
    generateFollowUps(clientId, periodId)
      .then((result) => {
        setGroups(result.groups);
        setState("done");
      })
      .catch(() => setState("failed"));
  };

  const copy = (group: FollowUpGroup) => {
    void navigator.clipboard.writeText(group.message).then(() => setCopied(group.supplier_gstin));
  };

  const copyAll = () => {
    const all = groups
      .map((g) => `— ${g.supplier_name} (${g.supplier_gstin}) —\n\n${g.message}`)
      .join("\n\n\n");
    void navigator.clipboard.writeText(all).then(() => setCopied("all"));
  };

  if (!clientId || !periodId) {
    return <div className="p-6 text-[13px] text-reco-t4">Choose a client and a period.</div>;
  }

  return (
    <div className="space-y-4 p-6">
      <header>
        <h1 className="text-[19px] font-medium text-reco-t1">Supplier follow-ups</h1>
        <p className="mt-1 text-[13px] text-reco-t4">
          One message per supplier who has not filed, naming the invoices and the credit at
          stake.
        </p>
      </header>

      {state === "idle" && (
        <div className="rounded border border-reco-line p-8 text-center">
          <p className="mb-4 text-[12.5px] text-reco-t4">
            Drafts a chase message for every supplier with an unfiled invoice this period.
          </p>
          <button type="button" onClick={generate} className={actionClass}>
            Generate messages
          </button>
        </div>
      )}

      {state === "working" && (
        <p className="text-[12.5px] text-reco-t4">Reading this period's unfiled invoices…</p>
      )}

      {state === "failed" && (
        <p className="text-[12.5px] text-reco-bad">Could not draft the messages.</p>
      )}

      {state === "done" && groups.length === 0 && (
        <p className="text-[12.5px] text-reco-t4">
          Every supplier has filed. There is nobody to chase this period.
        </p>
      )}

      {state === "done" && groups.length > 0 && (
        <>
          <div className="flex items-center gap-2">
            <span className="text-[12.5px] text-reco-t2">
              {groups.length} message{groups.length === 1 ? "" : "s"} drafted
            </span>
            <span className="flex-1" />
            <button type="button" onClick={copyAll} className={actionClass}>
              {copied === "all" ? "Copied all" : "Copy all"}
            </button>
            <button type="button" onClick={generate} className={actionClass}>
              Regenerate
            </button>
          </div>

          <div className="overflow-hidden rounded border border-reco-line">
            {groups.map((group) => {
              const expanded = open === group.supplier_gstin;
              return (
                <div key={group.supplier_gstin} className="border-b border-reco-line-2 last:border-b-0">
                  <button
                    type="button"
                    aria-expanded={expanded}
                    onClick={() => setOpen(expanded ? null : group.supplier_gstin)}
                    className="flex w-full items-center gap-3 px-4 py-3 text-left hover:bg-reco-panel-2"
                  >
                    <span className="text-[13px] text-reco-t1">{group.supplier_name}</span>
                    <span className="font-mono text-[10.5px] text-reco-t5">
                      {group.supplier_gstin}
                    </span>
                    {/* What the conversation is worth. Per-invoice amounts
                        would make one real problem look like several small
                        ones. */}
                    <span className="rounded bg-red-50 px-2 py-0.5 font-mono text-[10.5px] text-red-700">
                      {group.invoices.length} inv · {formatRupees(group.at_risk)} at risk
                    </span>
                    <span className="flex-1" />
                    <GeneratedBadge source={group.source} />
                    <span className="text-[12px] text-reco-t5">{expanded ? "⌃" : "⌄"}</span>
                  </button>

                  {expanded && (
                    <div className="border-t border-reco-line-2 bg-reco-panel-2/40 px-4 py-3">
                      <div className="mb-2 font-mono text-[9.5px] uppercase tracking-wider text-reco-t5">
                        {group.invoices.join(" · ")}
                      </div>
                      <pre className="whitespace-pre-wrap text-[12.5px] leading-relaxed text-reco-t2">
                        {group.message}
                      </pre>
                      <button
                        type="button"
                        onClick={() => copy(group)}
                        className={`mt-3 ${actionClass}`}
                      >
                        {copied === group.supplier_gstin ? "Copied" : "Copy message"}
                      </button>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}

const actionClass =
  "rounded border border-reco-line px-3 py-1.5 text-[12px] text-reco-t2 hover:border-reco-accent hover:text-reco-accent";
