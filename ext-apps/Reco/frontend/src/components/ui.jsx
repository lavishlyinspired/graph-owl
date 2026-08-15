import { X } from "lucide-react";
import { statusColor, statusLabel } from "../format.js";

export function Button({ children, variant = "primary", className = "", ...props }) {
  const styles = {
    primary:
      "bg-matcha-green text-matcha-bg hover:bg-matcha-green/90 disabled:bg-matcha-bg-tertiary disabled:text-matcha-text-tertiary disabled:cursor-not-allowed",
    outline:
      "border border-matcha-border text-matcha-text-primary hover:bg-matcha-bg-secondary disabled:text-matcha-text-tertiary disabled:cursor-not-allowed",
    ghost:
      "text-matcha-text-secondary hover:text-matcha-text-primary hover:bg-matcha-bg-secondary disabled:text-matcha-text-tertiary disabled:cursor-not-allowed",
  };
  return (
    <button
      className={`inline-flex items-center justify-center gap-2 rounded-lg px-4 py-2 text-sm font-medium transition-colors ${styles[variant]} ${className}`}
      {...props}
    >
      {children}
    </button>
  );
}

export function Card({ children, className = "" }) {
  return (
    <div className={`bg-matcha-bg-secondary border border-matcha-border rounded-xl ${className}`}>
      {children}
    </div>
  );
}

export function StatusPill({ status, small = false }) {
  return (
    <span
      className={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium ${statusColor(status)} ${small ? "" : ""}`}
    >
      {statusLabel(status)}
    </span>
  );
}

export function Modal({ open, title, onClose, children }) {
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div className="absolute inset-0 bg-black/60" onClick={onClose} />
      <div className="relative w-full max-w-md bg-matcha-bg-secondary border border-matcha-border rounded-2xl p-6 shadow-2xl">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold">{title}</h2>
          <button onClick={onClose} className="text-matcha-text-tertiary hover:text-matcha-text-primary">
            <X size={18} />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

export function Tabs({ tabs, active, onChange }) {
  return (
    <div className="flex items-center gap-1 border-b border-matcha-border">
      {tabs.map((tab) => (
        <button
          key={tab.key}
          onClick={() => onChange(tab.key)}
          className={[
            "px-3 py-2 text-sm font-medium -mb-px border-b-2 transition-colors",
            active === tab.key
              ? "text-matcha-green border-matcha-green"
              : "text-matcha-text-tertiary border-transparent hover:text-matcha-text-secondary",
          ].join(" ")}
        >
          {tab.label}
        </button>
      ))}
    </div>
  );
}

export function LoadingScreen() {
  return (
    <div className="min-h-screen flex items-center justify-center bg-matcha-bg">
      <svg
        className="animate-spin text-matcha-green"
        width="24"
        height="24"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d="M21 12a9 9 0 1 1-6.219-8.56" />
      </svg>
    </div>
  );
}
