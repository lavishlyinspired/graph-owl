import { useState } from "react";
import { askQuestion, type AskResult } from "../lib/api";
import { strings } from "../lib/strings";

interface AskPanelProps {
  readonly clientId: string;
  readonly periodId: string;
  readonly onClose: () => void;
}

export function AskPanel({ clientId, periodId, onClose }: AskPanelProps) {
  const [question, setQuestion] = useState("");
  const [result, setResult] = useState<AskResult | null>(null);
  const [busy, setBusy] = useState(false);

  const runAsk = async () => {
    if (!question.trim()) return;
    setBusy(true);
    try {
      const answer = await askQuestion(clientId, periodId, question.trim());
      setResult(answer);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="border-b border-reco-line bg-reco-panel px-5 pt-4.5 pb-5">
      <div className="mx-auto max-w-[900px]">
        <div className="mb-3.5 flex h-[38px] items-center gap-2.5 rounded-lg border border-reco-line-3 bg-reco-panel-2 px-3.5">
          <span className="text-reco-t5">⌕</span>
          <input
            autoFocus
            value={question}
            onChange={(e) => setQuestion(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && runAsk()}
            placeholder={strings.askPlaceholder}
            className="flex-1 bg-transparent text-[13px] text-reco-t1 outline-none"
          />
          <button type="button" onClick={onClose} className="text-[11.5px] text-reco-t4">
            {strings.askCloseLabel}
          </button>
        </div>

        {result && (
          <div className="grid grid-cols-[1fr_260px] gap-4">
            <div>
              <div className="mb-3 text-[13px] leading-relaxed text-reco-t1">{result.answer}</div>
              {!result.grounded && (
                <button
                  type="button"
                  onClick={runAsk}
                  disabled={busy}
                  className="rounded-lg bg-reco-t0 px-3 py-1.5 text-[12px] text-white"
                >
                  Try again
                </button>
              )}
            </div>
            <div className="rounded-lg border border-reco-accent-border bg-reco-accent-bg px-3.5 py-3">
              <div className="mb-2 font-mono text-[9.5px] tracking-[0.12em] text-reco-accent-hi">
                {strings.askAnsweredFrom}
              </div>
              {result.citations.map((c) => (
                <div key={c} className="border-b border-reco-accent-border py-1.5 text-[11.5px] text-reco-t2 last:border-b-0">
                  {c}
                </div>
              ))}
              <div className="mt-2 text-[10.5px] leading-relaxed text-reco-t4">
                No sentence is shown without a source. Questions it cannot ground return "not enough evidence".
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
