import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import { fetchApprovals, type Approval } from "../lib/api";
import type { CellData, RowData, KpiItem } from "../lib/screenConfigs";
import type { WorkspaceState } from "../lib/workspace";

function formatRupees(amount: number | null): string {
  if (amount == null) return "—";
  return `₹${amount.toLocaleString("en-IN", { maximumFractionDigits: 0 })}`;
}

function toRows(approvals: readonly Approval[]): readonly RowData[] {
  return approvals.map((a) => ({
    cells: [
      { t: a.decision_type },
      { t: formatRupees(a.amount), font: "mono" as const },
      { t: a.status, color: a.status === "pending" ? "#a86a2c" : a.status === "approved" ? "#2f6b4d" : "#a13f28" },
    ] satisfies readonly CellData[],
  }));
}

function toKpis(approvals: readonly Approval[]): readonly KpiItem[] {
  const pending = approvals.filter((a) => a.status === "pending").length;
  const approved = approvals.filter((a) => a.status === "approved").length;
  const total = approvals.reduce((sum, a) => sum + (a.amount ?? 0), 0);
  return [
    { label: "TOTAL", value: String(approvals.length), sub: "approval requests", color: "#1c1b18" },
    { label: "PENDING", value: String(pending), sub: "awaiting decision", color: "#a86a2c" },
    { label: "APPROVED", value: String(approved), sub: "completed", color: "#2f6b4d" },
    { label: "TOTAL VALUE", value: formatRupees(total), sub: "across all requests", color: "#1c1b18" },
  ];
}

export default function ApprovalsRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [rows, setRows] = useState<readonly RowData[] | undefined>(undefined);
  const [kpis, setKpis] = useState<readonly KpiItem[] | undefined>(undefined);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchApprovals(clientId, periodId)
      .then((a) => { setRows(toRows(a)); setKpis(toKpis(a)); })
      .catch(() => { setRows(undefined); setKpis(undefined); })
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  return <GenericScreen config={screenConfig("approvals")} liveRows={rows} liveKpis={kpis} loading={loading} />;
}
