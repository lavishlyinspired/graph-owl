import { useRef, useState } from "react";
import {
  UploadCloud,
  FolderOpen,
  FileSpreadsheet,
  History,
  BookOpen,
  Loader2,
  Sparkles,
  AlertCircle,
} from "lucide-react";
import { Button, Card } from "../components/ui.jsx";
import { api } from "../api.js";

export default function UploadPage({ onDataLoaded }) {
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const [tolerance, setTolerance] = useState(1);
  const fileRef = useRef(null);
  const folderRef = useRef(null);

  const handleFiles = async (files) => {
    if (!files || files.length === 0) return;
    setBusy(true);
    setError(null);
    try {
      const result = await api.upload(files);
      if (!result.ok) {
        setError(result.error || "Upload failed");
        setBusy(false);
        return;
      }
      onDataLoaded(result);
    } catch (err) {
      setError(err.message || "Upload failed");
      setBusy(false);
    }
  };

  const handleSample = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await api.sample();
      onDataLoaded(result);
    } catch (err) {
      setError(err.message || "Failed to load sample data");
      setBusy(false);
    }
  };

  const dropProps = {
    onDragOver: (e) => {
      e.preventDefault();
      setDragging(true);
    },
    onDragLeave: () => setDragging(false),
    onDrop: (e) => {
      e.preventDefault();
      setDragging(false);
      handleFiles(e.dataTransfer.files);
    },
  };

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">Drop Your Data</h1>
        <p className="text-matcha-text-secondary mt-2">
          Upload your Books (purchase register) and GSTR-2B files below
        </p>
        <p className="text-matcha-text-tertiary text-sm mt-1">
          Drag and drop into the box below, or click to browse. AI will auto-detect and map columns.
        </p>
      </div>

      {/* Upload drop zone */}
      <Card
        className={`p-8 border-2 border-dashed transition-colors cursor-pointer ${
          dragging ? "border-matcha-accent bg-matcha-accent-surface/50" : "border-matcha-border hover:border-matcha-accent/40"
        }`}
        {...dropProps}
        onClick={() => fileRef.current?.click()}
      >
        <div className="flex flex-col items-center text-center py-6">
          <div className={`w-16 h-16 rounded-2xl flex items-center justify-center mb-5 transition-colors ${
            dragging ? "bg-matcha-accent/20" : "bg-matcha-bg"
          }`}>
            <UploadCloud size={32} className="text-matcha-accent" />
          </div>
          <p className="text-lg font-semibold">Drop your files here</p>
          <p className="text-matcha-text-tertiary mt-1 mb-2">or click anywhere in this box to browse</p>
          <p className="text-sm text-matcha-text-secondary mb-6">
            Supports: Excel (.xlsx, .xls), CSV, JSON — Books & GSTR-2B
          </p>
          <div className="flex items-center gap-3" onClick={(e) => e.stopPropagation()}>
            <Button
              onClick={() => fileRef.current?.click()}
              disabled={busy}
            >
              {busy ? <Loader2 size={16} className="animate-spin" /> : <FileSpreadsheet size={16} />}
              {busy ? "Processing…" : "Choose Files"}
            </Button>
            <Button variant="outline" onClick={() => folderRef.current?.click()} disabled={busy}>
              <FolderOpen size={16} />
              Upload Entire Folder
            </Button>
          </div>
          <input
            ref={fileRef}
            type="file"
            multiple
            hidden
            accept=".csv,.xlsx,.xls,.json"
            onChange={(e) => handleFiles(e.target.files)}
          />
          <input
            ref={folderRef}
            type="file"
            multiple
            hidden
            webkitdirectory=""
            onChange={(e) => handleFiles(e.target.files)}
          />
        </div>
      </Card>

      {error && (
        <div className="flex items-center gap-2 text-sm text-matcha-red bg-matcha-red/10 border border-matcha-red/30 rounded-lg px-4 py-3">
          <AlertCircle size={16} />
          {error}
        </div>
      )}

      {/* Bottom cards */}
      <div className="grid md:grid-cols-2 gap-6">
        <Card className="p-5">
          <div className="flex items-center gap-2 mb-1">
            <History size={16} className="text-matcha-amber" />
            <h3 className="font-semibold">Prior Period Excess</h3>
          </div>
          <p className="text-sm text-matcha-text-tertiary mb-4">
            Upload prior period GSTR-2B files to reconcile B2BA cross-period credits.
          </p>
          <Button variant="outline" className="w-full" disabled>
            Upload Prior Period Files
          </Button>
        </Card>

        <Card className="p-5">
          <div className="flex items-center gap-2 mb-1">
            <BookOpen size={16} className="text-matcha-blue" />
            <h3 className="font-semibold">B2B History Knowledge Bank</h3>
          </div>
          <p className="text-sm text-matcha-text-tertiary mb-4">
            Upload historic GSTR-2B files for automated B2BA cross-period lookup.
          </p>
          <Button variant="outline" className="w-full" disabled>
            Upload 2B Files
          </Button>
        </Card>
      </div>

      <Card className="p-5 flex flex-wrap items-center justify-between gap-4">
        <div>
          <h3 className="font-semibold">Mismatch Tolerance</h3>
          <p className="text-sm text-matcha-text-tertiary">
            Differences below this amount (₹) are treated as matched
          </p>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-sm text-matcha-text-secondary font-mono">₹</span>
          <input
            type="number"
            min="0"
            step="1"
            value={tolerance}
            onChange={(e) => setTolerance(Number(e.target.value))}
            className="w-24 bg-matcha-bg border border-matcha-border rounded-lg px-3 py-2 text-sm font-mono focus:outline-none focus:border-matcha-accent"
          />
        </div>
      </Card>

      <div className="flex flex-wrap items-center gap-3">
        <Button variant="outline" onClick={handleSample} disabled={busy}>
          {busy ? <Loader2 size={16} className="animate-spin" /> : <Sparkles size={16} />}
          Try Sample Data
        </Button>
        <span className="text-sm text-matcha-text-tertiary">
          — or drop your own files above to start
        </span>
      </div>
    </div>
  );
}
