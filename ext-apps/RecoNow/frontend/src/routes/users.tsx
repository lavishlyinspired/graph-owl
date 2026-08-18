import { useEffect, useState } from "react";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData, KpiItem } from "../lib/screenConfigs";
import { fetchUsers, type User } from "../lib/api";

export default function UsersRoute() {
  const [rows, setRows] = useState<readonly User[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    fetchUsers()
      .then(setRows)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, []);

  const tableRows: readonly RowData[] = rows.map((u) => ({
    cells: [
      { t: u.name },
      { t: u.email, font: "mono" as const },
      { t: u.role },
      { t: String(u.assigned_cases) },
    ],
  }));

  const kpis: readonly KpiItem[] = [
    { label: "Total Users", value: String(rows.length), sub: "", color: "blue" },
    { label: "Preparers", value: String(rows.filter((u) => u.role === "preparer").length), sub: "", color: "green" },
    { label: "Reviewers", value: String(rows.filter((u) => u.role === "reviewer").length), sub: "", color: "amber" },
    { label: "Admins", value: String(rows.filter((u) => u.role === "admin").length), sub: "", color: "red" },
  ];

  return <GenericScreen config={screenConfig("users")} liveRows={tableRows} liveKpis={kpis} loading={loading} />;
}
