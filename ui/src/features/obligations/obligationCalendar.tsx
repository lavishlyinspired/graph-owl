/** The obligation calendar — Epic 105 P8/F4, the platform doc's one new
 *  console route (`plans/105h-obligation-calendar.md`). Reads
 *  `GET /packs/{pack}/obligations`; the difference between a GST filing
 *  deadline and a future domain's obligation lives in `pack.toml`'s
 *  `[findings.span]` band, never here — the same one-surface-every-domain
 *  discipline `findingsQueue.tsx` already established for findings.
 *
 *  **`due` is derived, `anchor` is asserted, and they must never render the
 *  same way** — `00f` non-negotiable 4, the identical rule this console
 *  already applies to a reasoner-derived edge (`derived` tags in
 *  `findingsQueue.tsx`). `due` carries a status tag; `anchor` is plain
 *  text — a reviewer reading either must be able to tell, without opening
 *  the network tab, which one a document actually recorded. */

import { useCallback, useEffect, useState } from "react";
import { Alert, Space, Spin, Table, Tag, Typography } from "antd";
import { ApiError, api, type Obligation } from "../../api";
import { displayTerm } from "../review/findingsQueue";
import { readParam, writeParam } from "../deepLink";
import { installedPacks } from "../packs/packSurfaces";

const { Text, Title } = Typography;

export type ObligationStatus = "overdue" | "dueSoon" | "upcoming";

/** A month is the review cadence this console assumes until a pack says
 *  otherwise — there is no pack-config field for it yet, so it is a display
 *  threshold, not a business rule any finding depends on. */
const DUE_SOON_HORIZON_DAYS = 30;

const COPY = {
  title: "Obligation calendar",
  intro:
    "Every open obligation a pack's rules track, due date first — including ones not yet overdue, which a finding alone never shows.",
  loading: "Loading…",
  loadError: "Could not load the obligation calendar.",
  empty: "Nothing open for this pack.",
  subjectColumn: "Subject",
  labelColumn: "Rule",
  anchorColumn: "Anchor",
  dueColumn: "Due",
  statusColumn: "Status",
  governedByColumn: "Governed by",
  overdue: "overdue",
  dueSoon: "due soon",
  upcoming: "upcoming",
  daysOverdueSuffix: (days: number) => `${days}d overdue`,
  daysRemainingSuffix: (days: number) => `${days}d left`,
};

const STATUS_COLOR: Record<ObligationStatus, string> = {
  overdue: "red",
  dueSoon: "orange",
  upcoming: "default",
};

/** Which bucket an obligation falls in, purely from days remaining — no
 *  reference to "today", so it is the same function whether it runs at
 *  render time or in a test fixed to one instant. */
export function obligationStatus(daysRemaining: number): ObligationStatus {
  if (daysRemaining < 0) return "overdue";
  if (daysRemaining <= DUE_SOON_HORIZON_DAYS) return "dueSoon";
  return "upcoming";
}

/** `?window=` — obligations due within N days, keeping anything already
 *  overdue regardless of the window: something already late needs
 *  attention now, not filtering out because the window is narrow. `null`
 *  (no `?window=` set) returns every obligation, the same "absent means
 *  all" reading `ListFindings`'s own `status` param uses server-side. */
export function withinWindow(
  obligations: readonly Obligation[],
  windowDays: number | null,
): Obligation[] {
  if (windowDays === null) return [...obligations];
  return obligations.filter((o) => o.daysRemaining <= windowDays);
}

type LoadState =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "ready"; obligations: Obligation[] };

export function ObligationCalendar() {
  const requested = readParam("pack");
  const windowParam = readParam("window");
  const windowDays = windowParam !== null && windowParam !== "" ? Number(windowParam) : null;

  const [state, setState] = useState<LoadState>({ kind: "loading" });
  /** **Discovered, never defaulted to `"gst"`.** This read
   *  `readParam("pack") ?? "gst"`, which pointed a deployment that has never
   *  installed the GST pack at a pack that does not exist for them — the
   *  calendar then reported an error, or worse an empty calendar, for a
   *  question nobody asked. `plans/105-f1-pack-admin-tab.md` recorded the gap
   *  when the tab shipped and it was never closed.
   *
   *  `GET /namespaces` already answers "which packs does this deployment
   *  have", because the loader writes `declaredBy: "pack:<id>"` — the same
   *  discovery `packSurfaces.ts` uses, rather than a second mechanism. */
  const [pack, setPack] = useState<string | null>(requested);

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

  const load = useCallback(() => {
    if (pack === null) return;
    if (pack === "") {
      // No pack installed is a correct, informative answer — not an error, and
      // not a request for a pack this deployment does not have.
      setState({ kind: "ready", obligations: [] });
      return;
    }
    setState({ kind: "loading" });
    api.obligationCalendar(pack).then(
      (obligations) => setState({ kind: "ready", obligations }),
      (e: unknown) =>
        setState({ kind: "error", message: e instanceof ApiError ? e.message : "unknown error" }),
    );
  }, [pack]);

  useEffect(load, [load]);

  const rows =
    state.kind === "ready"
      ? withinWindow(state.obligations, windowDays !== null && !Number.isNaN(windowDays) ? windowDays : null)
      : [];

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <div>
        <Title level={3} style={{ margin: 0, fontWeight: 600, fontSize: 16 }}>
          {COPY.title}
        </Title>
        <Text type="secondary">{COPY.intro}</Text>
      </div>

      {state.kind === "loading" ? (
        <Spin />
      ) : state.kind === "error" ? (
        <Alert type="error" showIcon message={COPY.loadError} description={state.message} />
      ) : rows.length === 0 ? (
        <Text type="secondary">{COPY.empty}</Text>
      ) : (
        <>
          <Table<Obligation>
            dataSource={rows}
            rowKey={(o) => `${o.label}-${o.subject}`}
            pagination={false}
            columns={[
              {
                title: COPY.subjectColumn,
                dataIndex: "subject",
                render: (subject: string) => displayTerm(subject),
              },
              {
                title: COPY.labelColumn,
                dataIndex: "label",
                render: (label: string) => displayTerm(label),
              },
              {
                // Asserted — plain text, no tag, the deliberate visual
                // contrast with the derived "Due" column beside it.
                title: COPY.anchorColumn,
                dataIndex: "anchor",
              },
              {
                // Derived — carries the status tag `anchor` never does.
                title: COPY.dueColumn,
                dataIndex: "due",
                render: (due: string, o: Obligation) => (
                  <Space size={6}>
                    <Text>{due}</Text>
                    <Tag color={STATUS_COLOR[obligationStatus(o.daysRemaining)]}>
                      {o.daysRemaining < 0
                        ? COPY.daysOverdueSuffix(-o.daysRemaining)
                        : COPY.daysRemainingSuffix(o.daysRemaining)}
                    </Tag>
                  </Space>
                ),
              },
              {
                title: COPY.statusColumn,
                dataIndex: "daysRemaining",
                render: (daysRemaining: number) => {
                  const status = obligationStatus(daysRemaining);
                  return <Tag color={STATUS_COLOR[status]}>{COPY[status]}</Tag>;
                },
              },
              {
                title: COPY.governedByColumn,
                dataIndex: "governedBy",
                render: (governedBy: string) => <Tag>{governedBy}</Tag>,
              },
            ]}
          />
          {/* The table's own text cells already satisfy `00f`'s non-visual
           *  equivalent for this pattern — unlike a canvas or chart, a
           *  table has no pixels-only content to duplicate. */}
        </>
      )}
    </Space>
  );
}

/** Kept for callers that want to change `?pack=`/`?window=` without
 *  reaching into `deepLink.ts` directly — mirrors the one-line wrapper
 *  pattern `VocabularyBrowser.tsx` already uses for its own params. */
export function setObligationCalendarParams(params: { pack?: string; windowDays?: number | null }): void {
  if (params.pack !== undefined) writeParam("pack", params.pack);
  if (params.windowDays !== undefined) {
    writeParam("window", params.windowDays === null ? null : String(params.windowDays));
  }
}
