import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData, KpiItem } from "../lib/screenConfigs";
import { fetchAtRisk, type AtRiskSupplier } from "../lib/api";

export default function AtRiskRoute() {
  const { clientId, periodId } = useOutletContext<{ clientId: string; periodId: string }>();
  const [rows, setRows] = useState<readonly AtRiskSupplier[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchAtRisk(clientId, periodId)
      .then(setRows)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const tableRows: readonly RowData[] = rows.map((r) => ({
    cells: [
      { t: r.gstin, font: "mono" as const },
      { t: r.name ?? "—" },
      { t: `₹${r.at_risk_amount.toLocaleString()}` },
      { t: String(r.case_count) },
    ],
  }));

  const totalAtRisk = rows.reduce((sum, r) => sum + r.at_risk_amount, 0);
  const totalCases = rows.reduce((sum, r) => sum + r.case_count, 0);
  const kpis: readonly KpiItem[] = [
    { label: "Total at risk", value: `₹${totalAtRisk.toLocaleString()}`, sub: "", color: "red" },
    { label: "Suppliers affected", value: String(rows.length), sub: "", color: "blue" },
    { label: "Cases", value: String(totalCases), sub: "", color: "amber" },
    { label: "Pending", value: String(rows.reduce((s, r) => s + r.case_count, 0)), sub: "", color: "red" },
  ];

  // Headings describe the cells this route builds — see liveCols.
  const cols = ["GSTIN", "SUPPLIER", "AT RISK", "CASES"] as const;
  const grid = "1.1fr 1.3fr 140px 90px";

  return <GenericScreen liveCols={cols} liveGrid={grid} config={screenConfig("atrisk")} liveRows={tableRows} liveKpis={kpis} loading={loading} />;
}
