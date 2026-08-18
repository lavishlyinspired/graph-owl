import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData } from "../lib/screenConfigs";
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

  return <GenericScreen config={screenConfig("followups")} liveRows={tableRows} loading={loading} />;
}
