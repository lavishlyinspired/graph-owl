// The ingestion client — Epic 16 Slice E.
//
// Decision 5: "SDKs are generated from the OpenAPI contract, hand-wrapped for
// ergonomics. Generated-only clients are unpleasant; hand-written ones drift."
// So the *types* come from `src/generated/api.d.ts`, which is produced from
// `openapi.json` and deliberately not committed, and everything in this file is
// the ergonomics layer: the four things every pusher would otherwise write
// itself, badly.
//
//   1. batching, because the server refuses more than 1000 items in one request
//   2. idempotency keys, because a retry without one duplicates
//   3. retry with backoff, because at-least-once transport is the normal case
//   4. a builder, because assembling the envelope by hand invites typos in
//      field names that only fail at the server

/** How many items and edges the service accepts in one synchronous push. */
export const MAX_ITEMS_PER_PUSH = 1000;

export interface EntityDraft {
  kind: string;
  name: string;
  parentFqn?: string;
  description?: string;
  properties?: Record<string, unknown>;
}

export interface EdgeDraft {
  fromFqn: string;
  toFqn: string;
  relationship: string;
  query?: string;
  description?: string;
}

export interface IngestRequest {
  items: EntityDraft[];
  edges: EdgeDraft[];
}

export interface ItemResult {
  index: number;
  status: number;
  id?: string;
  problem?: string;
}

export interface PushResult {
  accepted: number;
  rejected: number;
  results: ItemResult[];
}

export interface JobHandle {
  id: string;
  state: string;
  poll: string;
}

export interface Job {
  id: string;
  state: "queued" | "running" | "succeeded" | "partial" | "failed";
  rowsRead: number;
  accepted: number;
  rejected: number;
  failures: { row: number; detail: string }[];
  haltReason?: string | null;
}

/** Assemble a push without hand-writing the envelope. */
export class IngestBuilder {
  private readonly items: EntityDraft[] = [];
  private readonly edges: EdgeDraft[] = [];

  entity(draft: EntityDraft): this {
    this.items.push(draft);
    return this;
  }

  /**
   * An edge between two entities, named by FQN.
   *
   * Endpoints may be entities added to this same push: the service orders a
   * batch itself, so a pusher never has to submit in dependency order.
   */
  edge(draft: EdgeDraft): this {
    this.edges.push(draft);
    return this;
  }

  build(): IngestRequest {
    return { items: [...this.items], edges: [...this.edges] };
  }
}

/**
 * Split a push into requests the service will accept.
 *
 * The ceiling counts items **and** edges together, because the service does —
 * splitting a load across the two fields would otherwise double what one
 * request costs it.
 *
 * Edges follow all the entities rather than riding along with them. An edge
 * whose endpoints landed in an earlier chunk still resolves; an edge sent
 * before its endpoints does not, and a pusher who had to think about that would
 * be doing the catalog's job.
 */
export function chunk(
  request: IngestRequest,
  limit: number = MAX_ITEMS_PER_PUSH,
): IngestRequest[] {
  if (limit < 1) throw new Error("a chunk holds at least one item");

  const chunks: IngestRequest[] = [];
  for (let at = 0; at < request.items.length; at += limit) {
    chunks.push({ items: request.items.slice(at, at + limit), edges: [] });
  }
  for (let at = 0; at < request.edges.length; at += limit) {
    chunks.push({ items: [], edges: request.edges.slice(at, at + limit) });
  }
  // An empty push is still one request: the caller asked for something to
  // happen, and silently doing nothing is the least debuggable answer.
  return chunks.length > 0 ? chunks : [{ items: [], edges: [] }];
}

/**
 * How long to wait before attempt `n` (0-based), in milliseconds.
 *
 * Exponential, capped, and jittered. The cap matters because an uncapped
 * doubling reaches hours; the jitter matters because a fleet of adapters that
 * all retry on the same schedule reconverges into the same spike that made them
 * retry.
 */
export function backoffMs(
  attempt: number,
  base = 200,
  cap = 30_000,
  random: () => number = Math.random,
): number {
  const ceiling = Math.min(cap, base * 2 ** attempt);
  return Math.round(ceiling * (0.5 + random() / 2));
}

/** A status worth retrying: the server said "not now", not "not ever". */
export function isRetryable(status: number): boolean {
  // 409 is deliberately absent. An idempotency conflict means this key was used
  // for *different* content, and retrying with the same key cannot ever succeed
  // — it is a bug in the caller, and looping on it hides that for 30 seconds.
  return status === 429 || (status >= 500 && status <= 599);
}

export interface ClientOptions {
  baseUrl: string;
  token?: string;
  maxAttempts?: number;
  fetch?: typeof globalThis.fetch;
  /** Injected so the retry schedule is testable without waiting for it. */
  sleep?: (ms: number) => Promise<void>;
  newKey?: () => string;
}

export class GraphOwlError extends Error {
  constructor(
    readonly status: number,
    readonly body: unknown,
  ) {
    super(`graph-owl responded ${status}: ${JSON.stringify(body)}`);
  }
}

export class GraphOwlClient {
  private readonly baseUrl: string;
  private readonly token?: string;
  private readonly maxAttempts: number;
  private readonly doFetch: typeof globalThis.fetch;
  private readonly sleep: (ms: number) => Promise<void>;
  private readonly newKey: () => string;

  constructor(options: ClientOptions) {
    this.baseUrl = options.baseUrl.replace(/\/$/, "");
    this.token = options.token;
    this.maxAttempts = options.maxAttempts ?? 5;
    this.doFetch = options.fetch ?? globalThis.fetch.bind(globalThis);
    this.sleep = options.sleep ?? ((ms) => new Promise((r) => setTimeout(r, ms)));
    this.newKey = options.newKey ?? (() => crypto.randomUUID());
  }

  /**
   * Push, splitting and retrying as needed.
   *
   * **Each chunk gets one key, and every retry of that chunk reuses it.** A key
   * per attempt would make the retry a second push, which is the exact
   * duplication the key exists to prevent — the single most common way a
   * hand-written client gets this wrong.
   */
  async push(request: IngestRequest): Promise<PushResult> {
    const merged: PushResult = { accepted: 0, rejected: 0, results: [] };
    let offset = 0;
    for (const part of chunk(request)) {
      const key = this.newKey();
      const answer = await this.send<PushResult>("POST", "/ingest", {
        body: part,
        idempotencyKey: key,
      });
      merged.accepted += answer.accepted;
      merged.rejected += answer.rejected;
      // Indexes are renumbered against the *caller's* list. A client that sent
      // 2500 items and got three reports each starting at index 0 would have to
      // reconstruct the arithmetic this SDK exists to hide.
      for (const result of answer.results) {
        merged.results.push({ ...result, index: result.index + offset });
      }
      offset += part.items.length + part.edges.length;
    }
    return merged;
  }

  /** Upload a batch file. Returns immediately with a handle to poll. */
  async pushFile(body: string | Uint8Array, format: "jsonl" | "csv"): Promise<JobHandle> {
    return this.send<JobHandle>("POST", "/ingest/batch", {
      body,
      contentType: format === "jsonl" ? "application/x-ndjson" : "text/csv",
    });
  }

  async job(id: string): Promise<Job> {
    return this.send<Job>("GET", `/ingest/jobs/${id}`);
  }

  async cancelJob(id: string): Promise<Job> {
    return this.send<Job>("DELETE", `/ingest/jobs/${id}`);
  }

  /** Poll until the job stops moving. */
  async awaitJob(id: string, attempts = 600): Promise<Job> {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      const job = await this.job(id);
      if (job.state !== "queued" && job.state !== "running") return job;
      await this.sleep(backoffMs(Math.min(attempt, 4), 100, 2000));
    }
    throw new Error(`job ${id} never settled`);
  }

  private async send<T>(
    method: string,
    path: string,
    options: {
      body?: unknown;
      idempotencyKey?: string;
      contentType?: string;
    } = {},
  ): Promise<T> {
    const headers: Record<string, string> = {};
    if (this.token) headers.authorization = `Bearer ${this.token}`;
    if (options.idempotencyKey) headers["idempotency-key"] = options.idempotencyKey;

    let body: BodyInit | undefined;
    if (options.body !== undefined) {
      if (typeof options.body === "string" || options.body instanceof Uint8Array) {
        headers["content-type"] = options.contentType ?? "application/octet-stream";
        body = options.body as BodyInit;
      } else {
        headers["content-type"] = "application/json";
        body = JSON.stringify(options.body);
      }
    }

    let last: GraphOwlError | undefined;
    for (let attempt = 0; attempt < this.maxAttempts; attempt += 1) {
      const response = await this.doFetch(`${this.baseUrl}${path}`, {
        method,
        headers,
        body,
      });
      const text = await response.text();
      const parsed: unknown = text.length > 0 ? JSON.parse(text) : null;
      if (response.ok || response.status === 207) return parsed as T;

      last = new GraphOwlError(response.status, parsed);
      if (!isRetryable(response.status)) throw last;
      await this.sleep(backoffMs(attempt));
    }
    throw last ?? new Error("no attempt was made");
  }
}
