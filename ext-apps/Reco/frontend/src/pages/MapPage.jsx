import { useState } from "react";
import { Plus, Check, Eye, EyeOff } from "lucide-react";
import { Button, Card, Tabs } from "../components/ui.jsx";
import { api } from "../api.js";

const FIELDS = [
  { key: "invoice_no", label: "Invoice Number", required: true },
  { key: "supplier_gstin", label: "Supplier GSTIN", required: false },
  { key: "supplier_name", label: "Supplier Name", required: false },
  { key: "taxable", label: "Taxable Amount", required: true },
  { key: "invoice_date", label: "Invoice Date", required: false },
  { key: "place_of_supply", label: "Place of Supply", required: false },
  { key: "hsn", label: "HSN Code", required: false },
  { key: "ims_status", label: "IMS Status", required: false },
  { key: "reverse_charge", label: "Reverse Charge", required: false },
  { key: "note_type", label: "Note Type", required: false },
  { key: "voucher_type", label: "Voucher Type", required: false },
  { key: "original_invoice_no", label: "Original Invoice No", required: false },
  { key: "voucher_no", label: "Voucher No", required: false },
  { key: "igst", label: "IGST", required: false, multi: true },
  { key: "cgst", label: "CGST", required: false, multi: true },
  { key: "sgst", label: "SGST", required: false, multi: true },
  { key: "cess", label: "Cess", required: false, multi: true },
];

const COLUMN_LABELS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

function toColumnArray(value) {
  if (value === null || value === undefined) return [];
  if (Array.isArray(value)) return value.filter((v) => v !== null);
  if (value === "") return [];
  return [value];
}

export default function MapPage({ overview, onMapped, onBack }) {
  const datasets = overview.datasets || {};
  const kinds = Object.keys(datasets);
  const [active, setActive] = useState(kinds[0] || "books");
  const [mapping, setMapping] = useState(
    () =>
      Object.fromEntries(
        kinds.map((kind) => [
          kind,
          Object.fromEntries(
            FIELDS.map((f) => [f.key, toColumnArray(datasets[kind]?.mapping?.[f.key])])
          ),
        ])
      )
  );
  const [rowSel, setRowSel] = useState(() =>
    Object.fromEntries(kinds.map((kind) => [kind, 0]))
  );
  const [showRaw, setShowRaw] = useState(false);
  const [confirmed, setConfirmed] = useState({});

  const dataset = datasets[active];
  const previewRows = dataset?.preview || [];
  const headers = dataset?.headers || [];

  const mappedCount = Object.values(mapping[active] || {}).flat().length;
  const confirmedCount = kinds.filter((k) => confirmed[k]).length;
  const allConfirmed = kinds.length > 0 && confirmedCount === kinds.length;

  const setColumn = (fieldKey, index, column) => {
    setMapping((prev) => {
      const next = { ...prev };
      const list = [...(next[active][fieldKey] || [])];
      if (column === null) {
        list.splice(index, 1);
      } else {
        list[index] = column;
      }
      next[active] = { ...next[active], [fieldKey]: list };
      return next;
    });
  };

  const addColumn = (fieldKey) => {
    setMapping((prev) => {
      const next = { ...prev };
      next[active] = {
        ...next[active],
        [fieldKey]: [...(next[active][fieldKey] || []), null],
      };
      return next;
    });
  };

  const confirmDataset = (kind) => {
    setConfirmed((prev) => ({ ...prev, [kind]: true }));
  };

  const saveMapping = () => {
    const payload = {
      tolerance: overview.tolerance ?? 1,
      period: overview.period,
      mapping: Object.fromEntries(
        kinds.map((kind) => [
          kind,
          Object.fromEntries(
            Object.entries(mapping[kind]).map(([key, list]) => [
              key,
              list.length ? (list.length === 1 ? list[0] : list) : null,
            ])
          ),
        ])
      ),
    };
    return api.saveMapping(payload);
  };

  const handleConfirm = async () => {
    if (!active) return;
    confirmDataset(active);
  };

  const handleReconcile = async () => {
    if (!allConfirmed) return;
    await saveMapping();
    await onMapped();
  };

  const selectedColumns = new Set(
    Object.values(mapping[active] || {}).flat().filter((v) => v !== null)
  );

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Map Columns</h1>
        <p className="text-matcha-text-secondary mt-2">
          Confirm the column mapping for all {kinds.length} files ({confirmedCount}/{kinds.length} done) to proceed to reconciliation.
        </p>
        <p className="text-sm text-matcha-text-tertiary mt-1">
          AI mapped your columns. Review and confirm each dataset.
        </p>
      </div>

      {kinds.length > 1 && (
        <Tabs
          tabs={kinds.map((kind) => ({
            key: kind,
            label: datasets[kind].name,
          }))}
          active={active}
          onChange={setActive}
        />
      )}

      <Card className="overflow-hidden">
        <div className="flex items-center justify-between px-4 py-3 border-b border-matcha-border">
          <h3 className="font-semibold">{datasets[active]?.name}</h3>
          <button
            onClick={() => setShowRaw((v) => !v)}
            className="inline-flex items-center gap-1.5 text-sm text-matcha-text-secondary hover:text-matcha-text-primary"
          >
            {showRaw ? <EyeOff size={14} /> : <Eye size={14} />}
            Show Raw Data
          </button>
        </div>

        {showRaw && (
          <div className="overflow-x-auto border-b border-matcha-border">
            <table className="w-full text-xs">
              <thead>
                <tr className="bg-matcha-bg">
                  {headers.map((h) => (
                    <th key={h} className="px-3 py-2 text-left font-mono text-matcha-text-secondary whitespace-nowrap">
                      {h}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {previewRows.map((row, i) => (
                  <tr key={i} className="border-t border-matcha-border/50">
                    {headers.map((h) => (
                      <td key={h} className="px-3 py-1.5 font-mono text-matcha-text-primary whitespace-nowrap">
                        {String(row[h] ?? "")}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="bg-matcha-bg text-left text-xs text-matcha-text-tertiary uppercase tracking-wide">
                <th className="px-4 py-2.5 font-medium">Field</th>
                <th className="px-4 py-2.5 font-medium w-32">Row</th>
                <th className="px-4 py-2.5 font-medium w-72">Column</th>
                <th className="px-4 py-2.5 font-medium">Status</th>
              </tr>
            </thead>
            <tbody>
              {FIELDS.map((field) => {
                const list = mapping[active]?.[field.key] || [];
                return (
                  <tr key={field.key} className="border-t border-matcha-border/50">
                    <td className="px-4 py-2.5">
                      <span className="font-medium">
                        {field.label}
                        {field.required && <span className="text-matcha-red ml-0.5">*</span>}
                      </span>
                      {field.multi && (
                        <span className="ml-2 text-xs text-matcha-text-tertiary">(multi)</span>
                      )}
                    </td>
                    <td className="px-4 py-2.5">
                      <select
                        value={rowSel[active]}
                        onChange={(e) =>
                          setRowSel((prev) => ({ ...prev, [active]: Number(e.target.value) }))
                        }
                        className="bg-matcha-bg border border-matcha-border rounded-md px-2 py-1.5 text-xs focus:outline-none focus:border-matcha-green"
                      >
                        {previewRows.map((_, i) => (
                          <option key={i} value={i}>R{i + 1}</option>
                        ))}
                      </select>
                    </td>
                    <td className="px-4 py-2.5">
                      <div className="flex flex-col gap-1.5">
                        {list.map((column, idx) => (
                          <select
                            key={idx}
                            value={column ?? ""}
                            onChange={(e) =>
                              setColumn(
                                field.key,
                                idx,
                                e.target.value === "" ? null : Number(e.target.value)
                              )
                            }
                            className="bg-matcha-bg border border-matcha-border rounded-md px-2 py-1.5 text-xs focus:outline-none focus:border-matcha-green w-full"
                          >
                            <option value="">Not mapped</option>
                            {headers.map((header, ci) => (
                              <option key={ci} value={ci} disabled={selectedColumns.has(ci)}>
                                {COLUMN_LABELS[ci]}: {header}
                              </option>
                            ))}
                          </select>
                        ))}
                        {field.multi && (
                          <button
                            onClick={() => addColumn(field.key)}
                            className="inline-flex items-center gap-1 text-xs text-matcha-green hover:underline self-start"
                          >
                            <Plus size={12} /> Add column
                          </button>
                        )}
                      </div>
                    </td>
                    <td className="px-4 py-2.5 font-mono text-xs text-matcha-text-primary">
                      {list.map((column, idx) => {
                        const value = column !== null ? previewRows[rowSel[active]]?.[headers[column]] : null;
                        return (
                          <div key={idx} className="whitespace-nowrap">
                            {value === null || value === undefined || value === ""
                              ? <span className="text-matcha-text-tertiary">Not mapped</span>
                              : String(value)}
                          </div>
                        );
                      })}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>

        <div className="flex items-center justify-between px-4 py-3 border-t border-matcha-border">
          <span className="text-sm text-matcha-text-tertiary">
            {mappedCount} fields mapped · Data starts at row 2 ({dataset?.total_rows} rows)
          </span>
          <div className="flex items-center gap-3">
            <Button variant="ghost" onClick={onBack}>Back</Button>
            <Button
              variant="outline"
              onClick={handleConfirm}
              disabled={!active || confirmed[active]}
            >
              <Check size={16} />
              {confirmed[active] ? "Confirmed" : "Confirm Mapping"}
            </Button>
            <Button onClick={handleReconcile} disabled={!allConfirmed}>
              Reconcile
            </Button>
          </div>
        </div>
      </Card>
    </div>
  );
}
