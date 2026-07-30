// The end-to-end criterion: "an end-to-end test pushes through each SDK against
// a running service." Skipped unless one is pointed at, so `npm test` stays
// runnable without Docker — `scripts/verify-sdks.sh` is what supplies the URL.

import { describe, expect, it } from "vitest";
import { GraphOwlClient, IngestBuilder } from "./ingest.js";

const baseUrl = process.env.GRAPH_OWL_BASE_URL;
const live = baseUrl ? describe : describe.skip;

live("against a live service", () => {
  const client = () => new GraphOwlClient({ baseUrl: baseUrl! });
  const suffix = Math.random().toString(36).slice(2, 8);

  it("pushes a hierarchy and an edge in one call", async () => {
    const request = new IngestBuilder()
      .entity({ kind: "service", name: `ts-${suffix}` })
      .entity({ kind: "database", name: "core", parentFqn: `ts-${suffix}` })
      .entity({ kind: "schema", name: "public", parentFqn: `ts-${suffix}.core` })
      .entity({ kind: "table", name: "orders", parentFqn: `ts-${suffix}.core.public` })
      .entity({ kind: "table", name: "shipments", parentFqn: `ts-${suffix}.core.public` })
      .edge({
        fromFqn: `ts-${suffix}.core.public.orders`,
        toFqn: `ts-${suffix}.core.public.shipments`,
        relationship: "feeds",
      })
      .build();

    const result = await client().push(request);

    expect(result.rejected).toBe(0);
    expect(result.accepted).toBe(6);
  });

  // A retry is the normal case for at-least-once transport, and the whole point
  // of the key is that the second attempt changes nothing.
  it("a replayed push creates nothing the second time", async () => {
    const key = crypto.randomUUID();
    const fixed = new GraphOwlClient({ baseUrl: baseUrl!, newKey: () => key });
    const request = new IngestBuilder()
      .entity({ kind: "service", name: `ts-idem-${suffix}` })
      .build();

    const first = await fixed.push(request);
    const second = await fixed.push(request);

    expect(second).toEqual(first);
  });

  it("uploads a batch file and polls it to a verdict", async () => {
    const file =
      `{"kind":"service","name":"ts-batch-${suffix}"}\n` +
      `{"kind":"database","name":"core","parentFqn":"ts-batch-${suffix}"}\n` +
      `this line is not json\n`;

    const handle = await client().pushFile(file, "jsonl");
    const job = await client().awaitJob(handle.id);

    expect(job.state).toBe("partial");
    expect(job.accepted).toBe(2);
    expect(job.failures[0]!.row).toBe(3);
  });
});
