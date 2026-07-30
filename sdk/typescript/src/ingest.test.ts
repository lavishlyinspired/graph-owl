import { describe, expect, it } from "vitest";
import {
  backoffMs,
  chunk,
  GraphOwlClient,
  IngestBuilder,
  isRetryable,
  MAX_ITEMS_PER_PUSH,
  type IngestRequest,
} from "./ingest.js";

function entities(count: number) {
  return Array.from({ length: count }, (_, n) => ({ kind: "service", name: `svc-${n}` }));
}

describe("chunking", () => {
  it("leaves a push that already fits as one request", () => {
    const request: IngestRequest = { items: entities(3), edges: [] };

    expect(chunk(request)).toHaveLength(1);
  });

  // The ceiling counts items and edges together because the service does —
  // splitting a load across the two fields would otherwise double what one
  // request costs it.
  it("splits a push larger than the ceiling", () => {
    const parts = chunk({ items: entities(MAX_ITEMS_PER_PUSH + 1), edges: [] });

    expect(parts).toHaveLength(2);
    expect(parts[0]!.items).toHaveLength(MAX_ITEMS_PER_PUSH);
    expect(parts[1]!.items).toHaveLength(1);
  });

  // **Edges come last.** An edge whose endpoints landed in an earlier chunk
  // still resolves; one sent before them does not, and a pusher who had to
  // think about that would be doing the catalog's job.
  it("sends every entity before any edge", () => {
    const parts = chunk(
      {
        items: entities(MAX_ITEMS_PER_PUSH + 5),
        edges: [{ fromFqn: "a", toFqn: "b", relationship: "feeds" }],
      },
      MAX_ITEMS_PER_PUSH,
    );

    const firstEdgeChunk = parts.findIndex((part) => part.edges.length > 0);
    const lastItemChunk = parts.map((part) => part.items.length > 0).lastIndexOf(true);
    expect(firstEdgeChunk).toBeGreaterThan(lastItemChunk);
  });

  // An empty push is still one request. The caller asked for something to
  // happen, and silently doing nothing is the least debuggable answer there is.
  it("still sends one request for an empty push", () => {
    expect(chunk({ items: [], edges: [] })).toHaveLength(1);
  });
});

describe("retry policy", () => {
  it("retries what the server said it could not do now", () => {
    expect(isRetryable(429)).toBe(true);
    expect(isRetryable(503)).toBe(true);
  });

  // **409 is not retryable, and that is the interesting case.** An idempotency
  // conflict means the key was used for different content: retrying it can
  // never succeed, and looping hides a caller bug behind 30 seconds of silence.
  it("does not retry a request that can never succeed", () => {
    expect(isRetryable(409)).toBe(false);
    expect(isRetryable(400)).toBe(false);
    expect(isRetryable(404)).toBe(false);
  });

  it("backs off exponentially up to a cap", () => {
    const noJitter = () => 1;

    expect(backoffMs(0, 200, 30_000, noJitter)).toBe(200);
    expect(backoffMs(1, 200, 30_000, noJitter)).toBe(400);
    expect(backoffMs(20, 200, 30_000, noJitter)).toBe(30_000);
  });

  // Jitter, because a fleet of adapters retrying on the same schedule
  // reconverges into the spike that made them retry.
  it("spreads retries out rather than synchronising them", () => {
    expect(backoffMs(3, 200, 30_000, () => 0)).toBeLessThan(
      backoffMs(3, 200, 30_000, () => 1),
    );
  });
});

describe("the client", () => {
  interface Sent {
    path: string;
    key: string | null;
    body: unknown;
  }

  function recording(statuses: number[]) {
    const sent: Sent[] = [];
    let attempt = 0;
    const fetch = (async (url: string, init: RequestInit) => {
      const headers = init.headers as Record<string, string>;
      sent.push({
        path: new URL(url).pathname,
        key: headers["idempotency-key"] ?? null,
        body: init.body ? JSON.parse(init.body as string) : null,
      });
      const status = statuses[Math.min(attempt, statuses.length - 1)]!;
      attempt += 1;
      return new Response(JSON.stringify({ accepted: 1, rejected: 0, results: [] }), {
        status,
      });
    }) as unknown as typeof globalThis.fetch;
    return { sent, fetch };
  }

  function client(fetch: typeof globalThis.fetch, keys: string[] = ["k1", "k2", "k3"]) {
    let next = 0;
    return new GraphOwlClient({
      baseUrl: "http://catalog.test",
      fetch,
      sleep: async () => {},
      newKey: () => keys[Math.min(next++, keys.length - 1)]!,
    });
  }

  it("sends an idempotency key on every push", async () => {
    const { sent, fetch } = recording([207]);

    await client(fetch).push({ items: entities(1), edges: [] });

    expect(sent[0]!.key).toBe("k1");
  });

  // **The mistake this SDK exists to prevent.** A key per *attempt* makes the
  // retry a second push, which is exactly the duplication the key is for.
  it("reuses one key across the retries of a chunk", async () => {
    const { sent, fetch } = recording([503, 503, 207]);

    await client(fetch).push({ items: entities(1), edges: [] });

    expect(sent).toHaveLength(3);
    expect(new Set(sent.map((s) => s.key))).toEqual(new Set(["k1"]));
  });

  // And a *different* chunk is a different request, so it must not share one.
  it("gives each chunk its own key", async () => {
    const { sent, fetch } = recording([207]);

    await client(fetch).push({ items: entities(MAX_ITEMS_PER_PUSH + 1), edges: [] });

    expect(sent).toHaveLength(2);
    expect(sent[0]!.key).not.toBe(sent[1]!.key);
  });

  it("gives up rather than looping on an answer that cannot change", async () => {
    const { sent, fetch } = recording([409]);

    await expect(client(fetch).push({ items: entities(1), edges: [] })).rejects.toThrow(
      /409/,
    );
    expect(sent).toHaveLength(1);
  });

  // Indexes are renumbered against the caller's list: three reports each
  // starting at 0 would leave a client doing the arithmetic this SDK hides.
  it("reports item indexes against the submitted list, not the chunk", async () => {
    let call = 0;
    const fetch = (async () => {
      const index = call === 0 ? 7 : 3;
      call += 1;
      return new Response(
        JSON.stringify({
          accepted: 0,
          rejected: 1,
          results: [{ index, status: 400, problem: "no" }],
        }),
        { status: 207 },
      );
    }) as unknown as typeof globalThis.fetch;

    const result = await client(fetch).push({
      items: entities(MAX_ITEMS_PER_PUSH + 1),
      edges: [],
    });

    expect(result.results.map((r) => r.index)).toEqual([7, MAX_ITEMS_PER_PUSH + 3]);
  });
});

describe("the builder", () => {
  it("assembles entities and edges into one envelope", () => {
    const request = new IngestBuilder()
      .entity({ kind: "service", name: "payments" })
      .entity({ kind: "database", name: "core", parentFqn: "payments" })
      .edge({ fromFqn: "payments.core", toFqn: "payments", relationship: "feeds" })
      .build();

    expect(request.items).toHaveLength(2);
    expect(request.edges).toHaveLength(1);
  });

  // Built envelopes are snapshots. A builder reused after `build()` must not
  // reach back into a request already on its way to the server.
  it("does not let later additions change an envelope already built", () => {
    const builder = new IngestBuilder().entity({ kind: "service", name: "one" });
    const first = builder.build();

    builder.entity({ kind: "service", name: "two" });

    expect(first.items).toHaveLength(1);
  });
});
