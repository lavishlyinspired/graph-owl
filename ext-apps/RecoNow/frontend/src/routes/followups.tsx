import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData, KpiItem } from "../lib/screenConfigs";
import { formatRupees } from "../lib/format";
import { fetchFollowUps, type FollowUp } from "../lib/api";

export default function FollowupsRoute() {
  const { clientId, periodId } = useOutletContext<{ clientId: string; periodId: string }>();
  const [rows, setRows] = useState<readonly FollowUp[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchFollowUps(clientId, periodId)
      .then(setRows)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const tableRows: readonly RowData[] = rows.map((f) => ({
    cells: [
      { t: f.invoice_no },
      { t: f.supplier_name ?? "—" },
      { t: f.reason_code ?? "—" },
      { t: `₹${f.exposure.toLocaleString()}` },
      { t: f.status, color: f.status === "pending" ? "amber" : "green" },
      { t: f.subject ?? f.summary?.slice(0, 60) ?? "—" },
    ],
  }));


  // Totals derived from the same rows rendered below, so a KPI cannot
  // disagree with its own table.
  const rows_ = rows;
  const kpis: readonly KpiItem[] = rows_.length
    ? [
        { label: "FOLLOW-UPS", value: String(rows_.length), sub: "cases needing contact", color: "#1c1b18" },
        { label: "SUPPLIERS", value: String(new Set(rows_.map((r) => r.supplier_name ?? "?")).size), sub: "to contact", color: "#41508f" },
        { label: "EXPOSURE", value: formatRupees(rows_.reduce((s, r) => s + r.exposure, 0)), sub: "recoverable if resolved", color: "#a13f28" },
      ]
    : [];

  // Headings describe the cells this route builds, not the mockup's
  // own column shape — see GenericScreen's liveCols.
  const cols = ["INVOICE", "SUPPLIER", "REASON", "EXPOSURE", "STATUS", "SUBJECT"] as const;
  const grid = "0.9fr 1.1fr 1.1fr 120px 90px 1.3fr";

  return <GenericScreen liveCols={cols} liveGrid={grid} config={screenConfig("followups")} liveRows={tableRows} liveKpis={kpis} loading={loading} />;
}
