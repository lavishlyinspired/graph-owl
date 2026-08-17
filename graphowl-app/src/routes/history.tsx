import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import { TraceDetail } from "./TraceDetail";
import { toHistoryConfig } from "../lib/trace";
import { fetchAsset, fetchAssetVersions, type Asset, type AssetVersion } from "../lib/api";
import { strings } from "../lib/strings";

export default function HistoryRoute() {
  const { id } = useParams<{ id?: string }>();
  const [asset, setAsset] = useState<Asset | null>(null);
  const [versions, setVersions] = useState<readonly AssetVersion[] | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    setAsset(null);
    setVersions(null);
    setError(false);
    if (!id) return;
    let live = true;
    Promise.all([fetchAsset(id), fetchAssetVersions(id)])
      .then(([assetData, versionData]) => {
        if (!live) return;
        setAsset(assetData);
        setVersions(versionData);
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
  if (!asset || !versions) {
    return <div className="p-8 text-[13px] text-gowl-t5">{strings.traceLoading}</div>;
  }

  return <TraceDetail config={toHistoryConfig(versions, asset.name)} id={id} />;
}
