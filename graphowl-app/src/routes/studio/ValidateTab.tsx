import { useState } from "react";

interface ValidationIssue {
  readonly id: string;
  readonly term: string;
  readonly severity: "error" | "warning" | "info";
  readonly rule: string;
  readonly message: string;
}

const MOCK_ISSUES: readonly ValidationIssue[] = [
  { id: "v1", term: "Vendor", severity: "error", rule: "definition-required", message: "Missing definition. Every glossary term must have a human-readable definition." },
  { id: "v2", term: "Amount Due", severity: "warning", rule: "duplicate-suspect", message: "Similar to 'Amount Payable'. Consider merging or adding a disambiguation note." },
  { id: "v3", term: "Source Record", severity: "warning", rule: "domain-mapping", message: "No domain class mapped. Assign a domain class to enable graph linking." },
  { id: "v4", term: "Filing Party", severity: "info", rule: "usage-check", message: "Used by 3 facts. Consider whether a broader alias would help discoverability." },
  { id: "v5", term: "Entity Resolution", severity: "error", rule: "definition-required", message: "Definition references 'sameAs' without explaining what that means in this context." },
  { id: "v6", term: "Dual Filing", severity: "info", rule: "cross-reference", message: "No cross-references to related terms. Link to 'Filing Party' and 'Duplicate Detection'." },
];

const SEVERITY_STYLES: Record<ValidationIssue["severity"], { bg: string; text: string; icon: string }> = {
  error: { bg: "bg-gowl-bad-bg", text: "text-gowl-bad", icon: "✕" },
  warning: { bg: "bg-gowl-amber-bg", text: "text-gowl-amber", icon: "!" },
  info: { bg: "bg-gowl-accent-deep", text: "text-gowl-accent", icon: "i" },
};

export function ValidateTab({ glossaryId: _glossaryId }: { readonly glossaryId: string }) {
  const [ran, setRan] = useState(false);
  const [issues, setIssues] = useState<readonly ValidationIssue[]>([]);

  const run = () => {
    setIssues(MOCK_ISSUES);
    setRan(true);
  };

  const errorCount = issues.filter((i) => i.severity === "error").length;
  const warnCount = issues.filter((i) => i.severity === "warning").length;
  const infoCount = issues.filter((i) => i.severity === "info").length;

  return (
    <div>
      <div className="mb-4 flex items-center justify-between">
        <div>
          {!ran ? (
            <p className="text-[14px] text-gowl-t5">Run validation to check the glossary against ontology rules and graph consistency.</p>
          ) : (
            <div className="flex gap-3 text-[13px]">
              <span className="text-gowl-bad">{errorCount} errors</span>
              <span className="text-gowl-amber">{warnCount} warnings</span>
              <span className="text-gowl-accent">{infoCount} suggestions</span>
            </div>
          )}
        </div>
        <button
          type="button"
          onClick={run}
          className="rounded-md bg-gowl-accent px-4 py-1.5 text-[13.5px] font-semibold text-gowl-accent-on"
        >
          {ran ? "Re-run" : "Run validation"}
        </button>
      </div>

      {ran && (
        <div className="space-y-2">
          {issues.length === 0 ? (
            <div className="rounded-md border border-gowl-ok-border bg-gowl-ok-bg p-6 text-center">
              <div className="text-[14.5px] font-semibold text-gowl-ok">All checks passed</div>
              <div className="mt-1 text-[13px] text-gowl-t5">No issues found in this glossary.</div>
            </div>
          ) : (
            issues.map((issue) => {
              const ss = SEVERITY_STYLES[issue.severity];
              return (
                <div key={issue.id} className="flex items-start gap-3 rounded-md border border-gowl-line bg-gowl-panel p-3">
                  <span className={`mt-0.5 flex-none h-5 w-5 rounded-full text-center text-[12.5px] leading-5 ${ss.bg} ${ss.text} font-bold`}>
                    {ss.icon}
                  </span>
                  <div className="flex-1">
                    <div className="flex items-center gap-2">
                      <span className="text-[13.5px] font-semibold text-gowl-t1">{issue.term}</span>
                      <span className="font-mono text-[11.5px] text-gowl-t6">{issue.rule}</span>
                    </div>
                    <p className="mt-0.5 text-[13px] text-gowl-t4">{issue.message}</p>
                  </div>
                </div>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}
