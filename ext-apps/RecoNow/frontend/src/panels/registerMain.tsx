import { useEffect, useState } from "react";
import { Link, useOutletContext, useSearchParams } from "react-router-dom";
import { fetchRegister, type Register } from "../lib/api";
import { ExplainCase } from "../components/ExplainCase";
import { CaseExplorer } from "../components/CaseExplorer";
import { groupFindings } from "../lib/findingGroups";
import { DetailDrawer } from "../components/DetailDrawer";
import type { WorkspaceState } from "../lib/workspace";

function formatRupees(amount: number): string {
  return `₹${amount.toLocaleString("en-IN", { maximumFractionDigits: 0 })}`;
}

export default function RegisterRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [params, setParams] = useSearchParams();
  const reasonCode = params.get("reason_code") ?? undefined;
  const [register, setRegister] = useState<Register | null>(null);
  // Case detail is merged in here rather than living on its own route: you
  // open a case *from* this list, and a route arrived at without a selection
  // shows an empty screen.
  const [openCase, setOpenCase] = useState<string | null>(null);
  const selected = register?.rows.find((r) => r.id === openCase) ?? null;
  const groups = groupFindings(register?.rows ?? []);

  useEffect(() => {
    if (!clientId || !periodId) return;
    fetchRegister(clientId, periodId, reasonCode)
      .then(setRegister)
      .catch(() => setRegister(null));
  }, [clientId, periodId, reasonCode]);

  if (!clientId || !periodId) {
    return <div className="p-8 text-[13px] text-reco-t4">Select a client and a period first.</div>;
  }

  return (
    <div className="p-6">
      <div className="mb-4 flex items-end justify-between">
        <div>
          <h1 className="mb-1 text-[20px] font-bold text-reco-t1">Findings</h1>
          <p className="mb-1 text-[12.5px] text-reco-t4">
            What needs attention and why. Not every mismatch is a finding, and not every
            finding is a mismatch — blocked credit sits on invoices that match perfectly.
          </p>
          {reasonCode && (
            <p className="text-[12.5px] text-reco-t4">
              Filtered to <span className="font-mono text-reco-t2">{reasonCode}</span> —{" "}
              <Link to="/register" className="text-reco-accent">
                clear filter
              </Link>
            </p>
          )}
        </div>
        {register && (
          <div className="text-[12.5px] text-reco-t4">
            {register.rows.length} row(s) · {formatRupees(register.total_exposure)} exposure in this filter
          </div>
        )}
      </div>

      {/* **Grouped by who has to act, not listed flat.** A flat exception
          table makes a reviewer decide for every row whether it is their
          problem, the supplier's, or the law's — twelve times a period, from
          a rule label. The categories are authored in the pack, because the
          remedy is domain knowledge. */}
      {groups.map((group) => (
        <section key={group.category} className="mb-4">
          <div className="mb-1.5 flex items-baseline gap-2">
            <h2 className="text-[13px] font-semibold text-reco-t1">{group.label}</h2>
            <span className="font-mono text-[11px] text-reco-t5">
              {group.count} · {formatRupees(group.exposure)}
            </span>
            <span className="text-[11.5px] text-reco-t4">{group.blurb}</span>
          </div>

          <div className="overflow-hidden rounded-lg border border-reco-line bg-reco-panel">
            {group.rules.map((rule) => (
              <div key={rule.reason_code}>
                <div className="flex items-baseline gap-2 border-b border-reco-line bg-reco-panel-2 px-4 py-1.5">
                  <span className="text-[12px] text-reco-t1">{rule.title}</span>
                  <span className="font-mono text-[10px] text-reco-t5">{rule.reason_code}</span>
                  <span className="flex-1" />
                  <span className="font-mono text-[11px] text-reco-t2">
                    {rule.rows.length} · {formatRupees(rule.exposure)}
                  </span>
                </div>

                {rule.rows.map((row) => (
                  <button
                    key={row.id}
                    type="button"
                    onClick={() => {
                      const next = openCase === row.id ? null : row.id;
                      setOpenCase(next);
                      const updated = new URLSearchParams(params);
                      if (next) updated.set("id", next);
                      else updated.delete("id");
                      setParams(updated, { replace: true });
                    }}
                    className="grid w-full grid-cols-[130px_1fr_120px_120px_110px] items-center gap-3 border-b border-reco-line-2 px-4 py-2.5 text-left text-[12.5px] last:border-b-0 hover:bg-reco-panel-2"
                  >
                    <span className="font-mono text-reco-t1">{row.invoice_no}</span>
                    <div>
                      <div className="text-reco-t1">{row.supplier_name ?? "—"}</div>
                      <div className="font-mono text-[10px] text-reco-t5">
                        {row.supplier_gstin ?? ""}
                      </div>
                    </div>
                    <span className="text-right font-mono text-reco-t2">
                      {row.books_amount != null ? formatRupees(row.books_amount) : "—"}
                    </span>
                    <span className="text-right font-mono text-reco-t2">
                      {row.portal_amount != null ? formatRupees(row.portal_amount) : "—"}
                    </span>
                    <span className="text-right font-mono text-reco-bad">
                      {formatRupees(row.exposure)}
                    </span>
                  </button>
                ))}
              </div>
            ))}
          </div>
        </section>
      ))}

      {groups.length === 0 && (
        <div className="rounded border border-reco-line px-4 py-8 text-center text-[12.5px] text-reco-t4">
          Nothing needs attention in this period.
        </div>
      )}

      {/* Case detail in the drawer rather than expanded in the row: the row
          grew the list every time one was opened, so the next case a reviewer
          wanted moved down the page as they worked. */}
      <DetailDrawer
        open={selected !== null}
        title={selected?.title ?? selected?.reason_code ?? ""}
        subtitle={selected ? `${selected.invoice_no} · ${selected.supplier_name ?? ""}` : undefined}
        onClose={() => {
          setOpenCase(null);
          const updated = new URLSearchParams(params);
          updated.delete("id");
          setParams(updated, { replace: true });
        }}
      >
        {selected && (
          <div className="space-y-4">
            <dl className="grid grid-cols-2 gap-x-3 gap-y-2 text-[12px]">
              <Figure label="Books" value={selected.books_amount} />
              <Figure label="GSTR-2B" value={selected.portal_amount} />
              <Figure label="Difference" value={selected.exposure} tone="bad" />
            </dl>

            {selected.narrative && (
              <p className="text-[12.5px] leading-relaxed text-reco-t2">{selected.narrative}</p>
            )}

            {selected.next_action && (
              <div className="rounded border border-reco-line bg-reco-panel-2 p-3">
                <div className="mb-1 font-mono text-[9.5px] uppercase tracking-wider text-reco-t5">
                  What to do
                </div>
                <p className="text-[12px] leading-relaxed text-reco-t2">{selected.next_action}</p>
              </div>
            )}

            {clientId && periodId && (
              <>
                <ExplainCase clientId={clientId} periodId={periodId} caseId={selected.id} />
                <div>
                  <div className="mb-1.5 font-mono text-[9.5px] uppercase tracking-wider text-reco-t5">
                    Neighbourhood
                  </div>
                  <CaseExplorer clientId={clientId} periodId={periodId} caseId={selected.id} />
                </div>
              </>
            )}
          </div>
        )}
      </DetailDrawer>
    </div>
  );
}

function Figure({
  label,
  value,
  tone,
}: {
  readonly label: string;
  readonly value: number | null | undefined;
  readonly tone?: "bad";
}) {
  return (
    <div>
      <dt className="font-mono text-[9.5px] uppercase tracking-wider text-reco-t5">{label}</dt>
      <dd className={`font-mono text-[13px] ${tone === "bad" ? "text-reco-bad" : "text-reco-t1"}`}>
        {value == null ? "—" : formatRupees(value)}
      </dd>
    </div>
  );
}
