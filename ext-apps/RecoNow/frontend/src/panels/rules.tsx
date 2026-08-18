import { useEffect, useState } from "react";
import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";
import type { RowData, KpiItem } from "../lib/screenConfigs";
import { fetchRules, type Rule } from "../lib/api";

export default function RulesRoute() {
  const [rows, setRows] = useState<readonly Rule[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    fetchRules()
      .then(setRows)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, []);

  const tableRows: readonly RowData[] = rows.map((r) => ({
    cells: [
      { t: r.code, font: "mono" as const },
      { t: r.name },
      { t: r.severity, color: r.severity === "high" ? "red" : r.severity === "medium" ? "amber" : "green" },
      { t: r.enabled ? "On" : "Off", color: r.enabled ? "green" : "red" },
      { t: String(r.case_count) },
    ],
  }));

  const kpis: readonly KpiItem[] = [
    { label: "Total Rules", value: String(rows.length), sub: "", color: "blue" },
    { label: "Active", value: String(rows.filter((r) => r.enabled).length), sub: "", color: "green" },
    { label: "High Severity", value: String(rows.filter((r) => r.severity === "high").length), sub: "", color: "red" },
    { label: "Total Matches", value: String(rows.reduce((s, r) => s + r.case_count, 0)), sub: "", color: "amber" },
  ];

  // Headings describe the cells this route builds, not the mockup's
  // own column shape — see GenericScreen's liveCols.
  const cols = ["CODE", "RULE", "SEVERITY", "STATE", "CASES"] as const;
  const grid = "1fr 1.5fr 110px 90px 90px";

  return <GenericScreen liveCols={cols} liveGrid={grid} config={screenConfig("rules")} liveRows={tableRows} liveKpis={kpis} loading={loading} />;
}
