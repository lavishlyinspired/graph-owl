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
  ],
};
