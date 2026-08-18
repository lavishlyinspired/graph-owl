import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import { fetchClients, type Client } from "../lib/api";
import type { CellData, RowData, KpiItem } from "../lib/screenConfigs";
import type { WorkspaceState } from "../lib/workspace";

function toRows(clients: readonly Client[]): readonly RowData[] {
  return clients.map((c) => ({
    cells: [
      { t: c.name },
      { t: c.gstin, font: "mono" as const },
      { t: c.state },
    ] satisfies readonly CellData[],
  }));
}

function toKpis(clients: readonly Client[]): readonly KpiItem[] {
  const states = new Set(clients.map((c) => c.state));
  return [
    { label: "GSTINs", value: String(clients.length), sub: "registered", color: "#1c1b18" },
    { label: "STATES", value: String(states.size), sub: "distinct", color: "#1c1b18" },
    { label: "STATUS", value: clients.length > 0 ? "ACTIVE" : "NONE", sub: clients.length > 0 ? "clients registered" : "no clients", color: clients.length > 0 ? "#2f6b4d" : "#a86a2c" },
    { label: "FILING", value: "OWN BOOKS", sub: "calendar basis", color: "#1c1b18" },
  ];
}

export default function GstinsRoute() {
  const { clientId } = useOutletContext<WorkspaceState>();
  const [rows, setRows] = useState<readonly RowData[] | undefined>(undefined);
  const [kpis, setKpis] = useState<readonly KpiItem[] | undefined>(undefined);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    setLoading(true);
    fetchClients()
      .then((c) => { setRows(toRows(c)); setKpis(toKpis(c)); })
      .catch(() => { setRows(undefined); setKpis(undefined); })
      .finally(() => setLoading(false));
  }, [clientId]);

  // Headings describe the cells this route builds, not the mockup's
  // own column shape — see GenericScreen's liveCols.
  const cols = ["CLIENT", "GSTIN", "STATE"] as const;
  const grid = "1.3fr 1.2fr 1fr";

  return <GenericScreen liveCols={cols} liveGrid={grid} config={screenConfig("gstins")} liveRows={rows} liveKpis={kpis} loading={loading} />;
}
