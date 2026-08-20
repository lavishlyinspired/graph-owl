/** The shape a trace-style detail view reduces to — Plan 122a A4, since
 *  narrowed to Paths alone (Explore's own Entity tab absorbed Lineage,
 *  History and Evidence, each with real per-entity data `TraceDetail`'s
 *  generic KPI/table shape did not carry — provenance sources, real
 *  upstream/downstream counts, live SPARQL, none of which fit "four
 *  screens, one config shape"). Kept pure and separate from the component
 *  on purpose — `00f` requires graph/trace tests to assert the model
 *  (which KPI, which row survived) rather than the picture, and a config
 *  object is what makes that possible without rendering anything. */

import type { FoundPath } from "./api";
import { strings } from "./strings";
import type { RouteName } from "./routes";

export interface TraceKpi {
  readonly label: string;
  readonly value: string;
  readonly sub?: string;
}

export interface TraceCell {
  readonly text: string;
  readonly sub?: string;
  readonly mono?: boolean;
}

export interface TraceRow {
  readonly key: string;
  readonly cells: readonly TraceCell[];
}

export interface TraceRelated {
  readonly label: string;
  readonly route: RouteName;
  readonly id?: string;
}

export interface TraceConfig {
  readonly title: string;
  readonly description: string;
  readonly kpis: readonly TraceKpi[];
  readonly columns: readonly string[];
  readonly rows: readonly TraceRow[];
  readonly emptyMessage: string;
  readonly noteTitle: string;
  readonly noteBody: string;
  readonly related: readonly TraceRelated[];
}

interface PathSearchResultLike {
  readonly paths: readonly FoundPath[];
  readonly truncated: boolean;
}

export function toPathsConfig(result: PathSearchResultLike, from: string, to: string): TraceConfig {
  return {
    title: strings.pathsTitle,
    description: strings.pathsDescription,
    kpis: [
      { label: strings.pathsKpiFrom, value: from },
      { label: strings.pathsKpiTo, value: to },
      { label: strings.pathsKpiFound, value: String(result.paths.length) },
      {
        label: strings.pathsKpiTruncated,
        value: result.truncated ? strings.lineageTruncatedYes : strings.lineageTruncatedNo,
      },
    ],
    columns: [strings.pathsColPath, strings.pathsColRoute, strings.pathsColHops],
    rows: result.paths.map((path, index) => ({
      key: `${index}`,
      cells: [
        { text: String(index + 1), mono: true },
        { text: path.nodes.join(" → "), mono: true },
        { text: String(path.length) },
      ],
    })),
    emptyMessage: strings.pathsEmpty,
    noteTitle: strings.pathsNoteTitle,
    noteBody: strings.pathsNoteBody,
    related: [
      { label: strings.traceRelatedExplore, route: "explore", id: from },
    ],
  };
}
