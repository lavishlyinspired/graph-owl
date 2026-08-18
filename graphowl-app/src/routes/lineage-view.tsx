import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import { TraceDetail } from "./TraceDetail";
import { toLineageConfig } from "../lib/trace";
import { fetchAsset, fetchLineage, type Asset, type LineageGraph } from "../lib/api";
import { strings } from "../lib/strings";

export default function LineageRoute() {
  const { id } = useParams<{ id?: string }>();
  const [asset, setAsset] = useState<Asset | null>(null);
  const [graph, setGraph] = useState<LineageGraph | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    setAsset(null);
    setGraph(null);
    setError(false);
    if (!id) return;
    let live = true;
    Promise.all([fetchAsset(id), fetchLineage(id, { upstream: 2, downstream: 2 })])
      .then(([assetData, graphData]) => {
        if (!live) return;
        setAsset(assetData);
        setGraph(graphData);
      })
      .catch(() => {
        if (live) setError(true);
      });
    return () => {
      live = false;
    };
  }, [id]);

  if (!id) {
    return <div className="p-8 text-[13px] text-gowl-t5">{strings.traceNoSeed}</div>;
  }
  if (error) {
    return <div className="p-8 text-[13px] text-gowl-bad">{strings.traceError}</div>;
  }
  if (!asset || !graph) {
    return <div className="p-8 text-[13px] text-gowl-t5">{strings.traceLoading}</div>;
  }

  return <TraceDetail config={toLineageConfig(graph, asset.name)} id={id} />;
}
