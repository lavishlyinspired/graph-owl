import { useEffect, useState } from "react";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData, KpiItem } from "../lib/screenConfigs";
import { fetchResetStatus, type ResetStatus } from "../lib/api";

export default function ResetRoute() {
  const [status, setStatus] = useState<ResetStatus | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    fetchResetStatus()
      .then(setStatus)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, []);

  const rows: readonly RowData[] = status
    ? [
        { cells: [{ t: "Clients" }, { t: String(status.clients) }] },
        { cells: [{ t: "Periods" }, { t: String(status.periods) }] },
        { cells: [{ t: "Cases" }, { t: String(status.cases) }] },
        { cells: [{ t: "Approvals" }, { t: String(status.approvals) }] },
        { cells: [{ t: "Users" }, { t: String(status.users) }] },
      ]
    : [];

  const kpis: readonly KpiItem[] = status
    ? [
        { label: "Clients", value: String(status.clients), sub: "", color: "blue" },
        { label: "Cases", value: String(status.cases), sub: "", color: "amber" },
        { label: "Approvals", value: String(status.approvals), sub: "", color: "green" },
        { label: "Users", value: String(status.users), sub: "", color: "blue" },
      ]
    : [];

  // Headings describe the cells this route builds — see liveCols.
  const cols = ["ENTITY", "COUNT"] as const;
  const grid = "1.4fr 140px";

  return <GenericScreen liveCols={cols} liveGrid={grid} config={screenConfig("reset")} liveRows={rows} liveKpis={kpis} loading={loading} />;
}
