import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData, KpiItem } from "../lib/screenConfigs";
import { formatRupees } from "../lib/format";
import { fetchDashboard } from "../lib/api";
import { fetchAuthority, type AuthorityRow } from "../lib/api";

export default function AuthorityRoute() {
  const { clientId, periodId } = useOutletContext<{ clientId: string; periodId: string }>();
  const [rows, setRows] = useState<readonly AuthorityRow[]>([]);
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
    fetchAuthority(clientId, periodId)
      .then(setRows)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const tableRows: readonly RowData[] = rows.map((r) => ({
    cells: [
      { t: r.authority },
      { t: String(r.case_count) },
      { t: `₹${r.exposure.toLocaleString()}` },
    ],
  }));


  // Totals derived from the same rows rendered below, so a KPI cannot
  // disagree with its own table.
  const rows_ = rows;
  const kpis: readonly KpiItem[] = rows_.length
    ? [
        { label: "PROVISIONS", value: String(rows_.length), sub: "cited this period", color: "#1c1b18" },
        { label: "CASES", value: String(rows_.reduce((s, r) => s + r.case_count, 0)), sub: "governed by a provision", color: "#a86a2c" },
        { label: "PERIOD EXPOSURE", value: formatRupees(periodTotal), sub: "each invoice counted once", color: "#a13f28" },
      ]
    : [];

  // Headings describe the cells this route builds, not the mockup's
  // own column shape — see GenericScreen's liveCols.
  const cols = ["PROVISION", "CASES", "EXPOSURE"] as const;
  const grid = "1.6fr 90px 140px";

  return <GenericScreen liveCols={cols} liveGrid={grid} config={screenConfig("authority")} liveRows={tableRows} liveKpis={kpis} loading={loading} />;
}
