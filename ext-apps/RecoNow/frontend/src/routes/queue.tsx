import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import { fetchRegister, type RegisterRow } from "../lib/api";
import type { CellData, RowData, KpiItem } from "../lib/screenConfigs";
import type { WorkspaceState } from "../lib/workspace";

function formatRupees(amount: number | null): string {
  if (amount == null) return "—";
  return `₹${amount.toLocaleString("en-IN", { maximumFractionDigits: 0 })}`;
}

function toRows(rows: readonly RegisterRow[]): readonly RowData[] {
  return rows.map((r) => ({
    cells: [
      { t: r.invoice_no },
      { t: r.supplier_name ?? "—", sub: r.supplier_gstin ?? undefined },
      { t: r.reason_code ?? "—", color: r.reason_code ? "#a86a2c" : "#2f6b4d" },
      { t: formatRupees(r.exposure), font: "mono" as const, color: r.exposure > 0 ? "#a13f28" : "#2f6b4d" },
    ] satisfies readonly CellData[],
  }));
}

function toKpis(rows: readonly RegisterRow[]): readonly KpiItem[] {
  const open = rows.filter((r) => r.status === "open").length;
  const totalExposure = rows.reduce((sum, r) => sum + r.exposure, 0);
  const reasons = new Set(rows.filter((r) => r.reason_code).map((r) => r.reason_code));
  return [
    { label: "OPEN CASES", value: String(open), sub: "awaiting decision", color: "#a86a2c" },
    { label: "TOTAL EXPOSURE", value: formatRupees(totalExposure), sub: "ITC at risk", color: "#a13f28" },
    { label: "REASON CODES", value: String(reasons.size), sub: "distinct reasons", color: "#1c1b18" },
    { label: "INVOICES", value: String(rows.length), sub: "in register", color: "#1c1b18" },
  ];
}

export default function QueueRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [rows, setRows] = useState<readonly RowData[] | undefined>(undefined);
  const [kpis, setKpis] = useState<readonly KpiItem[] | undefined>(undefined);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchRegister(clientId, periodId)
      .then((r) => { setRows(toRows(r.rows)); setKpis(toKpis(r.rows)); })
      .catch(() => { setRows(undefined); setKpis(undefined); })
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  return <GenericScreen config={screenConfig("queue")} liveRows={rows} liveKpis={kpis} loading={loading} />;
}
