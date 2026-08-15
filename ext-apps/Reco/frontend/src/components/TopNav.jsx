import { RotateCcw } from "lucide-react";

const STEPS = [
  { key: "upload", label: "Upload" },
  { key: "map", label: "Map" },
  { key: "reconcile", label: "Reconcile" },
  { key: "intelligence", label: "Intelligence" },
  { key: "act", label: "Act" },
];

const STEP_HINTS = {
  upload: "You're on Upload",
  map: "You're on Map",
  reconcile: "You're on Reconcile",
  intelligence: "You're on Intelligence",
  act: "You're on Act",
};

const PAGE_HINTS = {
  upload: "Use the upload box below to add your files",
  map: "Map your columns in the editor below",
  reconcile: "Review matches and mismatches below",
  intelligence: "View insights and analysis below",
  act: "Download reports and take action below",
};

export default function TopNav({ page, hasData, disabled, onNavigate, onRestart }) {
  return (
    <header className="border-b border-matcha-border bg-matcha-bg sticky top-0 z-40">
      <div className="max-w-7xl mx-auto px-4 sm:px-6">
        <div className="h-14 flex items-center gap-4">
          <button
            onClick={onRestart}
            className="flex items-center gap-2 text-matcha-green hover:text-matcha-green/80 font-semibold tracking-tight"
          >
            <span className="text-lg">🔄</span>
            <span className="text-xl">RecoNow</span>
            <RotateCcw size={14} className="text-matcha-text-tertiary" />
          </button>

          <nav className="flex items-center gap-1 ml-2 flex-1">
            {STEPS.map((step) => {
              const isCurrent = page === step.key;
              const isDisabled = step.key !== "upload" && !hasData;
              return (
                <button
                  key={step.key}
                  onClick={() => onNavigate(step.key)}
                  disabled={isDisabled}
                  className={[
                    "px-3 py-1.5 rounded-lg text-sm font-medium transition-colors",
                    isCurrent
                      ? "bg-matcha-green-surface text-matcha-green"
                      : isDisabled
                        ? "text-matcha-text-tertiary cursor-not-allowed"
                        : "text-matcha-text-secondary hover:text-matcha-text-primary hover:bg-matcha-bg-secondary",
                  ].join(" ")}
                >
                  {step.label}
                </button>
              );
            })}
          </nav>
        </div>

        <div className="pb-3 flex flex-wrap items-center gap-x-4 gap-y-1">
          <h1 className="text-sm font-semibold text-matcha-text-primary">
            {STEP_HINTS[page]}
          </h1>
          <span className="text-sm text-matcha-text-tertiary">{PAGE_HINTS[page]}</span>
        </div>
      </div>
    </header>
  );
}
