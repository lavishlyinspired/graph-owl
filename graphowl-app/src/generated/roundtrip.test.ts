/** Epic 1 Slice K: the contract describes the service.
 *
 *  A spec can be valid and wrong — every `$ref` resolving says nothing about
 *  whether the *shapes* match what the server sends. This drives a real service
 *  through types generated from `openapi.json`, so a spec that has drifted from
 *  the implementation fails to compile or fails here.
 *
 *  Run by `scripts/verify-generated-client.sh`, which regenerates the contract,
 *  regenerates the client, starts a server, and points this at it. It skips
 *  when `GRAPH_OWL_BASE_URL` is unset, because a test that silently passes
 *  without a server is worse than one that is visibly not run.
 */
import { describe, expect, it } from "vitest";
import type { components, paths } from "./api";

const BASE = process.env.GRAPH_OWL_BASE_URL;

/** The generated types, used as types — a compile error here *is* the test. */
type Asset = components["schemas"]["Asset"];
type Problem = components["schemas"]["Problem"];
type AssetPage = components["schemas"]["Page_Asset"];
type UpsertAsset = components["schemas"]["UpsertAsset"];
type AssetUpdate = components["schemas"]["AssetUpdate"];

/** Every path the round-trip uses must exist in the contract. A rename that
 *  the spec did not follow fails to compile rather than at runtime. */
type _CreatePath = paths["/assets"]["post"];
type _ReadPath = paths["/assets/{id}"]["get"];
type _PatchPath = paths["/assets/{id}"]["patch"];
type _DeletePath = paths["/assets/{id}"]["delete"];

async function call<T>(path: string, init?: RequestInit): Promise<{ status: number; body: T }> {
  const response = await fetch(`${BASE}${path}`, {
    ...init,
    headers: { "content-type": "application/json", ...init?.headers },
  });
  return { status: response.status, body: (await response.json()) as T };
}

describe.skipIf(!BASE)("the generated client against a live service", () => {
  it("round-trips create → read → list → patch → delete", async () => {
    const create: UpsertAsset = { kind: "service", name: `svc-${Date.now()}` };
    const created = await call<Asset>("/assets", {
      method: "POST",
      body: JSON.stringify(create),
    });
    expect(created.status).toBe(201);
    expect(created.body.id).toBeTruthy();
    expect(created.body.fullyQualifiedName).toBe(create.name);

    const read = await call<Asset>(`/assets/${created.body.id}`);
    expect(read.status).toBe(200);
    expect(read.body.id).toBe(created.body.id);

    const listed = await call<AssetPage>("/assets?limit=5");
    expect(listed.status).toBe(200);
    expect(Array.isArray(listed.body.data)).toBe(true);

    const update: AssetUpdate = { description: "described by the generated client" };
    const patched = await call<Asset>(`/assets/${created.body.id}`, {
      method: "PATCH",
      body: JSON.stringify(update),
    });
    expect(patched.status).toBe(200);
    expect(patched.body.description).toBe(update.description);
    // The envelope moved, which is what proves the PATCH did something rather
    // than echoing the body back.
    expect(patched.body.version).not.toEqual(created.body.version);

    // `200` with a cascade count, not `204`. The count is the point: a delete
    // that silently tombstoned four hundred columns and returned no body would
    // leave an operator unable to tell whether it did what they meant. The
    // contract said `204` until this test ran against a real server.
    const deleted = await call<{ deleted: number }>(`/assets/${created.body.id}`, {
      method: "DELETE",
    });
    expect(deleted.status).toBe(200);
    expect(deleted.body.deleted).toBeGreaterThan(0);
  });

  /** The half clients get wrong. A typed problem is the difference between a
   *  client that branches on `type` and one that string-matches a message. */
  it("deserializes an error into typed problem+json", async () => {
    const missing = await call<Problem>("/assets/99999999-9999-4999-8999-999999999999");

    expect(missing.status).toBe(404);
    expect(missing.body.status).toBe(404);
    expect(missing.body.type).toMatch(/^https:\/\//);
    expect(missing.body.title).toBeTruthy();
  });

  /** Pagination through the client, following a cursor — the part a generated
   *  client gets wrong silently, by treating the opaque token as optional. */
  it("follows a cursor to the next page", async () => {
    for (let i = 0; i < 3; i += 1) {
      await call<Asset>("/assets", {
        method: "POST",
        body: JSON.stringify({ kind: "service", name: `page-${Date.now()}-${i}` }),
      });
    }

    const first = await call<AssetPage>("/assets?limit=2");
    expect(first.body.data.length).toBe(2);
    const cursor = first.body.paging.after;
    expect(cursor).toBeTruthy();

    const second = await call<AssetPage>(`/assets?limit=2&after=${encodeURIComponent(cursor!)}`);
    expect(second.status).toBe(200);

    const firstIds = first.body.data.map((a) => a.id);
    const secondIds = second.body.data.map((a) => a.id);
    expect(secondIds.some((id) => firstIds.includes(id))).toBe(false);
  });
});
