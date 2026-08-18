import { useEffect, useRef, useState } from "react";
import { createPeriod, fetchPeriods, type Period } from "../lib/api";
import { strings } from "../lib/strings";
import { useClickOutside } from "../lib/useClickOutside";

interface PeriodPickerProps {
  readonly clientId: string | null;
  readonly periodId: string | null;
  readonly onSelect: (periodId: string) => void;
}

export function PeriodPicker({ clientId, periodId, onSelect }: PeriodPickerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [periods, setPeriods] = useState<readonly Period[]>([]);
  useClickOutside(containerRef, () => setOpen(false), open);
  const [creating, setCreating] = useState(false);
  const [month, setMonth] = useState("");
  const [year, setYear] = useState("");

  const refresh = (id: string) => {
    fetchPeriods(id)
      .then(setPeriods)
      .catch(() => setPeriods([]));
  };

  useEffect(() => {
    if (clientId) refresh(clientId);
    else setPeriods([]);
  }, [clientId]);

  const current = periods.find((p) => p.id === periodId);

  const runCreate = async () => {
    if (!clientId || !month.trim() || !year.trim()) return;
    const created = await createPeriod(clientId, { month: month.trim(), year: Number(year) });
    setMonth("");
    setYear("");
    setCreating(false);
    refresh(clientId);
    onSelect(created.id);
  };

  if (!clientId) return null;

  return (
    <div className="relative" ref={containerRef}>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1.5 rounded-md border border-reco-line px-2.5 py-[5px] text-[12px] text-reco-t2 hover:border-reco-line-3"
      >
        <span className="font-semibold">{current ? `${current.month} ${current.year}` : "Select a period"}</span>
        <span className="text-reco-t5">▾</span>
      </button>

      {open && (
        <div className="absolute top-[38px] left-0 z-[60] w-[260px] overflow-hidden rounded-lg border border-reco-line-3 bg-reco-panel shadow-2xl">
          {periods.map((p) => (
            <button
              key={p.id}
              type="button"
              onClick={() => {
                onSelect(p.id);
                setOpen(false);
              }}
              className="block w-full border-b border-reco-line-2 px-3.5 py-2 text-left text-[12.5px] text-reco-t1 hover:bg-reco-panel-2"
            >
              {p.month} {p.year} <span className="ml-1 font-mono text-[10px] text-reco-t4">{p.status}</span>
            </button>
          ))}
          {creating ? (
            <div className="flex flex-col gap-1.5 p-3">
              <input
                value={month}
                onChange={(e) => setMonth(e.target.value)}
                placeholder={strings.newPeriodMonth}
                className="rounded border border-reco-line px-2 py-1 text-[12px]"
              />
              <input
                value={year}
                onChange={(e) => setYear(e.target.value)}
                placeholder={strings.newPeriodYear}
                className="rounded border border-reco-line px-2 py-1 font-mono text-[12px]"
              />
              <button
                type="button"
                onClick={runCreate}
                className="rounded-md bg-reco-t0 px-2 py-1.5 text-[12px] font-medium text-white"
              >
                {strings.addPeriod}
              </button>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setCreating(true)}
              className="block w-full px-3.5 py-2 text-left text-[12px] text-reco-accent hover:bg-reco-panel-2"
            >
              + {strings.addPeriod}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
