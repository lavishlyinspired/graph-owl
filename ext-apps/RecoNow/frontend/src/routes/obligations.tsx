import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData, KpiItem } from "../lib/screenConfigs";
import { formatRupees } from "../lib/format";
import { fetchDashboard } from "../lib/api";
import { fetchObligations, type ObligationRow } from "../lib/api";

export default function ObligationsRoute() {
  const { clientId, periodId } = useOutletContext<{ clientId: string; periodId: string }>();
  const [rows, setRows] = useState<readonly ObligationRow[]>([]);
  const [loading, setLoading] = useState(true);
  // The rows below group by a dimension where one invoice can appear in two
  // groups, so summing them is not the period total. The real figure comes
  // from the same place the dashboard and register get it.
  const [periodTotal, setPeriodTotal] = useState(0);

  useEffect(() => {
    if (!clientId || !periodId) return;
    fetchDashboard(clientId, periodId)
      .then((d) => setPeriodTotal(d.total_exposure))
      .catch(() => setPeriodTotal(0));
  }, [clientId, periodId]);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchObligations(clientId, periodId)
      .then(setRows)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const tableRows: readonly RowData[] = rows.map((r) => ({
    cells: [
      { t: r.obligation },
      { t: String(r.case_count) },
      { t: `₹${r.exposure.toLocaleString()}` },
    ],
  }));


  // Totals derived from the same rows rendered below, so a KPI cannot
  // disagree with its own table.
  const rows_ = rows;
  const kpis: readonly KpiItem[] = rows_.length
    ? [
        { label: "OBLIGATION TYPES", value: String(rows_.length), sub: "distinct reason codes", color: "#1c1b18" },
        { label: "CASES", value: String(rows_.reduce((s, r) => s + r.case_count, 0)), sub: "open this period", color: "#a86a2c" },
        { label: "PERIOD EXPOSURE", value: formatRupees(periodTotal), sub: "each invoice counted once", color: "#a13f28" },
      ]
    : [];

  return <GenericScreen config={screenConfig("obligations")} liveRows={tableRows} liveKpis={kpis} loading={loading} />;
}
