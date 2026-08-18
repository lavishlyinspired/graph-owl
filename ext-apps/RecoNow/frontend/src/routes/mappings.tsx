import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData, KpiItem } from "../lib/screenConfigs";
import { fetchMappings, type MappingRecord } from "../lib/api";

export default function MappingsRoute() {
  const { clientId, periodId } = useOutletContext<{ clientId: string; periodId: string }>();
  const [rows, setRows] = useState<readonly MappingRecord[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchMappings(clientId, periodId)
      .then(setRows)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const tableRows: readonly RowData[] = rows.map((m) => {
    const mapped = Object.values(m.mapping).filter((v) => v !== null).length;
    const total = Object.keys(m.mapping).length;
    return {
      cells: [
        { t: m.dataset_kind },
        { t: `${mapped}/${total}` },
        { t: `${(m.tolerance * 100).toFixed(0)}%` },
        { t: new Date(m.updated_at).toLocaleDateString() },
      ],
    };
  });

  const kpis: readonly KpiItem[] = [
    { label: "Mappings", value: String(rows.length), sub: "", color: "blue" },
    { label: "Avg Tolerance", value: rows.length ? `${(rows.reduce((s, r) => s + r.tolerance, 0) / rows.length * 100).toFixed(0)}%` : "—", sub: "", color: "green" },
  ];

  // Headings describe the cells this route builds, not the mockup's
  // own column shape — see GenericScreen's liveCols.
  const cols = ["DATASET", "FIELDS MAPPED", "TOLERANCE", "UPDATED"] as const;
  const grid = "1.2fr 130px 110px 130px";

  return <GenericScreen liveCols={cols} liveGrid={grid} config={screenConfig("mappings")} liveRows={tableRows} liveKpis={kpis} loading={loading} />;
}
