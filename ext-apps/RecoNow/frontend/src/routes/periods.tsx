import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import { fetchPeriods, type Period } from "../lib/api";
import type { CellData, RowData, KpiItem } from "../lib/screenConfigs";
import type { WorkspaceState } from "../lib/workspace";

function toRows(periods: readonly Period[]): readonly RowData[] {
  return periods.map((p) => ({
    cells: [
      { t: p.month + " " + p.year, font: "sans" as const },
      { t: p.status, color: p.status === "open" ? "#a86a2c" : "#2f6b4d" },
      { t: "—" },
    ] satisfies readonly CellData[],
  }));
}

function toKpis(periods: readonly Period[]): readonly KpiItem[] {
  const open = periods.filter((p) => p.status === "open").length;
  const closed = periods.length - open;
  return [
    { label: "TOTAL PERIODS", value: String(periods.length), sub: "across all months", color: "#1c1b18" },
    { label: "OPEN", value: String(open), sub: "awaiting close", color: "#a86a2c" },
    { label: "CLOSED", value: String(closed), sub: "completed", color: "#2f6b4d" },
    { label: "STATUS", value: open > 0 ? "ACTIVE" : "ALL CLEAR", sub: open > 0 ? "open periods remain" : "no open periods", color: open > 0 ? "#a86a2c" : "#2f6b4d" },
  ];
}

export default function PeriodsRoute() {
  const { clientId } = useOutletContext<WorkspaceState>();
  const [rows, setRows] = useState<readonly RowData[] | undefined>(undefined);
  const [kpis, setKpis] = useState<readonly KpiItem[] | undefined>(undefined);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!clientId) return;
    setLoading(true);
    fetchPeriods(clientId)
      .then((periods) => { setRows(toRows(periods)); setKpis(toKpis(periods)); })
      .catch(() => { setRows(undefined); setKpis(undefined); })
      .finally(() => setLoading(false));
  }, [clientId]);

  return <GenericScreen config={screenConfig("periods")} liveRows={rows} liveKpis={kpis} loading={loading} />;
}
