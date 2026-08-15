import { ChevronDown } from "lucide-react";

const MONTHS = [
  "January", "February", "March", "April", "May", "June",
  "July", "August", "September", "October", "November", "December",
];
const YEARS = [2024, 2025, 2026];

export default function PeriodPicker({ month, year, onChange }) {
  return (
    <div className="flex items-center gap-2">
      <label className="relative">
        <select
          value={month}
          onChange={(e) => onChange(e.target.value, year)}
          className="appearance-none bg-matcha-bg-secondary border border-matcha-border rounded-lg pl-3 pr-8 py-1.5 text-sm text-matcha-text-primary focus:outline-none focus:border-matcha-green cursor-pointer"
        >
          {MONTHS.map((m) => (
            <option key={m} value={m}>{m}</option>
          ))}
        </select>
        <ChevronDown size={14} className="absolute right-2 top-1/2 -translate-y-1/2 pointer-events-none text-matcha-text-tertiary" />
      </label>
      <label className="relative">
        <select
          value={year}
          onChange={(e) => onChange(month, Number(e.target.value))}
          className="appearance-none bg-matcha-bg-secondary border border-matcha-border rounded-lg pl-3 pr-8 py-1.5 text-sm text-matcha-text-primary focus:outline-none focus:border-matcha-green cursor-pointer"
        >
          {YEARS.map((y) => (
            <option key={y} value={y}>{y}</option>
          ))}
        </select>
        <ChevronDown size={14} className="absolute right-2 top-1/2 -translate-y-1/2 pointer-events-none text-matcha-text-tertiary" />
      </label>
    </div>
  );
}
