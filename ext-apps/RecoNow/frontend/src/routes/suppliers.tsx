import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData } from "../lib/screenConfigs";
import { fetchSuppliers, type SupplierSummary } from "../lib/api";

export default function SuppliersRoute() {
  const { clientId, periodId } = useOutletContext<{ clientId: string; periodId: string }>();
  const [suppliers, setSuppliers] = useState<readonly SupplierSummary[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchSuppliers(clientId, periodId)
      .then(setSuppliers)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const rows: readonly RowData[] = suppliers.map((s) => ({
    cells: [
      { t: s.gstin, font: "mono" as const },
      { t: s.name ?? "—" },
      { t: String(s.case_count) },
      { t: `₹${s.total_exposure.toLocaleString()}` },
      { t: String(s.pending_count), color: s.pending_count > 0 ? "amber" : undefined },
    ],
  }));

  return <GenericScreen config={screenConfig("suppliers")} liveRows={rows} loading={loading} />;
}
