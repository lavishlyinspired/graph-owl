/** Mutation testing for the console's pure logic.
 *
 *  Scoped to `src/graph/` and `src/auth/` deliberately. That is the code where being wrong is
 *  invisible to the reader — a node counted twice, a truncation flag dropped,
 *  a removal rendered as an absence — and it is the code `00f` says the tests
 *  must assert, as a model rather than as a picture. React components are
 *  covered by their own tests when they have them; mutating markup produces
 *  survivors that mean nothing.
 */
export default {
  testRunner: "vitest",
  coverageAnalysis: "perTest",
  reporters: ["clear-text", "progress"],
  mutate: [
    "src/graph/**/*.ts",
    "!src/graph/**/*.test.ts",
    // `src/auth/pkce.ts` for the same reason, and one more: its failures are
    // silent by nature. A state check that always passes, a verifier that is
    // never consumed, a `plain` code challenge — none of them break a sign-in,
    // they only stop it being safe. `index.tsx` is excluded: it is the
    // side-effecting shell (redirects, storage, React), and mutating it
    // produces survivors that mean nothing.
    "src/auth/pkce.ts",
    // `src/admin/` for the same reason, and it was an omission rather than a
    // decision: `schemaForm.ts` reached 100% by being run explicitly, so the score
    // was real but nothing enforced it. Being in `mutate` is what makes a
    // regression fail a run somebody actually does.
    "src/admin/**/*.ts",
    "!src/admin/**/*.test.ts",
    // `src/trust/` (Epic 39 Slice E) for the same reason: confidence
    // banding, the asserted/derived distinction, and the certification
    // fallback are decisions a screen renders but does not make — the
    // deciding is here, in functions with no DOM. `TrustComponents.tsx` is
    // deliberately excluded; it only calls these and draws the result.
    "src/trust/**/*.ts",
    "!src/trust/**/*.test.ts",
  ],
};
