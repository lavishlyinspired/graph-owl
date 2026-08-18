import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData, KpiItem } from "../lib/screenConfigs";
import { fetchItcPosition, type ItcPosition } from "../lib/api";

export default function ItcRoute() {
  const { clientId, periodId } = useOutletContext<{ clientId: string; periodId: string }>();
  const [itc, setItc] = useState<ItcPosition | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchItcPosition(clientId, periodId)
      .then(setItc)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const rows: readonly RowData[] = itc
    ? [
        { cells: [{ t: "Books Amount" }, { t: `₹${itc.books_amount.toLocaleString()}` }] },
        { cells: [{ t: "Portal Amount" }, { t: `₹${itc.portal_amount.toLocaleString()}` }] },
        { cells: [{ t: "Net Exposure" }, { t: `₹${itc.exposure.toLocaleString()}`, color: itc.exposure > 0 ? "red" : undefined }] },
      ]
    : [];

  const kpis: readonly KpiItem[] = itc
    ? [
        { label: "Total Cases", value: String(itc.case_count), sub: "", color: "blue" },
        { label: "Pending Review", value: String(itc.pending_count), sub: "", color: "amber" },
        { label: "Net Exposure", value: `₹${itc.exposure.toLocaleString()}`, sub: "", color: "red" },
        { label: "Books Total", value: `₹${itc.books_amount.toLocaleString()}`, sub: "", color: "green" },
      ]
    : [];

  return <GenericScreen config={screenConfig("itc")} liveRows={rows} liveKpis={kpis} loading={loading} />;
}
