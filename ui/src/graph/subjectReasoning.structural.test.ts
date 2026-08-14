import { describe, expect, it } from "vitest";
import app from "../App.tsx?raw";
import explorer from "./SubjectExplorer.tsx?raw";
import reasoning from "./ReasoningView.tsx?raw";

/** Plan 113 Slice D — the reasoner's per-subject view, for any subject.
 *
 *  **`ReasoningView` built its own identity: `api.derivedAbout(`1:${assetId}`)`.**
 *  That is a `dsc:`-namespaced Sid over a catalog asset's UUID, and a GST
 *  invoice has neither a UUID nor a `dsc:` identity — so the one place this
 *  console shows *what follows that nobody wrote down* was reachable only for
 *  assets. The server half of this slice widened `GET /reasoning/derived` to
 *  resolve an IRI; this half stops the component manufacturing an identity
 *  only an asset can have.
 *
 *  Structural, because the component only renders anything against a real
 *  reasoning overlay — the same `?raw` technique
 *  `ReviewQueue.structural.test.ts` established, for the same reason: a
 *  rendered-output assertion cannot tell a real generalization from a
 *  component that happens to agree with one on the asset path. The wire
 *  behaviour this depends on is covered for real by
 *  `crates/graph-owl-server/tests/reasoning_for_any_subject.rs`. */

describe("the reasoner's conclusions are reachable for any subject", () => {
  it("takes the subject it should ask about rather than building one from an asset id", () => {
    expect(reasoning).toMatch(/function ReasoningView\(\{\s*subject,/);
    expect(reasoning).toMatch(/api\s*\.derivedAbout\(subject\)/);
  });

  /** The mistake this slice exists to undo, asserted as absent rather than
   *  merely replaced: a component that still knows how to mint a `dsc:` Sid
   *  will grow a second caller that does it again. */
  it("no longer manufactures a dsc-namespaced identity from a UUID", () => {
    expect(reasoning).not.toMatch(/derivedAbout\(`1:\$\{assetId\}`\)/);
    expect(app).not.toMatch(/derivedAbout\(`1:\$\{assetId\}`\)/);
  });

  /** The asset path must keep working — this widens what `ReasoningView`
   *  accepts, it does not move the burden onto every caller to be right. */
  it("still asks about a catalog asset by its dsc identity at the asset call site", () => {
    expect(app).toMatch(/<ReasoningView subject=\{`1:\$\{asset\.id\}`\}/);
  });

  /** And the point of the whole slice: a pack subject reaches it. */
  it("offers the same conclusions beside a pack subject's neighbourhood", () => {
    expect(explorer).toMatch(/ReasoningView/);
    expect(explorer).toMatch(/subject=\{seed\}/);
  });
});
