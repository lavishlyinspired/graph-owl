import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData, KpiItem } from "../lib/screenConfigs";
import { fetchDeliverables, type Deliverable } from "../lib/api";

export default function DeliverablesPanel() {
  const { clientId, periodId } = useOutletContext<{ clientId: string; periodId: string }>();
  const [rows, setRows] = useState<readonly Deliverable[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchDeliverables(clientId, periodId)
      .then(setRows)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const tableRows: readonly RowData[] = rows.map((d) => ({
    cells: [
      { t: d.kind },
      { t: d.status, color: d.status === "drafted" ? "amber" : "green" },
      { t: new Date(d.generated_at).toLocaleDateString() },
    ],
  }));

  const kpis: readonly KpiItem[] = [
    { label: "Total Deliverables", value: String(rows.length), sub: "", color: "blue" },
    { label: "Drafted", value: String(rows.filter((d) => d.status === "drafted").length), sub: "", color: "amber" },
    { label: "Final", value: String(rows.filter((d) => d.status === "final").length), sub: "", color: "green" },
    { label: "Export Ready", value: String(rows.filter((d) => d.status !== "drafted").length), sub: "", color: "green" },
  ];

  // Headings describe the cells this route builds, not the mockup's
  // own column shape — see GenericScreen's liveCols.
  const cols = ["DELIVERABLE", "STATUS", "GENERATED"] as const;
  const grid = "1.5fr 120px 140px";

  return <GenericScreen liveCols={cols} liveGrid={grid} config={screenConfig("deliverables")} liveRows={tableRows} liveKpis={kpis} loading={loading} />;
}
