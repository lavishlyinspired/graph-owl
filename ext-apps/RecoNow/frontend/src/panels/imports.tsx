import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData, KpiItem } from "../lib/screenConfigs";
import { fetchImports, type ImportRecord } from "../lib/api";

export default function ImportsRoute() {
  const { clientId, periodId } = useOutletContext<{ clientId: string; periodId: string }>();
  const [rows, setRows] = useState<readonly ImportRecord[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchImports(clientId, periodId)
      .then(setRows)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const tableRows: readonly RowData[] = rows.map((r) => ({
    cells: [
      { t: r.kind },
      { t: String(r.columns_mapped) },
      { t: `${(r.tolerance * 100).toFixed(0)}%` },
      { t: new Date(r.imported_at).toLocaleDateString() },
    ],
  }));

  const kpis: readonly KpiItem[] = [
    { label: "Datasets Imported", value: String(rows.length), sub: "", color: "blue" },
    { label: "Avg Tolerance", value: rows.length ? `${(rows.reduce((s, r) => s + r.tolerance, 0) / rows.length * 100).toFixed(0)}%` : "—", sub: "", color: "green" },
  ];

  // Headings describe the cells this route builds — see liveCols.
  const cols = ["DATASET", "COLUMNS MAPPED", "TOLERANCE", "IMPORTED"] as const;
  const grid = "1.2fr 140px 110px 130px";

  return <GenericScreen liveCols={cols} liveGrid={grid} config={screenConfig("imports")} liveRows={tableRows} liveKpis={kpis} loading={loading} />;
}
