/** Filing Periods — Plan 107 Slice 4, the console surface for Slices 1-3's
 *  registered pack queries (`period-list`/`period-summary`/`period-diff`).
 *  Read-only, matching `run_pack_query`'s own non-admin-gated posture: no
 *  affordance here edits anything, it only asks the graph a question.
 *
 *  A pack with no `[[queries]]` entry named `period-list` has simply not
 *  adopted the filing-period pattern — not every pack needs one, the same
 *  way `ObligationCalendar` treats "no pack installed" as a correct,
 *  informative answer rather than an error.
 *
 *  **The component itself is a thin fetch-and-render shell**, the same
 *  posture `obligationCalendar.tsx` already takes; `planPeriodQuery` and
 *  `periodsFromRows` are what is worth testing directly. */

import { useCallback, useEffect, useState } from "react";
import { Alert, Select, Space, Spin, Table, Text, Typography } from "../../components/ui/antd-compat";
import { ApiError, api, type SparqlResult } from "../../api";
import { display as displayTerm, lexical, type Solution } from "../../workbench/results";
import { readParam, writeParam } from "../deepLink";
import { installedPacks } from "../packs/packSurfaces";

const { Title } = Typography;

const ALL_PERIODS = "__all__";

const COPY = {
  title: "Filing periods",
  intro: "What belongs to one filing period, or what changed between two — read-only, nothing here edits the graph.",
  periodALabel: "Period",
  periodBLabel: "Compare against",
  noSecondPeriod: "None",
  loading: "Loading…",
  loadError: "Could not run this query.",
  noPeriods: "This pack has no filing periods declared.",
  pickAPeriod: "Pick a period above to see what's in it.",
  subjectColumn: "Subject",
  typeColumn: "Type",
  periodColumn: "Period",
};

export interface PeriodOption {
  readonly iri: string;
  readonly label: string;
}

/** `period-list`'s raw N-Triples rows, made readable — `lexical()`, not
 *  `display()`: the IRI is what a subsequent `period-summary`/`period-diff`
 *  call needs to bind, so it must stay the full IRI, not truncated to a
 *  local name. Order is `period-list.sparql`'s own `ORDER BY`, preserved
 *  rather than re-sorted here. */
export function periodsFromRows(rows: readonly Solution[]): PeriodOption[] {
  return rows.map((row) => ({
    iri: lexical(row.period ?? ""),
    label: lexical(row.periodLabel ?? ""),
  }));
}

export type PeriodQueryPlan =
  | { readonly name: "period-summary"; readonly bindings: { readonly period: string } }
  | { readonly name: "period-diff"; readonly bindings: { readonly periodA: string; readonly periodB: string } };

/** Which registered query a picker selection maps to. `null` means
 *  nothing is picked yet — there is no honest query to run. Picking the
 *  *same* period twice degrades to `period-summary` rather than a
 *  self-diff: `period-diff.sparql` already guards that case correctly
 *  (Slice 2's `SELECT DISTINCT` fix), but asking it is a pointless
 *  request when `period-summary` answers the identical question with
 *  one bound period instead of two. */
export function planPeriodQuery(periodA: string | null, periodB: string | null): PeriodQueryPlan | null {
  if (periodA === null) return null;
  if (periodB === null || periodB === periodA) {
    return { name: "period-summary", bindings: { period: periodA } };
  }
  return { name: "period-diff", bindings: { periodA, periodB } };
}

/** Whether the *loaded* result has an `onlyIn` column — not whether the
 *  picker is currently in diff mode. `plan?.name === "period-diff"`
 *  flips synchronously the instant a second period is picked, one
 *  render before the async fetch for that plan resolves, so a table
 *  keyed off the plan would try to read `row.onlyIn` off rows that are
 *  still the *previous* (period-summary-shaped) result. Deriving from
 *  the result's own `variables` ties the column set to data that is
 *  actually there — found by running this exact scenario live in a
 *  browser, not assumed: `TypeError: Cannot read properties of
 *  undefined (reading 'replace')` inside `displayTerm`, no unit test
 *  could have caught it because every fixture in this file already had
 *  matching plan/result shapes by construction. */
export function hasDiffColumn(result: SparqlResult | null): boolean {
  return result !== null && result.variables.includes("onlyIn");
}

type LoadState =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "ready"; periods: PeriodOption[] };

type ResultState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "ready"; result: SparqlResult };

export function FilingPeriods() {
  const requested = readParam("pack");
  const [pack, setPack] = useState<string | null>(requested);
  const [periodsState, setPeriodsState] = useState<LoadState>({ kind: "loading" });
  const [periodA, setPeriodA] = useState<string | null>(readParam("periodA"));
  const [periodB, setPeriodB] = useState<string | null>(readParam("periodB"));
  const [resultState, setResultState] = useState<ResultState>({ kind: "idle" });

  useEffect(() => {
    if (requested !== null && requested !== "") return;
    let live = true;
    api
      .namespaces()
      .then((rows) => {
        if (!live) return;
        const installed = installedPacks(rows);
        setPack(installed[0]?.packId ?? "");
      })
      .catch(() => {
        if (live) setPack("");
      });
    return () => {
      live = false;
    };
  }, [requested]);

  const loadPeriods = useCallback(() => {
    if (pack === null) return;
    if (pack === "") {
      setPeriodsState({ kind: "ready", periods: [] });
      return;
    }
    setPeriodsState({ kind: "loading" });
    api.runPackQuery(pack, "period-list", {}).then(
      (result) => setPeriodsState({ kind: "ready", periods: periodsFromRows(result.rows) }),
      (e: unknown) => {
        // A pack with no period-list query has simply not adopted this
        // pattern — a correct, informative empty state, not an error.
        if (e instanceof ApiError && e.problem.status === 404) {
          setPeriodsState({ kind: "ready", periods: [] });
          return;
        }
        setPeriodsState({ kind: "error", message: e instanceof ApiError ? e.message : "unknown error" });
      },
    );
  }, [pack]);

  useEffect(loadPeriods, [loadPeriods]);

  const plan = planPeriodQuery(periodA, periodB);

  useEffect(() => {
    if (pack === null || pack === "" || plan === null) {
      setResultState({ kind: "idle" });
      return;
    }
    let live = true;
    setResultState({ kind: "loading" });
    api.runPackQuery(pack, plan.name, plan.bindings).then(
      (result) => {
        if (live) setResultState({ kind: "ready", result });
      },
      (e: unknown) => {
        if (!live) return;
        setResultState({ kind: "error", message: e instanceof ApiError ? e.message : "unknown error" });
      },
    );
    return () => {
      live = false;
    };
    // `plan` is derived from `periodA`/`periodB` each render; depending on
    // its own fields keeps this effect from re-firing on an unrelated
    // parent re-render that produces a structurally-identical plan.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pack, plan?.name, periodA, periodB]);

  const periods = periodsState.kind === "ready" ? periodsState.periods : [];
  const isDiff = hasDiffColumn(resultState.kind === "ready" ? resultState.result : null);

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <div>
        <Title level={3} style={{ margin: 0, fontWeight: 600, fontSize: 16 }}>
          {COPY.title}
        </Title>
        <Text type="secondary">{COPY.intro}</Text>
      </div>

      {periodsState.kind === "loading" ? (
        <Spin />
      ) : periodsState.kind === "error" ? (
        <Alert type="error" showIcon message={COPY.loadError} description={periodsState.message} />
      ) : periods.length === 0 ? (
        <Text type="secondary">{COPY.noPeriods}</Text>
      ) : (
        <>
          <Space wrap>
            <Space direction="vertical" size={2}>
              <Text type="secondary" style={{ fontSize: 12 }}>
                {COPY.periodALabel}
              </Text>
              <Select
                style={{ width: 200 }}
                value={periodA ?? undefined}
                placeholder={COPY.periodALabel}
                options={periods.map((p) => ({ label: p.label, value: p.iri }))}
                onChange={(v) => {
                  const next = typeof v === "string" ? v : null;
                  setPeriodA(next);
                  writeParam("periodA", next);
                }}
              />
            </Space>
            <Space direction="vertical" size={2}>
              <Text type="secondary" style={{ fontSize: 12 }}>
                {COPY.periodBLabel}
              </Text>
              <Select
                style={{ width: 200 }}
                allowClear
                value={periodB ?? ALL_PERIODS}
                options={[
                  { label: COPY.noSecondPeriod, value: ALL_PERIODS },
                  ...periods.map((p) => ({ label: p.label, value: p.iri })),
                ]}
                onChange={(v) => {
                  const next = typeof v === "string" && v !== ALL_PERIODS ? v : null;
                  setPeriodB(next);
                  writeParam("periodB", next);
                }}
              />
            </Space>
          </Space>

          {resultState.kind === "idle" ? (
            <Text type="secondary">{COPY.pickAPeriod}</Text>
          ) : resultState.kind === "loading" ? (
            <Spin />
          ) : resultState.kind === "error" ? (
            <Alert type="error" showIcon message={COPY.loadError} description={resultState.message} />
          ) : (
            <Table<Solution>
              dataSource={[...resultState.result.rows]}
              rowKey={(row, i) => `${row.subject ?? ""}-${i}`}
              pagination={false}
              columns={[
                {
                  title: COPY.subjectColumn,
                  dataIndex: "subject",
                  render: (subject: string) => displayTerm(subject),
                },
                {
                  title: COPY.typeColumn,
                  dataIndex: "type",
                  render: (type: string) => displayTerm(type),
                },
                ...(isDiff
                  ? [
                      {
                        title: COPY.periodColumn,
                        dataIndex: "onlyIn",
                        render: (onlyIn: string) => displayTerm(onlyIn),
                      },
                    ]
                  : []),
              ]}
            />
          )}
        </>
      )}
    </Space>
  );
}
