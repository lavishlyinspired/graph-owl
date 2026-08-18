import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData, KpiItem } from "../lib/screenConfigs";
import { fetchEligibility, type EligibilityRow } from "../lib/api";

export default function EligibilityRoute() {
  const { clientId, periodId } = useOutletContext<{ clientId: string; periodId: string }>();
  const [rows, setRows] = useState<readonly EligibilityRow[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchEligibility(clientId, periodId)
      .then(setRows)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const tableRows: readonly RowData[] = rows.map((r) => ({
    cells: [
      { t: r.invoice_no },
      { t: r.name ?? r.gstin, font: "mono" as const },
      { t: r.eligibility, color: r.eligibility === "eligible" ? "green" : "red" },
      { t: `₹${r.books_amount.toLocaleString()}` },
      { t: `₹${r.portal_amount.toLocaleString()}` },
    ],
  }));

  const eligible = rows.filter((r) => r.eligibility === "eligible").length;
  const kpis: readonly KpiItem[] = [
    { label: "Total Invoices", value: String(rows.length), sub: "", color: "blue" },
    { label: "Eligible", value: String(eligible), sub: "", color: "green" },
    { label: "Not Eligible", value: String(rows.length - eligible), sub: "", color: "red" },
    { label: "Eligibility Rate", value: rows.length ? `${((eligible / rows.length) * 100).toFixed(0)}%` : "—", sub: "", color: eligible > rows.length / 2 ? "green" : "red" },
  ];

  return <GenericScreen config={screenConfig("eligibility")} liveRows={tableRows} liveKpis={kpis} loading={loading} />;
}
