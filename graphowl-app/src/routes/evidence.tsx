import { useEffect, useState } from "react";
import { TraceDetail } from "./TraceDetail";
import { toEvidenceConfig } from "../lib/trace";
import { fetchFindings, type Finding } from "../lib/api";
import { strings } from "../lib/strings";

export default function EvidenceRoute() {
  const [findings, setFindings] = useState<readonly Finding[] | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    let live = true;
    fetchFindings()
      .then((data) => {
        if (live) setFindings(data);
      })
      .catch(() => {
        if (live) setError(true);
      });
    return () => {
      live = false;
    };
  }, []);

  if (error) {
    return <div className="p-8 text-[13px] text-gowl-bad">{strings.traceError}</div>;
  }
  if (!findings) {
    return <div className="p-8 text-[13px] text-gowl-t5">{strings.traceLoading}</div>;
  }

  return <TraceDetail config={toEvidenceConfig(findings)} />;
}
