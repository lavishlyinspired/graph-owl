import { useEffect, useMemo, useState } from "react";
import { useOutletContext } from "react-router-dom";
import { fetchWorkingPaper, type WorkingPaper, type WorkingPaperLine } from "../lib/api";
import { formatRupees } from "../lib/format";
import { WhyPopover } from "../components/WhyPopover";
import type { FigureExplanation } from "../lib/api";
import { loadStateFor } from "../lib/loadState";
import { GeneratedBadge } from "../components/GeneratedBadge";
import { fetchWorkingPaperReport, type WorkingPaperReport } from "../lib/api";
import type { WorkspaceState } from "../lib/workspace";

const DIRECTION: Record<
  WorkingPaper["filed"]["direction"],
  { readonly headline: string; readonly note: string; readonly tone: string }
> = {
  excess: {
    headline: "Claimed more than the portal supports",
    note: "Interest under s.50 and a demand under s.73/74 follow an excess claim. This is the direction that costs money now.",
    tone: "text-reco-red",
  },
  unclaimed: {
    headline: "Credit available and not claimed",
    note: "Recoverable, until s.16(4) closes the window for this period.",
    tone: "text-reco-amber",
  },
  agrees: {
    headline: "Table 4A agrees with the GSTR-2B",
    note: "The portal's figure and the filed figure are the same number.",
    tone: "text-reco-green",
  },
  not_evaluated: {
    headline: "Not checked against a filed return",
    note: "No GSTR-3B was uploaded for this period, so nothing is being asserted either way.",
    tone: "text-reco-t4",
  },
};

export default function WorkingPaperRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [paper, setPaper] = useState<WorkingPaper | null>(null);
  const [loading, setLoading] = useState(true);
  const [writeUp, setWriteUp] = useState<WorkingPaperReport | null>(null);
  const [writing, setWriting] = useState(false);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchWorkingPaper(clientId, periodId)
      .then(setPaper)
      .catch(() => setPaper(null))
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const filed = paper?.filed;
  const direction = useMemo(
    () => (filed ? DIRECTION[filed.direction] : null),
    [filed],
  );

  const state = loadStateFor({ clientId, periodId, loading, data: paper });
  if (state === "no-workspace")
    return (
      <div className="p-6 text-[13px] text-reco-t4">
        Choose a client and a period to build a working paper.
      </div>
    );
  if (state === "loading") return <div className="p-6 text-[13px] text-reco-t4">Loading…</div>;
  if (state === "empty" || !paper)
    return (
      <div className="p-6 text-[13px] text-reco-t4">
        No data for this period yet — upload a GSTR-2B to start the chain.
      </div>
    );

  return (
    <div className="space-y-6 p-6">
      <header className="flex items-start justify-between gap-4">
        <div>
        <h1 className="text-[19px] font-medium text-reco-t1">GSTR-3B working paper</h1>
        <p className="mt-1 text-[13px] text-reco-t4">
          Every figure below names where it came from. A number a reviewer cannot trace is
          one they have to take on trust.
        </p>
        </div>
        {/* The document a partner or an officer receives. A working paper that
            cannot leave the screen is not a working paper. */}
        <button
          type="button"
          disabled={writing}
          onClick={() => {
            if (!clientId || !periodId) return;
            setWriting(true);
            fetchWorkingPaperReport(clientId, periodId)
              .then(setWriteUp)
              .catch(() => setWriteUp(null))
              .finally(() => setWriting(false));
          }}
          className="shrink-0 rounded border border-reco-line px-3 py-1.5 text-[12px] text-reco-t2 hover:border-reco-accent hover:text-reco-accent disabled:opacity-50"
        >
          {writing ? "Writing up…" : "Write up & download"}
        </button>
      </header>

      {writeUp && (
        <section className="rounded border border-reco-line p-4">
          <div className="mb-3 flex items-center gap-2">
            <GeneratedBadge source={writeUp.source} />
            <span className="flex-1" />
            <button
              type="button"
              onClick={() => {
                const blob = new Blob([writeUp.report], { type: "text/plain;charset=utf-8" });
                const url = URL.createObjectURL(blob);
                const link = document.createElement("a");
                link.href = url;
                link.download = writeUp.filename;
                link.click();
                URL.revokeObjectURL(url);
              }}
              className="rounded border border-reco-line px-3 py-1 text-[11.5px] text-reco-t2 hover:border-reco-accent hover:text-reco-accent"
            >
              Download {writeUp.filename}
            </button>
          </div>
          {writeUp.note && (
            <p className="mb-2 text-[11.5px] leading-relaxed text-reco-t4">
              {writeUp.note}
              {writeUp.refusal && <span className="mt-1 block">Refused: {writeUp.refusal}</span>}
            </p>
          )}
          <pre className="overflow-x-auto whitespace-pre-wrap text-[11.5px] leading-relaxed text-reco-t2">
            {writeUp.report}
          </pre>
        </section>
      )}

      {!paper.complete && (
        <p className="rounded border-2 border-reco-amber/40 bg-reco-amber/5 px-4 py-3 text-[12px] leading-relaxed text-reco-t2">
          <strong className="text-reco-amber">This position is not final.</strong> At least one
          deduction below was found but could not be sized, so the net figure is an{" "}
          <em>upper bound</em> on what is claimable, not the answer.
        </p>
      )}

      <section className="overflow-x-auto rounded border border-reco-line">
        <table className="w-full min-w-[680px] text-[13px]">
          <thead>
            <tr className="border-b border-reco-line text-[10.5px] uppercase tracking-wider text-reco-t5">
              <th className="px-4 py-2 text-left font-normal">Figure</th>
              <th className="px-4 py-2 text-right font-normal">Amount</th>
              <th className="px-4 py-2 text-left font-normal">Source</th>
              <th className="px-4 py-2 text-left font-normal">Provision</th>
            </tr>
          </thead>
          <tbody>
            {paper.lines.map((line) => (
              <Line key={line.key} line={line} explain={paper.explain} />
            ))}
          </tbody>
        </table>
      </section>

      {paper.unmodelled.length > 0 && (
        <section className="rounded border-2 border-reco-amber/40 bg-reco-amber/5 p-4">
          <h2 className="text-[12px] font-medium uppercase tracking-wider text-reco-amber">
            Findings this paper has no line for
          </h2>
          <p className="mt-1 text-[12px] leading-relaxed text-reco-t3">
            These are <em>not</em> deducted above. Deducting them silently would attribute a
            reduction to a provision nobody chose; dropping them silently would make the net
            figure overstate what is claimable. They are listed so the decision stays yours.
          </p>
          <ul className="mt-3 space-y-1">
            {paper.unmodelled.map((entry) => (
              <li key={entry.label} className="flex justify-between font-mono text-[12px]">
                <span className="text-reco-t2">{entry.label}</span>
                <span className="text-reco-t3">{formatRupees(entry.amount)}</span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {paper.compare_note && (
        <p className="rounded border border-reco-line bg-reco-panel-2 px-4 py-3 text-[12px] leading-relaxed text-reco-t3">
          {paper.compare_note}
        </p>
      )}

      {filed && direction && (
        <section className="rounded border border-reco-line p-4">
          <h2 className="text-[10.5px] uppercase tracking-wider text-reco-t5">
            Against the filed return
          </h2>
          <p className={`mt-2 text-[15px] ${direction.tone}`}>
            {direction.headline}
            {filed.difference !== null && filed.difference > 0 && (
              <span className="ml-2 font-mono">{formatRupees(filed.difference)}</span>
            )}
          </p>
          <p className="mt-1 text-[12px] leading-relaxed text-reco-t4">{direction.note}</p>

          {filed.direction !== "not_evaluated" && (
            <dl className="mt-4 grid grid-cols-2 gap-x-6 gap-y-1 text-[12px] sm:grid-cols-4">
              <Figure label="Available (2B)" value={filed.available_2b} />
              <Figure label="Table 4A claimed" value={filed.gross_claimed} />
              <Figure label="Table 4B reversed" value={filed.reversed} />
              <Figure label="Table 4C net" value={filed.net_claimed} />
            </dl>
          )}

          {filed.arithmetic_ok === false && (
            <p className="mt-3 text-[12px] text-reco-red">
              The filed return does not satisfy its own arithmetic — 4C is not 4A less 4B.
              Every figure downstream of it is unreliable until that is resolved.
            </p>
          )}
          {filed.needs && (
            <p className="mt-3 text-[12px] text-reco-t4">Needs: {filed.needs}</p>
          )}
        </section>
      )}
    </div>
  );
}

function Line({
  line,
  explain,
}: {
  readonly line: WorkingPaperLine;
  readonly explain: Record<string, FigureExplanation> | undefined;
}) {
  const emphasis =
    line.kind === "closing"
      ? "border-t-2 border-reco-line font-medium text-reco-t1"
      : line.kind === "opening"
        ? "text-reco-t1"
        : "text-reco-t2";

  return (
    <tr className={`border-b border-reco-line/60 ${emphasis}`}>
      <td className="px-4 py-2">
        {line.kind === "deduction" ? `less  ${line.label}` : line.label}
        <WhyPopover title={line.label} explanation={explain?.[line.key]} />
      </td>
      <td className="px-4 py-2 text-right font-mono">
        {line.kind === "deduction" && line.amount > 0 ? "−" : ""}
        {formatRupees(line.amount)}
        {line.unquantified > 0 && (
          <span
            className="ml-2 text-[11px] text-reco-amber"
            title={`${line.unquantified} finding(s) on this line carry no amount, so this figure is understated`}
          >
            +{line.unquantified} unsized
          </span>
        )}
      </td>
      <td className="px-4 py-2 text-[11.5px] text-reco-t4">{line.source}</td>
      <td className="px-4 py-2 text-[11.5px] text-reco-t4">{line.citation ?? ""}</td>
    </tr>
  );
}

function Figure({ label, value }: { readonly label: string; readonly value: number | null }) {
  return (
    <div>
      <dt className="text-[10.5px] uppercase tracking-wider text-reco-t5">{label}</dt>
      <dd className="font-mono text-reco-t2">{value === null ? "—" : formatRupees(value)}</dd>
    </div>
  );
}
