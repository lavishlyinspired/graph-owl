import { describe, expect, it } from "vitest";
import { formatCount, formatPct, relativeTime } from "./format";

const NOW = new Date("2026-08-17T12:00:00Z");

describe("relativeTime", () => {
  it("reads as just now inside the first minute", () => {
    expect(relativeTime("2026-08-17T11:59:30Z", NOW)).toBe("just now");
  });

  it("reads in minutes under an hour", () => {
    expect(relativeTime("2026-08-17T11:48:00Z", NOW)).toBe("12 min ago");
  });

  it("reads in hours under a day", () => {
    expect(relativeTime("2026-08-17T09:00:00Z", NOW)).toBe("3 h ago");
  });

  it("reads in days beyond that", () => {
    expect(relativeTime("2026-08-12T12:00:00Z", NOW)).toBe("5 d ago");
  });

  /** The boundary a naive `< HOUR_MS` off-by-one gets wrong: exactly 60
   *  minutes must read as hours, not as "60 min ago". */
  it("crosses cleanly from minutes to hours at the hour boundary", () => {
    expect(relativeTime("2026-08-17T11:00:00Z", NOW)).toBe("1 h ago");
  });
});

describe("formatCount", () => {
  it("groups thousands", () => {
    expect(formatCount(1843220)).toBe("1,843,220");
  });

  it("leaves a small number ungrouped", () => {
    expect(formatCount(42)).toBe("42");
  });
});

describe("formatPct", () => {
  it("rounds to the nearest whole percent", () => {
    expect(formatPct(90.6)).toBe("91%");
  });

  it("does not round 99.x down to 99 when the input is below it — the negative case", () => {
    expect(formatPct(99.5)).toBe("100%");
    expect(formatPct(99.4)).toBe("99%");
  });
});
