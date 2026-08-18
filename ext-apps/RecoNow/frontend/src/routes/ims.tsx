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
      { t: r.supplier_name ?? "—" },
      { t: r.reason_code ?? "—", color: "#a86a2c" },
      { t: formatRupees(r.exposure), font: "mono" as const },
    ] satisfies readonly CellData[],
  }));
}

function toKpis(rows: readonly RegisterRow[]): readonly KpiItem[] {
  const totalExposure = rows.reduce((sum, r) => sum + r.exposure, 0);
  return [
    { label: "IMS RECORDS", value: String(rows.length), sub: "pending decisions", color: "#a86a2c" },
    { label: "TOTAL EXPOSURE", value: formatRupees(totalExposure), sub: "at risk if ignored", color: "#a13f28" },
    { label: "DEEMED AT 3B", value: String(rows.filter((r) => r.status === "open").length), sub: "will auto-accept", color: "#a86a2c" },
    { label: "STATUS", value: "PENDING", sub: "action required", color: "#a86a2c" },
  ];
}

export default function ImsRoute() {
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

  // Headings describe the cells this route builds, not the mockup's
  // own column shape — see GenericScreen's liveCols.
  const cols = ["INVOICE", "SUPPLIER", "REASON", "EXPOSURE"] as const;
  const grid = "1fr 1.3fr 1.2fr 130px";

  return <GenericScreen liveCols={cols} liveGrid={grid} config={screenConfig("ims")} liveRows={rows} liveKpis={kpis} loading={loading} />;
}
