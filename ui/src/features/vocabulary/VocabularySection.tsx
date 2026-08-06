/** Epic 42 Slice B: the page-level wiring `App.tsx` renders for its
 *  "Vocabulary" nav item. Picking *which* vocabulary (and, for glossary and
 *  ontology pack, *which instance* of it) is itself vocabulary-specific —
 *  so it lives here, one level above `VocabularyBrowser.tsx`, which must
 *  stay free of exactly this kind of branch (see its own structural test).
 *
 *  Deep-linking keeps Slice A's existing `?vocabulary=<id>&term=<id>` shape
 *  working unchanged for glossary links already in the wild — `kind`
 *  defaults to `"glossary"` when absent, so an old link still lands on the
 *  glossary it named. */

import { useCallback, useEffect, useMemo, useState } from "react";
import { Alert, Empty, Segmented, Select, Space, Spin, Typography } from "antd";
import { api, type Glossary, type OntologyPack } from "../../api";
import { VocabularyBrowser } from "./VocabularyBrowser";
import {
  classificationVocabulary,
  domainVocabulary,
  glossaryVocabulary,
  ontologyPackVocabulary,
  type VocabularyConfig,
} from "./vocabularies";
import { readParam, writeParam } from "./deepLink";

const { Text } = Typography;

const COPY = {
  loading: "Loading vocabularies…",
  loadError: "Vocabularies could not be loaded.",
  kindLabel: "Vocabulary",
  instanceLabel: "Instance",
  noGlossaries: "No glossaries yet",
  noGlossariesDescription: "Create a glossary to start building its terms.",
  noPacks: "No ontology packs imported",
  noPacksDescription: "Import a pack to browse its terms here.",
};

type VocabularyKind = "glossary" | "classification" | "domain" | "ontology-pack";

const KIND_OPTIONS: { readonly label: string; readonly value: VocabularyKind }[] = [
  { label: "Glossary", value: "glossary" },
  { label: "Classifications", value: "classification" },
  { label: "Domains", value: "domain" },
  { label: "Ontology packs", value: "ontology-pack" },
];

function isVocabularyKind(value: string | null): value is VocabularyKind {
  return value === "glossary" || value === "classification" || value === "domain" || value === "ontology-pack";
}

export function VocabularySection() {
  const [kind, setKindRaw] = useState<VocabularyKind>(() => {
    const named = readParam("kind");
    return isVocabularyKind(named) ? named : "glossary";
  });
  const [glossaries, setGlossaries] = useState<readonly Glossary[] | null>(null);
  const [packs, setPacks] = useState<readonly OntologyPack[] | null>(null);
  const [instanceId, setInstanceIdRaw] = useState<string | null>(() => readParam("vocabulary"));
  const [error, setError] = useState<string | null>(null);

  const setKind = useCallback((next: VocabularyKind) => {
    setKindRaw(next);
    writeParam("kind", next === "glossary" ? null : next);
    setInstanceIdRaw(null);
    writeParam("vocabulary", null);
    writeParam("term", null);
  }, []);

  const setInstanceId = useCallback((next: string | null) => {
    setInstanceIdRaw(next);
    writeParam("vocabulary", next);
    writeParam("term", null);
  }, []);

  useEffect(() => {
    if (kind !== "glossary") return;
    let cancelled = false;
    api
      .glossaries()
      .then((fetched) => {
        if (cancelled) return;
        setGlossaries(fetched);
        const first = fetched[0];
        if (instanceId === null && first) setInstanceId(first.id);
      })
      .catch(() => {
        if (!cancelled) setError(COPY.loadError);
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- runs once per `kind` switch, not on every instanceId change
  }, [kind]);

  useEffect(() => {
    if (kind !== "ontology-pack") return;
    let cancelled = false;
    api
      .ontologyPacks()
      .then((fetched) => {
        if (cancelled) return;
        setPacks(fetched);
        const first = fetched[0];
        if (instanceId === null && first) setInstanceId(first.id);
      })
      .catch(() => {
        if (!cancelled) setError(COPY.loadError);
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- runs once per `kind` switch, not on every instanceId change
  }, [kind]);

  const selectedPack = useMemo(
    () => packs?.find((p) => p.id === instanceId) ?? null,
    [packs, instanceId],
  );

  const config: VocabularyConfig | null = useMemo(() => {
    if (kind === "classification") return classificationVocabulary();
    if (kind === "domain") return domainVocabulary();
    if (kind === "glossary") return instanceId ? glossaryVocabulary(instanceId) : null;
    return selectedPack ? ontologyPackVocabulary(selectedPack) : null;
  }, [kind, instanceId, selectedPack]);

  if (error) return <Alert type="error" message={error} showIcon />;

  return (
    <Space direction="vertical" size="large" style={{ width: "100%" }}>
      <Space wrap>
        <Segmented
          value={kind}
          onChange={(value) => setKind(value as VocabularyKind)}
          options={KIND_OPTIONS}
          aria-label={COPY.kindLabel}
        />
        {kind === "glossary" && glossaries && glossaries.length > 0 && (
          <Select
            value={instanceId ?? undefined}
            onChange={setInstanceId}
            style={{ minWidth: 220 }}
            aria-label={COPY.instanceLabel}
            options={glossaries.map((g) => ({ label: g.name, value: g.id }))}
          />
        )}
        {kind === "ontology-pack" && packs && packs.length > 0 && (
          <Select
            value={instanceId ?? undefined}
            onChange={setInstanceId}
            style={{ minWidth: 220 }}
            aria-label={COPY.instanceLabel}
            options={packs.map((p) => ({ label: `${p.packId} ${p.version}`, value: p.id }))}
          />
        )}
      </Space>

      {kind === "glossary" && glossaries === null ? (
        <Space direction="vertical" align="center" style={{ width: "100%", padding: 48 }}>
          <Spin />
          <Text>{COPY.loading}</Text>
        </Space>
      ) : kind === "glossary" && glossaries?.length === 0 ? (
        <Empty description={<Text>{COPY.noGlossariesDescription}</Text>}>{COPY.noGlossaries}</Empty>
      ) : kind === "ontology-pack" && packs === null ? (
        <Space direction="vertical" align="center" style={{ width: "100%", padding: 48 }}>
          <Spin />
          <Text>{COPY.loading}</Text>
        </Space>
      ) : kind === "ontology-pack" && packs?.length === 0 ? (
        <Empty description={<Text>{COPY.noPacksDescription}</Text>}>{COPY.noPacks}</Empty>
      ) : config ? (
        <VocabularyBrowser key={`${kind}:${instanceId ?? ""}`} config={config} />
      ) : null}
    </Space>
  );
}
