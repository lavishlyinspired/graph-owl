import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData, KpiItem } from "../lib/screenConfigs";
import { fetchCrossPeriod, type CrossPeriodRow } from "../lib/api";

export default function CrossPeriodRoute() {
  const { clientId, periodId } = useOutletContext<{ clientId: string; periodId: string }>();
  const [rows, setRows] = useState<readonly CrossPeriodRow[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchCrossPeriod(clientId, periodId)
      .then(setRows)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const tableRows: readonly RowData[] = rows.map((r) => ({
    cells: [
      { t: r.period },
      { t: r.gstin, font: "mono" as const },
      { t: r.name ?? "—" },
      { t: String(r.case_count) },
      { t: `₹${r.exposure.toLocaleString()}` },
    ],
  }));

  const kpis: readonly KpiItem[] = rows.length
    ? [
        { label: "Periods Compared", value: String(new Set(rows.map((r) => r.period_id)).size), sub: "", color: "blue" },
        { label: "Suppliers", value: String(new Set(rows.map((r) => r.gstin)).size), sub: "", color: "green" },
        { label: "Total Exposure", value: `₹${rows.reduce((s, r) => s + r.exposure, 0).toLocaleString()}`, sub: "", color: "amber" },
      ]
    : [];

  return <GenericScreen config={screenConfig("crossperiod")} liveRows={tableRows} liveKpis={kpis} loading={loading} />;
}
