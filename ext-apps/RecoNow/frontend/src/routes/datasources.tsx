import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import { fetchDatasets, type DatasetSummary } from "../lib/api";
import type { CellData, RowData, KpiItem } from "../lib/screenConfigs";
import type { WorkspaceState } from "../lib/workspace";

function toRows(datasets: readonly DatasetSummary[]): readonly RowData[] {
  return datasets.map((d) => ({
    cells: [
      { t: d.kind.toUpperCase() },
      { t: d.name },
      { t: String(d.total_rows), font: "mono" as const },
      { t: d.confirmed ? "Confirmed" : "Pending", color: d.confirmed ? "#2f6b4d" : "#a86a2c" },
    ] satisfies readonly CellData[],
  }));
}

function toKpis(datasets: readonly DatasetSummary[]): readonly KpiItem[] {
  const confirmed = datasets.filter((d) => d.confirmed).length;
  const totalRows = datasets.reduce((sum, d) => sum + d.total_rows, 0);
  return [
    { label: "DATASETS", value: String(datasets.length), sub: "uploaded", color: "#1c1b18" },
    { label: "CONFIRMED", value: String(confirmed), sub: "mapping locked", color: "#2f6b4d" },
    { label: "TOTAL ROWS", value: String(totalRows), sub: "across all datasets", color: "#1c1b18" },
    { label: "STATUS", value: confirmed === datasets.length && datasets.length > 0 ? "READY" : "IN PROGRESS", sub: confirmed === datasets.length && datasets.length > 0 ? "all mappings confirmed" : "awaiting mapping", color: confirmed === datasets.length && datasets.length > 0 ? "#2f6b4d" : "#a86a2c" },
  ];
}

export default function DatasourcesRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [rows, setRows] = useState<readonly RowData[] | undefined>(undefined);
  const [kpis, setKpis] = useState<readonly KpiItem[] | undefined>(undefined);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchDatasets(clientId, periodId)
      .then((d) => { setRows(toRows(d)); setKpis(toKpis(d)); })
      .catch(() => { setRows(undefined); setKpis(undefined); })
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  // Headings describe the cells this route builds — see liveCols.
  const cols = ["KIND", "FILE", "ROWS", "STATE"] as const;
  const grid = "120px 1.6fr 90px 120px";

  return <GenericScreen liveCols={cols} liveGrid={grid} config={screenConfig("datasources")} liveRows={rows} liveKpis={kpis} loading={loading} />;
}
