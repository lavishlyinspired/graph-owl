import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData, KpiItem } from "../lib/screenConfigs";
import { fetchRisk, type RiskSupplier } from "../lib/api";

export default function RiskRoute() {
  const { clientId, periodId } = useOutletContext<{ clientId: string; periodId: string }>();
  const [rows, setRows] = useState<readonly RiskSupplier[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchRisk(clientId, periodId)
      .then(setRows)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const tableRows: readonly RowData[] = rows.map((r) => ({
    cells: [
      { t: r.gstin, font: "mono" as const },
      { t: r.name ?? "—" },
      { t: String(r.case_count) },
      { t: `₹${r.total_exposure.toLocaleString()}` },
      { t: `₹${r.max_exposure.toLocaleString()}` },
      { t: String(r.pending_count), color: r.pending_count > 0 ? "amber" : undefined },
    ],
  }));

  const kpis: readonly KpiItem[] = [
    { label: "Total at risk", value: `₹${rows.reduce((s, r) => s + r.total_exposure, 0).toLocaleString()}`, sub: "", color: "red" },
    { label: "Suppliers", value: String(rows.length), sub: "", color: "blue" },
    { label: "Cases", value: String(rows.reduce((s, r) => s + r.case_count, 0)), sub: "", color: "amber" },
    { label: "Max single exposure", value: `₹${Math.max(0, ...rows.map((r) => r.max_exposure)).toLocaleString()}`, sub: "", color: "red" },
  ];

  // Headings describe the cells this route builds, not the mockup's
  // own column shape — see GenericScreen's liveCols.
  const cols = ["GSTIN", "SUPPLIER", "CASES", "TOTAL EXPOSURE", "LARGEST", "PENDING"] as const;
  const grid = "1.1fr 1.2fr 80px 140px 120px 90px";

  return <GenericScreen liveCols={cols} liveGrid={grid} config={screenConfig("risk")} liveRows={tableRows} liveKpis={kpis} loading={loading} />;
}
