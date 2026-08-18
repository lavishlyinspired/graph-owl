import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData } from "../lib/screenConfigs";
import { fetchObligations, type ObligationRow } from "../lib/api";

export default function ObligationsRoute() {
  const { clientId, periodId } = useOutletContext<{ clientId: string; periodId: string }>();
  const [rows, setRows] = useState<readonly ObligationRow[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchObligations(clientId, periodId)
      .then(setRows)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const tableRows: readonly RowData[] = rows.map((r) => ({
    cells: [
      { t: r.obligation },
      { t: String(r.case_count) },
      { t: `₹${r.exposure.toLocaleString()}` },
    ],
  }));

  return <GenericScreen config={screenConfig("obligations")} liveRows={tableRows} loading={loading} />;
}
