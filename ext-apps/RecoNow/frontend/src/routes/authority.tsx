import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData } from "../lib/screenConfigs";
import { fetchAuthority, type AuthorityRow } from "../lib/api";

export default function AuthorityRoute() {
  const { clientId, periodId } = useOutletContext<{ clientId: string; periodId: string }>();
  const [rows, setRows] = useState<readonly AuthorityRow[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchAuthority(clientId, periodId)
      .then(setRows)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const tableRows: readonly RowData[] = rows.map((r) => ({
    cells: [
      { t: r.authority },
      { t: String(r.case_count) },
      { t: `₹${r.exposure.toLocaleString()}` },
    ],
  }));

  return <GenericScreen config={screenConfig("authority")} liveRows={tableRows} loading={loading} />;
}
