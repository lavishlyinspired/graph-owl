import { strings } from "../lib/strings";

interface Task {
  readonly id: string;
  readonly title: string;
  readonly assignee: string;
  readonly priority: "high" | "medium" | "low";
  readonly status: "open" | "in-progress" | "done";
  readonly due: string;
  readonly source: string;
}

const MOCK_TASKS: readonly Task[] = [
  { id: "t1", title: "Review entity resolution merge for Supplier ABC", assignee: "Data Steward", priority: "high", status: "open", due: "2 days", source: "Contradictions" },
  { id: "t2", title: "Approve ontology suggestion: 'Exposure Threshold' class", assignee: "Governance Lead", priority: "medium", status: "in-progress", due: "5 days", source: "Studio > Proposals" },
  { id: "t3", title: "Validate SKOS export before quarterly filing", assignee: "Data Steward", priority: "high", status: "open", due: "1 day", source: "Studio > Validate" },
  { id: "t4", title: "Investigate drift alert on 'Vendor' schema", assignee: "Analyst", priority: "medium", status: "open", due: "3 days", source: "Drift" },
  { id: "t5", title: "Confirm dismissal: dual-filing false positive INV-0892", assignee: "Data Steward", priority: "low", status: "done", due: "done", source: "Resolution" },
  { id: "t6", title: "Review agent grounding drop for 'Ontology suggester'", assignee: "Platform Admin", priority: "medium", status: "open", due: "4 days", source: "Agents" },
];

const PRIORITY_STYLES: Record<Task["priority"], { bg: string; text: string }> = {
  high: { bg: "bg-gowl-bad-bg", text: "text-gowl-bad" },
  medium: { bg: "bg-gowl-amber-bg", text: "text-gowl-amber" },
  low: { bg: "bg-gowl-panel-2", text: "text-gowl-t5" },
};

const STATUS_STYLES: Record<Task["status"], { bg: string; text: string }> = {
  open: { bg: "bg-gowl-panel-2", text: "text-gowl-t2" },
  "in-progress": { bg: "bg-gowl-accent-deep", text: "text-gowl-accent" },
  done: { bg: "bg-gowl-ok-bg", text: "text-gowl-ok" },
};

export default function TasksRoute() {
  const openCount = MOCK_TASKS.filter((t) => t.status !== "done").length;

  return (
    <div className="p-8">
      <div className="mb-5 flex items-end justify-between">
        <div>
          <h1 className="mb-1 text-[21px] font-semibold text-gowl-t1">{strings.tasksTitle}</h1>
          <p className="text-[12.5px] text-gowl-t5">{strings.tasksDescription}</p>
        </div>
        <div className="text-[12px] text-gowl-t5">{openCount} open tasks</div>
      </div>

      <div className="overflow-hidden rounded-lg border border-gowl-line">
        <div className="grid grid-cols-[1fr_120px_90px_90px_80px_110px] gap-2 border-b border-gowl-line bg-gowl-panel-2 px-4 py-2.5 font-mono text-[8.5px] tracking-wider text-gowl-t6">
          <span>TASK</span>
          <span>ASSIGNEE</span>
          <span>PRIORITY</span>
          <span>STATUS</span>
          <span>DUE</span>
          <span>SOURCE</span>
        </div>
        {MOCK_TASKS.map((task) => {
          const ps = PRIORITY_STYLES[task.priority];
          const ss = STATUS_STYLES[task.status];
          return (
            <div
              key={task.id}
              className="grid grid-cols-[1fr_120px_90px_90px_80px_110px] items-center gap-2 border-b border-gowl-row px-4 py-3 last:border-b-0"
            >
              <span className="text-[12.5px] text-gowl-t1">{task.title}</span>
              <span className="text-[11.5px] text-gowl-t4">{task.assignee}</span>
              <span className={`rounded-full px-2 py-0.5 text-center font-mono text-[8.5px] ${ps.bg} ${ps.text}`}>
                {task.priority.toUpperCase()}
              </span>
              <span className={`rounded-full px-2 py-0.5 text-center font-mono text-[8.5px] ${ss.bg} ${ss.text}`}>
                {task.status.toUpperCase()}
              </span>
              <span className="font-mono text-[11px] text-gowl-t5">{task.due}</span>
              <span className="text-[10.5px] text-gowl-t5">{task.source}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
