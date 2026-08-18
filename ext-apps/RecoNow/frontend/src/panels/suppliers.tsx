import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData, KpiItem } from "../lib/screenConfigs";
import { formatRupees } from "../lib/format";
import { fetchSuppliers, type SupplierSummary } from "../lib/api";

export default function SuppliersRoute() {
  const { clientId, periodId } = useOutletContext<{ clientId: string; periodId: string }>();
  const [suppliers, setSuppliers] = useState<readonly SupplierSummary[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchSuppliers(clientId, periodId)
      .then(setSuppliers)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const rows: readonly RowData[] = suppliers.map((s) => ({
    cells: [
      { t: s.gstin, font: "mono" as const },
      { t: s.name ?? "—" },
      { t: String(s.case_count) },
      { t: formatRupees(s.total_exposure) },
      { t: String(s.pending_count), color: s.pending_count > 0 ? "amber" : undefined },
    ],
  }));

  // Derived from the same rows the table shows, so a KPI cannot disagree
  // with the list beneath it.
  const kpis: readonly KpiItem[] = suppliers.length
    ? [
        { label: "SUPPLIERS", value: String(suppliers.length), sub: "with cases this period", color: "#1c1b18" },
        {
          label: "WITH EXCEPTIONS",
          value: String(suppliers.filter((s) => s.case_count > 0).length),
          sub: "at least one case",
          color: "#a86a2c",
        },
        {
          label: "ITC AT RISK",
          value: formatRupees(suppliers.reduce((sum, s) => sum + s.total_exposure, 0)),
          sub: "across all suppliers",
          color: "#a13f28",
        },
        {
          label: "PENDING",
          value: String(suppliers.reduce((sum, s) => sum + s.pending_count, 0)),
          sub: "awaiting a decision",
          color: "#41508f",
        },
      ]
    : [];

  // Headings describe the cells this route builds, not the mockup's
  // own column shape — see GenericScreen's liveCols.
  const cols = ["GSTIN", "SUPPLIER", "CASES", "ITC AT RISK", "PENDING"] as const;
  const grid = "1.15fr 1.15fr 80px 130px 96px";

  return <GenericScreen liveCols={cols} liveGrid={grid} config={screenConfig("suppliers")} liveRows={rows} liveKpis={kpis} loading={loading} />;
}
