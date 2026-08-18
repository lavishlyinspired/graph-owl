import { useEffect, useRef, useState } from "react";
import { createClient, fetchClients, type Client } from "../lib/api";
import { strings } from "../lib/strings";
import { useClickOutside } from "../lib/useClickOutside";

interface ClientSwitcherProps {
  readonly clientId: string | null;
  readonly onSelect: (clientId: string) => void;
}

export function ClientSwitcher({ clientId, onSelect }: ClientSwitcherProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [clients, setClients] = useState<readonly Client[]>([]);
  useClickOutside(containerRef, () => setOpen(false), open);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [gstin, setGstin] = useState("");
  const [state, setState] = useState("");

  const refresh = () => {
    fetchClients()
      .then(setClients)
      .catch(() => setClients([]));
  };

  useEffect(refresh, []);

  const current = clients.find((c) => c.id === clientId);

  const runCreate = async () => {
    if (!name.trim() || !gstin.trim() || !state.trim()) return;
    const created = await createClient({ name: name.trim(), gstin: gstin.trim(), state: state.trim() });
    setName("");
    setGstin("");
    setState("");
    setCreating(false);
    refresh();
    onSelect(created.id);
  };

  return (
    <div className="relative" ref={containerRef}>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-2.5 rounded-md border border-reco-line px-2.5 py-1 hover:border-reco-line-3"
      >
        <div className="flex flex-col items-start gap-0.5">
          <span className="text-[12.5px] font-semibold text-reco-t1">{current?.name ?? "Select a client"}</span>
          {current && (
            <span className="font-mono text-[10px] text-reco-t4">
              {current.gstin} · {current.state}
            </span>
          )}
        </div>
        <span className="text-[11px] text-reco-t5">▾</span>
      </button>

      {open && (
        <div className="absolute top-[44px] left-0 z-[60] w-[320px] overflow-hidden rounded-lg border border-reco-line-3 bg-reco-panel shadow-2xl">
          <div className="border-b border-reco-line-2 px-3.5 py-2.5 font-mono text-[9px] tracking-[0.14em] text-reco-t4">
            {strings.clientPickerHint}
          </div>
          {clients.map((c) => (
            <button
              key={c.id}
              type="button"
              onClick={() => {
                onSelect(c.id);
                setOpen(false);
              }}
              className="block w-full border-b border-reco-line-2 px-3.5 py-2.5 text-left hover:bg-reco-panel-2"
            >
              <div className="text-[12.5px] font-medium text-reco-t1">{c.name}</div>
              <div className="mt-0.5 font-mono text-[10px] text-reco-t4">
                {c.gstin} · {c.state}
              </div>
            </button>
          ))}
          {creating ? (
            <div className="flex flex-col gap-1.5 border-b border-reco-line-2 p-3">
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={strings.newClientName}
                className="rounded border border-reco-line px-2 py-1 text-[12px]"
              />
              <input
                value={gstin}
                onChange={(e) => setGstin(e.target.value)}
                placeholder={strings.newClientGstin}
                className="rounded border border-reco-line px-2 py-1 font-mono text-[12px]"
              />
              <input
                value={state}
                onChange={(e) => setState(e.target.value)}
                placeholder={strings.newClientState}
                className="rounded border border-reco-line px-2 py-1 text-[12px]"
              />
              <button
                type="button"
                onClick={runCreate}
                className="rounded-md bg-reco-t0 px-2 py-1.5 text-[12px] font-medium text-white"
              >
                {strings.addClient}
              </button>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setCreating(true)}
              className="block w-full border-b border-reco-line-2 px-3.5 py-2.5 text-left text-[12px] text-reco-accent hover:bg-reco-panel-2"
            >
              + {strings.addClient}
            </button>
          )}
          <div className="px-3.5 py-2.5 text-[11px] text-reco-t4">{strings.clientIsolationCopy}</div>
        </div>
      )}
    </div>
  );
}
