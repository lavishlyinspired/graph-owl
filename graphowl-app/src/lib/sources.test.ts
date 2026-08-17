import { describe, expect, it } from "vitest";
import { rollupSources } from "./sources";
import type { ConnectorRun } from "./api";

function run(overrides: Partial<ConnectorRun>): ConnectorRun {
  return {
    id: "r1",
    connector: "postgres",
    serviceName: "erp",
    startedAt: "2026-08-15T00:00:00Z",
    finishedAt: "2026-08-15T00:01:00Z",
    created: 10,
    skipped: 0,
    failed: 0,
    deleted: 0,
    refusal: null,
    triggeredBy: "schedule",
    ...overrides,
  };
}

const NOW = new Date("2026-08-18T00:00:00Z");

describe("rollupSources — Plan 122a A6", () => {
  it("groups runs into one row per service, not one row per run", () => {
    const runs = [
      run({ id: "r1", serviceName: "erp" }),
      run({ id: "r2", serviceName: "erp", startedAt: "2026-08-16T00:00:00Z" }),
      run({ id: "r3", serviceName: "crm" }),
    ];
    const sources = rollupSources(runs, NOW);
    expect(sources).toHaveLength(2);
  });

  it("sums created minus deleted across every run for the service, not just the latest", () => {
    const runs = [
      run({ id: "r1", serviceName: "erp", created: 100, deleted: 0 }),
      run({ id: "r2", serviceName: "erp", created: 20, deleted: 5, startedAt: "2026-08-16T00:00:00Z" }),
    ];
    const sources = rollupSources(runs, NOW);
    expect(sources[0]?.objects).toBe(115);
  });

  it("never reports negative objects when deletions outpace creations", () => {
    const runs = [run({ created: 5, deleted: 20 })];
    const sources = rollupSources(runs, NOW);
    expect(sources[0]?.objects).toBe(0);
  });

  it("reports the most recent run's start time as lastSyncAt, not the first", () => {
    const runs = [
      run({ id: "r1", startedAt: "2026-08-10T00:00:00Z" }),
      run({ id: "r2", startedAt: "2026-08-17T00:00:00Z" }),
    ];
    const sources = rollupSources(runs, NOW);
    expect(sources[0]?.lastSyncAt).toBe("2026-08-17T00:00:00Z");
  });

  it("marks a source degraded when its most recent run failed, even if an earlier run succeeded", () => {
    const runs = [
      run({ id: "r1", startedAt: "2026-08-10T00:00:00Z", failed: 0 }),
      run({ id: "r2", startedAt: "2026-08-17T23:00:00Z", failed: 3 }),
    ];
    const sources = rollupSources(runs, NOW);
    expect(sources[0]?.health).toBe("degraded");
  });

  /** Mutator: checking whether *any* run ever failed, rather than the most
   *  recent one, would also call this "degraded" — a since-fixed source
   *  must read as healthy or stale, not stuck degraded forever. */
  it("does not stay degraded once a later run succeeds", () => {
    const runs = [
      run({ id: "r1", startedAt: "2026-08-10T00:00:00Z", failed: 5 }),
      run({ id: "r2", startedAt: "2026-08-17T23:30:00Z", failed: 0 }),
    ];
    const sources = rollupSources(runs, NOW);
    expect(sources[0]?.health).toBe("healthy");
  });

  it("marks a source stale when its last sync is more than 7 days old", () => {
    const runs = [run({ startedAt: "2026-08-05T00:00:00Z", failed: 0 })];
    const sources = rollupSources(runs, NOW);
    expect(sources[0]?.health).toBe("stale");
  });

  it("marks a source healthy when recently synced with no failures", () => {
    const runs = [run({ startedAt: "2026-08-17T23:00:00Z", failed: 0 })];
    const sources = rollupSources(runs, NOW);
    expect(sources[0]?.health).toBe("healthy");
  });

  it("orders sources by most recently synced first", () => {
    const runs = [
      run({ id: "r1", serviceName: "old", startedAt: "2026-08-01T00:00:00Z" }),
      run({ id: "r2", serviceName: "fresh", startedAt: "2026-08-17T00:00:00Z" }),
    ];
    const sources = rollupSources(runs, NOW);
    expect(sources.map((s) => s.serviceName)).toEqual(["fresh", "old"]);
  });

  it("is empty, not broken, when there is no run history", () => {
    expect(rollupSources([], NOW)).toEqual([]);
  });
});
