import { useEffect, useState } from "react";
import { fetchMcpTools, type McpTool } from "../lib/api";
import { strings } from "../lib/strings";

export default function McpRoute() {
  const [tools, setTools] = useState<readonly McpTool[] | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    fetchMcpTools()
      .then(setTools)
      .catch(() => setError(true));
  }, []);

  if (error) {
    return <div className="p-8 text-[17px] text-gowl-bad">{strings.governError}</div>;
  }
  if (!tools) {
    return <div className="p-8 text-[17px] text-gowl-t5">{strings.governLoading}</div>;
  }

  return (
    <div className="p-8">
      <h1 className="mb-1 text-[25px] font-semibold text-gowl-t1">{strings.mcpTitle}</h1>
      <p className="mb-5 text-[16.5px] text-gowl-t5">{strings.mcpDescription}</p>

      <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
        <div className="grid grid-cols-[200px_1fr] gap-3 border-b border-gowl-line bg-gowl-panel-2 px-4 py-2 font-mono text-[13.5px] tracking-wider text-gowl-t6">
          <span>{strings.mcpColName}</span>
          <span>{strings.mcpColDescription}</span>
        </div>
        {tools.length === 0 ? (
          <div className="p-6 text-[16.5px] text-gowl-t5">{strings.mcpEmpty}</div>
        ) : (
          tools.map((tool) => (
            <div
              key={tool.name}
              className="grid grid-cols-[200px_1fr] items-center gap-3 border-b border-gowl-row px-4 py-2.5 last:border-b-0"
            >
              <span className="truncate font-mono text-[16px] text-gowl-t1">{tool.name}</span>
              <span className="truncate text-[16px] text-gowl-t5">{tool.description ?? "—"}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
